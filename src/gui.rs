use eframe::egui;
use file_converter::converters::{ConversionKind, ConversionOptions};
use file_converter::engines::{EngineManager, EngineManifest, EngineProgress};
use file_converter::job::{ConversionResultStatus, JobController};
use file_converter::selection;
use file_converter::tools::{ToolDiagnostic, ToolResolver};
use rfd::FileDialog;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::time::Duration;

enum EngineEvent {
    Progress(EngineProgress),
    Finished(Result<Vec<ToolDiagnostic>, String>),
}

#[derive(Default)]
pub struct ConverterApp {
    kind: Option<ConversionKind>,
    job_kind: Option<ConversionKind>,
    files: selection::Selection,
    options: ConversionOptions,
    destination: Option<PathBuf>,
    jobs: JobController,
    messages: Vec<String>,
    tool_diagnostics: Vec<ToolDiagnostic>,
    tool_check: Option<std::sync::mpsc::Receiver<Result<Vec<ToolDiagnostic>, String>>>,
    tool_check_error: Option<String>,
    engine_worker: Option<mpsc::Receiver<EngineEvent>>,
    engine_cancel: Arc<AtomicBool>,
    engine_progress: Vec<EngineProgress>,
    setup_prompt: bool,
    setup_dismissed: bool,
    repair_requested: bool,
    close_after_setup: bool,
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}

