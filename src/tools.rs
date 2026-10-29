//! External tools are resolved relative to the executable, never the working directory.
use anyhow::{bail, Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Ffmpeg,
    Ffprobe,
    Pandoc,
    Typst,
}

impl Tool {
    pub const ALL: [Self; 4] = [Self::Ffmpeg, Self::Ffprobe, Self::Pandoc, Self::Typst];

    pub fn name(self) -> &'static str {
        match self {
            Self::Ffmpeg => "FFmpeg",
            Self::Ffprobe => "FFprobe",
            Self::Pandoc => "Pandoc",
            Self::Typst => "Typst",
        }
    }

    pub fn file_name(self) -> &'static str {
        match self {
            Self::Ffmpeg => {
                if cfg!(windows) {
                    "ffmpeg.exe"
                } else {
                    "ffmpeg"
                }
            }
            Self::Ffprobe => {
                if cfg!(windows) {
                    "ffprobe.exe"
                } else {
                    "ffprobe"
                }
            }
            Self::Pandoc => {
                if cfg!(windows) {
                    "pandoc.exe"
                } else {
                    "pandoc"
                }
            }
            Self::Typst => {
                if cfg!(windows) {
                    "typst.exe"
                } else {
                    "typst"
                }
            }
        }
    }

    fn version_arg(self) -> &'static str {
        match self {
            Self::Ffmpeg | Self::Ffprobe => "-version",
            Self::Pandoc | Self::Typst => "--version",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolOrigin {
    Managed,
    Bundled,
    Configured,
    Path,
}

#[derive(Debug, Clone)]
pub struct ResolvedTool {
    pub path: PathBuf,
    pub origin: ToolOrigin,
}

#[derive(Clone)]
pub struct ToolResolver {
    managed: Option<PathBuf>,
    bundled: PathBuf,
    configured: Option<PathBuf>,
    search_path: Vec<PathBuf>,
}

impl ToolResolver {
    pub fn from_environment() -> Result<Self> {
        Self::new(
            &std::env::current_exe().context("Cannot locate the application executable")?,
            std::env::var_os("RFC_TOOLS_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from),
            std::env::var_os("PATH"),
        )
        .map(|resolver| resolver.with_managed_root(crate::engines::managed_root()))
    }

    pub fn new(
        executable: &Path,
        configured: Option<PathBuf>,
        path: Option<OsString>,
    ) -> Result<Self> {
        if !executable.is_absolute() || configured.as_ref().is_some_and(|path| !path.is_absolute())
        {
            bail!("The executable path and RFC_TOOLS_DIR must be absolute paths");
        }
        Ok(Self {
            managed: None,
            bundled: executable
                .parent()
                .context("Application has no parent directory")?
                .join("tools"),
            configured,
            // Empty and relative PATH entries must not accidentally search the current folder.
            search_path: path
                .as_deref()
                .map(std::env::split_paths)
                .into_iter()
                .flatten()
                .filter(|path| path.is_absolute())
                .collect(),
        })
    }

    pub fn resolve(&self, tool: Tool) -> Result<ResolvedTool> {
        self.resolve_with(tool, |path, tool| {
            if cfg!(windows) && tool == Tool::Typst && path == self.bundled.join(tool.file_name()) {
                crate::engines::verify_bundled_typst(path).context("The supplied Typst PDF engine is damaged; extract the complete application ZIP again")?;
            }
            probe(path, tool)
        })
    }

    pub fn with_managed_root(mut self, root: Option<PathBuf>) -> Self {
        self.managed = root;
        self
    }

    fn resolve_with(
        &self,
        tool: Tool,
        mut check: impl FnMut(&Path, Tool) -> Result<()>,
    ) -> Result<ResolvedTool> {
        let supplied_typst = if tool == Tool::Typst {
            match std::fs::symlink_metadata(self.bundled.join(tool.file_name())) {
                Ok(_) => true,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                Err(error) => return Err(error).context("Cannot inspect supplied Typst"),
            }
        } else {
            false
        };
        if let Some(root) = self.managed.as_ref().filter(|_| !supplied_typst) {
            if let Some(path) = crate::engines::managed_executable(root, tool)
                .context("Managed engine is damaged or obsolete; use Repair engines")?
            {
                check(&path, tool)?;
                return Ok(ResolvedTool {
                    path,
                    origin: ToolOrigin::Managed,
                });
            }
        }
        let candidates = std::iter::once((&self.bundled, ToolOrigin::Bundled))
            .chain(
                self.configured
                    .iter()
                    .map(|path| (path, ToolOrigin::Configured)),
            )
            .chain(self.search_path.iter().map(|path| (path, ToolOrigin::Path)));
        for (directory, origin) in candidates {
            let path = directory.join(tool.file_name());
            match std::fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("Cannot inspect {}: {}", tool.name(), path.display())
                    })
                }
                Ok(metadata)
                    if !(metadata.is_file() || metadata.is_symlink() && path.is_file()) =>
                {
                    bail!(
                        "{} is not a regular executable file: {}",
                        tool.name(),
                        path.display()
                    )
                }
                Ok(_) => {}
            }
            // A corrupt higher-priority executable is an error, not a silent version switch.
            check(&path, tool).with_context(|| format!(
                "{} is present but unusable: {}. Repair the portable tools folder or the selected tool.",
                tool.name(), path.display()
            ))?;
            return Ok(ResolvedTool { path, origin });
        }
        bail!(
            "{} is missing. Open Conversion engines and choose Download & Continue or Repair engines. Offline/source builds may use tools/, RFC_TOOLS_DIR or PATH.",
            tool.name()
        )
    }

    pub fn diagnostics(&self) -> Vec<ToolDiagnostic> {
        Tool::ALL
            .into_iter()
            .map(|tool| ToolDiagnostic {
                tool,
                result: self.resolve(tool).map_err(|error| format!("{error:#}")),
            })
            .collect()
    }
}

