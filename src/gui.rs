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
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(72, 126, 176);
        style.visuals.widgets.active.fg_stroke.color = egui::Color32::WHITE;
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(85, 140, 190);
        style.visuals.window_rounding = egui::Rounding::same(10.0);
        style.spacing.button_padding = egui::vec2(10.0, 8.0);
        style.spacing.item_spacing = egui::vec2(10.0, 10.0);
        cc.egui_ctx.set_style(style);

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
        // Use standard vertical layout first to avoid double-centering issues
        ui.vertical(|ui| {
             // Center the header
            ui.vertical_centered(|ui| {
                ui.add_space(50.0);
                ui.heading(egui::RichText::new("FILE CONVERTER").size(40.0).strong().color(egui::Color32::from_rgb(100, 200, 255)));
                ui.label(egui::RichText::new("By pixe71").size(14.0).color(egui::Color32::from_gray(180)));
                ui.add_space(40.0);
            });

            let btn_size = egui::vec2(300.0, 80.0);
            
            // --- CORRECTION ICI ---
            // Helper for card-like buttons
            let mut mk_card = |ui: &mut egui::Ui, title: &str, icon: &str, desc: &str, color: egui::Color32| -> bool {
                // 1. On alloue l'espace et on demande la détection de clic sur tout le rectangle
                let (rect, response) = ui.allocate_exact_size(btn_size, egui::Sense::click());
                
                let visuals = ui.style().interact(&response);
                let bg_color = if response.hovered() { 
                    ui.visuals().widgets.hovered.bg_fill 
                } else { 
                    ui.visuals().widgets.inactive.bg_fill 
                };
                
                ui.painter().rect_filled(rect, 10.0, bg_color);
                ui.painter().rect_stroke(rect, 10.0, visuals.bg_stroke);

                 // Left-aligned content with icon
                 ui.allocate_ui_at_rect(rect, |ui| {
                     // 2. CRITICAL FIX: On désactive la sélection de texte.
                     // Cela permet aux clics de traverser le texte et d'atteindre le bouton (rect) en dessous.
                     ui.style_mut().interaction.selectable_labels = false;

                     ui.horizontal_centered(|ui| {
                         ui.add_space(20.0);
                         ui.label(egui::RichText::new(icon).size(30.0));
                         ui.add_space(20.0);
                         ui.vertical(|ui| {
                             ui.add_space(18.0); // push text down a bit to center vertically manually
                             ui.label(egui::RichText::new(title).size(18.0).strong());
                             ui.label(egui::RichText::new(desc).size(12.0).color(egui::Color32::GRAY));
                         });
                     });
                 });

                // On retourne le clic détecté sur le rectangle global
                response.on_hover_text(desc).clicked()
            };

            ui.add_space(20.0);
            
            // Single column vertical layout
            ui.vertical_centered(|ui| {
                if mk_card(ui, "IMAGES", "🖼", "Convert JPG, PNG, WEBP...", egui::Color32::from_rgb(255, 100, 100)) {
                    self.to_state(AppState::Images, "jpg");
                }
                ui.add_space(15.0);

                if mk_card(ui, "DOCUMENTS", "📄", "Convert PDF, DOCX, TXT...", egui::Color32::from_rgb(100, 255, 100)) {
                    self.to_state(AppState::Documents, "pdf");
                }
                ui.add_space(15.0);

                if mk_card(ui, "AUDIO", "🎵", "Convert MP3, WAV, FLAC...", egui::Color32::from_rgb(100, 100, 255)) {
                    self.to_state(AppState::Audio, "mp3");
                }
                ui.add_space(15.0);

                if mk_card(ui, "VIDEO", "🎬", "Convert MP4, MKV, AVI...", egui::Color32::from_rgb(255, 255, 100)) {
                    self.to_state(AppState::Video, "mp4");
                }
            });
            
            // Footer
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(egui::RichText::new("v1.0.0").size(10.0).color(egui::Color32::from_gray(100)));
            });
        });
    }

    fn render_converter_ui(&mut self, ui: &mut egui::Ui, title: &str, formats: &[&str]) {
       egui::SidePanel::left("left_panel")
           .resizable(false)
           .default_width(200.0)
           .show_inside(ui, |ui| {
               ui.vertical_centered(|ui| {
                   ui.add_space(20.0);
                   if ui.add(egui::Button::new(egui::RichText::new("⬅ MENU").strong()).min_size(egui::vec2(150.0, 40.0))).clicked() {
                       self.state = AppState::Menu;
                       self.files.clear();
                       *self.status.lock().unwrap() = ConversionStatus::Idle;
                   }
                   ui.add_space(20.0);
                   ui.separator();
                   ui.add_space(20.0);
                   ui.heading(egui::RichText::new("Settings").size(18.0));
                   ui.add_space(10.0);
                   
                   egui::Grid::new("side_settings").spacing([10.0, 15.0]).show(ui, |ui| {
                       ui.label("Format:");
                       egui::ComboBox::from_id_salt("format_box")
                           .selected_text(&self.options.format)
                           .width(80.0)
                           .show_ui(ui, |ui| {
                               for fmt in formats {
                                   ui.selectable_value(&mut self.options.format, fmt.to_string(), *fmt);
                               }
                           });
                       ui.end_row();

                       match self.state {
                           AppState::Images => {
                               // Quality slider removed as requested
                           },
                            AppState::Audio => {
                                 ui.label("Bitrate:");
                                 egui::ComboBox::from_id_salt("bitrate_box")
                                    .selected_text(&self.options.bitrate)
                                    .width(80.0)
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
                                    .width(80.0)
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
           });

       egui::CentralPanel::default().show_inside(ui, |ui| {
           ui.vertical_centered(|ui| {
               ui.add_space(20.0);
               ui.heading(egui::RichText::new(title).size(28.0).strong().color(egui::Color32::from_rgb(100, 200, 255)));
               ui.add_space(20.0);
           });

           // File Selection Area
           let frame = egui::Frame::group(ui.style())
               .rounding(egui::Rounding::same(8.0))
               .inner_margin(20.0);
           
           frame.show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                   ui.label(egui::RichText::new("Selected Files").strong().size(16.0));
                   ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                       if ui.add(egui::Button::new("➕ Add Files").min_size(egui::vec2(100.0, 30.0))).clicked() {
                           let mut dialog = FileDialog::new();
                           match self.state {
                               AppState::Images => dialog = dialog.add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "webp", "ico"]),
                               AppState::Audio => dialog = dialog.add_filter("Audio", &["mp3", "wav", "ogg", "flac", "m4a", "aac"]),
                               AppState::Video => dialog = dialog.add_filter("Video", &["mp4", "avi", "mkv", "mov", "webm"]),
                               AppState::Documents => dialog = dialog.add_filter("Documents", &["pdf", "docx", "txt"]),
                               _ => {}
                           }
                           
                           if let Some(paths) = dialog.pick_files() {
                               self.files.extend(paths);
                           }
                       }
                       if !self.files.is_empty() {
                           if ui.button("🗑 Clear All").clicked() {
                               self.files.clear();
                           }
                       }
                   });
                });
                
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                egui::ScrollArea::vertical().min_scrolled_height(200.0).max_height(250.0).id_salt("file_scroll").show(ui, |ui| {
                      if self.files.is_empty() {
                          ui.vertical_centered(|ui| {
                              ui.add_space(40.0);
                              ui.label(egui::RichText::new("No files selected").size(16.0).color(egui::Color32::from_gray(100)));
                              ui.label(egui::RichText::new("Click '+ Add Files' to get started").italics());
                          });
                      } else {
                        for (idx, f) in self.files.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}.", idx + 1));
                                ui.label("📄");
                                ui.label(egui::RichText::new(f.file_name().unwrap_or_default().to_string_lossy()).color(egui::Color32::LIGHT_GRAY));
                            });
                            ui.add_space(2.0);
                        }
                      }
                 });
            });

           ui.add_space(20.0);

           // Status & Action Area
           let status = self.status.lock().unwrap().clone();
           
           ui.vertical_centered(|ui| {
               match &status {
                   ConversionStatus::Processing(current) => {
                       ui.add_space(10.0);
                       ui.spinner();
                       ui.label(egui::RichText::new("Converting...").strong().size(16.0));
                       ui.label(format!("Current file: {}", current));
                   },
                   ConversionStatus::Idle | ConversionStatus::Success(_) | ConversionStatus::Error(_) | ConversionStatus::Completed {..} => {
                       let btn = egui::Button::new(egui::RichText::new("🚀 START CONVERSION").size(20.0).strong())
                           .min_size(egui::vec2(250.0, 60.0))
                           .rounding(10.0);
                       
                       let btn = if self.files.is_empty() {
                           btn
                       } else {
                           btn.fill(egui::Color32::from_rgb(0, 120, 215)) // Windows blue accent
                       };
                           
                       if ui.add_enabled(!self.files.is_empty(), btn).clicked() {
                            self.start_conversion();
                       }
                   }
               }

               ui.add_space(15.0);

               match status {
                   ConversionStatus::Completed { success, failures } => {
                        let color = if failures == 0 { egui::Color32::GREEN } else { egui::Color32::YELLOW };
                        ui.label(egui::RichText::new(format!("✔ All Done! {} Success, {} Failures", success, failures)).size(18.0).strong().color(color));
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
                AppState::Images => self.render_converter_ui(ui, "🖼 Image Converter", &["jpg", "png", "bmp", "gif", "ico", "webp"]),
                AppState::Documents => self.render_converter_ui(ui, "📄 Document Converter", &["pdf", "txt", "docx"]),
                AppState::Audio => self.render_converter_ui(ui, "🎵 Audio Converter", &["mp3", "wav", "ogg", "flac", "aac"]),
                AppState::Video => self.render_converter_ui(ui, "🎬 Video Converter", &["mp4", "mkv", "avi", "mov"]),
            }
        });
    }
}