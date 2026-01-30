use std::path::PathBuf;
use image::imageops::FilterType;
use anyhow::{Context, Result};
use super::ConversionOptions;

pub fn convert_image(input_path: &PathBuf, output_folder: &PathBuf, options: &ConversionOptions) -> Result<PathBuf> {
    let mut img = image::open(input_path)
        .with_context(|| format!("Failed to open image: {:?}", input_path))?;

    if let (Some(w), Some(h)) = (options.width, options.height) {
        img = img.resize_exact(w, h, FilterType::Lanczos3);
    } else if let Some(w) = options.width {
        let ratio = w as f64 / img.width() as f64;
        let h = (img.height() as f64 * ratio) as u32;
        img = img.resize(w, h, FilterType::Lanczos3);
    } else if let Some(h) = options.height {
        let ratio = h as f64 / img.height() as f64;
        let w = (img.width() as f64 * ratio) as u32;
        img = img.resize(w, h, FilterType::Lanczos3);
    }

    let file_stem = input_path.file_stem().unwrap_or_default();
    let mut output_filename = PathBuf::from(file_stem);
    output_filename.set_extension(&options.format);
    let output_path = output_folder.join(output_filename);

    img.save(&output_path)
        .with_context(|| format!("Failed to save image to: {:?}", output_path))?;

    Ok(output_path)
}
