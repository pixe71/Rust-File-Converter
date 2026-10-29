use crate::converters::ConversionKind;
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(unix)]
type FileIdentity = (u64, u64);
#[cfg(not(unix))]
type FileIdentity = same_file::Handle;

fn file_identity(path: &Path) -> Result<FileIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = path.metadata().context("Cannot identify selected file")?;
        Ok((metadata.dev(), metadata.ino()))
    }
    #[cfg(not(unix))]
    {
        // Keep the Windows handle alive: an ID can be reused after it is closed.
        same_file::Handle::from_path(path).context("Cannot identify selected file")
    }
}

struct SelectedFile {
    path: PathBuf,
    identity: Arc<FileIdentity>,
}

/// Ordered selection with cached identities. No pairwise filesystem comparisons.
/// Unix identities do not retain descriptors; Windows retains one handle per file.
#[derive(Default)]
pub struct Selection {
    files: Vec<SelectedFile>,
    paths: HashSet<PathBuf>,
    identities: HashSet<Arc<FileIdentity>>,
}

impl Selection {
    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &PathBuf> {
        self.files.iter().map(|file| &file.path)
    }

    pub fn to_paths(&self) -> Vec<PathBuf> {
        self.iter().cloned().collect()
    }

    pub fn clear(&mut self) {
        self.files.clear();
        self.paths.clear();
        self.identities.clear();
    }

    pub fn remove(&mut self, index: usize) {
        let file = self.files.remove(index);
        self.paths.remove(&file.path);
        self.identities.remove(&file.identity);
    }

    /// Shared entry point for native selection and drag & drop.
    /// Checks accessibility and extension; decoding remains in the conversion worker.
    pub fn add_file(&mut self, kind: ConversionKind, path: &Path) -> Result<bool> {
        self.add_with_identity(kind, path, file_identity)
    }

    fn add_with_identity(
        &mut self,
        kind: ConversionKind,
        path: &Path,
        identify: impl FnOnce(&Path) -> Result<FileIdentity>,
    ) -> Result<bool> {
        kind.validate_input(path)?;
        // Resolve the parent, preserving a selected symlink's name and extension.
        let absolute = std::path::absolute(path).context("Cannot resolve selected path")?;
        let normalized = absolute
            .parent()
            .context("Selected file has no parent")?
            .canonicalize()
            .context("Cannot resolve selected folder")?
            .join(
                absolute
                    .file_name()
                    .context("Selected path has no file name")?,
            );
        if self.paths.contains(&normalized) {
            return Ok(false);
        }
        let identity = Arc::new(identify(path)?);
        if !self.identities.insert(identity.clone()) {
            return Ok(false);
        }
        self.paths.insert(normalized.clone());
        self.files.push(SelectedFile {
            path: normalized,
            identity,
        });
        Ok(true)
    }
}

