#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod converters;
mod gui;

use eframe::egui;

fn main() -> eframe::Result<()> {
    println!("Starting Universal Converter...");

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([700.0, 650.0])
            .with_min_inner_size([600.0, 500.0])
            .with_title("🔄 Universal Converter Pro (Rust)"),
        ..Default::default()
    };

    eframe::run_native(
        "Universal Converter",
        native_options,
        Box::new(|cc| Ok(Box::new(gui::ConverterApp::new(cc)))),
    )
}
