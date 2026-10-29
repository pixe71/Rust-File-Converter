use super::{process, ConversionOptions};
use crate::tools::{Tool, ToolResolver};
use anyhow::Result;
use std::path::Path;
use std::process::Command;

pub(super) fn convert_video(
    input: &Path,
    output: &Path,
    options: &ConversionOptions,
) -> Result<()> {
    let ffmpeg = ToolResolver::from_environment()?.resolve(Tool::Ffmpeg)?;
    let mut command = Command::new(&ffmpeg.path);
    command
        .args(["-nostdin", "-hide_banner", "-loglevel", "warning", "-y"])
        .arg("-i")
        .arg(input);
    if options.width.is_some() || options.height.is_some() {
        let width = options
            .width
            .map_or_else(|| "-2".into(), |value| value.to_string());
        let height = options
            .height
            .map_or_else(|| "-2".into(), |value| value.to_string());
        command.arg("-vf").arg(format!("scale={width}:{height}"));
    }
    command.arg("-c:v").arg(&options.codec).arg(output);
    process::run_with_details(
        &mut command,
        "FFmpeg",
        &format!(
            "FFmpeg executable: {}\nRequested output: {}; video codec: {}",
            ffmpeg.path.display(),
            options.format,
            options.codec
        ),
    )
}