#[derive(Debug)]
pub struct ToolDiagnostic {
    pub tool: Tool,
    pub result: std::result::Result<ResolvedTool, String>,
}

/// Bounded launch check, with no stdout/stderr pipes that could deadlock.
fn probe(path: &Path, tool: Tool) -> Result<()> {
    let mut command = Command::new(path);
    command
        .arg(tool.version_arg())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().context("Executable could not be started")?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => bail!("Version check failed ({status})"),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                result.context("Cannot check executable status")?;
                bail!("Version check exceeded five seconds");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture(root: &Path, directory: &str) -> Result<PathBuf> {
        let folder = root.join(directory);
        fs::create_dir_all(&folder)?;
        let file = folder.join(Tool::Ffmpeg.file_name());
        fs::write(&file, b"fake executable; launcher is injected")?;
        Ok(file)
    }

    fn resolver(
        root: &Path,
        configured: Option<PathBuf>,
        path: &[PathBuf],
    ) -> Result<ToolResolver> {
        ToolResolver::new(
            &root.join("app").join("converter.exe"),
            configured,
            Some(std::env::join_paths(path)?),
        )
    }

    #[test]
    fn supplied_typst_precedes_even_a_damaged_managed_generation() -> Result<()> {
        let root = tempfile::tempdir()?;
        let tools = root.path().join("app/tools");
        fs::create_dir_all(&tools)?;
        let supplied = tools.join(Tool::Typst.file_name());
        fs::write(&supplied, b"injected launch check")?;
        let managed = root.path().join("user tools");
        fs::create_dir_all(managed.join("typst"))?;
        fs::write(
            managed.join("typst/active.json"),
            b"obsolete damaged marker",
        )?;
        let resolver = resolver(root.path(), None, &[])?.with_managed_root(Some(managed));
        let found = resolver.resolve_with(Tool::Typst, |path, _| {
            assert_eq!(path, supplied);
            Ok(())
        })?;
        assert_eq!(found.origin, ToolOrigin::Bundled);
        assert!(resolver
            .resolve_with(Tool::Typst, |_, _| bail!("damaged supplied engine"))
            .is_err());
        Ok(())
    }

    #[cfg(windows)]
    #[test]
    fn damaged_supplied_typst_is_rejected_before_launch() -> Result<()> {
        let root = tempfile::tempdir()?;
        let tools = root.path().join("app/tools");
        fs::create_dir_all(&tools)?;
        fs::write(tools.join("typst.exe"), b"invalid executable")?;
        let error = resolver(root.path(), None, &[])?
            .resolve(Tool::Typst)
            .unwrap_err();
        assert!(format!("{error:#}").contains("supplied Typst PDF engine is damaged"));
        Ok(())
    }

    #[test]
    fn bundled_tool_has_priority_over_configured_and_path() -> Result<()> {
        let root = tempfile::tempdir()?;
        let bundled = fixture(root.path(), "app/tools")?;
        fixture(root.path(), "configured")?;
        fixture(root.path(), "system")?;
        let mut checked = Vec::new();
        let result = resolver(
            root.path(),
            Some(root.path().join("configured")),
            &[root.path().join("system")],
        )?
        .resolve_with(Tool::Ffmpeg, |path, _| {
            checked.push(path.to_path_buf());
            Ok(())
        })?;
        assert_eq!(result.path, bundled);
        assert_eq!(result.origin, ToolOrigin::Bundled);
        assert_eq!(checked.len(), 1);
        Ok(())
    }

    #[test]
    fn configured_directory_precedes_path_when_bundled_tool_is_absent() -> Result<()> {
        let root = tempfile::tempdir()?;
        let configured = fixture(root.path(), "configured")?;
        fixture(root.path(), "system")?;
        let result = resolver(
            root.path(),
            Some(root.path().join("configured")),
            &[root.path().join("system")],
        )?
        .resolve_with(Tool::Ffmpeg, |_, _| Ok(()))?;
        assert_eq!(result.path, configured);
        assert_eq!(result.origin, ToolOrigin::Configured);
        Ok(())
    }

    #[test]
    fn path_is_used_only_when_higher_priority_tools_are_absent() -> Result<()> {
        let root = tempfile::tempdir()?;
        let system = fixture(root.path(), "system")?;
        let result = resolver(root.path(), None, &[root.path().join("system")])?
            .resolve_with(Tool::Ffmpeg, |_, _| Ok(()))?;
        assert_eq!(result.path, system);
        assert_eq!(result.origin, ToolOrigin::Path);
        Ok(())
    }

    #[test]
    fn spaces_and_unicode_are_preserved_without_a_working_directory_dependency() -> Result<()> {
        let root = tempfile::tempdir()?;
        let executable = root.path().join("Portable été 日本").join("converter.exe");
        let expected = fixture(executable.parent().unwrap(), "tools")?;
        let resolver = ToolResolver::new(&executable, None, None)?;
        assert_eq!(
            resolver
                .resolve_with(Tool::Ffmpeg, |path, _| {
                    assert_eq!(path, expected);
                    Ok(())
                })?
                .path,
            expected
        );
        Ok(())
    }

    #[test]
    fn existing_but_invalid_executable_is_rejected_without_silent_fallback() -> Result<()> {
        let root = tempfile::tempdir()?;
        fixture(root.path(), "app/tools")?;
        fixture(root.path(), "system")?;
        let error = resolver(root.path(), None, &[root.path().join("system")])?
            .resolve(Tool::Ffmpeg)
            .unwrap_err();
        assert!(format!("{error:#}").contains("present but unusable"));
        Ok(())
    }

    #[test]
    fn invalid_managed_installation_never_falls_back_to_adjacent_or_system_tools() -> Result<()> {
        let root = tempfile::tempdir()?;
        fixture(root.path(), "app/tools")?;
        fixture(root.path(), "system")?;
        let managed = root.path().join("user tools");
        fs::create_dir_all(managed.join("ffmpeg"))?;
        fs::write(managed.join("ffmpeg/active.json"), b"damaged marker")?;
        let error = resolver(root.path(), None, &[root.path().join("system")])?
            .with_managed_root(Some(managed))
            .resolve_with(Tool::Ffmpeg, |_, _| {
                panic!("must not silently switch tools")
            })
            .unwrap_err();
        assert!(format!("{error:#}").contains("Repair engines"));
        Ok(())
    }

    #[test]
    fn missing_tool_and_relative_search_directories_are_rejected() -> Result<()> {
        let root = tempfile::tempdir()?;
        let error = resolver(root.path(), None, &[PathBuf::from("."), PathBuf::new()])?
            .resolve_with(Tool::Ffmpeg, |_, _| {
                panic!("missing tools must not be launched")
            })
            .unwrap_err();
        assert!(error.to_string().contains("missing"));
        assert!(
            ToolResolver::new(&root.path().join("app.exe"), Some("relative".into()), None).is_err()
        );
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn real_script_launch_checks_status_and_execute_permission() -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let root = tempfile::tempdir()?;
        let file = fixture(root.path(), "app/tools")?;
        fs::write(&file, b"#!/bin/sh\nexit 0\n")?;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o755))?;
        let resolver = resolver(root.path(), None, &[])?;
        assert!(resolver.resolve(Tool::Ffmpeg).is_ok());
        fs::write(&file, b"#!/bin/sh\nexit 1\n")?;
        assert!(resolver.resolve(Tool::Ffmpeg).is_err());
        fs::set_permissions(&file, fs::Permissions::from_mode(0o644))?;
        assert!(resolver.resolve(Tool::Ffmpeg).is_err());
        fs::remove_file(&file)?;
        std::os::unix::fs::symlink(root.path().join("missing-target"), &file)?;
        assert!(resolver.resolve(Tool::Ffmpeg).is_err());
        Ok(())
    }
}
