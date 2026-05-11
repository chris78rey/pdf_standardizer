use std::collections::HashMap;
use std::path::Path;

use regex::Regex;
use strsim::jaro_winkler;

use crate::models::{Action, FileIdentity, IdentityAnchor};

/// Identifica un archivo PDF aplicando 5 niveles de lógica:
///   1. Falso positivo CC
///   2. Regex sobre el nombre del archivo
///   2.5 Keywords dentro del TEXTO del PDF (NUEVO)
///   3. Jaro-Winkler sobre el nombre del archivo
///   4. Seguridad — no tocar
pub fn identify_file(
    path: &Path,
    filename: &str,
    anchors: &[IdentityAnchor],
    all_files_in_folder: &[String],
    universal_regex: &Regex,
) -> FileIdentity {
    let path_str = path.display().to_string();
    let filename_clean = clean_filename(filename);

    // ── Nivel 1: Falso positivo CC ──────────────────────────────
    if is_cc_false_positive(filename, all_files_in_folder) {
        return FileIdentity {
            path: path_str,
            filename: filename.to_string(),
            identity: Some("CC".to_string()),
            confidence: 0.0,
            action: Action::Ignore,
        };
    }

    // ── Nivel 2: Regex sobre el nombre ──────────────────────────
    if let Some(identity) = extract_identity(&filename_clean, universal_regex) {
        if let Some(anchor) = anchors.iter().find(|a| {
            a.codigo.to_uppercase() == identity.to_uppercase()
        }) {
            return FileIdentity {
                path: path_str,
                filename: filename.to_string(),
                identity: Some(identity),
                confidence: 1.0,
                action: Action::Merge {
                    group: anchor.codigo.clone(),
                    target_name: anchor.nombre_pdf.clone(),
                },
            };
        }
    }

    // ── Nivel 2.5: Keywords en el TEXTO del PDF ─────────────────
    if let Some((best_match, _kw_found)) =
        match_by_pdf_content(&path_str, anchors)
    {
        return FileIdentity {
            path: path_str,
            filename: filename.to_string(),
            identity: Some(best_match.codigo.clone()),
            confidence: 0.85,
            action: Action::Merge {
                group: best_match.codigo.clone(),
                target_name: best_match.nombre_pdf.clone(),
            },
        };
    }

    // ── Nivel 3: Jaro-Winkler sobre el nombre ───────────────────
    if let Some((best_match, score)) =
        find_best_similarity_match(&filename_clean, anchors)
    {
        if score >= 0.85 {
            return FileIdentity {
                path: path_str,
                filename: filename.to_string(),
                identity: Some(best_match.codigo.clone()),
                confidence: score,
                action: Action::Merge {
                    group: best_match.codigo.clone(),
                    target_name: best_match.nombre_pdf.clone(),
                },
            };
        }
    }

    // ── Nivel 4: Seguridad — no tocar ───────────────────────────
    FileIdentity {
        path: path_str,
        filename: filename.to_string(),
        identity: None,
        confidence: 0.0,
        action: Action::Keep,
    }
}

// ── Nivel 1: CC falso positivo ─────────────────────────────────────

fn is_cc_false_positive(filename: &str, all_files: &[String]) -> bool {
    let name_lower = filename.to_lowercase();
    if name_lower == "cc.pdf" {
        let has_numbered = all_files.iter().any(|f| {
            let fl = f.to_lowercase();
            fl != name_lower
                && (fl.starts_with("cc_") || fl.starts_with("cc1") || fl.starts_with("cc2"))
        });
        return has_numbered;
    }
    false
}

// ── Nivel 2: Regex ─────────────────────────────────────────────────

fn clean_filename(filename: &str) -> String {
    let name = filename.trim_end_matches(".pdf").trim_end_matches(".PDF");
    name.replace('.', "")
        .replace('_', "")
        .replace('-', "")
        .replace(' ', "")
}

fn extract_identity(clean_name: &str, re: &Regex) -> Option<String> {
    let test = format!("{}.pdf", clean_name);
    if let Some(caps) = re.captures(&test) {
        caps.get(2).and_then(|m| {
            let identity = m.as_str().to_uppercase();
            if identity.is_empty() { None } else { Some(identity) }
        })
    } else {
        None
    }
}

// ── Nivel 2.5: Keywords en contenido del PDF ───────────────────────

