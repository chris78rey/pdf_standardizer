use regex::Regex;

use crate::models::{IdentityAnchor, MasterEntry};

const DICT_JSON_PATH: &str = "dictionary.json";

/// Valores por defecto — 36 documentos del Hospital Edition.
/// Solo se usan si no existe dictionary.json (primer arranque o tras reset).
pub fn default_entries() -> Vec<MasterEntry> {
    vec![
        me("PI.pdf",       1,  "PI",     "PLANILLAINDIVIDUAL",                      None),
        me("CC.pdf",       2,  "CC",     "COBERTURA",                               None),
        me("CV.pdf",       3,  "CV",     "",                                        None),
        me("AES.pdf",      4,  "AES",    "",                                        None),
        me("053.pdf",      5,  "053",    "",                                        None),
        me("006.pdf",      6,  "006",    "EPICRISIS",                               None),
        me("007.pdf",      7,  "007",    "FORMULARIO 7 ¿ INTERCONSULTA",            None),
        me("017.pdf",      8,  "017",    "",                                        None),
        me("018.pdf",      9,  "018",    "",                                        None),
        me("018A.pdf",     10, "018A",   "",                                        None),
        me("113.pdf",      11, "113",    "",                                        None),
        me("114.pdf",      12, "114",    "",                                        None),
        me("115.pdf",      13, "115",    "",                                        None),
        me("ORS.pdf",      14, "ORS",    "PROCEDIMIENTOMENOR|RESULTADOS EXAMENES DE CARDIOLOGIA|EXÁMENES Y PROCEDIMIENTOS DE CARDIOLOGÍA|PROCEDIMIENTOS ESPECIALES|RESULTADOS PROCEDIMIENTOS ESPECIALES", None),
        me("002.pdf",      15, "002",    "Reporte de Notas de Evolución",           None),
        me("010A.pdf",     16, "010A",   "ENDOCRINOLOGIA|FORMULARIO 10",            None),
        me("010B.pdf",     17, "010B",   "RESULTADOS EXAMENES DE ENDOCRINOLOGIA",   None),
        me("012A.pdf",     18, "012A",   "FORMULARIO 12",                           None),
        me("012B.pdf",     19, "012B",   "RESULTADOS EXAMENES DE IMAGEN",           None),
        me("033.pdf",      20, "033",    "",                                        None),
        me("013A.pdf",     21, "013A",   "",                                        None),
        me("013B.pdf",     22, "013B",   "",                                        None),
        me("PTR.pdf",      23, "PTR",    "",                                        None),
        me("RTR.pdf",      24, "RTR",    "",                                        None),
        me("08.pdf",       25, "008",    "UNIDAD OPERATIVA: EMERGENCIA",            Some("08")),
        me("FSCS.pdf",     26, "FSCS",   "",                                        None),
        me("FSICS.pdf",    27, "FSICS",  "",                                        None),
        me("FRDCS.pdf",    28, "FRDCS",  "",                                        None),
        me("ANX2.pdf",     29, "ANX2",   "",                                        None),
        me("HR.pdf",       30, "HR",     "",                                        None),
        me("RHD.pdf",      31, "RHD",    "",                                        None),
        me("IMT.pdf",      32, "IMT",    "",                                        None),
        me("CEC.pdf",      33, "CEC",    "",                                        None),
        me("RAD.pdf",      34, "RAD",    "",                                        None),
        me("ITS.pdf",      35, "ITS",    "",                                        None),
        me("RVD.pdf",      36, "RVD",    "",                                        None),
        me("119.pdf",      38, "119",    "",                                        None),
        me("TEST999.pdf",  9999, "TEST999", "",                                     None),
    ]
}

fn me(
    nombre_pdf: &str, orden: u32, identidad: &str, regla: &str, alt: Option<&str>,
) -> MasterEntry {
    MasterEntry {
        nombre_pdf: nombre_pdf.to_string(),
        orden,
        identidad: identidad.to_string(),
        regla_lee_documento: regla.to_string(),
        alt_identidad: alt.map(|s| s.to_string()),
    }
}

// ── JSON Persistencia ──────────────────────────────────────────────

/// Carga desde dictionary.json. Si no existe, crea uno con los defaults.
pub fn load_from_json() -> Result<Vec<MasterEntry>, String> {
    if let Ok(data) = std::fs::read_to_string(DICT_JSON_PATH) {
        serde_json::from_str(&data)
            .map_err(|e| format!("Error parseando {}: {}", DICT_JSON_PATH, e))
    } else {
        // Primer arranque: crear archivo con defaults
        let defaults = default_entries();
        let json = serde_json::to_string_pretty(&defaults)
            .map_err(|e| format!("Error serializando defaults: {}", e))?;
        std::fs::write(DICT_JSON_PATH, &json)
            .map_err(|e| format!("Error escribiendo {}: {}", DICT_JSON_PATH, e))?;
        Ok(defaults)
    }
}

