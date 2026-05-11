mod dictionary;
mod matcher;
mod merger;
mod models;
mod processor;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui;
use rfd::FileDialog;

use models::{MasterEntry, ProcessReport, RunMode};

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1300.0, 850.0])
            .with_title("PDF Standardizer — HE-1 | Estandarización de Repositorio Digital"),
        ..Default::default()
    };

    eframe::run_native(
        "pdf-standardizer",
        options,
        Box::new(|_cc| {
            let mut app = App::default();
            app.init_dictionary();
            Ok(Box::new(app))
        }),
    )
}

#[derive(Default)]
struct App {
    // Rutas
    repo_path: String,

    // Diccionario editable (persiste en dictionary.json)
    dict_entries: Vec<MasterEntry>,
    dict_status: String,

    // Procesamiento en background
    running: bool,
    should_cancel: Option<Arc<AtomicBool>>,
    progress_cur: u64,
    progress_total: u64,
    progress_msg: String,
    run_mode: RunModeChoice,

    // Comunicación con thread
    progress_arc: Option<Arc<Mutex<(u64, u64, String)>>>,
    log_arc: Option<Arc<Mutex<Vec<String>>>>,
    report_arc: Option<Arc<Mutex<Option<Result<ProcessReport, String>>>>>,

    // Resultados
    report: Option<ProcessReport>,
    log_lines: Vec<String>,
    result_filter: String,
    report_error: Option<String>,

    // Tabs
    active_tab: Tab,
}

#[derive(Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Config,
    Dictionary,
    Results,
}

#[derive(Default, PartialEq, Eq, Clone, Copy)]
enum RunModeChoice {
    #[default]
    DryRun,
    Execute,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_progress();

        // Panel superior
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("🏥 PDF Standardizer HE-1");
                ui.separator();
                ui.label("Estandarización de Repositorio Digital — Hospital Edition");
            });
        });

        // Tabs
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.active_tab, Tab::Config, "⚙️ Config & Ejecución");
                ui.selectable_value(&mut self.active_tab, Tab::Dictionary, "📝 Diccionario");
                ui.selectable_value(&mut self.active_tab, Tab::Results, "📊 Resultados");
            });
        });

        // Panel central según tab
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                match self.active_tab {
                    Tab::Config => {
                        self.show_config_section(ui);
                        ui.separator();
                        self.show_execution_section(ui);
                    }
                    Tab::Dictionary => {
                        self.show_dictionary_section(ui);
                    }
                    Tab::Results => {
                        self.show_results_section(ui);
                    }
                }
            });
        });

        if self.running {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
    }
}

impl App {
    fn init_dictionary(&mut self) {
        match dictionary::load_from_json() {
            Ok(entries) => {
                let count = entries.len();
                self.dict_entries = entries;
                self.dict_status = format!("✅ {} documentos cargados desde dictionary.json", count);
            }
            Err(e) => {
                self.dict_status = format!("❌ Error: {}", e);
            }
        }
    }

    fn poll_progress(&mut self) {
        if let Some(ref arc) = self.progress_arc {
            if let Ok(p) = arc.lock() {
                self.progress_cur = p.0;
                self.progress_total = p.1;
                self.progress_msg = p.2.clone();
            }
        }
        if let Some(ref arc) = self.log_arc {
            if let Ok(log) = arc.lock() {
                self.log_lines = log.clone();
            }
        }
        let report_taken = if let Some(ref arc) = self.report_arc {
            arc.lock().ok().and_then(|mut g| g.take())
        } else {
            None
        };
        if let Some(result) = report_taken {
            match result {
                Ok(report) => {
                    self.report = Some(report);
                    self.report_error = None;
                }
                Err(e) => {
                    self.report_error = Some(e);
                }
            }
            self.running = false;
            self.progress_arc = None;
            self.log_arc = None;
            self.report_arc = None;
        }
    }

    // ── Config & Ejecución ──────────────────────────────────────

