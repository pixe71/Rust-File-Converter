use super::{process, ConversionKind, ConversionOptions};
use crate::tools::{Tool, ToolResolver};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

fn command(
    pandoc: &Path,
    input: &Path,
    output: &Path,
    options: &ConversionOptions,
    pdf_engine: Option<&Path>,
) -> Result<Command> {
    ConversionKind::Documents.validate_input(input)?;
    let mut command = Command::new(pandoc);
    let reader = if input
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("docx"))
    {
        "docx"
    } else {
        "markdown"
    };
    command.args(["--from", reader]);
    match options.format.as_str() {
        "txt" => {
            command.args(["--to", "plain", "--wrap=none"]);
        }
        "docx" => {
            command.args(["--to", "docx"]);
        }
        "pdf" => {
            let engine = pdf_engine.context("PDF output requires an available Typst engine")?;
            anyhow::ensure!(engine.is_absolute(), "The PDF engine path must be absolute");
            let mut argument = std::ffi::OsString::from("--pdf-engine=");
            argument.push(engine);
            command.args(["--to", "typst"]).arg(argument);
            // DOCX images must be extracted into the application's owned staging folder.
            command.arg("--extract-media").arg(
                output
                    .parent()
                    .context("PDF output has no parent directory")?
                    .join("media"),
            );
        }
        other => anyhow::bail!("Unsupported document output: {other}"),
    }
    command.arg("--output").arg(output).arg(input);
    Ok(command)
}

pub(super) fn convert_doc(input: &Path, output: &Path, options: &ConversionOptions) -> Result<()> {
    // Input PDF is rejected before discovering or starting any external executable.
    ConversionKind::Documents.validate_input(input)?;
    let resolver = ToolResolver::from_environment()?;
    convert_doc_with(
        input,
        output,
        options,
        |tool| resolver.resolve(tool).map(|resolved| resolved.path),
        |command| {
            let mut details = format!(
                "Pandoc executable: {}",
                Path::new(command.get_program()).display()
            );
            if let Some(engine) = command
                .get_args()
                .find(|arg| arg.to_string_lossy().starts_with("--pdf-engine="))
            {
                details.push('\n');
                details.push_str(&engine.to_string_lossy());
            }
            process::run_with_details(command, "Pandoc", &details)
        },
    )
}

