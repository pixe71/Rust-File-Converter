//! Explicit opt-in tests. Missing tools fail these tests rather than masquerading
//! as successful conversions. Ordinary cargo test does not execute them.
use anyhow::{bail, Context, Result};
use file_converter::converters::{convert_file, ConversionKind, ConversionOptions};
use file_converter::output::OutputPlanner;
use file_converter::tools::{Tool, ToolResolver};
use std::path::{Path, PathBuf};
use std::process::Command;

fn tool_command(tool: Tool) -> Result<Command> {
    Ok(Command::new(
        ToolResolver::from_environment()?.resolve(tool)?.path,
    ))
}

fn execute(command: &mut Command) -> Result<()> {
    let output = command
        .output()
        .context("Required external test tool could not be started")?;
    if !output.status.success() {
        bail!(
            "External fixture/validation failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn convert(
    kind: ConversionKind,
    source: &Path,
    destination: &Path,
    format: &str,
) -> Result<PathBuf> {
    let mut planner = OutputPlanner::new(destination, &[source.into()])?;
    convert_file(
        kind,
        source,
        &mut planner,
        &ConversionOptions {
            format: format.into(),
            ..Default::default()
        },
    )
}

#[test]
#[ignore = "requires FFmpeg in tools/, RFC_TOOLS_DIR or PATH"]
fn audio_wav_to_flac() -> Result<()> {
    let folder = tempfile::tempdir()?;
    let source = folder.path().join("tone.wav");
    execute(
        tool_command(Tool::Ffmpeg)?
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=0.2",
            ])
            .arg(&source),
    )?;
    let output = convert(ConversionKind::Audio, &source, folder.path(), "flac")?;
    execute(
        tool_command(Tool::Ffmpeg)?
            .args(["-nostdin", "-v", "error", "-i"])
            .arg(output)
            .args(["-f", "null", "-"]),
    )?;
    assert!(source.exists());
    Ok(())
}

#[test]
#[ignore = "requires FFmpeg with libvorbis in tools/, RFC_TOOLS_DIR or PATH"]
fn audio_wav_to_ogg() -> Result<()> {
    let folder = tempfile::tempdir()?;
    for (channels, sample_rate) in [(1, 8_000), (1, 44_100), (1, 48_000), (2, 44_100)] {
        let source = folder
            .path()
            .join(format!("tone-{channels}-{sample_rate}.wav"));
        execute(
            tool_command(Tool::Ffmpeg)?
                .args(["-nostdin", "-v", "error", "-f", "lavfi", "-i"])
                .arg(format!(
                    "sine=frequency=440:duration=0.2:sample_rate={sample_rate}"
                ))
                .arg("-ac")
                .arg(channels.to_string())
                .arg(&source),
        )?;
        let output = convert(ConversionKind::Audio, &source, folder.path(), "ogg")?;
        let bytes = std::fs::read(&output)?;
        assert!(bytes.starts_with(b"OggS"));
        assert!(bytes.windows(7).any(|bytes| bytes == b"\x01vorbis"));
        execute(
            tool_command(Tool::Ffmpeg)?
                .args(["-nostdin", "-v", "error", "-i"])
                .arg(&output)
                .args(["-f", "null", "-"]),
        )?;
        assert!(source.exists());
    }
    assert_eq!(std::fs::read_dir(folder.path())?.count(), 8);
    Ok(())
}

#[test]
#[ignore = "requires FFmpeg with libx264 in tools/, RFC_TOOLS_DIR or PATH"]
fn video_standard_conversion() -> Result<()> {
    let folder = tempfile::tempdir()?;
    let source = folder.path().join("clip.mkv");
    execute(
        tool_command(Tool::Ffmpeg)?
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=blue:s=64x48:r=10:d=0.2",
                "-c:v",
                "ffv1",
            ])
            .arg(&source),
    )?;
    let output = convert(ConversionKind::Video, &source, folder.path(), "mp4")?;
    execute(
        tool_command(Tool::Ffmpeg)?
            .args(["-nostdin", "-v", "error", "-i"])
            .arg(output)
            .args(["-f", "null", "-"]),
    )?;
    Ok(())
}

#[test]
#[ignore = "requires Pandoc in tools/, RFC_TOOLS_DIR or PATH"]
fn txt_docx_plain_txt() -> Result<()> {
    let folder = tempfile::tempdir()?;
    let source = folder.path().join("document.txt");
    std::fs::write(
        &source,
        "# Bonjour\n\nUn texte **important** avec des accents : été.\n",
    )?;
    let docx = convert(ConversionKind::Documents, &source, folder.path(), "docx")?;
    assert!(std::fs::read(&docx)?.starts_with(b"PK\x03\x04"));
    // Independently read the generated DOCX with Pandoc before the app's TXT conversion.
    let reference = folder.path().join("reference.txt");
    execute(
        tool_command(Tool::Pandoc)?
            .args(["--from", "docx", "--to", "plain", "--wrap=none", "--output"])
            .arg(&reference)
            .arg(&docx),
    )?;
    let txt = convert(ConversionKind::Documents, &docx, folder.path(), "txt")?;
    let plain = std::fs::read_to_string(txt)?;
    assert_eq!(plain, std::fs::read_to_string(&reference)?);
    assert!(plain.contains("Bonjour"));
    assert!(plain.contains("important"));
    assert!(plain.contains("été"));
    assert!(!plain.contains("**") && !plain.contains("# Bonjour"));
    assert_eq!(std::fs::read_dir(folder.path())?.count(), 4);
    Ok(())
}

#[test]
#[ignore = "requires resolved Pandoc and Typst"]
fn document_pdf_output() -> Result<()> {
    let folder = tempfile::tempdir()?;
    let source = folder.path().join("document.txt");
    std::fs::write(&source, "A short document.\n")?;
    let pdf = convert(ConversionKind::Documents, &source, folder.path(), "pdf")?;
    assert!(std::fs::read(pdf)?.starts_with(b"%PDF"));
    Ok(())
}
