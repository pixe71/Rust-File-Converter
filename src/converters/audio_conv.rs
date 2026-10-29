use super::{process, ConversionOptions};
use crate::tools::{Tool, ToolResolver};
use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

const OGG_CODEC: &str = "libvorbis";
const OGG_QUALITY: &str = "5";

fn select_ogg_codec(encoders: &str) -> Result<&'static str> {
    let available = encoders.lines().any(|line| {
        let mut columns = line.split_whitespace();
        let flags = columns.next().unwrap_or_default().as_bytes();
        let name = columns.next().unwrap_or_default();
        flags.len() == 6 && flags[0] == b'A' && flags[3] != b'X' && name == OGG_CODEC
    });
    if !available {
        bail!("OGG output requires the non-experimental Vorbis encoder libvorbis, which is not available in this FFmpeg build. Use an FFmpeg build with libvorbis enabled.");
    }
    Ok(OGG_CODEC)
}

fn command(
    ffmpeg: &Path,
    input: &Path,
    output: &Path,
    options: &ConversionOptions,
    encoders: Option<&str>,
) -> Result<Command> {
    let mut command = Command::new(ffmpeg);
    // -y applies ONLY to the application's private staging file.
    command
        .args(["-nostdin", "-hide_banner", "-loglevel", "warning", "-y"])
        .arg("-i")
        .arg(input);
    if options.format == "ogg" {
        let codec =
            select_ogg_codec(encoders.context("Cannot determine available Vorbis encoders")?)?;
        // Bitrate targets such as 256k can be invalid for mono/low-rate sources.
        // VBR preserves channels and sample rate without forcing an invalid rate.
        command.args(["-c:a", codec, "-q:a", OGG_QUALITY, "-f", "ogg"]);
    } else if ["mp3", "aac"].contains(&options.format.as_str()) {
        command.arg("-b:a").arg(&options.bitrate);
    }
    command.arg(output);
    Ok(command)
}

fn convert_audio_with(
    ffmpeg: &Path,
    input: &Path,
    output: &Path,
    options: &ConversionOptions,
    probe: impl FnOnce() -> Result<String>,
    run: impl FnOnce(&mut Command, &str) -> Result<()>,
) -> Result<()> {
    let encoders = if options.format == "ogg" {
        Some(probe()?)
    } else {
        None
    };
    let mut command = command(ffmpeg, input, output, options, encoders.as_deref())?;
    let details = if options.format == "ogg" {
        format!("Requested output: ogg; audio codec: {OGG_CODEC}; rate control: VBR quality {OGG_QUALITY}")
    } else {
        format!(
            "Requested output: {}; audio codec: automatic (FFmpeg); bitrate setting: {}",
            options.format, options.bitrate
        )
    };
    run(
        &mut command,
        &format!("FFmpeg executable: {}\n{details}", ffmpeg.display()),
    )
}

