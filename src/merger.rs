use lopdf::{Document, Object, ObjectId};
use std::collections::BTreeMap;

/// Límite de profundidad para prevenir referencias circulares en PDFs maliciosos/corruptos
const MAX_DEPTH: usize = 100;

/// Une múltiples PDFs en uno solo.
/// Copia recursivamente todas las páginas y objetos referenciados.
pub fn merge_pdfs(input_paths: &[String], output_path: &str) -> Result<(), String> {
    if input_paths.is_empty() {
        return Err("No hay archivos para unir".to_string());
    }

    // Validar existencia, pero NO bloquear por parseo de lopdf
    for path in input_paths {
        if !std::path::Path::new(path).exists() {
            return Err(format!("Archivo no encontrado: {}", path));
        }
    }

    // Caso trivial: un solo archivo → copiar/renombrar SIN validar parseo
    if input_paths.len() == 1 {
        let src = &input_paths[0];
        if src != output_path {
            std::fs::copy(src, output_path)
                .map_err(|e| format!("Error copiando {} → {}: {}", src, output_path, e))?;
        }
        return Ok(());
    }

    // Cargar los documentos, saltando los que fallen
    let mut docs: Vec<(String, Document)> = Vec::new();
    let mut load_errors: Vec<String> = Vec::new();

    for path in input_paths {
        match Document::load(path) {
            Ok(doc) => docs.push((path.clone(), doc)),
            Err(e) => {
                load_errors.push(format!("{} → lopdf: {}", path, e));
            }
        }
    }

    if docs.is_empty() {
        return Err(format!(
            "Ningún PDF pudo ser cargado. Errores: {}",
            load_errors.join(" | ")
        ));
    }

    // Si hay errores de carga, los reportamos como warning (no bloquean el merge)
    let warning = if !load_errors.is_empty() {
        Some(format!(" ({} archivos omitidos: {})", load_errors.len(), load_errors.join("; ")))
    } else {
        None
    };

    // Cargar el primer documento válido como base
    let (_, first_doc) = docs.remove(0);
    let mut merged = first_doc;

    // Obtener la raíz del árbol de páginas del merged
    let pages_root_id = find_pages_root(&merged)
        .ok_or("No se encontró el árbol de páginas en el documento base")?;

    // Fusionar cada documento adicional
    for (_path, doc) in &docs {

        // Mapa de renumeración: old_id → new_id
        let mut id_map: BTreeMap<ObjectId, ObjectId> = BTreeMap::new();

        // Obtener páginas del documento fuente (ordenadas por número)
        let src_pages = doc.get_pages(); // BTreeMap<u32, ObjectId>
        let mut page_nums: Vec<_> = src_pages.keys().copied().collect();
        page_nums.sort();

        for page_num in &page_nums {
            if let Some(&page_obj_id) = src_pages.get(page_num) {
                // Copiar recursivamente la página y sus dependencias
                deep_copy(&doc, &mut merged, page_obj_id, &mut id_map, 0);

                // Añadir la página copiada al Pages root
                if let Some(&new_page_id) = id_map.get(&page_obj_id) {
                    add_kid_to_pages(&mut merged, pages_root_id, new_page_id);
                }
            }
        }
    }

    merged
        .save(output_path)
        .map_err(|e| format!("Error guardando {}: {}", output_path, e))?;

    // Si hubo warnings, los adjuntamos al Ok
    if let Some(w) = warning {
        return Err(format!("Merge parcial (algunos PDFs no pudieron cargarse): {}", w));
    }

    Ok(())
}

/// Busca el ObjectId de la raíz del árbol de páginas (Pages root)
fn find_pages_root(doc: &Document) -> Option<ObjectId> {
    // Buscar el catálogo
    for (_id, obj) in doc.objects.iter() {
        if let Object::Dictionary(dict) = obj {
            if let Ok(Object::Name(name)) = dict.get(b"Type") {
                if name == b"Catalog" {
                    // Buscar /Pages en el catálogo
                    if let Ok(Object::Reference(pages_ref)) = dict.get(b"Pages") {
                        return Some(*pages_ref);
                    }
                }
            }
        }
    }
    None
}

