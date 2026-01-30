#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod converters;
mod gui;

use eframe::egui;

fn load_icon() -> egui::IconData {
	// ℹ️ Pour mettre votre propre icône :
	// 1. Placez votre image (ex: app_icon.png) à côté du dossier src (à la racine)
	// 2. Décommentez le bloc ci-dessous et commentez la partie "Icône par défaut"
	
	let (icon_rgba, icon_width, icon_height) = {
		let image = image::load_from_memory(include_bytes!("../icon.png"))
			.expect("Failed to open icon path")
			.into_rgba8();
		let (width, height) = image.dimensions();
		let rgba = image.into_raw();
		(rgba, width, height)
	};

	/*
	// Icône par défaut (carré bleu) pour que ça compile sans fichier
	let (icon_rgba, icon_width, icon_height) = {
		let width = 32;
		let height = 32;
		let mut rgba = Vec::with_capacity((width * height * 4) as usize);
		for _ in 0..height {
			for _ in 0..width {
				rgba.extend_from_slice(&[72, 126, 176, 255]); // Couleur du thème
			}
		}
		(rgba, width, height)
	};
	*/

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
