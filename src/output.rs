//! Collision-safe naming and transactional publication shared by every engine.
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub struct OutputPlanner {
    directory: PathBuf,
    protected: HashSet<String>,
    reserved: HashSet<String>,
}

// Conservative case folding may add a suffix on case-sensitive volumes.
// A lossy key can only add a suffix, never overwrite or discard an input.
fn reservation_key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

impl OutputPlanner {
    pub fn new(directory: &Path, sources: &[PathBuf]) -> Result<Self> {
        let directory = directory
            .canonicalize()
            .context("Cannot access output folder")?;
        if !directory.is_dir() {
            bail!("The output destination must be a folder");
        }
        let mut protected = HashSet::new();
        for source in sources {
            let absolute = std::path::absolute(source).context("Cannot resolve source path")?;
            protected.insert(reservation_key(&absolute));
            if let (Some(parent), Some(name)) = (absolute.parent(), absolute.file_name()) {
                if let Ok(parent) = parent.canonicalize() {
                    protected.insert(reservation_key(&parent.join(name)));
                }
            }
            if let Ok(canonical) = source.canonicalize() {
                protected.insert(reservation_key(&canonical));
            }
        }
        Ok(Self {
            directory,
            protected,
            reserved: HashSet::new(),
        })
    }

    pub fn plan(&mut self, source: &Path, format: &str) -> Result<PathBuf> {
        if format.is_empty() || !format.bytes().all(|c| c.is_ascii_alphanumeric()) {
            bail!("Invalid output format: {format}");
        }
        let stem = source
            .file_stem()
            .filter(|s| !s.is_empty())
            .context("Source has no file name")?;
        for suffix in 0..=u32::MAX {
            let mut name = OsString::from(stem);
            if suffix != 0 {
                name.push(format!(" ({suffix})"));
            }
            name.push(".");
            name.push(format);
            let candidate = self.directory.join(name);
            let key = reservation_key(&candidate);
            if self.protected.contains(&key) || self.reserved.contains(&key) {
                continue;
            }
            match fs::symlink_metadata(&candidate) {
                Ok(_) => continue, // Includes folders and dangling symlinks.
                Err(error) if error.kind() == ErrorKind::NotFound => {}
                Err(error) => return Err(error).context("Cannot inspect output destination"),
            }
            self.reserved.insert(key);
            return Ok(candidate);
        }
        bail!("No available output name for {}", source.display())
    }

