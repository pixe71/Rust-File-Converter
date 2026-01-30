use std::path::PathBuf;
use std::process::Command;
use anyhow::{Result, bail, Context};
use super::ConversionOptions;

pub fn is_ffmpeg_available() -> bool {
    Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn convert_video(input_path: &PathBuf, output_folder: &PathBuf, options: &ConversionOptions) -> Result<PathBuf> {
    if !is_ffmpeg_available() {
        bail!("FFmpeg not found. Please install FFmpeg and add it to your PATH.");
    }

    let file_stem = input_path.file_stem().unwrap_or_default();
    let mut output_filename = PathBuf::from(file_stem);
    output_filename.set_extension(&options.format);
    let output_path = output_folder.join(output_filename);

    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
       .arg("-i").arg(input_path);
    
    if let (Some(w), Some(h)) = (options.width, options.height) {
        cmd.arg("-vf").arg(format!("scale={}:{}", w, h));
    }

    if options.codec != "copy" {
         cmd.arg("-c:v").arg(&options.codec);
    } else {
        cmd.arg("-c:v").arg("copy");
    }

    cmd.arg(&output_path);

    let output = cmd.output().context("Failed to execute ffmpeg")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("FFmpeg error: {}", stderr);
    }

    Ok(output_path)
}