pub(super) fn convert_audio(
    input: &Path,
    output: &Path,
    options: &ConversionOptions,
) -> Result<()> {
    let ffmpeg = ToolResolver::from_environment()?.resolve(Tool::Ffmpeg)?;
    convert_audio_with(
        &ffmpeg.path,
        input,
        output,
        options,
        || process::ffmpeg_encoders(&ffmpeg.path),
        |command, details| process::run_with_details(command, "FFmpeg", details),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputPlanner;
    use std::ffi::OsStr;
    use std::fs;

    const ENCODERS: &str = "Encoders:\n A..X.D vorbis Native experimental Vorbis\n A....D libvorbis libvorbis (codec vorbis)\n";

    fn ogg_options() -> ConversionOptions {
        ConversionOptions {
            format: "ogg".into(),
            ..Default::default()
        }
    }

    #[test]
    fn ogg_requires_exact_non_experimental_libvorbis_audio_encoder() -> Result<()> {
        assert_eq!(select_ogg_codec(ENCODERS)?, "libvorbis");
        for listing in [
            "",
            " A..X.D vorbis Vorbis\n",
            " A....D libvorbis_other libvorbis\n",
            " V..... libvorbis Not audio\n",
            " A..X.D libvorbis Experimental\n",
        ] {
            let error = select_ogg_codec(listing).unwrap_err();
            assert!(error.to_string().contains("libvorbis"));
            assert!(error.to_string().contains("not available"));
        }
        Ok(())
    }

    #[test]
    fn ogg_arguments_explicitly_select_container_codec_and_vbr() -> Result<()> {
        let output = Path::new("output-staging.ogg");
        for bitrate in ["64k", "128k", "192k", "256k", "320k"] {
            let mut options = ogg_options();
            options.bitrate = bitrate.into();
            let command = command(
                Path::new("fake-ffmpeg"),
                Path::new("input with spaces.wav"),
                output,
                &options,
                Some(ENCODERS),
            )?;
            let args: Vec<_> = command.get_args().collect();
            for pair in [
                ["-c:a", "libvorbis"],
                ["-q:a", "5"],
                ["-f", "ogg"],
                ["-i", "input with spaces.wav"],
            ] {
                assert!(args.windows(2).any(|args| args == pair.map(OsStr::new)));
            }
            assert!(!args.contains(&OsStr::new("-b:a")));
            assert!(!args.contains(&OsStr::new("-strict")));
            assert_eq!(args.last(), Some(&output.as_os_str()));
            assert!(args.contains(&OsStr::new("-nostdin")));
        }
        Ok(())
    }

    #[test]
    fn ogg_staging_keeps_extension_and_publishes_only_after_success() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("tone.wav");
        fs::write(&source, b"source fixture")?;
        let mut planner = OutputPlanner::new(folder.path(), std::slice::from_ref(&source))?;
        let output = planner.convert(&source, "ogg", |staged| {
            assert_eq!(staged.extension(), Some(OsStr::new("ogg")));
            assert!(staged
                .parent()
                .unwrap()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".rust-file-converter-"));
            assert!(!folder.path().join("tone.ogg").exists());
            convert_audio_with(
                Path::new("fake-ffmpeg"),
                &source,
                staged,
                &ogg_options(),
                || Ok(ENCODERS.into()),
                |command, details| {
                    assert_eq!(command.get_args().last(), Some(staged.as_os_str()));
                    assert!(details.contains("libvorbis"));
                    fs::write(staged, b"OggS simulated successful output")?;
                    Ok(())
                },
            )
        })?;
        assert!(fs::read(output)?.starts_with(b"OggS"));
        assert_eq!(fs::read_dir(folder.path())?.count(), 2);
        Ok(())
    }

    #[test]
    fn missing_vorbis_encoder_never_starts_conversion_or_leaves_partial_output() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("tone.wav");
        fs::write(&source, b"source fixture")?;
        let mut planner = OutputPlanner::new(folder.path(), std::slice::from_ref(&source))?;
        let error = planner
            .convert(&source, "ogg", |staged| {
                convert_audio_with(
                    Path::new("fake-ffmpeg"),
                    &source,
                    staged,
                    &ogg_options(),
                    || Ok(" A..X.D vorbis Vorbis\n".into()),
                    |_, _| panic!("a conversion without libvorbis must not be started"),
                )
            })
            .unwrap_err();
        assert!(error.to_string().contains("libvorbis"));
        assert_eq!(fs::read_dir(folder.path())?.count(), 1);
        assert_eq!(fs::read(source)?, b"source fixture");
        Ok(())
    }

    #[test]
    fn other_audio_formats_keep_their_bitrate_behavior_without_encoder_probe() -> Result<()> {
        for format in ["mp3", "aac", "wav", "flac"] {
            let options = ConversionOptions {
                format: format.into(),
                bitrate: "192k".into(),
                ..Default::default()
            };
            convert_audio_with(
                Path::new("fake-ffmpeg"),
                Path::new("input.wav"),
                Path::new("staged"),
                &options,
                || panic!("non-OGG conversions must not probe Vorbis"),
                |command, _| {
                    let args: Vec<_> = command.get_args().collect();
                    assert_eq!(
                        args.contains(&OsStr::new("-b:a")),
                        ["mp3", "aac"].contains(&format)
                    );
                    Ok(())
                },
            )?;
        }
        Ok(())
    }
}