    pub fn convert(
        &mut self,
        source: &Path,
        format: &str,
        write_and_validate: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<PathBuf> {
        let mut destination = self.plan(source, format)?;
        // Private staging on the destination filesystem; never clean unrelated files.
        let staging = tempfile::Builder::new()
            .prefix(".rust-file-converter-")
            .tempdir_in(&self.directory)
            .context("Cannot create Rust File Converter staging folder")?;
        let mut temporary = tempfile::Builder::new()
            .prefix("output-")
            .suffix(&format!(".{format}"))
            .tempfile_in(staging.path())?
            .into_temp_path(); // Close before subprocess access.
        write_and_validate(&temporary)?;
        if !fs::symlink_metadata(&temporary)
            .context("The engine did not create an output file")?
            .is_file()
        {
            bail!("The conversion output is not a regular file");
        }
        fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(&temporary)?
            .sync_all()
            .context("Cannot flush the conversion output")?;
        loop {
            // Unlike fs::rename on Unix, this never replaces an existing file.
            match temporary.persist_noclobber(&destination) {
                Ok(()) => return Ok(destination),
                Err(error) if error.error.kind() == ErrorKind::AlreadyExists => {
                    temporary = error.path;
                    destination = self.plan(source, format)?;
                }
                Err(error) => return Err(error.error).context("Cannot finalize conversion output"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_all_stem_components() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let mut planner = OutputPlanner::new(folder.path(), &[])?;
        for (input, expected) in [
            ("photo.png", "photo.jpg"),
            ("photo.v1.png", "photo.v1.jpg"),
            ("archive.final.v2.png", "archive.final.v2.jpg"),
            ("été.v2.png", "été.v2.jpg"),
        ] {
            assert_eq!(
                planner.plan(Path::new(input), "jpg")?.file_name(),
                Some(std::ffi::OsStr::new(expected))
            );
        }
        Ok(())
    }

    #[test]
    fn reserves_names_for_homonyms_and_existing_outputs() -> Result<()> {
        let folder = tempfile::tempdir()?;
        fs::write(folder.path().join("image.jpg"), b"user output")?;
        let mut planner = OutputPlanner::new(folder.path(), &[])?;
        for index in 1..=3 {
            let source = PathBuf::from(format!("folder{index}/image.png"));
            assert_eq!(
                planner.plan(&source, "jpg")?,
                folder
                    .path()
                    .canonicalize()?
                    .join(format!("image ({index}).jpg"))
            );
        }
        assert_eq!(fs::read(folder.path().join("image.jpg"))?, b"user output");
        Ok(())
    }

    #[test]
    fn protects_every_source_even_if_removed_after_planning() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("image.jpg");
        fs::write(&source, b"source")?;
        let mut planner = OutputPlanner::new(folder.path(), std::slice::from_ref(&source))?;
        fs::remove_file(&source)?;
        assert_eq!(
            planner.plan(&source, "jpg")?.file_name().unwrap(),
            "image (1).jpg"
        );
        Ok(())
    }

    #[test]
    fn cleans_failed_staging_without_touching_user_files() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let user_temporary = folder.path().join("unrelated.tmp");
        fs::write(&user_temporary, b"keep")?;
        let mut planner = OutputPlanner::new(folder.path(), &[])?;
        let result = planner.convert(Path::new("image.png"), "jpg", |path| {
            fs::write(path, b"partial")?;
            assert!(!folder.path().join("image.jpg").exists());
            bail!("simulated failure")
        });
        assert!(result.is_err());
        assert_eq!(fs::read(user_temporary)?, b"keep");
        assert_eq!(fs::read_dir(folder.path())?.count(), 1);
        Ok(())
    }

    #[test]
    fn publishes_only_successful_output_and_handles_late_collision() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let mut planner = OutputPlanner::new(folder.path(), &[])?;
        let output = planner.convert(Path::new("image.png"), "jpg", |path| {
            assert!(!folder.path().join("image.jpg").exists());
            assert!(path.extension().is_some_and(|ext| ext == "jpg"));
            fs::write(path, b"complete")?;
            fs::write(folder.path().join("image.jpg"), b"created concurrently")?;
            Ok(())
        })?;
        assert_eq!(output.file_name().unwrap(), "image (1).jpg");
        assert_eq!(fs::read(output)?, b"complete");
        assert_eq!(
            fs::read(folder.path().join("image.jpg"))?,
            b"created concurrently"
        );
        assert_eq!(fs::read_dir(folder.path())?.count(), 2);
        Ok(())
    }

    fn concurrent_conversions(
        directory: &Path,
        fail_preparation: Option<usize>,
    ) -> Result<Vec<PathBuf>> {
        use std::sync::mpsc;
        use std::time::Duration;
        let timeout = Duration::from_secs(10);
        let outputs = std::thread::scope(|scope| {
            let (ready_sender, ready_receiver) = mpsc::channel();
            let mut release_senders = Vec::new();
            let mut handles = Vec::new();
            for (index, contents) in [b"first".as_slice(), b"second".as_slice()]
                .into_iter()
                .enumerate()
            {
                let ready_sender = ready_sender.clone();
                let (release_sender, release_receiver) = mpsc::channel();
                release_senders.push(release_sender);
                handles.push(scope.spawn(move || -> Result<PathBuf> {
                    if fail_preparation == Some(index) {
                        bail!("simulated preparation failure in worker {index}");
                    }
                    let mut planner = OutputPlanner::new(directory, &[])?;
                    planner.convert(Path::new("image.png"), "jpg", |staged| {
                        fs::write(staged, contents)?;
                        ready_sender.send(())?;
                        // A worker must not keep readiness connected while awaiting release.
                        drop(ready_sender);
                        release_receiver
                            .recv_timeout(timeout)
                            .context("Finalization was not released")?;
                        Ok(())
                    })
                }));
            }
            drop(ready_sender);
            let ready = (0..handles.len()).try_for_each(|_| ready_receiver.recv_timeout(timeout));
            if ready.is_ok() {
                for sender in &release_senders {
                    // Joining below reports a worker failure, including a disconnected channel.
                    let _ = sender.send(());
                }
            }
            // Also releases waiting workers if any preparation failed.
            drop(release_senders);
            let results: Vec<_> = handles
                .into_iter()
                .map(|handle| {
                    handle
                        .join()
                        .unwrap_or_else(|_| Err(anyhow::anyhow!("Concurrent worker panicked")))
                })
                .collect();
            let errors: Vec<_> = results
                .iter()
                .filter_map(|result| result.as_ref().err())
                .map(|error| format!("{error:#}"))
                .collect();
            if !errors.is_empty() {
                bail!("{}", errors.join("; "));
            }
            ready.context("Workers did not finish preparation")?;
            results.into_iter().collect::<Result<Vec<_>>>()
        })?;
        Ok(outputs)
    }

    #[test]
    fn concurrent_finalization_never_overwrites_another_conversion() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let outputs = concurrent_conversions(folder.path(), None)?;
        assert_ne!(outputs[0], outputs[1]);
        assert_eq!(fs::read(&outputs[0])?, b"first");
        assert_eq!(fs::read(&outputs[1])?, b"second");
        assert_eq!(fs::read_dir(folder.path())?.count(), 2);
        Ok(())
    }

    #[test]
    fn concurrent_preparation_failure_releases_the_other_worker() -> Result<()> {
        for failed_worker in [0, 1] {
            let folder = tempfile::tempdir()?;
            let error = concurrent_conversions(folder.path(), Some(failed_worker)).unwrap_err();
            assert!(format!("{error:#}").contains("simulated preparation failure"));
            assert_eq!(fs::read_dir(folder.path())?.count(), 0);
        }
        Ok(())
    }

    #[test]
    fn unwinding_cleans_only_the_owned_staging_directory() -> Result<()> {
        let folder = tempfile::tempdir()?;
        fs::write(folder.path().join("user.tmp"), b"keep")?;
        let mut planner = OutputPlanner::new(folder.path(), &[])?;
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            planner.convert(Path::new("image.png"), "jpg", |staged| {
                fs::write(staged, b"partial")?;
                panic!("simulated worker panic")
            })
        }));
        assert!(result.is_err());
        assert_eq!(fs::read_dir(folder.path())?.count(), 1);
        assert_eq!(fs::read(folder.path().join("user.tmp"))?, b"keep");
        Ok(())
    }

    #[test]
    fn source_and_output_can_be_in_the_same_folder() -> Result<()> {
        let folder = tempfile::tempdir()?;
        let source = folder.path().join("image.png");
        fs::write(&source, b"source")?;
        let mut planner = OutputPlanner::new(folder.path(), std::slice::from_ref(&source))?;
        let output = planner.convert(&source, "png", |path| {
            fs::write(path, b"converted")?;
            Ok(())
        })?;
        assert_eq!(fs::read(source)?, b"source");
        assert_eq!(output.file_name().unwrap(), "image (1).png");
        Ok(())
    }
}