    fn show_config_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("⚙️ Configuración")
            .default_open(true)
            .show(ui, |ui| {
                ui.group(|ui| {
                    ui.label("Repositorio a procesar:");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.repo_path)
                                .hint_text("Ruta a la carpeta raíz...")
                                .desired_width(400.0),
                        );
                        if ui.button("📁 Examinar...").clicked() {
                            if let Some(path) = FileDialog::new().pick_folder() {
                                self.repo_path = path.display().to_string();
                            }
                        }
                    });
                    if !self.repo_path.is_empty()
                        && std::path::Path::new(&self.repo_path).exists()
                    {
                        ui.label(
                            egui::RichText::new(format!("📁 {}", self.repo_path))
                                .color(egui::Color32::LIGHT_BLUE),
                        );
                    }
                });
            });
    }

    fn show_execution_section(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("🚀 Ejecución")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Modo:");
                    ui.radio_value(&mut self.run_mode, RunModeChoice::DryRun, "🔍 Dry-Run (simular)");
                    ui.radio_value(&mut self.run_mode, RunModeChoice::Execute, "⚡ Ejecutar (real)");
                });

                ui.add_space(8.0);

                let can_run = !self.repo_path.is_empty() && !self.running && !self.dict_entries.is_empty();

                ui.horizontal(|ui| {
                    let btn_text = match self.run_mode {
                        RunModeChoice::DryRun => "🔍 Ejecutar Dry-Run",
                        RunModeChoice::Execute => "⚡ Ejecutar Cambios Reales",
                    };

                    if ui
                        .add_enabled(
                            can_run,
                            egui::Button::new(egui::RichText::new(btn_text).size(16.0))
                                .min_size(egui::vec2(250.0, 40.0)),
                        )
                        .clicked()
                    {
                        self.launch_processing();
                    }

                    if self.running {
                        ui.spinner();
                        if ui.button("🛑 Cancelar").clicked() {
                            if let Some(ref cancel) = self.should_cancel {
                                cancel.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                });

                if self.running || self.progress_total > 0 {
                    let fraction = if self.progress_total > 0 {
                        self.progress_cur as f32 / self.progress_total as f32
                    } else {
                        0.0
                    };
                    ui.add(
                        egui::ProgressBar::new(fraction).text(format!(
                            "{} / {} — {}",
                            self.progress_cur, self.progress_total, self.progress_msg
                        )),
                    );
                }

                if let Some(ref err) = self.report_error {
                    ui.label(egui::RichText::new(format!("❌ Error: {}", err)).color(egui::Color32::RED));
                }

                if !self.log_lines.is_empty() {
                    ui.add_space(4.0);
                    ui.label("📋 Log:");
                    egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
                        for line in &self.log_lines {
                            ui.monospace(line);
                        }
                    });
                }
            });
    }

    // ── Diccionario Editable ─────────────────────────────────────

    fn show_dictionary_section(&mut self, ui: &mut egui::Ui) {
        ui.heading("📝 Diccionario Maestro (dictionary.json)");
        ui.label(&self.dict_status);
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            if ui.button("➕ Añadir entrada").clicked() {
                let next_orden = self.dict_entries.iter().map(|e| e.orden).max().unwrap_or(0) + 1;
                self.dict_entries.push(MasterEntry {
                    nombre_pdf: "NUEVO.pdf".into(),
                    orden: next_orden,
                    identidad: "NUEVO".into(),
                    regla_lee_documento: String::new(),
                    alt_identidad: None,
                });
            }
            if ui.button("🔄 Restablecer defaults").clicked() {
                match dictionary::reset_to_defaults() {
                    Ok(entries) => {
                        self.dict_entries = entries;
                        self.dict_status = format!("✅ Restablecido a {} documentos default", self.dict_entries.len());
                    }
                    Err(e) => {
                        self.dict_status = format!("❌ Error al restablecer: {}", e);
                    }
                }
            }
            if ui.button("💾 Guardar a dictionary.json").clicked() {
                match dictionary::save_to_json(&self.dict_entries) {
                    Ok(()) => {
                        self.dict_status = format!("✅ {} documentos guardados", self.dict_entries.len());
                    }
                    Err(e) => {
                        self.dict_status = format!("❌ {}", e);
                    }
                }
            }
        });

        ui.add_space(8.0);
        ui.label(format!("{} entradas", self.dict_entries.len()));

        // Tabla editable
        egui::ScrollArea::vertical()
            .max_height(500.0)
            .show(ui, |ui| {
                let mut to_remove: Option<usize> = None;

                egui::Grid::new("dict_grid")
                    .striped(true)
                    .min_col_width(40.0)
                    .show(ui, |ui| {
                        ui.strong("ID");
                        ui.strong("Nombre PDF");
                        ui.strong("Alt ID");
                        ui.strong("Keywords (|)");
                        ui.strong("Orden");
                        ui.strong("");
                        ui.end_row();

                        for (idx, entry) in self.dict_entries.iter_mut().enumerate() {
                            ui.add(
                                egui::TextEdit::singleline(&mut entry.identidad)
                                    .desired_width(70.0),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut entry.nombre_pdf)
                                    .desired_width(100.0),
                            );

                            let mut alt = entry.alt_identidad.clone().unwrap_or_default();
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut alt)
                                        .hint_text("(vacío)")
                                        .desired_width(60.0),
                                )
                                .changed()
                            {
                                entry.alt_identidad = if alt.is_empty() { None } else { Some(alt) };
                            }

                            ui.add(
                                egui::TextEdit::singleline(&mut entry.regla_lee_documento)
                                    .desired_width(200.0),
                            );

                            let mut ord = entry.orden.to_string();
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut ord)
                                        .desired_width(50.0),
                                )
                                .changed()
                            {
                                entry.orden = ord.parse().unwrap_or(entry.orden);
                            }

                            if ui.button("🗑").clicked() {
                                to_remove = Some(idx);
                            }
                            ui.end_row();
                        }
                    });

                if let Some(idx) = to_remove {
                    self.dict_entries.remove(idx);
                }
            });

        ui.add_space(8.0);
        ui.label("💡 Tip: Tras editar, pulsá 'Guardar a dictionary.json'. Los cambios aplican al próximo procesamiento.");
    }

    // ── Resultados ───────────────────────────────────────────────

    fn show_results_section(&mut self, ui: &mut egui::Ui) {
        if let Some(ref report) = self.report {
            ui.group(|ui| {
                ui.label(egui::RichText::new("📈 Resumen").strong().size(14.0));
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "Carpetas: {} | Modificadas: {}",
                        report.total_folders, report.folders_modified
                    ));
                });
                ui.label(format!(
                    "Mergeados: {} | Mantenidos: {} | Ignorados: {} | Errores: {}",
                    report.files_merged,
                    report.files_kept,
                    report.files_ignored,
                    report.errors.len(),
                ));
                ui.label(format!("Timestamp: {}", report.timestamp));
            });

            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label("🔍 Filtrar carpeta:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.result_filter)
                        .hint_text("nombre de carpeta...")
                        .desired_width(250.0),
                );
                let _ = ui.small_button("🗑️").clicked().then(|| self.result_filter.clear());
            });

            let filtered: Vec<&models::FolderResult> = if self.result_filter.is_empty() {
                report.folder_results.iter().collect()
            } else {
                let f = self.result_filter.to_lowercase();
                report
                    .folder_results
                    .iter()
                    .filter(|fr| fr.folder.to_lowercase().contains(&f))
                    .collect()
            };

            ui.label(format!(
                "Mostrando {} de {} carpetas",
                filtered.len(),
                report.folder_results.len()
            ));

            egui::ScrollArea::vertical().max_height(400.0).show(ui, |ui| {
                egui::Grid::new("results_grid")
                    .striped(true)
                    .min_col_width(40.0)
                    .show(ui, |ui| {
                        ui.strong("Carpeta");
                        ui.strong("Archivo Original");
                        ui.strong("Acción");
                        ui.strong("Destino");
                        ui.end_row();

                        for fr in &filtered {
                            if fr.files_processed.is_empty() {
                                ui.label(&fr.folder);
                                ui.label("(sin PDFs)");
                                ui.label("-");
                                ui.label("-");
                                ui.end_row();
                            }
                            for f in &fr.files_processed {
                                ui.label(&fr.folder);
                                ui.label(&f.original);
                                let (color, label) = match f.action.as_str() {
                                    "MERGE" => (egui::Color32::GREEN, "🔀 MERGE"),
                                    "KEEP" => (egui::Color32::YELLOW, "🔒 KEEP"),
                                    "IGNORE" => (egui::Color32::RED, "🚫 IGNORE"),
                                    _ => (egui::Color32::GRAY, f.action.as_str()),
                                };
                                ui.colored_label(color, label);
                                ui.label(&f.target);
                                ui.end_row();
                            }
                        }
                    });
            });

            if !report.errors.is_empty() {
                ui.add_space(8.0);
                egui::CollapsingHeader::new(format!("⚠️ Errores ({})", report.errors.len()))
                    .show(ui, |ui| {
                        for err in &report.errors {
                            ui.label(egui::RichText::new(err).color(egui::Color32::RED));
                        }
                    });
            }
        } else {
            ui.label("Ejecutá un Dry-Run para ver resultados aquí.");
        }
    }

    // ── Procesamiento ────────────────────────────────────────────

    fn launch_processing(&mut self) {
        let repo_path = self.repo_path.clone();
        let mode = match self.run_mode {
            RunModeChoice::DryRun => RunMode::DryRun,
            RunModeChoice::Execute => RunMode::Execute,
        };
        let entries = self.dict_entries.clone();

        self.running = true;
        self.progress_cur = 0;
        self.progress_total = 0;
        self.progress_msg.clear();
        self.report = None;
        self.report_error = None;

        let cancel_flag = Arc::new(AtomicBool::new(false));
        self.should_cancel = Some(cancel_flag.clone());

        let progress = Arc::new(Mutex::new((0u64, 0u64, String::new())));
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let report_result = Arc::new(Mutex::new(None::<Result<ProcessReport, String>>));

        self.progress_arc = Some(progress.clone());
        self.log_arc = Some(log.clone());
        self.report_arc = Some(report_result.clone());

        std::thread::spawn(move || {
            let result = processor::run_pipeline(
                &repo_path,
                mode,
                move |cur, total, msg| {
                    if let Ok(mut p) = progress.lock() {
                        *p = (cur, total, msg.to_string());
                    }
                    if let Ok(mut l) = log.lock() {
                        l.push(format!("[{}/{}] {}", cur, total, msg));
                    }
                },
                Some(cancel_flag),
                &entries,
            );

            if let Ok(mut r) = report_result.lock() {
                *r = Some(result);
            }
        });

        // Cambiar a tab de resultados para ver progreso
        self.active_tab = Tab::Results;
    }
}
