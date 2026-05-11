use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use chrono::Local;
use rayon::prelude::*;
use walkdir::WalkDir;

use crate::dictionary::{build_identity_anchors, build_universal_regex_from_entries};
use crate::matcher::identify_file;
use crate::merger::{diagnose_pdf, merge_pdfs};
use crate::models::{
    Action, FileEntryResult, FileIdentity, FolderResult, IdentityAnchor, MasterEntry,
    ProcessReport, RunMode,
};

/// Ejecuta el pipeline completo: dry-run o ejecución real
pub fn run_pipeline(
    repo_path: &str,
    mode: RunMode,
    progress_callback: impl Fn(u64, u64, &str) + Send + Sync,
    cancel_flag: Option<Arc<AtomicBool>>,
    entries: &[MasterEntry],
) -> Result<ProcessReport, String> {
    // 0. Validar que exista la carpeta
    let repo_path_buf = Path::new(repo_path);
    if !repo_path_buf.exists() || !repo_path_buf.is_dir() {
        return Err(format!("Repositorio no existe o no es carpeta: {}", repo_path));
    }

    // 1. Cargar diccionario maestro (desde las entradas provistas por la GUI)
    progress_callback(0, 0, "Preparando diccionario...");
    let anchors = build_identity_anchors(entries);
    let universal_regex = build_universal_regex_from_entries(entries);

    progress_callback(0, 0, &format!(
        "Diccionario cargado: {} identidades",
        anchors.len()
    ));

    // 2. Recorrer todas las carpetas con PDFs
    let folders = discover_folders_with_pdfs(repo_path);
    let total = folders.len() as u64;

    // Verificar cancelación antes de procesar
    if let Some(ref cancel) = cancel_flag {
        if cancel.load(Ordering::Relaxed) {
            return Err("Procesamiento cancelado por el usuario".to_string());
        }
    }

    progress_callback(0, total, &format!(
        "Encontradas {} carpetas con PDFs",
        total
    ));

    // 3. Procesar cada carpeta (en paralelo con rayon, contador atómico)
    let counter = Arc::new(AtomicU64::new(0));

    let results: Vec<FolderResult> = folders
        .par_iter()
        .map(|folder| {
            let current = counter.fetch_add(1, Ordering::Relaxed) + 1;
            progress_callback(current, total, &format!(
                "Procesando carpeta {} de {}",
                current, total
            ));

            process_folder(folder, &anchors, &universal_regex, mode)
        })
        .collect();

    // 4. Generar reporte
    let mut report = ProcessReport {
        timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        total_folders: total,
        folders_modified: 0,
        files_merged: 0,
        files_kept: 0,
        files_ignored: 0,
        errors: Vec::new(),
        folder_results: results.clone(),
    };

    for fr in &results {
        if !fr.merged_pdf.is_none() || fr.files_processed.iter().any(|f| f.action == "MERGE") {
            report.folders_modified += 1;
        }
        for f in &fr.files_processed {
            match f.action.as_str() {
                "MERGE" => report.files_merged += 1,
                "KEEP" => report.files_kept += 1,
                "IGNORE" => report.files_ignored += 1,
                _ => {}
            }
        }
        report.errors.extend(fr.errors.clone());
    }

    progress_callback(total, total, "Procesamiento completado");
    Ok(report)
}

/// Descubre todas las carpetas que contienen archivos PDF
fn discover_folders_with_pdfs(root: &str) -> Vec<PathBuf> {
    let mut folders: HashMap<PathBuf, bool> = HashMap::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy().to_lowercase();
            if name.ends_with(".pdf") {
                if let Some(parent) = entry.path().parent() {
                    folders.entry(parent.to_path_buf()).or_insert(true);
                }
            }
        }
    }

    let mut result: Vec<PathBuf> = folders.into_keys().collect();
    result.sort();
    result
}