fn render_error_details(
    ui: &mut egui::Ui,
    error: &str,
    id: egui::Id,
) -> egui::scroll_area::ScrollAreaOutput<()> {
    egui::ScrollArea::vertical()
        .id_salt(id)
        .max_height(160.0)
        .show(ui, |ui| {
            ui.add(egui::Label::new(error).wrap().selectable(true));
        })
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
        let mut app = Self::default();
        app.check_engines(&cc.egui_ctx);
        app
    }

    fn check_engines(&mut self, ctx: &egui::Context) {
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("engine-diagnostics".into())
            .spawn(move || {
                let result = ToolResolver::from_environment()
                    .map(|resolver| resolver.diagnostics())
                    .map_err(|error| format!("{error:#}"));
                let _ = sender.send(result);
                ctx.request_repaint();
            }) {
            Ok(_) => {
                self.tool_check = Some(receiver);
                self.tool_check_error = None;
            }
            Err(error) => self.tool_check_error = Some(error.to_string()),
        }
    }

    fn prepare_engines(&mut self, ctx: &egui::Context) {
        // Discard an older diagnostic result before publishing new installation state.
        self.tool_check = None;
        let (sender, receiver) = mpsc::channel();
        let ctx = ctx.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        self.engine_cancel = cancel.clone();
        let repair = self.repair_requested;
        match std::thread::Builder::new()
            .name("engine-preparation".into())
            .spawn(move || {
                let result = (|| -> anyhow::Result<_> {
                    let manager = EngineManager::from_environment()?;
                    let resolver = ToolResolver::from_environment()?;
                    manager.prepare(&resolver, repair, &cancel, |progress| {
                        let _ = sender.send(EngineEvent::Progress(progress));
                        ctx.request_repaint();
                    })?;
                    let diagnostics = resolver.diagnostics();
                    anyhow::ensure!(
                        diagnostics
                            .iter()
                            .all(|diagnostic| diagnostic.result.is_ok()),
                        "An engine is still unavailable after preparation. Use Retry to repair it."
                    );
                    Ok(diagnostics)
                })()
                .map_err(|error| format!("{error:#}"));
                let _ = sender.send(EngineEvent::Finished(result));
                ctx.request_repaint();
            }) {
            Ok(_) => {
                self.engine_worker = Some(receiver);
                self.engine_progress.clear();
                self.tool_check_error = None;
            }
            Err(error) => self.tool_check_error = Some(error.to_string()),
        }
    }

    fn render_engine_setup(&mut self, ctx: &egui::Context) {
        if !self.setup_prompt {
            return;
        }
        egui::Window::new("Prepare conversion engines")
            .collapsible(false).resizable(false).default_width(440.0)
            .show(ctx, |ui| {
                if self.engine_worker.is_some() {
                    ui.heading("Preparing conversion engines...");
                    ui.spinner();
                    for name in ["FFmpeg", "Pandoc", "Typst"] {
                        if let Some(progress) = self.engine_progress.iter().find(|progress| progress.engine == name) {
                            ui.label(format!("{name}: {}", progress.phase));
                            if let Some(total) = progress.total.filter(|total| *total > 0) {
                                ui.add(egui::ProgressBar::new((progress.downloaded as f64 / total as f64) as f32).text(format!("{:.1} / {:.1} MiB", progress.downloaded as f64 / 1048576.0, total as f64 / 1048576.0)));
                            }
                        } else { ui.label(format!("{name}: Waiting")); }
                    }
                    if ui.button("Cancel preparation").clicked() { self.engine_cancel.store(true, Ordering::Relaxed); }
                } else {
                    ui.label("Rust File Converter needs to download missing conversion engines (FFmpeg/FFprobe and Pandoc). The Typst PDF engine is supplied with the application.");
                    ui.label("They will be stored locally for offline use. Downloads can be large. Your files are never uploaded.");
                    if let Ok(manager) = EngineManager::from_environment() {
                        ui.small(format!("Storage: {}", manager.root().display()));
                    }
                    if let Some(error) = &self.tool_check_error {
                        ui.colored_label(egui::Color32::LIGHT_RED, "Preparation failed. Check your Internet connection and available disk space, then retry.");
                        render_error_details(ui, error, egui::Id::new("engine_setup_error"));
                    }
                    ui.horizontal(|ui| {
                        let button = if self.tool_check_error.is_some() { "Retry" } else { "Download & Continue" };
                        if ui.button(button).clicked() { self.prepare_engines(ctx); }
                        if ui.button("Cancel").clicked() { self.setup_prompt = false; self.setup_dismissed = true; }
                    });
                    ui.small("You can continue using native image conversion without these engines.");
                }
            });
    }

    fn enter(&mut self, kind: ConversionKind) {
        if self.jobs.is_active() {
            self.kind = self.job_kind;
            return;
        }
        self.kind = Some(kind);
        self.files.clear();
        self.messages.clear();
        self.options = ConversionOptions {
            format: kind.output_formats()[0].into(),
            ..Default::default()
        };
    }

    fn add_paths(&mut self, paths: Vec<PathBuf>) {
        self.messages.clear();
        if self.jobs.is_active() {
            self.messages
                .push("A batch is running. Wait before adding more files.".into());
            return;
        }
        let mut messages = Vec::new();
        let mut duplicates = 0;
        let mut menu_selection = selection::Selection::default();
        for path in paths {
            let kind = match self
                .kind
                .map(Ok)
                .unwrap_or_else(|| selection::infer_kind(&path))
            {
                Ok(kind) => kind,
                Err(error) => {
                    messages.push(format!("{}: {error:#}", file_name(&path)));
                    continue;
                }
            };
            let files = if self.kind.is_none() {
                &mut menu_selection
            } else {
                &mut self.files
            };
            match files.add_file(kind, &path) {
                Ok(true) => {
                    if self.kind.is_none() {
                        // Only an accessible, supported file can choose the category.
                        self.enter(kind);
                        std::mem::swap(&mut self.files, &mut menu_selection);
                    }
                }
                Ok(false) => duplicates += 1,
                Err(error) => messages.push(format!("{}: {error:#}", file_name(&path))),
            }
        }
        if duplicates > 0 {
            messages.push(format!("{duplicates} duplicate file(s) ignored."));
        }
        self.messages = messages;
    }

    fn render_messages(&self, ui: &mut egui::Ui) {
        if !self.messages.is_empty() {
            egui::ScrollArea::vertical()
                .id_salt("messages")
                .max_height(90.0)
                .show(ui, |ui| {
                    for message in &self.messages {
                        ui.colored_label(egui::Color32::YELLOW, message);
                    }
                });
        }
    }

    fn handle_drop(&mut self, dropped: Vec<egui::DroppedFile>) {
        if dropped.is_empty() {
            return;
        }
        let paths: Vec<_> = dropped
            .iter()
            .filter_map(|file| file.path.clone())
            .collect();
        let missing_paths = dropped.len() - paths.len();
        if !paths.is_empty() {
            self.add_paths(paths);
        }
        if missing_paths > 0 {
            self.messages
                .push("Only files with a local path can be added.".into());
        }
    }

    fn render_menu(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(35.0);
            ui.heading(
                egui::RichText::new("FILE CONVERTER")
                    .size(40.0)
                    .strong()
                    .color(egui::Color32::from_rgb(100, 200, 255)),
            );
            ui.label(
                egui::RichText::new("By pixe71")
                    .size(14.0)
                    .color(egui::Color32::from_gray(180)),
            );
            ui.add_space(25.0);
            let cards = [
                (
                    ConversionKind::Images,
                    "IMAGES",
                    "🖼",
                    "Convert JPG, PNG, WEBP...",
                ),
                (
                    ConversionKind::Documents,
                    "DOCUMENTS",
                    "📄",
                    "DOCX / TXT → PDF, DOCX, TXT",
                ),
                (
                    ConversionKind::Audio,
                    "AUDIO",
                    "🎵",
                    "Convert MP3, WAV, FLAC...",
                ),
                (
                    ConversionKind::Video,
                    "VIDEO",
                    "🎬",
                    "Convert MP4, MKV, AVI...",
                ),
            ];
            if self.jobs.is_active() && ui.button("Back to the active conversion").clicked() {
                self.kind = self.job_kind;
            }
            ui.add_enabled_ui(!self.jobs.is_active(), |ui| {
                for (kind, title, icon, description) in cards {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(300.0, 80.0), egui::Sense::click());
                    let visuals = ui.style().interact(&response);
                    ui.painter().rect_filled(rect, 10.0, visuals.bg_fill);
                    ui.painter().rect_stroke(rect, 10.0, visuals.bg_stroke);
                    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(rect), |ui| {
                        ui.horizontal_centered(|ui| {
                            ui.add_space(20.0);
                            ui.label(egui::RichText::new(icon).size(30.0));
                            ui.add_space(20.0);
                            ui.vertical(|ui| {
                                ui.add_space(15.0);
                                ui.label(egui::RichText::new(title).size(18.0).strong());
                                ui.label(
                                    egui::RichText::new(description)
                                        .size(12.0)
                                        .color(egui::Color32::GRAY),
                                );
                            });
                        });
                    });
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), title)
                    });
                    if response.on_hover_text(description).clicked() {
                        self.enter(kind);
                    }
                    ui.add_space(10.0);
                }
            });
            ui.label("Drop files here, or choose a converter.");
            ui.label(
                egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                    .size(10.0)
                    .color(egui::Color32::from_gray(140)),
            );
        });
        self.render_tool_diagnostics(ui);
        self.render_messages(ui);
        self.render_results(ui);
    }

    fn render_tool_diagnostics(&mut self, ui: &mut egui::Ui) {
        egui::CollapsingHeader::new("Conversion engines").show(ui, |ui| {
            if ui
                .add_enabled(
                    self.tool_check.is_none()
                        && self.engine_worker.is_none()
                        && !self.jobs.is_active(),
                    egui::Button::new("Check engines"),
                )
                .clicked()
            {
                self.check_engines(ui.ctx());
            }
            if ui
                .add_enabled(
                    self.engine_worker.is_none()
                        && self.tool_check.is_none()
                        && !self.jobs.is_active()
                        && cfg!(all(windows, target_arch = "x86_64")),
                    egui::Button::new("Repair engines"),
                )
                .clicked()
            {
                self.repair_requested = true;
                self.setup_prompt = true;
                self.tool_check_error = None;
            }
            if let Ok(manifest) = EngineManifest::pinned() {
                for engine in manifest.tools {
                    ui.small(format!(
                        "{} {} — {}",
                        engine.name, engine.version, engine.license
                    ));
                    ui.hyperlink_to("Official source", engine.source_url);
                }
            }
            if self.tool_check.is_some() {
                ui.spinner();
            }
            if let Some(error) = &self.tool_check_error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
            egui::ScrollArea::vertical()
                .id_salt("engine_diagnostics")
                .max_height(180.0)
                .show(ui, |ui| {
                    for diagnostic in &self.tool_diagnostics {
                        match &diagnostic.result {
                            Ok(tool) => {
                                ui.label(format!(
                                    "{}: available ({:?})",
                                    diagnostic.tool.name(),
                                    tool.origin
                                ));
                                ui.add(
                                    egui::Label::new(tool.path.display().to_string())
                                        .wrap()
                                        .selectable(true),
                                );
                            }
                            Err(error) => {
                                ui.colored_label(
                                    egui::Color32::LIGHT_RED,
                                    format!("{}: unavailable\n{error}", diagnostic.tool.name()),
                                );
                            }
                        }
                    }
                });
        });
    }

    fn render_converter(&mut self, ui: &mut egui::Ui, kind: ConversionKind) {
        egui::SidePanel::left("left_panel")
            .resizable(false)
            .default_width(185.0)
            .show_inside(ui, |ui| {
                ui.add_space(20.0);
                if ui
                    .add(egui::Button::new("⬅ MENU").min_size(egui::vec2(150.0, 40.0)))
                    .clicked()
                {
                    // Navigation does not change the job, its queue or its results.
                    self.kind = None;
                }
                ui.add_space(20.0);
                ui.separator();
                ui.heading("Settings");
                ui.add_enabled_ui(!self.jobs.is_active(), |ui| {
                    ui.label("Format:");
                    egui::ComboBox::from_id_salt("format")
                        .selected_text(&self.options.format)
                        .show_ui(ui, |ui| {
                            for format in kind.output_formats() {
                                ui.selectable_value(
                                    &mut self.options.format,
                                    (*format).into(),
                                    *format,
                                );
                            }
                        });
                    if kind == ConversionKind::Audio {
                        if ["wav", "flac"].contains(&self.options.format.as_str()) {
                            ui.small("Lossless output: bitrate setting does not apply.");
                        } else if self.options.format == "ogg" {
                            ui.small("Vorbis VBR, quality 5. Requires libvorbis.");
                        } else {
                            ui.label("Bitrate:");
                            egui::ComboBox::from_id_salt("bitrate")
                                .selected_text(&self.options.bitrate)
                                .show_ui(ui, |ui| {
                                    for bitrate in ["64k", "128k", "192k", "256k", "320k"] {
                                        ui.selectable_value(
                                            &mut self.options.bitrate,
                                            bitrate.into(),
                                            bitrate,
                                        );
                                    }
                                });
                        }
                    }
                    if kind == ConversionKind::Video {
                        ui.label("Video codec:");
                        egui::ComboBox::from_id_salt("codec")
                            .selected_text(&self.options.codec)
                            .show_ui(ui, |ui| {
                                for codec in ["libx264", "libx265", "copy"] {
                                    ui.selectable_value(
                                        &mut self.options.codec,
                                        codec.into(),
                                        codec,
                                    );
                                }
                            });
                        if self.options.codec == "copy" {
                            ui.small("Copies video only. Audio follows FFmpeg defaults.");
                        }
                    }
                    if kind == ConversionKind::Documents {
                        ui.small("Input: DOCX or UTF-8 TXT (Markdown).");
                        ui.small("DOCX/TXT: Pandoc engine.");
                        ui.small("PDF output: Pandoc + Typst.");
                        ui.small("PDF input is not supported.");
                    }
                    if kind == ConversionKind::Images {
                        ui.small("Static images. Animation and metadata are not preserved.");
                        if self.options.format == "ico" {
                            ui.small("Fits the image on a transparent 256 × 256 canvas.");
                        }
                    }
                });
            });
        egui::CentralPanel::default().show_inside(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("converter_scroll")
                .show(ui, |ui| {
                    ui.heading(
                        egui::RichText::new(format!("{} Converter", kind.label()))
                            .size(28.0)
                            .strong()
                            .color(egui::Color32::from_rgb(100, 200, 255)),
                    );
                    ui.add_space(15.0);
                    egui::Frame::group(ui.style())
                        .rounding(8.0)
                        .inner_margin(12.0)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.strong("Selected Files");
                                ui.add_enabled_ui(!self.jobs.is_active(), |ui| {
                                    if ui.button("➕ Add Files").clicked() {
                                        if let Some(paths) = FileDialog::new()
                                            .add_filter(kind.label(), kind.input_extensions())
                                            .pick_files()
                                        {
                                            self.add_paths(paths);
                                        }
                                    }
                                    if ui
                                        .add_enabled(
                                            !self.files.is_empty(),
                                            egui::Button::new("Clear All"),
                                        )
                                        .clicked()
                                    {
                                        self.files.clear();
                                    }
                                });
                            });
                            ui.small("Drop files into this window to add them. Content is checked during conversion.");
                            egui::ScrollArea::vertical()
                                .id_salt("files")
                                .max_height(170.0)
                                .show(ui, |ui| {
                                    if self.files.is_empty() {
                                        ui.label("No files selected");
                                    }
                                    let mut remove = None;
                                    for (index, path) in self.files.iter().enumerate() {
                                        ui.horizontal(|ui| {
                                            if ui
                                                .add_enabled(
                                                    !self.jobs.is_active(),
                                                    egui::Button::new("×"),
                                                )
                                                .on_hover_text("Remove file")
                                                .clicked()
                                            {
                                                remove = Some(index);
                                            }
                                            ui.add(egui::Label::new(file_name(path)).truncate())
                                                .on_hover_text(path.display().to_string());
                                        });
                                    }
                                    if let Some(index) = remove {
                                        self.files.remove(index);
                                    }
                                });
                        });
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.strong("Destination");
                        if ui
                            .add_enabled(
                                !self.jobs.is_active(),
                                egui::Button::new("Choose folder…"),
                            )
                            .clicked()
                        {
                            if let Some(folder) = FileDialog::new().pick_folder() {
                                self.destination = Some(folder);
                            }
                        }
                    });
                    if let Some(folder) = &self.destination {
                        ui.label(folder.display().to_string());
                    } else {
                        ui.label("Choose an output folder before starting.");
                    }
                    ui.small(
                "Existing files are kept. Collisions are renamed: name (1).ext, name (2).ext…",
            );
                    self.render_messages(ui);
                    ui.add_space(10.0);
                    if self.jobs.is_active() {
                        ui.spinner();
                        let completed = self.jobs.state.results.len();
                        let total = self.jobs.state.sources.len();
                        ui.add(
                            egui::ProgressBar::new(completed as f32 / total.max(1) as f32)
                                .text(format!("{completed} / {total} files completed")),
                        );
                        if let Some(path) = &self.jobs.state.current {
                            ui.label(format!("Converting: {}", file_name(path)));
                        }
                    } else if ui
                        .add_enabled(
                            !self.files.is_empty() && self.destination.is_some()
                                && (kind == ConversionKind::Images || self.engine_worker.is_none()),
                            egui::Button::new(
                                egui::RichText::new("🚀 START CONVERSION")
                                    .size(20.0)
                                    .strong(),
                            )
                            .fill(egui::Color32::from_rgb(0, 120, 215))
                            .min_size(egui::vec2(250.0, 50.0)),
                        )
                        .clicked()
                    {
                        if let Some(destination) = self.destination.clone() {
                            let ctx = ui.ctx().clone();
                            match self.jobs.start(
                                kind,
                                self.files.to_paths(),
                                destination,
                                self.options.clone(),
                                move || ctx.request_repaint(),
                            ) {
                                Ok(_) => {
                                    self.job_kind = Some(kind);
                                    self.messages.clear();
                                }
                                Err(error) => self.messages = vec![format!("{error:#}")],
                            }
                        }
                    }
                    self.render_results(ui);
                });
        });
    }

    fn render_results(&self, ui: &mut egui::Ui) {
        if let Some(id) = self.jobs.state.id {
            ui.separator();
            let success = self
                .jobs
                .state
                .results
                .iter()
                .filter(|result| result.status == ConversionResultStatus::Success)
                .count();
            ui.strong(format!(
                "Batch #{} — {} success, {} failures{}",
                id.0,
                success,
                self.jobs.state.results.len() - success,
                if self.jobs.is_active() {
                    " (running)"
                } else {
                    ""
                }
            ));
            if let Some(error) = &self.jobs.state.error {
                ui.colored_label(egui::Color32::RED, error);
            }
            for (index, result) in self.jobs.state.results.iter().enumerate() {
                let (symbol, color, target) = match result.status {
                    ConversionResultStatus::Success => (
                        "✓",
                        egui::Color32::LIGHT_GREEN,
                        result
                            .destination
                            .as_deref()
                            .map(file_name)
                            .unwrap_or_else(|| "Output unavailable".into()),
                    ),
                    ConversionResultStatus::Failed => (
                        "✗",
                        egui::Color32::LIGHT_RED,
                        result
                            .message
                            .as_deref()
                            .or(result.error.as_deref())
                            .unwrap_or("Conversion failed")
                            .lines()
                            .next()
                            .unwrap_or("Conversion failed")
                            .to_owned(),
                    ),
                };
                egui::CollapsingHeader::new(
                    egui::RichText::new(format!(
                        "{symbol} {} → {target}",
                        file_name(&result.source)
                    ))
                    .color(color),
                )
                .id_salt(("result", id.0, index))
                .show(ui, |ui| {
                    ui.label(format!("Source: {}", result.source.display()));
                    if let Some(path) = &result.destination {
                        ui.label(format!("Output: {}", path.display()));
                        if ui.button("Copy output path").clicked() {
                            ui.ctx().copy_text(path.display().to_string());
                        }
                    }
                    if let Some(error) = &result.error {
                        render_error_details(
                            ui,
                            error,
                            egui::Id::new(("error_details", id.0, index)),
                        );
                        if ui.button("Copy error details").clicked() {
                            ui.ctx().copy_text(error.clone());
                        }
                    }
                });
            }
        }
    }
}

