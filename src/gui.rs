use eframe::egui;
use rfd::FileDialog;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use crate::converters::{self, ConversionOptions, ConversionStatus};

#[derive(PartialEq)]
enum AppState {
    Menu,
    Images,
    Documents,
    Audio,
    Video,
}

pub struct ConverterApp {
    state: AppState,
    files: Vec<PathBuf>,
    options: ConversionOptions,
    status: Arc<Mutex<ConversionStatus>>,
}

impl ConverterApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.widgets.active.bg_fill = egui::Color32::from_rgb(72, 126, 176);
        visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;
        cc.egui_ctx.set_visuals(visuals);

        Self {
            state: AppState::Menu,
            files: Vec::new(),
            options: ConversionOptions::default(),
            status: Arc::new(Mutex::new(ConversionStatus::Idle)),
        }
    }

    fn to_state(&mut self, state: AppState, format: &str) {
        self.state = state;
        self.files.clear();
        self.options = ConversionOptions::default();
        self.options.format = format.to_string();
    }

    fn render_menu(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.heading(egui::RichText::new("🔄 FILE CONVERTER").size(36.0).strong().color(egui::Color32::from_rgb(100, 200, 255)));
            ui.add_space(10.0);
            ui.label(egui::RichText::new("Fast & Secure File Conversion").size(16.0).italics().color(egui::Color32::LIGHT_GRAY));
            ui.add_space(50.0);

            let btn_size = egui::vec2(280.0, 50.0);
            let mk_btn = |ui: &mut egui::Ui, text: &str| {
                ui.add_sized(btn_size, egui::Button::new(egui::RichText::new(text).size(18.0).strong()).rounding(8.0))
            };

            if mk_btn(ui, "🖼️  Convert Images").clicked() {
                self.to_state(AppState::Images, "jpg");
            }
            ui.add_space(15.0);
            
            if mk_btn(ui, "📄  Convert Documents").clicked() {
                self.to_state(AppState::Documents, "pdf");
            }
            ui.add_space(15.0);

            if mk_btn(ui, "🎵  Convert Audio").clicked() {
                self.to_state(AppState::Audio, "mp3");
            }
            ui.add_space(15.0);

            if mk_btn(ui, "🎬  Convert Video").clicked() {
                self.to_state(AppState::Video, "mp4");
            }
        });
    }

    fn render_converter_ui(&mut self, ui: &mut egui::Ui, title: &str, formats: &[&str]) {
       ui.vertical(|ui| {
            // Header
            ui.horizontal(|ui| {
                if ui.button(egui::RichText::new("⬅ Back").size(16.0)).clicked() {
                    self.state = AppState::Menu;
                    self.files.clear();
                    *self.status.lock().unwrap() = ConversionStatus::Idle;
                }
                ui.add_space(20.0);
                ui.heading(egui::RichText::new(title).size(24.0).color(egui::Color32::from_rgb(100, 200, 255)));
            });
            ui.add_space(20.0);

            // Files Section
            ui.group(|ui| {
                 ui.label(egui::RichText::new("1. Select Files").strong());
                 ui.add_space(5.0);
                 
                 ui.horizontal(|ui| {
                    if ui.add(egui::Button::new("📂  Add Files...").min_size(egui::vec2(120.0, 30.0))).clicked() {
                        let mut dialog = FileDialog::new();
                        match self.state {
                            AppState::Images => dialog = dialog.add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "webp", "ico"]),
                            AppState::Audio => dialog = dialog.add_filter("Audio", &["mp3", "wav", "ogg", "flac", "m4a", "aac"]),
                            AppState::Video => dialog = dialog.add_filter("Video", &["mp4", "avi", "mkv", "mov", "webm"]),
                            AppState::Documents => dialog = dialog.add_filter("Documents", &["pdf", "docx", "txt"]),
                            _ => {}
                        }
                        
                        if let Some(paths) = dialog.pick_files() {
                            self.files = paths;
                        }
                    }
                    ui.label(format!("{} files selected", self.files.len()));
                 });
                 
                 ui.add_space(5.0);
                 
                 if !self.files.is_empty() {
                     egui::Frame::none()
                        .fill(ui.style().visuals.extreme_bg_color)
                        .rounding(4.0)
                        .inner_margin(5.0)
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical().max_height(150.0).show(ui, |ui| {
                                for f in &self.files {
                                    ui.horizontal(|ui| {
                                        ui.label("📄");
                                        ui.label(f.file_name().unwrap_or_default().to_string_lossy());
                                    });
                                }
                            });
                        });
                 }
            });

            ui.add_space(20.0);

            // Options Section
            ui.group(|ui| {
                ui.label(egui::RichText::new("2. Conversion Options").strong());
                ui.add_space(10.0);

                egui::Grid::new("options_grid").spacing([20.0, 10.0]).show(ui, |ui| {
                    ui.label("Output Format:");
                    egui::ComboBox::from_id_salt("format_box")
                        .selected_text(&self.options.format)
                        .show_ui(ui, |ui| {
                            for fmt in formats {
                                ui.selectable_value(&mut self.options.format, fmt.to_string(), *fmt);
                            }
                        });
                    ui.end_row();

                    match self.state {
                        AppState::Images => {
                            ui.label("Quality:");
                            ui.add(egui::Slider::new(&mut self.options.quality, 1..=100).text("%"));
                            ui.end_row();
                        },
                        AppState::Audio => {
                             ui.label("Bitrate:");
                             egui::ComboBox::from_id_salt("bitrate_box")
                                .selected_text(&self.options.bitrate)
                                .show_ui(ui, |ui| {
                                    for b in ["64k", "128k", "192k", "256k", "320k"] {
                                        ui.selectable_value(&mut self.options.bitrate, b.to_string(), b);
                                    }
                                });
                             ui.end_row();
                        },
                        AppState::Video => {
                            ui.label("Codec:");
                            egui::ComboBox::from_id_salt("codec_box")
                                .selected_text(&self.options.codec)
                                .show_ui(ui, |ui| {
                                    for c in ["libx264", "libx265", "copy"] {
                                        ui.selectable_value(&mut self.options.codec, c.to_string(), c);
                                    }
                                });
                             ui.end_row();
                        },
                        _ => {}
                    }
                });
            });

            ui.add_space(30.0);

            // Action / Status Section
            let status = self.status.lock().unwrap().clone();
            
            ui.vertical_centered(|ui| {
                match &status {
                    ConversionStatus::Processing(current) => {
                        ui.ctx().request_repaint();
                        ui.spinner();
                        ui.add_space(5.0);
                        ui.label(format!("Processing: {}", current));
                    },
                    ConversionStatus::Idle | ConversionStatus::Success(_) | ConversionStatus::Error(_) | ConversionStatus::Completed {..} => {
                        let btn = egui::Button::new(egui::RichText::new("🚀 START CONVERSION").size(18.0).strong())
                            .min_size(egui::vec2(200.0, 50.0))
                            .rounding(8.0);
                        
                        let btn = if self.files.is_empty() {
                            btn
                        } else {
                            btn.fill(egui::Color32::from_rgb(0, 120, 215))
                        };
                            
                        if ui.add_enabled(!self.files.is_empty(), btn).clicked() {
                             self.start_conversion();
                        }
                    }
                }

                ui.add_space(10.0);

                match status {
                    ConversionStatus::Completed { success, failures } => {
                         let color = if failures == 0 { egui::Color32::GREEN } else { egui::Color32::YELLOW };
                         ui.label(egui::RichText::new(format!("✔ Done! {} Success, {} Failures", success, failures)).size(16.0).color(color));
                    },
                    ConversionStatus::Error(e) => {
                        ui.label(egui::RichText::new(format!("❌ Error: {}", e)).size(16.0).color(egui::Color32::RED));
                    },
                    _ => {}
                }
            });
       });
    }

    fn start_conversion(&mut self) {
        if self.files.is_empty() {
            return;
        }

        let output_folder = FileDialog::new().pick_folder();
        if output_folder.is_none() {
            return;
        }
        let output_folder = output_folder.unwrap();

        let files = self.files.clone();
        let options = self.options.clone();
        let status_clone = self.status.clone();
        let state = match self.state {
            AppState::Images => "images",
            AppState::Audio => "audio",
            AppState::Video => "video",
            AppState::Documents => "docs",
            _ => "unknown"
        };
        let mode = state.to_string();


        thread::spawn(move || {
            let mut success_count = 0;
            let mut fail_count = 0;

            for file in files {
                {
                    let mut s = status_clone.lock().unwrap();
                    *s = ConversionStatus::Processing(file.file_name().unwrap().to_string_lossy().to_string());
                }

                let res = match mode.as_str() {
                    "images" => converters::image_conv::convert_image(&file, &output_folder, &options),
                    "audio" => converters::audio_conv::convert_audio(&file, &output_folder, &options),
                    "video" => converters::video_conv::convert_video(&file, &output_folder, &options),
                    "docs" => converters::doc_conv::convert_doc(&file, &output_folder, &options),
                    _ => Ok(PathBuf::new()),
                };

                match res {
                    Ok(_) => success_count += 1,
                    Err(e) => {
                        println!("Error converting {:?}: {}", file, e);
                        fail_count += 1;
                    }
                }
            }

            let mut s = status_clone.lock().unwrap();
            *s = ConversionStatus::Completed { success: success_count, failures: fail_count };
        });
    }
}

impl eframe::App for ConverterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            match self.state {
                AppState::Menu => self.render_menu(ui),
                AppState::Images => self.render_converter_ui(ui, "🖼️ Image Converter", &["jpg", "png", "bmp", "gif", "ico", "webp"]),
                AppState::Documents => self.render_converter_ui(ui, "📄 Document Converter", &["pdf", "txt", "docx"]),
                AppState::Audio => self.render_converter_ui(ui, "🎵 Audio Converter", &["mp3", "wav", "ogg", "flac", "aac"]),
                AppState::Video => self.render_converter_ui(ui, "🎬 Video Converter", &["mp4", "mkv", "avi", "mov"]),
            }
        });
    }
}
