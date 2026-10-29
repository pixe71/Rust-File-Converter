use anyhow::{bail, Result};
use file_converter::converters::{convert_file, ConversionKind, ConversionOptions};
use file_converter::output::OutputPlanner;
use file_converter::tools::{Tool, ToolResolver};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

fn execute(command: &mut Command) -> Result<()> {
    let output = command.output()?;
    if !output.status.success() {
        bail!(
            "External test command failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

fn convert(kind: ConversionKind, input: &Path, directory: &Path, format: &str) -> Result<PathBuf> {
    let mut planner = OutputPlanner::new(directory, &[input.into()])?;
    convert_file(
        kind,
        input,
        &mut planner,
        &ConversionOptions {
            format: format.into(),
            ..Default::default()
        },
    )
}

pub fn verify_conversions(resolver: &ToolResolver) -> Result<()> {
    let folder = tempfile::tempdir()?;
    let png = folder.path().join("native.png");
    image::RgbImage::new(900, 600).save(&png)?;
    let jpg = convert(ConversionKind::Images, &png, folder.path(), "jpg")?;
    let roundtrip = convert(ConversionKind::Images, &jpg, folder.path(), "png")?;
    assert_eq!(image::image_dimensions(roundtrip)?, (900, 600));
    let ico = convert(ConversionKind::Images, &png, folder.path(), "ico")?;
    assert_eq!(image::image_dimensions(ico)?, (256, 256));
    let ffmpeg = resolver.resolve(Tool::Ffmpeg)?.path;
    let wav = folder.path().join("mono.wav");
    execute(
        Command::new(&ffmpeg)
            .args([
                "-nostdin",
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=0.2",
            ])
            .arg(&wav),
    )?;
    for format in ["ogg", "mp3", "flac"] {
        let audio = convert(ConversionKind::Audio, &wav, folder.path(), format)?;
        execute(
            Command::new(&ffmpeg)
                .args(["-nostdin", "-v", "error", "-i"])
                .arg(audio)
                .args(["-f", "null", "-"]),
        )?;
    }
    let video = folder.path().join("source.mkv");
    execute(
        Command::new(&ffmpeg)
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
            .arg(&video),
    )?;
    let mp4 = convert(ConversionKind::Video, &video, folder.path(), "mp4")?;
    execute(
        Command::new(&ffmpeg)
            .args(["-nostdin", "-v", "error", "-i"])
            .arg(&mp4)
            .args(["-f", "null", "-"]),
    )?;
    let ffprobe = resolver.resolve(Tool::Ffprobe)?.path;
    execute(
        Command::new(ffprobe)
            .args(["-v", "error", "-show_entries", "format=format_name"])
            .arg(&mp4),
    )?;
    let txt = folder.path().join("document.txt");
    let picture = folder.path().join("picture.png");
    image::RgbImage::new(8, 6).save(&picture)?;
    fs::write(
        &txt,
        format!(
            "# Bonjour\n\nUn texte **important**, été.\n\n![Image](<{}>)\n",
            picture.display().to_string().replace('\\', "/")
        ),
    )?;
    let docx = convert(ConversionKind::Documents, &txt, folder.path(), "docx")?;
    let plain = convert(ConversionKind::Documents, &docx, folder.path(), "txt")?;
    let text = fs::read_to_string(plain)?;
    assert!(text.contains("été") && text.contains("important") && !text.contains("**"));
    for source in [&txt, &docx] {
        let pdf = convert(ConversionKind::Documents, source, folder.path(), "pdf")?;
        assert!(fs::read(pdf)?.starts_with(b"%PDF"));
    }
    assert!(!fs::read_dir(folder.path())?
        .filter_map(|entry| entry.ok())
        .any(|entry| entry
            .file_name()
            .to_string_lossy()
            .starts_with(".rust-file-converter-")));
    println!("PNG/JPG/ICO, WAV→OGG/MP3/FLAC, video→MP4, TXT→DOCX, DOCX→TXT, TXT/DOCX→PDF: passed with empty PATH");
    Ok(())
}