/// Infer only a supported category. The caller must still add through Selection,
/// so missing/unreadable files cannot choose the menu's category.
pub fn infer_kind(path: &Path) -> Result<ConversionKind> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if extension == "pdf" {
        bail!("PDF input is not supported. Pandoc only reads DOCX and TXT in this version.");
    }
    [
        ConversionKind::Images,
        ConversionKind::Documents,
        ConversionKind::Audio,
        ConversionKind::Video,
    ]
    .into_iter()
    .find(|kind| kind.input_extensions().contains(&extension.as_str()))
    .context("Unsupported input format")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn deduplicates_paths_and_file_aliases() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("source.PNG");
        fs::write(&source, b"test")?;
        let alias = folder.path().join("alias.png");
        fs::hard_link(&source, &alias)?;
        let mut files = Selection::default();
        assert!(files.add_file(ConversionKind::Images, &source)?);
        assert!(!files.add_file(ConversionKind::Images, &folder.path().join("./source.PNG"))?);
        #[cfg(windows)]
        assert!(!files.add_file(ConversionKind::Images, &folder.path().join("SOURCE.png"))?);
        assert!(!files.add_file(ConversionKind::Images, &alias)?);
        assert_eq!(files.len(), 1);
        Ok(())
    }

    #[test]
    fn removal_and_clear_release_all_index_entries() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let directory = folder.path().canonicalize()?;
        let paths: Vec<_> = ["first.png", "second.png", "third.png"]
            .map(|name| directory.join(name))
            .into();
        let mut files = Selection::default();
        for path in &paths {
            fs::write(path, b"test")?;
            assert!(files.add_file(ConversionKind::Images, path)?);
        }
        files.remove(1);
        assert_eq!(files.to_paths(), vec![paths[0].clone(), paths[2].clone()]);
        assert!(files.add_file(ConversionKind::Images, &paths[1])?);
        assert!(!files.add_file(ConversionKind::Images, &paths[0])?);
        files.clear();
        assert!(files.is_empty());
        assert!(files.paths.is_empty() && files.identities.is_empty());
        assert!(files.add_file(ConversionKind::Images, &paths[0])?);
        Ok(())
    }

    #[test]
    fn large_batches_identify_each_new_path_once_and_never_reopen_previous_files() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let mut files = Selection::default();
        let mut identity_reads = 0;
        const COUNT: usize = 1_024;
        for index in 0..COUNT {
            let source = folder.path().join(format!("{index}.png"));
            let alias = folder.path().join(format!("alias-{index}.png"));
            fs::write(&source, b"test")?;
            fs::hard_link(&source, &alias)?;
            assert!(
                files.add_with_identity(ConversionKind::Images, &source, |path| {
                    identity_reads += 1;
                    file_identity(path)
                })?
            );
            // Exact/equivalent paths need no new identity query.
            assert!(
                !files.add_with_identity(ConversionKind::Images, &source, |_| {
                    panic!("an indexed path must not request another identity")
                })?
            );
            assert!(!files.add_with_identity(
                ConversionKind::Images,
                &folder.path().join(format!("./{index}.png")),
                |_| panic!("an equivalent indexed path must not request another identity")
            )?);
            // Removing previous paths makes accidental pairwise reopening fail.
            fs::remove_file(&source)?;
            assert!(
                !files.add_with_identity(ConversionKind::Images, &alias, |path| {
                    identity_reads += 1;
                    file_identity(path)
                })?
            );
        }
        assert_eq!(identity_reads, 2 * COUNT);
        assert_eq!(files.len(), COUNT);
        assert_eq!(files.paths.len(), COUNT);
        assert_eq!(files.identities.len(), COUNT);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn symlink_selection_keeps_its_supported_extension_and_name() -> Result<()> {
        use crate::converters::{convert_file, ConversionOptions};
        use crate::output::OutputPlanner;
        let folder = tempfile::tempdir()?;
        let target = folder.path().join("image-data");
        image::RgbImage::new(4, 4).save_with_format(&target, image::ImageFormat::Png)?;
        let alias = folder.path().join("photo.v1.png");
        let other_alias = folder.path().join("other.png");
        std::os::unix::fs::symlink(&target, &alias)?;
        std::os::unix::fs::symlink(&target, &other_alias)?;
        let mut files = Selection::default();
        assert!(files.add_file(ConversionKind::Images, &alias)?);
        assert!(!files.add_file(ConversionKind::Images, &other_alias)?);
        let paths = files.to_paths();
        assert_eq!(paths[0].file_name().unwrap(), "photo.v1.png");
        let mut planner = OutputPlanner::new(folder.path(), &paths)?;
        let output = convert_file(
            ConversionKind::Images,
            &paths[0],
            &mut planner,
            &ConversionOptions {
                format: "jpg".into(),
                ..Default::default()
            },
        )?;
        assert_eq!(output.file_name().unwrap(), "photo.v1.jpg");
        assert_eq!(image::open(output)?.width(), 4);
        assert!(target.exists() && alias.exists());
        Ok(())
    }

    #[test]
    fn rejects_directories_missing_files_and_pdf_documents() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let mut files = Selection::default();
        assert!(files
            .add_file(ConversionKind::Images, folder.path())
            .is_err());
        assert!(files
            .add_file(ConversionKind::Images, &folder.path().join("missing.png"))
            .is_err());
        let pdf = folder.path().join("document.pdf");
        fs::write(&pdf, b"%PDF")?;
        assert!(files.add_file(ConversionKind::Documents, &pdf).is_err());
        assert!(files.is_empty());
        Ok(())
    }
}
