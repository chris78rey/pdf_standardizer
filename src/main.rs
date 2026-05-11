mod dictionary;
mod matcher;
mod merger;
mod models;
mod processor;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, Frame, Margin, Stroke, Vec2};
use rfd::FileDialog;

use models::{MasterEntry, ProcessReport, RunMode};

// ── Constantes de estilo accesible ──────────────────────────────────

const BORDER_COLOR: Color32 = Color32::from_rgb(80, 80, 100);
const BORDER_WIDTH: f32 = 1.5;
const HEADER_BG: Color32 = Color32::from_rgb(50, 55, 70);

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

// ── Helpers de UI accesible ─────────────────────────────────────────

/// Marco con borde visible grueso
fn bordered_frame(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui)) {
    Frame::default()
        .stroke(Stroke::new(BORDER_WIDTH, BORDER_COLOR))
        .inner_margin(Margin::symmetric(12, 8))
        .outer_margin(Margin::symmetric(0, 4))
        .show(ui, |ui| {
            add_contents(ui);
        });
}

/// Línea separadora gruesa y visible
fn thick_separator(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    let rect = ui.available_rect_before_wrap();
    ui.painter().hline(
        rect.x_range(),
        ui.cursor().top(),
        Stroke::new(1.5, BORDER_COLOR),
    );
    ui.add_space(4.0);
}

/// Cabecera de sección con fondo oscuro
fn section_header(ui: &mut egui::Ui, text: &str) {
    let desired_width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(desired_width, 26.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 3.0, HEADER_BG);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(15.0),
        Color32::WHITE,
    );
    ui.add_space(4.0);
}

// ── App ─────────────────────────────────────────────────────────────

#[derive(Default)]
struct App {
    repo_path: String,
    dict_entries: Vec<MasterEntry>,
    dict_status: String,

    running: bool,
    should_cancel: Option<Arc<AtomicBool>>,
    progress_cur: u64,
    progress_total: u64,
    progress_msg: String,
    run_mode: RunModeChoice,

    progress_arc: Option<Arc<Mutex<(u64, u64, String)>>>,
    log_arc: Option<Arc<Mutex<Vec<String>>>>,
    report_arc: Option<Arc<Mutex<Option<Result<ProcessReport, String>>>>>,

    report: Option<ProcessReport>,
    log_lines: Vec<String>,
    result_filter: String,
    report_error: Option<String>,

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

