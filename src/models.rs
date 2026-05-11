use serde::{Deserialize, Serialize};

/// Entrada del diccionario maestro cargado desde NOMBRES_VALIDOS.xlsx
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterEntry {
    /// Nombre final del PDF (ej: "053.pdf")
    pub nombre_pdf: String,
    /// Prioridad (número de orden)
    pub orden: u32,
    /// Palabras clave para identificar por contenido (separadas por |)
    pub regla_lee_documento: String,
    /// Identidad extraída (ej: "053", "PI", "010A")
    pub identidad: String,
    /// Código alternativo de REGLA_SIMILARIDAD (ej: "08" → "008")
    pub alt_identidad: Option<String>,
}

/// Una identidad reconocida con su regex
#[derive(Debug, Clone)]
pub struct IdentityAnchor {
    pub codigo: String,
    pub nombre_pdf: String,
    pub orden: u32,
    pub regex: regex::Regex,
    pub keywords: Vec<String>,
}

/// Resultado del reconocimiento de un archivo
#[derive(Debug, Clone)]
pub struct FileIdentity {
    pub path: String,
    pub filename: String,
    pub identity: Option<String>,
    pub confidence: f64,
    pub action: Action,
}

/// Acción a tomar sobre un archivo
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Action {
    /// Unir con otros del mismo grupo, renombrar según diccionario
    Merge { group: String, target_name: String },
    /// No tocar (regla de seguridad)
    Keep,
    /// Ignorar (falso positivo CC)
    Ignore,
}

/// Resultado por carpeta
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FolderResult {
    pub folder: String,
    pub files_processed: Vec<FileEntryResult>,
    pub merged_pdf: Option<String>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntryResult {
    pub original: String,
    pub action: String,
    pub target: String,
}

/// Estado global de procesamiento
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessReport {
    pub timestamp: String,
    pub total_folders: u64,
    pub folders_modified: u64,
    pub files_merged: u64,
    pub files_kept: u64,
    pub files_ignored: u64,
    pub errors: Vec<String>,
    pub folder_results: Vec<FolderResult>,
}

/// Modo de operación
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RunMode {
    DryRun,
    Execute,
}
