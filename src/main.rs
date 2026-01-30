#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod converters;
mod gui;

use eframe::egui;

fn load_icon() -> egui::IconData {
	
	let (icon_rgba, icon_width, icon_height) = {
		let image = image::load_from_memory(include_bytes!("../icon.png"))
			.expect("Failed to open icon path")
			.into_rgba8();
		let (width, height) = image.dimensions();
		let rgba = image.into_raw();
		(rgba, width, height)
	};

	egui::IconData {
		rgba: icon_rgba,
		width: icon_width,
		height: icon_height,
	}
}

fn main() -> eframe::Result<()> {
    println!("Starting File Converter...");

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([700.0, 650.0])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_title("File Converter")
            .with_icon(load_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "File Converter",
        native_options,
        Box::new(|cc| Ok(Box::new(gui::ConverterApp::new(cc)))),
    )
}