        // ── Barra superior ──────────────────────────────────────
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            Frame::default()
                .stroke(Stroke::new(2.5, BORDER_COLOR))
                .inner_margin(Margin::symmetric(14, 6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading(
                            egui::RichText::new("🏥 PDF Standardizer HE-1")
                                .color(Color32::WHITE)
                                .size(20.0),
                        );
                        ui.separator();
                        ui.label(
                            egui::RichText::new("Estandarización de Repositorio Digital — Hospital Edition")
                                .color(Color32::from_rgb(180, 180, 200)),
                        );
                    });
                });
        });

        // ── Pestañas con bordes ─────────────────────────────────
        egui::TopBottomPanel::top("tabs").show(ctx, |ui| {
            Frame::default()
                .stroke(Stroke::new(1.5, BORDER_COLOR))
                .inner_margin(Margin::symmetric(10, 4))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (tab, label) in [
                            (Tab::Config, "⚙️  Configuración y Ejecución"),
                            (Tab::Dictionary, "📝  Diccionario Maestro"),
                            (Tab::Results, "📊  Resultados"),
                        ] {
                            let selected = self.active_tab == tab;
                            let (fill, text_color, border) = if selected {
                                (HEADER_BG, Color32::WHITE, Stroke::new(1.5, BORDER_COLOR))
                            } else {
                                (Color32::TRANSPARENT, Color32::from_rgb(160, 160, 180), Stroke::new(0.5, Color32::from_rgb(60, 60, 75)))
                            };
                            let btn = egui::Button::new(
                                egui::RichText::new(label).size(14.0).color(text_color),
                            )
                            .fill(fill)
                            .stroke(border)
                            .min_size(Vec2::new(190.0, 32.0));

                            if ui.add(btn).clicked() {
                                self.active_tab = tab;
                            }
                        }
                    });
                });
        });

        // ── Contenido central ───────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.add_space(4.0);
                match self.active_tab {
                    Tab::Config => {
                        self.show_config_section(ui);
                        thick_separator(ui);
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

    // ═══════════════════════════════════════════════════════════════
    //  CONFIG & EJECUCIÓN
    // ═══════════════════════════════════════════════════════════════

    fn show_config_section(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "⚙️  CONFIGURACIÓN");

        bordered_frame(ui, |ui| {
            ui.label(egui::RichText::new("📁 Repositorio a procesar:").size(14.0).strong());
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.repo_path)
                        .hint_text("Ruta a la carpeta raíz...")
                        .desired_width(450.0)
                        .font(egui::TextStyle::Monospace),
                );
                if ui
                    .add(
                        egui::Button::new("📂  Examinar...")
                            .stroke(Stroke::new(1.0, BORDER_COLOR))
                            .min_size(Vec2::new(130.0, 0.0)),
                    )
                    .clicked()
                {
                    if let Some(path) = FileDialog::new().pick_folder() {
                        self.repo_path = path.display().to_string();
                    }
                }
            });
            if !self.repo_path.is_empty() && std::path::Path::new(&self.repo_path).exists() {
                ui.label(
                    egui::RichText::new(format!("  ▶ {}", self.repo_path))
                        .color(Color32::from_rgb(130, 200, 255)),
                );
            }
        });
    }

    fn show_execution_section(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "🚀  EJECUCIÓN");

        bordered_frame(ui, |ui| {
            ui.label(egui::RichText::new("Modo de operación:").strong());
            ui.horizontal(|ui| {
                ui.radio_value(&mut self.run_mode, RunModeChoice::DryRun, "🔍  Dry-Run (simular, no modifica archivos)");
                ui.add_space(20.0);
                ui.radio_value(&mut self.run_mode, RunModeChoice::Execute, "⚡  Ejecutar (real, aplica cambios)");
            });

            thick_separator(ui);

            let can_run = !self.repo_path.is_empty() && !self.running && !self.dict_entries.is_empty();
            ui.horizontal(|ui| {
                let btn_text = match self.run_mode {
                    RunModeChoice::DryRun => "🔍  EJECUTAR DRY-RUN",
                    RunModeChoice::Execute => "⚡  EJECUTAR CAMBIOS REALES",
                };
                let (fill, border_c) = if can_run {
                    (Color32::from_rgb(30, 120, 60), Color32::from_rgb(60, 200, 100))
                } else {
                    (Color32::from_rgb(50, 50, 55), Color32::from_rgb(70, 70, 80))
                };
                let btn = egui::Button::new(egui::RichText::new(btn_text).size(16.0).strong())
                    .min_size(Vec2::new(280.0, 44.0))
                    .fill(fill)
                    .stroke(Stroke::new(2.0, border_c));

                if ui.add_enabled(can_run, btn).clicked() {
                    self.launch_processing();
                }

                if self.running {
                    ui.spinner();
                    if ui
                        .add(
                            egui::Button::new("🛑  CANCELAR")
                                .fill(Color32::from_rgb(140, 40, 30))
                                .stroke(Stroke::new(1.5, Color32::from_rgb(220, 80, 60)))
                                .min_size(Vec2::new(130.0, 44.0)),
                        )
                        .clicked()
                    {
                        if let Some(ref cancel) = self.should_cancel {
                            cancel.store(true, Ordering::Relaxed);
                        }
                    }
                }
            });

            if self.running || self.progress_total > 0 {
                ui.add_space(8.0);
                let fraction = if self.progress_total > 0 {
                    self.progress_cur as f32 / self.progress_total as f32
                } else {
                    0.0
                };
                ui.add(
                    egui::ProgressBar::new(fraction)
                        .desired_width(ui.available_width())
                        .text(format!("{} / {} — {}", self.progress_cur, self.progress_total, self.progress_msg))
                        .fill(Color32::from_rgb(60, 180, 100)),
                );
            }

            if let Some(ref err) = self.report_error {
                ui.add_space(4.0);
                ui.label(egui::RichText::new(format!("❌ {}", err)).color(Color32::from_rgb(255, 100, 80)).strong());
            }

            if !self.log_lines.is_empty() {
                ui.add_space(8.0);
                section_header(ui, "📋  LOG DE PROCESAMIENTO");
                bordered_frame(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(120.0).show(ui, |ui| {
                        for line in &self.log_lines {
                            ui.monospace(line);
                        }
                    });
                });
            }
        });
    }

    // ═══════════════════════════════════════════════════════════════
    //  DICCIONARIO EDITABLE
    // ═══════════════════════════════════════════════════════════════

    fn show_dictionary_section(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "📝  DICCIONARIO MAESTRO");

        ui.label(egui::RichText::new(&self.dict_status).strong());
        ui.add_space(6.0);

        bordered_frame(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add(
                        egui::Button::new("➕  Añadir entrada")
                            .stroke(Stroke::new(1.0, Color32::from_rgb(80, 180, 80)))
                            .min_size(Vec2::new(150.0, 30.0)),
                    )
                    .clicked()
                {
                    let next = self.dict_entries.iter().map(|e| e.orden).max().unwrap_or(0) + 1;
                    self.dict_entries.push(MasterEntry {
                        nombre_pdf: "NUEVO.pdf".into(),
                        orden: next,
                        identidad: "NUEVO".into(),
                        regla_lee_documento: String::new(),
                        alt_identidad: None,
                    });
                }
                if ui
                    .add(
                        egui::Button::new("🔄  Restablecer defaults")
                            .stroke(Stroke::new(1.0, Color32::from_rgb(200, 150, 50)))
                            .min_size(Vec2::new(180.0, 30.0)),
                    )
                    .clicked()
                {
                    match dictionary::reset_to_defaults() {
                        Ok(entries) => {
                            self.dict_entries = entries;
                            self.dict_status = format!("✅ Restablecido a {} documentos default", self.dict_entries.len());
                        }
                        Err(e) => self.dict_status = format!("❌ Error al restablecer: {}", e),
                    }
                }
                if ui
                    .add(
                        egui::Button::new("💾  Guardar a dictionary.json")
                            .stroke(Stroke::new(1.0, Color32::from_rgb(80, 150, 220)))
                            .min_size(Vec2::new(220.0, 30.0)),
                    )
                    .clicked()
                {
                    match dictionary::save_to_json(&self.dict_entries) {
                        Ok(()) => self.dict_status = format!("✅ {} documentos guardados", self.dict_entries.len()),
                        Err(e) => self.dict_status = format!("❌ {}", e),
                    }
                }
            });
        });

        ui.add_space(4.0);
        ui.label(format!("📋 {} entradas en el diccionario", self.dict_entries.len()));

        bordered_frame(ui, |ui| {
            egui::ScrollArea::vertical().max_height(460.0).show(ui, |ui| {
                let mut to_remove: Option<usize> = None;

                egui::Grid::new("dict_grid")
                    .striped(true)
                    .min_col_width(50.0)
                    .spacing(Vec2::new(8.0, 4.0))
                    .show(ui, |ui| {
                        // Cabecera
                        ui.colored_label(HEADER_BG, "  ID  ");
                        ui.colored_label(HEADER_BG, "  Nombre PDF  ");
                        ui.colored_label(HEADER_BG, "  Alt ID  ");
                        ui.colored_label(HEADER_BG, "  Keywords (|)  ");
                        ui.colored_label(HEADER_BG, "  Orden  ");
                        ui.colored_label(HEADER_BG, "  ");
                        ui.end_row();

                        for (idx, entry) in self.dict_entries.iter_mut().enumerate() {
                            ui.add(
                                egui::TextEdit::singleline(&mut entry.identidad)
                                    .desired_width(70.0)
                                    .font(egui::TextStyle::Monospace),
                            );
                            ui.add(
                                egui::TextEdit::singleline(&mut entry.nombre_pdf)
                                    .desired_width(100.0)
                                    .font(egui::TextStyle::Monospace),
                            );

                            let mut alt = entry.alt_identidad.clone().unwrap_or_default();
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut alt)
                                        .hint_text("vacío")
                                        .desired_width(60.0)
                                        .font(egui::TextStyle::Monospace),
                                )
                                .changed()
                            {
                                entry.alt_identidad = if alt.is_empty() { None } else { Some(alt) };
                            }

                            ui.add(
                                egui::TextEdit::singleline(&mut entry.regla_lee_documento)
                                    .desired_width(200.0)
                                    .font(egui::TextStyle::Monospace),
                            );

                            let mut ord = entry.orden.to_string();
                            if ui
                                .add(
                                    egui::TextEdit::singleline(&mut ord)
                                        .desired_width(55.0)
                                        .font(egui::TextStyle::Monospace),
                                )
                                .changed()
                            {
                                entry.orden = ord.parse().unwrap_or(entry.orden);
                            }

                            if ui
                                .add(
                                    egui::Button::new("🗑")
                                        .fill(Color32::from_rgb(100, 40, 30))
                                        .stroke(Stroke::new(1.0, Color32::from_rgb(200, 80, 60)))
                                        .min_size(Vec2::new(30.0, 0.0)),
                                )
                                .clicked()
                            {
                                to_remove = Some(idx);
                            }
                            ui.end_row();
                        }
                    });

                if let Some(idx) = to_remove {
                    self.dict_entries.remove(idx);
                }
            });
        });

        ui.add_space(4.0);
        ui.label(
            egui::RichText::new("💡 Tras editar, pulsá «Guardar a dictionary.json». Los cambios aplican al próximo procesamiento.")
                .color(Color32::from_rgb(160, 160, 180))
                .size(12.0),
        );
    }

    // ═══════════════════════════════════════════════════════════════
    //  RESULTADOS
    // ═══════════════════════════════════════════════════════════════

    fn show_results_section(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "📊  RESULTADOS");

        if let Some(ref report) = self.report {
            bordered_frame(ui, |ui| {
                ui.label(egui::RichText::new("📈 RESUMEN DEL PROCESAMIENTO").size(15.0).strong());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label(format!(
                        "📁 Carpetas procesadas: {}   |   ✏️ Modificadas: {}",
                        report.total_folders, report.folders_modified
                    ));
                });
                ui.horizontal(|ui| {
                    ui.colored_label(Color32::from_rgb(100, 220, 100), format!("🔀 Mergeados: {}", report.files_merged));
                    ui.colored_label(Color32::from_rgb(220, 200, 60), format!("  🔒 Mantenidos: {}", report.files_kept));
                    ui.colored_label(Color32::from_rgb(220, 100, 80), format!("  🚫 Ignorados: {}", report.files_ignored));
                    ui.colored_label(Color32::from_rgb(220, 80, 60), format!("  ⚠️ Errores: {}", report.errors.len()));
                });
                ui.label(format!("🕐 {}", report.timestamp));
            });

            ui.add_space(8.0);

            // Filtro
            bordered_frame(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label("🔍 Filtrar carpeta:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.result_filter)
                            .hint_text("escribí parte del nombre...")
                            .desired_width(280.0),
                    );
                    if ui
                        .add(
                            egui::Button::new("✖ Limpiar")
                                .stroke(Stroke::new(1.0, BORDER_COLOR))
                                .min_size(Vec2::new(80.0, 0.0)),
                        )
                        .clicked()
                    {
                        self.result_filter.clear();
                    }
                });
            });

            // Calcular filtrados fuera del frame para usarlos después
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

            ui.add_space(4.0);

            // Tabla de resultados
            if !filtered.is_empty() {
                bordered_frame(ui, |ui| {
                    egui::ScrollArea::vertical().max_height(350.0).show(ui, |ui| {
                        egui::Grid::new("results_grid")
                            .striped(true)
                            .min_col_width(40.0)
                            .spacing(Vec2::new(8.0, 3.0))
                            .show(ui, |ui| {
                                ui.colored_label(HEADER_BG, "  Carpeta  ");
                                ui.colored_label(HEADER_BG, "  Archivo Original  ");
                                ui.colored_label(HEADER_BG, "  Acción  ");
                                ui.colored_label(HEADER_BG, "  Destino  ");
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
                                        ui.monospace(&f.original);
                                        let (color, label) = match f.action.as_str() {
                                            "MERGE" => (Color32::from_rgb(100, 220, 100), "🔀 MERGE"),
                                            "KEEP" => (Color32::from_rgb(220, 200, 60), "🔒 KEEP"),
                                            "IGNORE" => (Color32::from_rgb(220, 100, 80), "🚫 IGNORE"),
                                            _ => (Color32::GRAY, f.action.as_str()),
                                        };
                                        ui.colored_label(color, label);
                                        ui.monospace(&f.target);
                                        ui.end_row();
                                    }
                                }
                            });
                    });
                });
            }

            // Errores
            if !report.errors.is_empty() {
                ui.add_space(8.0);
                bordered_frame(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!("⚠️  {} ERRORES", report.errors.len()))
                            .color(Color32::from_rgb(255, 120, 80))
                            .strong()
                            .size(14.0),
                    );
                    ui.add_space(4.0);
                    for err in &report.errors {
                        ui.label(egui::RichText::new(format!("  ▸ {}", err)).color(Color32::from_rgb(255, 150, 120)));
                    }
                });
            }
        } else {
            bordered_frame(ui, |ui| {
                ui.label(
                    egui::RichText::new("ℹ️  No hay resultados todavía.")
                        .color(Color32::from_rgb(160, 160, 180))
                        .size(14.0),
                );
                ui.label("Ejecutá un Dry-Run o Ejecución real desde la pestaña «Configuración y Ejecución».");
            });
        }
    }

    // ═══════════════════════════════════════════════════════════════
    //  PROCESAMIENTO EN BACKGROUND
    // ═══════════════════════════════════════════════════════════════

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

        self.active_tab = Tab::Results;
    }
}