/// Añade un page_id como "kid" al Pages root y actualiza el contador
fn add_kid_to_pages(doc: &mut Document, pages_root_id: ObjectId, new_page_id: ObjectId) {
    if let Ok(pages_obj) = doc.get_object_mut(pages_root_id) {
        if let Object::Dictionary(ref mut dict) = *pages_obj {
            // Añadir a la lista de Kids
            if let Ok(Object::Array(ref mut kids)) = dict.get_mut(b"Kids") {
                kids.push(Object::Reference(new_page_id));
                // Actualizar Count si existe
                if let Ok(Object::Integer(ref mut count)) = dict.get_mut(b"Count") {
                    *count += 1;
                }
            }
        }
    }
}

/// Copia un objeto y recursivamente todos los que referencia
fn deep_copy(
    src: &Document,
    dst: &mut Document,
    obj_id: ObjectId,
    id_map: &mut BTreeMap<ObjectId, ObjectId>,
    depth: usize,
) -> Option<ObjectId> {
    // Protección contra referencias circulares
    if depth > MAX_DEPTH {
        return None;
    }
    // Si ya fue copiado, devolver el ID nuevo
    if let Some(&mapped) = id_map.get(&obj_id) {
        return Some(mapped);
    }

    // Cargar el objeto del documento fuente
    let src_obj = match src.get_object(obj_id) {
        Ok(obj) => obj.clone(),
        Err(_) => return None,
    };

    // Remapear referencias dentro del objeto
    let mut new_obj = src_obj;
    remap_references(&mut new_obj, src, dst, id_map, depth);

    // Insertar en el documento destino
    let new_id = dst.add_object(new_obj);
    id_map.insert(obj_id, new_id);
    Some(new_id)
}

/// Reemplaza recursivamente todas las referencias (ObjectId) en un objeto
/// por sus equivalentes en el documento destino
fn remap_references(
    obj: &mut Object,
    src: &Document,
    dst: &mut Document,
    id_map: &mut BTreeMap<ObjectId, ObjectId>,
    depth: usize,
) {
    match obj {
        Object::Reference(ref_id) => {
            let src_id = *ref_id;
            // Si ya está mapeado, usar el nuevo
            if let Some(&mapped) = id_map.get(&src_id) {
                *obj = Object::Reference(mapped);
            } else {
                // Copiar recursivamente y actualizar la referencia
                if let Some(new_id) = deep_copy(src, dst, src_id, id_map, depth + 1) {
                    *obj = Object::Reference(new_id);
                }
            }
        }
        Object::Array(arr) => {
            for item in arr.iter_mut() {
                remap_references(item, src, dst, id_map, depth);
            }
        }
        Object::Dictionary(dict) => {
            for (_key, value) in dict.iter_mut() {
                remap_references(value, src, dst, id_map, depth);
            }
        }
        _ => {} // Stream, String, Name, Integer, etc. — no tienen referencias
    }
}

/// Diagnostica si un PDF puede ser cargado por lopdf (para logging).
/// Retorna Ok(()) si es válido, Err(motivo) si no.
pub fn diagnose_pdf(path: &str) -> Result<(), String> {
    if !std::path::Path::new(path).exists() {
        return Err("no existe".to_string());
    }
    match Document::load(path) {
        Ok(doc) => {
            let pages = doc.get_pages().len();
            if pages == 0 {
                Err("0 páginas".to_string())
            } else {
                Ok(())
            }
        }
        Err(e) => Err(format!("{}", e)),
    }
}

/// Verifica si un archivo es un PDF válido (tiene al menos una página).
/// Menos estricta: solo falla si no existe o no tiene páginas.
pub fn is_valid_pdf(path: &str) -> bool {
    diagnose_pdf(path).is_ok()
}
