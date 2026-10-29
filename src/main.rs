#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod gui;

use eframe::egui;

fn main() -> eframe::Result<()> {
    let title = format!("Rust File Converter {}", env!("CARGO_PKG_VERSION"));
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([700.0, 650.0])
        .with_min_inner_size([620.0, 500.0])
        .with_title(&title);
    if let Ok(icon) = image::load_from_memory(include_bytes!("../icon.png")) {
        let icon = icon.into_rgba8();
        let (width, height) = icon.dimensions();
        viewport = viewport.with_icon(egui::IconData {
            rgba: icon.into_raw(),
            width,
            height,
        });
    }
    eframe::run_native(
        &title,
        eframe::NativeOptions {
            viewport,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(gui::ConverterApp::new(cc)))),
    )
}
