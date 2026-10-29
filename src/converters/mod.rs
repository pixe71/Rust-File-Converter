mod audio_conv;
mod doc_conv;
mod image_conv;
mod process;
mod video_conv;

use crate::output::OutputPlanner;
use anyhow::{bail, Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversionKind {
    Images,
    Documents,
    Audio,
    Video,
}

impl ConversionKind {
    pub fn input_extensions(self) -> &'static [&'static str] {
        match self {
            Self::Images => &["png", "jpg", "jpeg", "bmp", "gif", "webp", "ico"],
            Self::Documents => &["docx", "txt"],
            Self::Audio => &["mp3", "wav", "ogg", "flac", "m4a", "aac"],
            Self::Video => &["mp4", "avi", "mkv", "mov", "webm"],
        }
    }

    pub fn output_formats(self) -> &'static [&'static str] {
        match self {
            Self::Images => &["jpg", "png", "bmp", "gif", "ico", "webp"],
            Self::Documents => &["pdf", "txt", "docx"],
            Self::Audio => &["mp3", "wav", "ogg", "flac", "aac"],
            Self::Video => &["mp4", "mkv", "avi", "mov"],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Images => "Images",
            Self::Documents => "Documents",
            Self::Audio => "Audio",
            Self::Video => "Video",
        }
    }

    pub fn validate_input(self, input: &Path) -> Result<()> {
        let extension = input
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if self == Self::Documents && extension == "pdf" {
            bail!("PDF input is not supported. Pandoc only reads DOCX and TXT in this version.");
        }
        if !self.input_extensions().contains(&extension.as_str()) {
            bail!(
                "Unsupported {} input: {}",
                self.label().to_lowercase(),
                input.display()
            );
        }
        let metadata = input
            .metadata()
            .with_context(|| format!("Cannot access source: {}", input.display()))?;
        if !metadata.is_file() {
            bail!("Source is not a regular file: {}", input.display());
        }
        std::fs::File::open(input)
            .with_context(|| format!("Cannot read source: {}", input.display()))?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct ConversionOptions {
    pub format: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub bitrate: String,
    pub codec: String,
}

impl Default for ConversionOptions {
    fn default() -> Self {
        Self {
            format: String::new(),
            width: None,
            height: None,
            bitrate: "256k".into(),
            codec: "libx264".into(),
        }
    }
}

impl ConversionOptions {
    pub fn validate(&self, kind: ConversionKind) -> Result<()> {
        if !kind.output_formats().contains(&self.format.as_str()) {
            bail!("Unsupported output format: {}", self.format);
        }
        if self.width == Some(0) || self.height == Some(0) {
            bail!("Image/video dimensions must be greater than zero");
        }
        if kind == ConversionKind::Audio
            && !["64k", "128k", "192k", "256k", "320k"].contains(&self.bitrate.as_str())
        {
            bail!("Unsupported audio bitrate: {}", self.bitrate);
        }
        if kind == ConversionKind::Video {
            if !["libx264", "libx265", "copy"].contains(&self.codec.as_str()) {
                bail!("Unsupported video codec: {}", self.codec);
            }
            if self.codec == "copy" && (self.width.is_some() || self.height.is_some()) {
                bail!("Video stream copy cannot be combined with resizing");
            }
        }
        Ok(())
    }
}

pub fn convert_file(
    kind: ConversionKind,
    input: &Path,
    planner: &mut OutputPlanner,
    options: &ConversionOptions,
) -> Result<PathBuf> {
    kind.validate_input(input)?;
    options.validate(kind)?;
    planner.convert(input, &options.format, |staged| {
        match kind {
            ConversionKind::Images => image_conv::convert_image(input, staged, options)?,
            ConversionKind::Documents => doc_conv::convert_doc(input, staged, options)?,
            ConversionKind::Audio => audio_conv::convert_audio(input, staged, options)?,
            ConversionKind::Video => video_conv::convert_video(input, staged, options)?,
        }
        validate_output(kind, staged, &options.format)
    })
}

fn validate_output(kind: ConversionKind, output: &Path, format: &str) -> Result<()> {
    let mut file =
        std::fs::File::open(output).context("The engine did not produce an output file")?;
    if file.metadata()?.len() == 0 && !(kind == ConversionKind::Documents && format == "txt") {
        bail!("The engine produced an empty output file");
    }
    if kind == ConversionKind::Images {
        image::image_dimensions(output).context("The generated image has an invalid header")?;
    } else if kind == ConversionKind::Documents && format != "txt" {
        let mut header = [0; 4];
        file.read_exact(&mut header)
            .context("The generated document is incomplete")?;
        let expected = if format == "pdf" {
            b"%PDF"
        } else {
            b"PK\x03\x04"
        };
        if &header != expected {
            bail!("The generated document has an invalid {format} header");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unsupported_and_incompatible_options() {
        let mut options = ConversionOptions {
            format: "invalid".into(),
            ..Default::default()
        };
        assert!(options.validate(ConversionKind::Images).is_err());
        options.format = "mp4".into();
        options.codec = "copy".into();
        options.width = Some(640);
        assert!(options.validate(ConversionKind::Video).is_err());
        options.width = Some(0);
        assert!(options.validate(ConversionKind::Video).is_err());
    }

    #[test]
    fn output_validation_rejects_empty_media_and_invalid_documents() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let output = folder.path().join("output.bin");
        std::fs::write(&output, b"")?;
        assert!(validate_output(ConversionKind::Audio, &output, "mp3").is_err());
        assert!(validate_output(ConversionKind::Documents, &output, "txt").is_ok());
        std::fs::write(&output, b"not a document")?;
        assert!(validate_output(ConversionKind::Documents, &output, "pdf").is_err());
        assert!(validate_output(ConversionKind::Documents, &output, "docx").is_err());
        Ok(())
    }
}
