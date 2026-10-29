use super::ConversionOptions;
use anyhow::{Context, Result};
use image::imageops::FilterType;
use std::io::{BufWriter, Write};
use std::path::Path;

pub(super) fn convert_image(
    input: &Path,
    output: &Path,
    options: &ConversionOptions,
) -> Result<()> {
    let mut img = image::open(input)
        .with_context(|| format!("Failed to decode image: {}", input.display()))?;
    // Preserve the existing optional resizing behavior.
    match (options.width, options.height) {
        (Some(w), Some(h)) if options.format == "ico" => {
            img = img.resize(w, h, FilterType::Lanczos3);
        }
        (Some(w), Some(h)) => img = img.resize_exact(w, h, FilterType::Lanczos3),
        (Some(w), None) => img = img.resize(w, u32::MAX, FilterType::Lanczos3),
        (None, Some(h)) => img = img.resize(u32::MAX, h, FilterType::Lanczos3),
        _ => {}
    }
    if options.format == "ico" {
        // ICO dimensions are at most 256x256. Fit first, then center on a
        // transparent square without stretching or enlarging small images.
        let fitted = if img.width() > 256 || img.height() > 256 {
            img.thumbnail(256, 256).to_rgba8()
        } else {
            img.to_rgba8()
        };
        let mut canvas = image::RgbaImage::new(256, 256);
        let x = (256 - fitted.width()) / 2;
        let y = (256 - fitted.height()) / 2;
        image::imageops::overlay(&mut canvas, &fitted, i64::from(x), i64::from(y));
        img = image::DynamicImage::ImageRgba8(canvas);
    }
    let format =
        image::ImageFormat::from_extension(&options.format).context("Unsupported image format")?;
    let mut writer =
        BufWriter::new(std::fs::File::create(output).context("Cannot create staged image")?);
    img.write_to(&mut writer, format)
        .context("Failed to encode image")?;
    writer.flush().context("Failed to flush encoded image")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converters::{convert_file, ConversionKind};
    use crate::output::OutputPlanner;

    #[test]
    fn png_jpg_roundtrip_and_in_place_protection() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("photo.v1.png");
        image::RgbImage::from_pixel(32, 16, image::Rgb([200, 80, 20])).save(&source)?;
        let original = std::fs::read(&source)?;
        let mut planner = OutputPlanner::new(folder.path(), std::slice::from_ref(&source))?;
        let jpeg = convert_file(
            ConversionKind::Images,
            &source,
            &mut planner,
            &ConversionOptions {
                format: "jpg".into(),
                ..Default::default()
            },
        )?;
        assert_eq!(jpeg.file_name().unwrap(), "photo.v1.jpg");
        assert_eq!(image::open(&jpeg)?.width(), 32);
        let png = convert_file(
            ConversionKind::Images,
            &jpeg,
            &mut planner,
            &ConversionOptions {
                format: "png".into(),
                ..Default::default()
            },
        )?;
        assert_eq!(png.file_name().unwrap(), "photo.v1 (1).png");
        assert_eq!(image::open(png)?.height(), 16);
        assert_eq!(std::fs::read(source)?, original);
        Ok(())
    }

    #[test]
    fn large_non_square_image_produces_valid_proportional_ico() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("large.png");
        image::RgbaImage::from_pixel(1024, 512, image::Rgba([255, 0, 0, 255])).save(&source)?;
        let mut planner = OutputPlanner::new(folder.path(), std::slice::from_ref(&source))?;
        for dimension in [None, Some(256)] {
            let output = convert_file(
                ConversionKind::Images,
                &source,
                &mut planner,
                &ConversionOptions {
                    format: "ico".into(),
                    width: dimension,
                    height: dimension,
                    ..Default::default()
                },
            )?;
            let icon = image::open(output)?.to_rgba8();
            assert_eq!(icon.dimensions(), (256, 256));
            assert_eq!(icon.get_pixel(128, 0).0[3], 0);
            assert_eq!(icon.get_pixel(128, 64).0, [255, 0, 0, 255]);
            assert_eq!(icon.get_pixel(128, 191).0[3], 255);
            assert_eq!(icon.get_pixel(128, 192).0[3], 0);
        }
        Ok(())
    }

    #[test]
    fn all_advertised_image_outputs_are_readable() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("input.png");
        image::RgbaImage::from_pixel(16, 8, image::Rgba([10, 40, 90, 255])).save(&source)?;
        let original = std::fs::read(&source)?;
        let mut planner = OutputPlanner::new(folder.path(), std::slice::from_ref(&source))?;
        for format in ConversionKind::Images.output_formats() {
            let output = convert_file(
                ConversionKind::Images,
                &source,
                &mut planner,
                &ConversionOptions {
                    format: (*format).into(),
                    ..Default::default()
                },
            )?;
            let decoded = image::open(&output)?;
            assert_eq!(
                (decoded.width(), decoded.height()),
                if *format == "ico" {
                    (256, 256)
                } else {
                    (16, 8)
                }
            );
            assert_eq!(
                image::guess_format(&std::fs::read(output)?)?,
                image::ImageFormat::from_extension(format).unwrap()
            );
        }
        assert_eq!(std::fs::read(source)?, original);
        Ok(())
    }

    #[test]
    fn corrupt_image_returns_error_and_leaves_no_final_or_staging() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("broken.png");
        std::fs::write(&source, b"not an image")?;
        let mut planner = OutputPlanner::new(folder.path(), std::slice::from_ref(&source))?;
        let error = convert_file(
            ConversionKind::Images,
            &source,
            &mut planner,
            &ConversionOptions {
                format: "jpg".into(),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains("decode"));
        assert_eq!(std::fs::read_dir(folder.path())?.count(), 1);
        assert_eq!(std::fs::read(source)?, b"not an image");
        Ok(())
    }
}