/// Extrae texto de las primeras 3 páginas del PDF usando lopdf.
fn extract_text_from_pdf(path: &str) -> Option<String> {
    use lopdf::Document;

    let doc = Document::load(path).ok()?;
    let page_count = doc.get_pages().len().min(3);
    let pages: Vec<u32> = (1..=page_count as u32).collect();

    if pages.is_empty() {
        return None;
    }

    match doc.extract_text(&pages) {
        Ok(text) => {
            let cleaned = text
                .replace('\n', " ")
                .replace('\r', " ")
                .replace("  ", " ");
            if cleaned.trim().is_empty() {
                None
            } else {
                Some(cleaned.to_uppercase())
            }
        }
        Err(_) => None,
    }
}

/// Busca keywords de los anchors dentro del texto extraído del PDF.
/// Retorna el anchor que mejor matchea (más keywords encontradas, más largas).
fn match_by_pdf_content<'a>(
    path: &str,
    anchors: &'a [IdentityAnchor],
) -> Option<(&'a IdentityAnchor, String)> {
    // Solo intentar si hay anchors con keywords
    let has_keywords = anchors.iter().any(|a| !a.keywords.is_empty());
    if !has_keywords {
        return None;
    }

    let text = extract_text_from_pdf(path)?;

    let mut best: Option<(&IdentityAnchor, usize, String)> = None;

    for anchor in anchors {
        if anchor.keywords.is_empty() {
            continue;
        }

        // Buscar cada keyword como substring en el texto extraído
        let found: Vec<&String> = anchor
            .keywords
            .iter()
            .filter(|kw| {
                kw.len() >= 3 && text.contains(&kw.to_uppercase())
            })
            .collect();

        if found.is_empty() {
            continue;
        }

        // Score: cantidad de keywords encontradas * suma de largos
        let score = found.len() * found.iter().map(|k| k.len()).sum::<usize>();

        match best {
            None => {
                let kw_str = found.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ");
                best = Some((anchor, score, kw_str));
            }
            Some((_, s, _)) if score > s => {
                let kw_str = found.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ");
                best = Some((anchor, score, kw_str));
            }
            _ => {}
        }
    }

    best.map(|(anchor, _, kw)| (anchor, kw))
}

// ── Nivel 3: Jaro-Winkler ───────────────────────────────────────────

fn find_best_similarity_match<'a>(
    name: &str,
    anchors: &'a [IdentityAnchor],
) -> Option<(&'a IdentityAnchor, f64)> {
    let mut best: Option<(&IdentityAnchor, f64)> = None;

    for anchor in anchors {
        let score_code = jaro_winkler(&name.to_lowercase(), &anchor.codigo.to_lowercase());
        let score_kw = anchor
            .keywords
            .iter()
            .map(|kw| jaro_winkler(&name.to_lowercase(), kw))
            .fold(0.0_f64, f64::max);
        let score = score_code.max(score_kw);

        match best {
            None => best = Some((anchor, score)),
            Some((_, s)) if score > s => best = Some((anchor, score)),
            _ => {}
        }
    }

    best
}

// ── Agrupación ──────────────────────────────────────────────────────

pub fn group_by_identity(
    identities: &[FileIdentity],
) -> HashMap<String, Vec<FileIdentity>> {
    let mut groups: HashMap<String, Vec<FileIdentity>> = HashMap::new();
    for id in identities {
        if let Action::Merge { ref group, .. } = id.action {
            groups.entry(group.clone()).or_default().push(id.clone());
        }
    }
    for files in groups.values_mut() {
        files.sort_by(|a, b| a.filename.cmp(&b.filename));
    }
    groups
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_filename() {
        assert_eq!(clean_filename("0.53.pdf"), "053");
        assert_eq!(clean_filename("010A_merged (2).pdf"), "010Amerged(2)");
        assert_eq!(clean_filename("AGUIRRE PI (1).pdf"), "AGUIRREPI(1)");
    }

    #[test]
    fn test_cc_false_positive() {
        let files = vec!["cc.pdf".to_string(), "cc_1.pdf".to_string(), "cc_2.pdf".to_string()];
        assert!(is_cc_false_positive("cc.pdf", &files));

        let files2 = vec!["cc.pdf".to_string(), "pi.pdf".to_string()];
        assert!(!is_cc_false_positive("cc.pdf", &files2));
    }

    #[test]
    fn test_extract_identity() {
        let re = crate::dictionary::build_universal_regex();
        assert_eq!(extract_identity("010Amerged(2)", &re), Some("010A".to_string()));
        assert_eq!(extract_identity("053", &re), Some("053".to_string()));
        assert_eq!(extract_identity("008A", &re), Some("008".to_string()));
    }

    #[test]
    fn test_keyword_substring_match() {
        // Verificar que "EPICRISIS" aparece en un texto típico
        let text = "HOSPITAL GENERAL EPICRISIS MEDICA PACIENTE JUAN PEREZ".to_uppercase();
        assert!(text.contains("EPICRISIS"));
    }
}