impl eframe::App for ConverterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let mut finished = None;
        if let Some(receiver) = &self.engine_worker {
            loop {
                match receiver.try_recv() {
                    Ok(EngineEvent::Progress(progress)) => {
                        if let Some(existing) = self
                            .engine_progress
                            .iter_mut()
                            .find(|existing| existing.engine == progress.engine)
                        {
                            *existing = progress;
                        } else {
                            self.engine_progress.push(progress);
                        }
                    }
                    Ok(EngineEvent::Finished(result)) => {
                        finished = Some(result);
                        break;
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        finished = Some(Err(
                            "Engine preparation stopped unexpectedly. Retry to repair it.".into(),
                        ));
                        break;
                    }
                }
            }
        }
        if let Some(result) = finished {
            self.engine_worker = None;
            match result {
                Ok(diagnostics) => {
                    self.tool_diagnostics = diagnostics;
                    self.setup_prompt = false;
                    self.tool_check_error = None;
                    self.repair_requested = false;
                    self.messages = vec![
                        "Everything is ready. Conversion engines are available offline.".into(),
                    ];
                }
                Err(error) => self.tool_check_error = Some(error),
            }
        }
        if self.engine_worker.is_some() {
            ctx.request_repaint_after(Duration::from_millis(150));
            if ctx.input(|input| input.viewport().close_requested()) {
                self.engine_cancel.store(true, Ordering::Relaxed);
                self.close_after_setup = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            }
        } else if self.close_after_setup {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        if let Some(receiver) = &self.tool_check {
            match receiver.try_recv() {
                Ok(Ok(diagnostics)) => {
                    if !self.setup_dismissed
                        && diagnostics
                            .iter()
                            .any(|diagnostic| diagnostic.result.is_err())
                        && cfg!(all(windows, target_arch = "x86_64"))
                    {
                        self.setup_prompt = true;
                    }
                    self.tool_diagnostics = diagnostics;
                    self.tool_check = None;
                }
                Ok(Err(error)) => {
                    self.tool_check_error = Some(error);
                    self.tool_check = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.tool_check_error = Some("Engine diagnostics stopped unexpectedly".into());
                    self.tool_check = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        self.jobs.poll();
        if self.jobs.is_active() {
            ctx.request_repaint_after(Duration::from_millis(150));
            // Avoid abandoning a normal conversion when the window is closed.
            if ctx.input(|input| input.viewport().close_requested()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.messages =
                    vec!["A conversion is running. Wait for completion before closing.".into()];
            }
        }
        self.handle_drop(ctx.input(|input| input.raw.dropped_files.clone()));
        egui::CentralPanel::default().show(ctx, |ui| match self.kind {
            None => {
                egui::ScrollArea::vertical()
                    .id_salt("menu_scroll")
                    .show(ui, |ui| self.render_menu(ui));
            }
            Some(kind) => self.render_converter(ui, kind),
        });
        self.render_engine_setup(ctx);
    }
}

impl Drop for ConverterApp {
    fn drop(&mut self) {
        self.engine_cancel.store(true, Ordering::Relaxed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drop_paths(app: &mut ConverterApp, paths: Vec<PathBuf>) {
        app.handle_drop(
            paths
                .into_iter()
                .map(|path| egui::DroppedFile {
                    path: Some(path),
                    ..Default::default()
                })
                .collect(),
        );
    }

    #[test]
    fn menu_drop_accepts_valid_images_independently_of_invalid_file_order() -> anyhow::Result<()> {
        let folder = tempfile::tempdir()?;
        let image = folder.path().canonicalize()?.join("photo.png");
        let pdf = folder.path().join("document.pdf");
        let missing_audio = folder.path().join("missing.mp3");
        let directory = folder.path().join("directory.wav");
        std::fs::write(&image, b"fixture")?;
        std::fs::write(&pdf, b"%PDF")?;
        std::fs::create_dir(&directory)?;
        for invalid in [pdf, missing_audio, directory] {
            for paths in [
                vec![invalid.clone(), image.clone()],
                vec![image.clone(), invalid.clone()],
            ] {
                let mut app = ConverterApp::default();
                drop_paths(&mut app, paths);
                assert_eq!(app.kind, Some(ConversionKind::Images));
                assert_eq!(app.files.to_paths(), vec![image.clone()]);
                assert_eq!(app.messages.len(), 1);
                assert!(app.messages[0].contains(&file_name(&invalid)));
            }
        }
        Ok(())
    }

    #[test]
    fn menu_drop_keeps_first_admissible_category_and_reports_each_refusal() -> anyhow::Result<()> {
        let folder = tempfile::tempdir()?;
        let directory = folder.path().canonicalize()?;
        let images = [directory.join("one.png"), directory.join("two.jpg")];
        let audio = directory.join("music.wav");
        let document = folder.path().join("document.txt");
        let pdf = folder.path().join("document.pdf");
        for path in images.iter().chain([&audio, &document, &pdf]) {
            std::fs::write(path, b"fixture")?;
        }
        let mut app = ConverterApp::default();
        drop_paths(
            &mut app,
            vec![
                pdf.clone(),
                images[0].clone(),
                audio.clone(),
                images[1].clone(),
                document.clone(),
            ],
        );
        assert_eq!(app.kind, Some(ConversionKind::Images));
        assert_eq!(app.files.to_paths(), images.to_vec());
        assert_eq!(app.messages.len(), 3);
        for refused in [&pdf, &audio, &document] {
            assert!(app
                .messages
                .iter()
                .any(|message| message.contains(&file_name(refused))));
        }
        let mut app = ConverterApp::default();
        drop_paths(
            &mut app,
            vec![pdf, audio.clone(), images[0].clone(), document],
        );
        assert_eq!(app.kind, Some(ConversionKind::Audio));
        assert_eq!(app.files.to_paths(), vec![audio]);
        assert_eq!(app.messages.len(), 3);
        Ok(())
    }

    #[test]
    fn invalid_menu_drop_does_not_enter_a_converter() -> anyhow::Result<()> {
        let folder = tempfile::tempdir()?;
        let mut app = ConverterApp::default();
        drop_paths(
            &mut app,
            vec![
                folder.path().join("missing.png"),
                folder.path().join("input.pdf"),
            ],
        );
        assert_eq!(app.kind, None);
        assert!(app.files.is_empty());
        assert_eq!(app.messages.len(), 2);
        Ok(())
    }

    #[test]
    fn dropped_and_selected_files_share_validation_and_deduplication() -> anyhow::Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("sample.png");
        std::fs::write(&source, b"fixture")?;
        let mut app = ConverterApp::default();
        app.handle_drop(vec![egui::DroppedFile {
            path: Some(source.clone()),
            ..Default::default()
        }]);
        assert_eq!(app.kind, Some(ConversionKind::Images));
        assert_eq!(app.files.len(), 1);
        app.add_paths(vec![source]);
        assert_eq!(app.files.len(), 1);
        assert!(app.messages[0].contains("duplicate"));
        let invalid = folder.path().join("document.pdf");
        app.handle_drop(vec![egui::DroppedFile {
            path: Some(invalid),
            ..Default::default()
        }]);
        assert_eq!(app.files.len(), 1);
        assert!(!app.messages.is_empty());
        Ok(())
    }

    #[test]
    fn virtual_drop_warning_is_not_lost_when_local_files_are_added() -> anyhow::Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("sample.png");
        std::fs::write(&source, b"fixture")?;
        let mut app = ConverterApp::default();
        app.handle_drop(vec![
            egui::DroppedFile {
                path: Some(source),
                ..Default::default()
            },
            egui::DroppedFile::default(),
        ]);
        assert_eq!(app.files.len(), 1);
        assert!(app
            .messages
            .iter()
            .any(|message| message.contains("local path")));
        Ok(())
    }

    #[test]
    fn large_error_details_have_a_bounded_viewport() {
        let ctx = egui::Context::default();
        let details = "FFmpeg diagnostic line\n".repeat(2_500);
        let mut viewport = None;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(620.0, 500.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let output = render_error_details(ui, &details, egui::Id::new("test_error"));
                    assert!(output.content_size.y > 160.0);
                    viewport = Some(output.inner_rect);
                });
            },
        );
        assert!(viewport.unwrap().height() <= 160.0);
        assert_eq!(details.lines().count(), 2_500);
    }

    #[test]
    fn converter_and_results_render_without_a_native_window() -> anyhow::Result<()> {
        use file_converter::job::{ConversionResult, JobEvent, JobEventKind, JobId};
        let folder = tempfile::tempdir()?;
        let source = folder.path().canonicalize()?.join("photo.v1.png");
        std::fs::write(&source, b"fixture")?;
        let mut app = ConverterApp::default();
        app.enter(ConversionKind::Images);
        app.add_paths(vec![source.clone()]);
        app.destination = Some("output".into());
        app.jobs.state.begin(JobId(1), app.files.to_paths())?;
        app.jobs.state.apply(JobEvent {
            id: JobId(1),
            kind: JobEventKind::FileFinished(ConversionResult {
                job_id: JobId(1),
                source,
                destination: None,
                status: ConversionResultStatus::Failed,
                message: Some("Failed to decode image".into()),
                error: Some("Failed to decode image: corrupted input\n".repeat(1_500)),
            }),
        });
        app.jobs.state.apply(JobEvent {
            id: JobId(1),
            kind: JobEventKind::Finished(None),
        });
        let ctx = egui::Context::default();
        for size in [egui::vec2(620.0, 500.0), egui::vec2(700.0, 650.0)] {
            let output = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| app.render_converter(ui, ConversionKind::Images));
                },
            );
            assert!(!output.shapes.is_empty());
            for primitive in ctx.tessellate(output.shapes, output.pixels_per_point) {
                if let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive {
                    assert!(mesh
                        .vertices
                        .iter()
                        .all(|vertex| vertex.pos.x.is_finite() && vertex.pos.y.is_finite()));
                }
            }
        }
        Ok(())
    }
}
