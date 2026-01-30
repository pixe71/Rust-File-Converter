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
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            state: AppState::Menu,
            files: Vec::new(),
            options: ConversionOptions::default(),
            status: Arc::new(Mutex::new(ConversionStatus::Idle)),
        }
    }

    fn render_menu(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(20.0);
            ui.heading(egui::RichText::new("🔄 UNIVERSAL CONVERTER").size(32.0).strong());
            ui.label("Images • Documents • Audio • Video");
            ui.add_space(40.0);

            let btn_size = egui::vec2(250.0, 50.0);

            if ui.add_sized(btn_size, egui::Button::new("🖼️ Convert Images")).clicked() {
                self.state = AppState::Images;
                self.files.clear();
                self.options = ConversionOptions::default();
                self.options.format = "jpg".to_string();
            }
            ui.add_space(10.0);
            
            if ui.add_sized(btn_size, egui::Button::new("📄 Convert Documents")).clicked() {
                self.state = AppState::Documents;
                self.files.clear();
                self.options = ConversionOptions::default();
                self.options.format = "pdf".to_string();
            }
            ui.add_space(10.0);

            if ui.add_sized(btn_size, egui::Button::new("🎵 Convert Audio")).clicked() {
                self.state = AppState::Audio;
                self.files.clear();
                self.options = ConversionOptions::default();
                self.options.format = "mp3".to_string();
            }
            ui.add_space(10.0);

            if ui.add_sized(btn_size, egui::Button::new("🎬 Convert Video")).clicked() {
                self.state = AppState::Video;
                self.files.clear();
                self.options = ConversionOptions::default();
                self.options.format = "mp4".to_string();
            }
        });
    }

    fn render_converter_ui(&mut self, ui: &mut egui::Ui, title: &str, formats: &[&str]) {
       ui.vertical(|ui| {
            ui.horizontal(|ui| {
                if ui.button("← Back").clicked() {
                    self.state = AppState::Menu;
                    self.files.clear();
                    *self.status.lock().unwrap() = ConversionStatus::Idle;
                }
                ui.heading(title);
            });
            ui.separator();

            if ui.button("📁 Select Files").clicked() {
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
            if !self.files.is_empty() {
                egui::ScrollArea::vertical().max_height(100.0).show(ui, |ui| {
                    for f in &self.files {
                        ui.label(f.file_name().unwrap_or_default().to_string_lossy());
                    }
                });
            }

            ui.separator();

            ui.horizontal(|ui| {
                ui.label("Output Format:");
                egui::ComboBox::from_id_salt("format_box")
                    .selected_text(&self.options.format)
                    .show_ui(ui, |ui| {
                        for fmt in formats {
                            ui.selectable_value(&mut self.options.format, fmt.to_string(), *fmt);
                        }
                    });
            });

            match self.state {
                AppState::Images => {
                    ui.add(egui::Slider::new(&mut self.options.quality, 1..=100).text("Quality (JPG)"));
                },
                AppState::Audio => {
                     ui.horizontal(|ui| {
                        ui.label("Bitrate:");
                        egui::ComboBox::from_id_salt("bitrate_box")
                        .selected_text(&self.options.bitrate)
                        .show_ui(ui, |ui| {
                            for b in ["64k", "128k", "192k", "256k", "320k"] {
                                ui.selectable_value(&mut self.options.bitrate, b.to_string(), b);
                            }
                        });
                     });
                },
                AppState::Video => {
                    ui.horizontal(|ui| {
                        ui.label("Codec:");
                        egui::ComboBox::from_id_salt("codec_box")
                        .selected_text(&self.options.codec)
                        .show_ui(ui, |ui| {
                            for c in ["libx264", "libx265", "copy"] {
                                ui.selectable_value(&mut self.options.codec, c.to_string(), c);
                            }
                        });
                     });
                },
                _ => {}
            }

            ui.add_space(20.0);

            let status = self.status.lock().unwrap().clone();
            
            match &status {
                ConversionStatus::Processing(current) => {
                    ui.ctx().request_repaint();
                    ui.spinner();
                    ui.label(format!("Processing: {}", current));
                },
                ConversionStatus::Idle | ConversionStatus::Success(_) | ConversionStatus::Error(_) | ConversionStatus::Completed {..} => {
                    if ui.add_sized(egui::vec2(200.0, 40.0), egui::Button::new("RUN CONVERSION")).clicked() {
                         self.start_conversion();
                    }
                }
            }

            match status {
                ConversionStatus::Completed { success, failures } => {
                     ui.label(egui::RichText::new(format!("Done! Success: {}, Failures: {}", success, failures)).color(egui::Color32::GREEN));
                },
                ConversionStatus::Error(e) => {
                    ui.label(egui::RichText::new(format!("Error: {}", e)).color(egui::Color32::RED));
                },
                _ => {}
            }

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