fn convert_doc_with(
    input: &Path,
    output: &Path,
    options: &ConversionOptions,
    mut resolve: impl FnMut(Tool) -> Result<PathBuf>,
    run: impl FnOnce(&mut Command) -> Result<()>,
) -> Result<()> {
    ConversionKind::Documents.validate_input(input)?;
    let pandoc = resolve(Tool::Pandoc)?;
    let engine = if options.format == "pdf" {
        Some(resolve(Tool::Typst).context(
            "PDF output requires Typst. Restore tools/ in the portable package; Pandoc alone cannot generate PDF."
        )?)
    } else {
        None
    };
    run(&mut command(
        &pandoc,
        input,
        output,
        options,
        engine.as_deref(),
    )?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::OutputPlanner;
    use std::ffi::OsStr;
    use std::fs;

    #[test]
    fn docx_to_txt_and_txt_to_docx_require_only_pandoc() -> Result<()> {
        for (input_format, output_format, reader, writer) in [
            ("docx", "txt", "docx", "plain"),
            ("txt", "docx", "markdown", "docx"),
        ] {
            let folder = tempfile::tempdir()?;
            let source = folder.path().join(format!("source.{input_format}"));
            fs::write(&source, b"simulated input")?;
            let options = ConversionOptions {
                format: output_format.into(),
                ..Default::default()
            };
            let mut planner = OutputPlanner::new(folder.path(), std::slice::from_ref(&source))?;
            let output = planner.convert(&source, output_format, |staged| {
                convert_doc_with(
                    &source,
                    staged,
                    &options,
                    |tool| {
                        assert_eq!(tool, Tool::Pandoc, "DOCX/TXT must not require a PDF engine");
                        Ok(folder.path().join(Tool::Pandoc.file_name()))
                    },
                    |command| {
                        let args: Vec<_> = command.get_args().collect();
                        for pair in [["--from", reader], ["--to", writer]] {
                            assert!(args.windows(2).any(|args| args == pair.map(OsStr::new)));
                        }
                        assert!(!args
                            .iter()
                            .any(|arg| arg.to_string_lossy().starts_with("--pdf-engine")));
                        if output_format == "txt" {
                            assert!(args.contains(&OsStr::new("--wrap=none")));
                        }
                        fs::write(staged, b"simulated successful output")?;
                        Ok(())
                    },
                )
            })?;
            assert!(output.exists());
            assert_eq!(fs::read_dir(folder.path())?.count(), 2);
        }
        Ok(())
    }

    #[test]
    fn missing_typst_never_runs_pandoc_or_publishes_partial_output() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("source.txt");
        fs::write(&source, b"user content")?;
        let options = ConversionOptions {
            format: "pdf".into(),
            ..Default::default()
        };
        let mut planner = OutputPlanner::new(folder.path(), std::slice::from_ref(&source))?;
        let error = planner
            .convert(&source, "pdf", |staged| {
                convert_doc_with(
                    &source,
                    staged,
                    &options,
                    |tool| {
                        if tool == Tool::Pandoc {
                            Ok(folder.path().join(tool.file_name()))
                        } else {
                            anyhow::bail!("Typst missing")
                        }
                    },
                    |_| panic!("Pandoc must not run without Typst"),
                )
            })
            .unwrap_err();
        assert!(error.to_string().contains("PDF output requires Typst"));
        assert!(format!("{error:#}").contains("Typst missing"));
        assert_eq!(fs::read_dir(folder.path())?.count(), 1);
        assert_eq!(fs::read(source)?, b"user content");
        Ok(())
    }

    #[test]
    fn simulated_typst_is_passed_as_full_unicode_path_and_staging_is_finalized() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("source.txt");
        fs::write(&source, b"input")?;
        let typst = folder
            .path()
            .join("tools avec espaces été")
            .join(Tool::Typst.file_name());
        let options = ConversionOptions {
            format: "pdf".into(),
            ..Default::default()
        };
        let mut planner = OutputPlanner::new(folder.path(), std::slice::from_ref(&source))?;
        let output = planner.convert(&source, "pdf", |staged| {
            convert_doc_with(
                &source,
                staged,
                &options,
                |tool| {
                    Ok(if tool == Tool::Typst {
                        typst.clone()
                    } else {
                        folder.path().join(tool.file_name())
                    })
                },
                |command| {
                    let args: Vec<_> = command.get_args().collect();
                    let mut expected = std::ffi::OsString::from("--pdf-engine=");
                    expected.push(&typst);
                    assert!(args.contains(&expected.as_os_str()));
                    assert!(args
                        .windows(2)
                        .any(|args| args == [OsStr::new("--to"), OsStr::new("typst")]));
                    assert_eq!(staged.extension(), Some(OsStr::new("pdf")));
                    assert!(!folder.path().join("source.pdf").exists());
                    fs::write(staged, b"%PDF simulated output")?;
                    Ok(())
                },
            )
        })?;
        assert!(fs::read(output)?.starts_with(b"%PDF"));
        assert_eq!(fs::read_dir(folder.path())?.count(), 2);
        Ok(())
    }

    #[test]
    fn pdf_input_is_rejected_before_external_tools() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let input = folder.path().join("input.pdf");
        fs::write(&input, b"%PDF")?;
        let error = convert_doc_with(
            &input,
            &folder.path().join("output.txt"),
            &ConversionOptions {
                format: "txt".into(),
                ..Default::default()
            },
            |_| panic!("PDF input must be rejected before tool discovery"),
            |_| panic!("PDF input must not start a converter"),
        )
        .unwrap_err();
        assert!(error.to_string().contains("PDF input is not supported"));
        Ok(())
    }

    #[test]
    fn pdf_engine_cannot_be_a_relative_path() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let input = folder.path().join("input.txt");
        fs::write(&input, b"text")?;
        let error = command(
            &folder.path().join("pandoc"),
            &input,
            &folder.path().join("out.pdf"),
            &ConversionOptions {
                format: "pdf".into(),
                ..Default::default()
            },
            Some(Path::new("typst")),
        )
        .unwrap_err();
        assert!(error.to_string().contains("absolute"));
        Ok(())
    }
}