/// Procesa una carpeta individual
fn process_folder(
    folder: &Path,
    anchors: &[IdentityAnchor],
    universal_regex: &regex::Regex,
    mode: RunMode,
) -> FolderResult {
    let folder_str = folder.display().to_string();
    let mut errors = Vec::new();

    // Leer todos los archivos PDF en la carpeta
    let pdf_files: Vec<String> = match fs::read_dir(folder) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .ends_with(".pdf")
            })
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect(),
        Err(e) => {
            errors.push(format!(
                "[{}] Error leyendo {}: {}",
                Local::now().format("%H:%M:%S"),
                folder_str,
                e
            ));
            return FolderResult {
                folder: folder_str,
                files_processed: vec![],
                merged_pdf: None,
                errors,
            };
        }
    };

    if pdf_files.is_empty() {
        return FolderResult {
            folder: folder_str,
            files_processed: vec![],
            merged_pdf: None,
            errors,
        };
    }

    // Identificar cada archivo
    let identities: Vec<FileIdentity> = pdf_files
        .iter()
        .map(|fname| {
            let full_path = folder.join(fname);
            identify_file(&full_path, fname, anchors, &pdf_files, universal_regex)
        })
        .collect();

    // Separar por acción
    let mut file_results = Vec::new();
    let mut merge_groups = Vec::new();
    let mut keep_files = Vec::new();
    let mut ignore_files = Vec::new();

    for id in &identities {
        match id.action {
            Action::Merge { ref group, ref target_name } => {
                merge_groups.push((id.path.clone(), group.clone(), target_name.clone()));
                file_results.push(FileEntryResult {
                    original: id.filename.clone(),
                    action: "MERGE".to_string(),
                    target: target_name.clone(),
                });
            }
            Action::Keep => {
                keep_files.push(id.filename.clone());
                file_results.push(FileEntryResult {
                    original: id.filename.clone(),
                    action: "KEEP".to_string(),
                    target: "".to_string(),
                });
            }
            Action::Ignore => {
                ignore_files.push(id.filename.clone());
                file_results.push(FileEntryResult {
                    original: id.filename.clone(),
                    action: "IGNORE".to_string(),
                    target: "".to_string(),
                });
            }
        }
    }

    // Ejecutar merges si no es dry-run
    let merged_name = if mode == RunMode::Execute && !merge_groups.is_empty() {
        // Agrupar por grupo
        let grouped: HashMap<String, Vec<String>> = {
            let mut map: HashMap<String, Vec<String>> = HashMap::new();
            for (path, group, _target) in &merge_groups {
                map.entry(group.clone()).or_default().push(path.clone());
            }
            // Ordenar cada grupo
            for files in map.values_mut() {
                files.sort();
            }
            map
        };

        for (group, files) in &grouped {
            let target = merge_groups
                .iter()
                .find(|(_, g, _)| g == group)
                .map(|(_, _, t)| t.clone())
                .unwrap_or(format!("{}.pdf", group));
            let output = folder.join(&target).display().to_string();

            // Diagnosticar cada PDF antes del merge
            for fpath in files {
                if let Err(detail) = diagnose_pdf(fpath) {
                    errors.push(format!(
                        "[{}] ⚠️ Diagnóstico {}: {}",
                        Local::now().format("%H:%M:%S"),
                        fpath,
                        detail
                    ));
                }
            }

            match merge_pdfs(files, &output) {
                Ok(_) => {
                    // Eliminar archivos originales después de merge exitoso
                    for original_path in files {
                        if *original_path != output {
                            if let Err(e) = fs::remove_file(original_path) {
                                errors.push(format!(
                                    "[{}] Advertencia: No se pudo eliminar {}: {}",
                                    Local::now().format("%H:%M:%S"),
                                    original_path,
                                    e
                                ));
                            }
                        }
                    }
                }
                Err(e) => {
                    errors.push(format!(
                        "[{}] Error merge {}: {}",
                        Local::now().format("%H:%M:%S"),
                        group,
                        e
                    ));
                    // No eliminar si el merge falla
                }
            }
        }
        Some(format!("{} grupos mergeados", grouped.len()))
    } else if !merge_groups.is_empty() {
        Some(format!(
            "{} grupos pendientes (dry-run)",
            merge_groups
                .iter()
                .map(|(_, g, _)| g)
                .collect::<std::collections::HashSet<_>>()
                .len()
        ))
    } else {
        None
    };

    FolderResult {
        folder: folder_str,
        files_processed: file_results,
        merged_pdf: merged_name,
        errors,
    }
}
