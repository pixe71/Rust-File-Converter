use std::path::PathBuf;
use std::process::Command;
use anyhow::{Result, bail, Context};
use super::ConversionOptions;

pub fn is_pandoc_available() -> bool {
    Command::new("pandoc")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn convert_doc(input_path: &PathBuf, output_folder: &PathBuf, options: &ConversionOptions) -> Result<PathBuf> {
    if !is_pandoc_available() {
        bail!("Pandoc not found. Please install Pandoc (https://pandoc.org/) and add it to your PATH.");
    }

    let file_stem = input_path.file_stem().unwrap_or_default();
    let mut output_filename = PathBuf::from(file_stem);
    output_filename.set_extension(&options.format);
    let output_path = output_folder.join(output_filename);

    let mut cmd = Command::new("pandoc");
    cmd.arg(input_path)
       .arg("-o")
       .arg(&output_path);

    // Basic options mapping
    if options.format == "pdf" {
        // PDF requires a PDF engine usually, pandoc defaults work well often but explicit is safer if needed.
        // For now we rely on default pdflatex or wkhtmltopdf if installed.
    }

    let output = cmd.output().context("Failed to execute pandoc")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("Pandoc conversion failed: {}", stderr);
    }

    Ok(output_path)
}