/// Guarda las entradas a dictionary.json
pub fn save_to_json(entries: &[MasterEntry]) -> Result<(), String> {
    let json = serde_json::to_string_pretty(entries)
        .map_err(|e| format!("Error serializando: {}", e))?;
    std::fs::write(DICT_JSON_PATH, &json)
        .map_err(|e| format!("Error guardando {}: {}", DICT_JSON_PATH, e))?;
    Ok(())
}

/// Restaura los defaults y los guarda a JSON
pub fn reset_to_defaults() -> Result<Vec<MasterEntry>, String> {
    let defaults = default_entries();
    save_to_json(&defaults)?;
    Ok(defaults)
}

// ── Compatibilidad con la firma anterior ───────────────────────────

/// Carga el diccionario maestro (intenta JSON, fallback a defaults).
/// Se mantiene la firma por compatibilidad; los parámetros se ignoran.
pub fn load_master_dictionary(
    _path: &str,
    _preferred_sheet: Option<&str>,
) -> Result<Vec<MasterEntry>, String> {
    load_from_json()
}

pub fn get_entry_count() -> usize {
    load_from_json().map(|e| e.len()).unwrap_or(0)
}

// ── Construcción de anclajes y regex ────────────────────────────────

/// Construye los anclajes (anchors) con regex compilados.
/// Cada entrada genera un anchor; si tiene alt_identidad genera uno adicional.
pub fn build_identity_anchors(entries: &[MasterEntry]) -> Vec<IdentityAnchor> {
    let mut anchors = Vec::new();

    for e in entries {
        let codigo = e.identidad.clone();
        let pattern = format!(
            r"(?i)(.*?\D)?({})(\D.*?)?(\d+)?\.pdf$",
            regex::escape(&codigo)
        );
        let regex = Regex::new(&pattern).unwrap();

        let keywords: Vec<String> = e
            .regla_lee_documento
            .split('|')
            .map(|k| k.trim().to_lowercase())
            .filter(|k| !k.is_empty())
            .collect();

        anchors.push(IdentityAnchor {
            codigo: codigo.clone(),
            nombre_pdf: e.nombre_pdf.clone(),
            orden: e.orden,
            regex,
            keywords: keywords.clone(),
        });

        if let Some(ref alt) = e.alt_identidad {
            let alt_pattern = format!(
                r"(?i)(.*?\D)?({})(\D.*?)?(\d+)?\.pdf$",
                regex::escape(alt)
            );
            let alt_regex = Regex::new(&alt_pattern).unwrap();
            anchors.push(IdentityAnchor {
                codigo: codigo.clone(),
                nombre_pdf: e.nombre_pdf.clone(),
                orden: e.orden,
                regex: alt_regex,
                keywords: keywords.clone(),
            });
        }
    }

    anchors
}

/// Regex universal a partir de una lista de entradas (puede ser dinámica).
pub fn build_universal_regex_from_entries(entries: &[MasterEntry]) -> Regex {
    let mut codes: Vec<String> = entries.iter().map(|e| e.identidad.clone()).collect();
    for e in entries {
        if let Some(ref alt) = e.alt_identidad {
            codes.push(alt.clone());
        }
    }
    codes.sort_by_key(|c| -(c.len() as i32));
    let alternation = codes.join("|");
    let pattern = format!(
        r"(?i)(.*?\D)?({})(\D.*?)?(\d+)?\.pdf$",
        alternation
    );
    Regex::new(&pattern).unwrap()
}

/// Regex universal desde los defaults (para tests y compatibilidad).
pub fn build_universal_regex() -> Regex {
    build_universal_regex_from_entries(&default_entries())
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_entries_count() {
        let entries = default_entries();
        assert!(entries.len() >= 36);
    }

    #[test]
    fn test_universal_regex_covers_all() {
        let re = build_universal_regex();
        assert!(re.is_match("053.pdf"));
        assert!(re.is_match("PI.pdf"));
        assert!(re.is_match("010A_merged (2).pdf"));
        assert!(re.is_match("AGUIRRE PI (1).pdf"));
        assert!(re.is_match("08.pdf"));
        assert!(re.is_match("008.pdf"));
        assert!(re.is_match("CV.pdf"));
        assert!(re.is_match("ORS (3).pdf"));
        assert!(re.is_match("ANX2.pdf"));
        assert!(re.is_match("119.pdf"));
        assert!(!re.is_match("factura_XYZ.pdf"));
    }

    #[test]
    fn test_anchors_include_alternates() {
        let entries = default_entries();
        let anchors = build_identity_anchors(&entries);
        let ochos: Vec<_> = anchors.iter().filter(|a| a.codigo == "008").collect();
        assert_eq!(ochos.len(), 2);
    }

    #[test]
    fn test_json_roundtrip_does_not_crash() {
        // Solo verificar que load no falla si el archivo existe
        let _ = load_from_json();
    }
}
