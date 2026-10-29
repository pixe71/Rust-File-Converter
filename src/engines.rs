//! Pinned, consent-driven Windows engine installation. No network I/O on lookup.
use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::tools::{Tool, ToolResolver};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct EngineSpec {
    pub name: String,
    pub version: String,
    pub url: String,
    pub sha256: String,
    pub executables: Vec<String>,
    pub binary_sha256: HashMap<String, String>,
    pub archive_root: String,
    pub binary_directory: String,
    pub notices: Vec<String>,
    pub notice_sha256: HashMap<String, String>,
    pub license: String,
    pub source_url: String,
    #[serde(default)]
    pub source_notices: Vec<SourceNotice>,
    #[serde(default = "automatic_download_default")]
    pub automatic_download: bool,
}

fn automatic_download_default() -> bool {
    true
}

#[derive(Deserialize)]
struct StaticTypst {
    version: String,
    binary_sha256: String,
    target: String,
}

pub fn verify_bundled_typst(path: &Path) -> Result<()> {
    let pinned: StaticTypst =
        serde_json::from_str(include_str!("../packaging/typst-static.lock.json"))?;
    ensure!(
        pinned.version == "0.15.1"
            && pinned.target == "x86_64-pc-windows-msvc"
            && valid_hash(&pinned.binary_sha256),
        "Invalid bundled Typst manifest"
    );
    verify_managed_hash(path, &pinned.binary_sha256)?;
    verify_pe64(path)
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SourceNotice {
    pub path: String,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct EngineManifest {
    pub platform: String,
    pub tools: Vec<EngineSpec>,
}

impl EngineManifest {
    pub fn pinned() -> Result<Self> {
        let manifest: Self =
            serde_json::from_str(include_str!("../packaging/windows-tools.lock.json"))?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<()> {
        ensure!(
            self.platform == "windows-x86_64",
            "Unsupported engine manifest"
        );
        ensure!(self.tools.len() == 3, "Expected three engine archives");
        let mut files = HashSet::new();
        for spec in &self.tools {
            if spec.name == "Typst" {
                ensure!(
                    !spec.automatic_download,
                    "Typst must be supplied by the static bundle"
                );
            }
            ensure!(
                ["FFmpeg", "Pandoc", "Typst"].contains(&spec.name.as_str()),
                "Unknown engine"
            );
            ensure!(
                spec.url.starts_with("https://github.com/") && !spec.url.contains("/latest"),
                "Unpinned engine URL"
            );
            ensure!(valid_hash(&spec.sha256), "Invalid archive checksum");
            safe_relative(&spec.archive_root)?;
            safe_relative(&spec.version)?;
            for file in &spec.executables {
                ensure!(files.insert(file.clone()), "Duplicate engine executable");
                safe_relative(file)?;
                ensure!(
                    valid_hash(
                        spec.binary_sha256
                            .get(file)
                            .context("Missing binary checksum")?
                    ),
                    "Invalid binary checksum"
                );
            }
            for notice in &spec.notices {
                safe_relative(notice)?;
                ensure!(
                    valid_hash(
                        spec.notice_sha256
                            .get(notice)
                            .context("Missing notice checksum")?
                    ),
                    "Invalid notice checksum"
                );
            }
        }
        ensure!(
            files
                == ["ffmpeg.exe", "ffprobe.exe", "pandoc.exe", "typst.exe"]
                    .map(String::from)
                    .into_iter()
                    .collect(),
            "Unexpected engine files"
        );
        Ok(())
    }
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// None means automatic installation is unavailable; manual local engines still work.
pub fn managed_root() -> Option<PathBuf> {
    #[cfg(windows)]
    let directory = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    #[cfg(not(windows))]
    let directory: Option<PathBuf> = None;
    directory
        .filter(|path| path.is_absolute())
        .map(|path| path.join("RustFileConverter").join("tools"))
}

#[derive(Debug, Serialize, Deserialize)]
struct ActiveInstallation {
    version: String,
    archive_sha256: String,
    directory: String,
}

fn group_directory(root: &Path, spec: &EngineSpec) -> PathBuf {
    root.join(spec.name.to_ascii_lowercase())
}

/// Missing marker is absent. An invalid marker/executable is an explicit error.
pub fn managed_executable(root: &Path, tool: Tool) -> Result<Option<PathBuf>> {
    let manifest = EngineManifest::pinned()?;
    managed_executable_in(root, tool, &manifest)
}

fn managed_executable_in(
    root: &Path,
    tool: Tool,
    manifest: &EngineManifest,
) -> Result<Option<PathBuf>> {
    // The downloaded manifest is Windows-specific; never execute it on other hosts.
    let filename = match tool {
        Tool::Ffmpeg => "ffmpeg.exe",
        Tool::Ffprobe => "ffprobe.exe",
        Tool::Pandoc => "pandoc.exe",
        Tool::Typst => "typst.exe",
    };
    let spec = manifest
        .tools
        .iter()
        .find(|spec| spec.executables.iter().any(|file| file == filename))
        .context("Engine not in manifest")?;
    let folder = group_directory(root, spec);
    let marker = folder.join("active.json");
    match fs::symlink_metadata(&marker) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
        Ok(metadata) => ensure!(
            metadata.is_file() && !metadata.is_symlink() && metadata.len() < 4096,
            "Invalid managed engine marker"
        ),
    }
    let active: ActiveInstallation = serde_json::from_slice(&fs::read(marker)?)?;
    ensure!(
        active.version == spec.version && active.archive_sha256 == spec.sha256,
        "Managed engine version differs from this release; use Repair engines"
    );
    ensure!(
        active.directory.starts_with("install-") && !active.directory.contains(['/', '\\']),
        "Invalid installation directory"
    );
    safe_relative(&active.directory)?;
    let installation = folder.join(active.directory);
    ensure!(
        !fs::symlink_metadata(&installation)?.is_symlink(),
        "Linked managed installation is not permitted"
    );
    let executable = installation.join(filename);
    verify_managed_hash(&executable, &spec.binary_sha256[filename])?;
    verify_pe64(&executable)?;
    Ok(Some(executable))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    use zip::write::SimpleFileOptions;

    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }

    fn zip_bytes(entries: &[(&str, &[u8])]) -> Result<Vec<u8>> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, bytes) in entries {
            writer.start_file(*name, SimpleFileOptions::default())?;
            writer.write_all(bytes)?;
        }
        Ok(writer.finish()?.into_inner())
    }

    fn fixture() -> Result<(EngineSpec, Vec<u8>)> {
        let mut spec = EngineManifest::pinned()?.tools.remove(0);
        let mut pe = vec![0; 256];
        pe[..2].copy_from_slice(b"MZ");
        pe[60..64].copy_from_slice(&128_u32.to_le_bytes());
        pe[128..134].copy_from_slice(b"PE\0\0\x64\x86");
        spec.archive_root = "package".into();
        spec.binary_directory.clear();
        spec.notices = vec!["LICENSE".into()];
        spec.notice_sha256 = HashMap::from([("LICENSE".into(), digest(b"original license\r\n"))]);
        spec.binary_sha256 = spec
            .executables
            .iter()
            .map(|file| (file.clone(), digest(&pe)))
            .collect();
        let zip = zip_bytes(&[
            ("package/ffmpeg.exe", &pe),
            ("package/ffprobe.exe", &pe),
            ("package/LICENSE", b"original license\r\n"),
        ])?;
        spec.sha256 = digest(&zip);
        Ok((spec, zip))
    }

    fn install(
        manager: &EngineManager,
        spec: &EngineSpec,
        bytes: &[u8],
        validate: impl FnOnce(&Path, &EngineSpec) -> Result<()>,
    ) -> Result<()> {
        manager.install_with(
            spec,
            &AtomicBool::new(false),
            &mut |_| {},
            |file, _, _| {
                file.write_all(bytes)?;
                Ok(())
            },
            validate,
        )
    }

    #[test]
    fn manifest_is_pinned_and_complete_without_network() -> Result<()> {
        let manifest = EngineManifest::pinned()?;
        assert_eq!(
            manifest
                .tools
                .iter()
                .map(|spec| spec.version.as_str())
                .collect::<Vec<_>>(),
            ["9.0.2", "3.13", "0.15.1"]
        );
        let mut invalid = manifest.clone();
        invalid.tools[0].url = "https://github.com/vendor/latest.exe".into();
        assert!(invalid.validate().is_err());
        invalid = manifest;
        invalid.tools[0].binary_sha256.clear();
        assert!(invalid.validate().is_err());
        Ok(())
    }

    #[test]
    fn downloader_requires_https_and_limits_redirects() {
        let agent = download_agent();
        assert!(agent.config().https_only());
        assert_eq!(agent.config().max_redirects(), 5);
        assert!(agent.config().max_redirects_will_error());
        let error = agent
            .get("http://127.0.0.1:9/not-permitted")
            .call()
            .unwrap_err();
        assert!(format!("{error:?}").to_ascii_lowercase().contains("https"));
    }

    #[test]
    fn checked_atomic_installation_reuses_offline_with_unicode_and_spaces() -> Result<()> {
        let root = tempfile::tempdir()?;
        let manager = EngineManager::new(root.path().join("Local été 日本/tools"))?;
        let (spec, bytes) = fixture()?;
        let parent = group_directory(manager.root(), &spec);
        install(&manager, &spec, &bytes, |folder, _| {
            assert!(!parent.join("active.json").exists());
            verify_pe64(&folder.join("ffmpeg.exe"))?;
            assert_eq!(
                fs::read(folder.join("licenses/LICENSE"))?,
                b"original license\r\n"
            );
            Ok(())
        })?;
        let manifest = EngineManifest {
            platform: "windows-x86_64".into(),
            tools: vec![spec],
        };
        let first = managed_executable_in(manager.root(), Tool::Ffmpeg, &manifest)?.unwrap();
        let second = managed_executable_in(manager.root(), Tool::Ffmpeg, &manifest)?.unwrap();
        assert_eq!(first, second);
        assert!(first.exists());
        assert!(!fs::read_dir(parent)?.any(|entry| entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".rust-file-converter")));
        Ok(())
    }

    #[test]
    fn incorrect_checksum_never_extracts_or_executes_and_cleans_only_owned_files() -> Result<()> {
        let root = tempfile::tempdir()?;
        let manager = EngineManager::new(root.path().to_owned())?;
        let (spec, _) = fixture()?;
        let parent = group_directory(root.path(), &spec);
        fs::create_dir_all(&parent)?;
        fs::write(parent.join("user.zip"), b"keep")?;
        let error = install(&manager, &spec, b"wrong", |_, _| {
            panic!("bad checksum must not execute")
        })
        .unwrap_err();
        assert!(error.to_string().contains("SHA-256"));
        assert_eq!(fs::read(parent.join("user.zip"))?, b"keep");
        assert_eq!(fs::read_dir(parent)?.count(), 1);
        Ok(())
    }

    #[test]
    fn corrupt_archive_and_wrong_version_never_activate() -> Result<()> {
        let root = tempfile::tempdir()?;
        let manager = EngineManager::new(root.path().to_owned())?;
        let (mut spec, bytes) = fixture()?;
        let expected = spec.sha256.clone();
        spec.sha256 = digest(b"not a zip");
        assert!(install(&manager, &spec, b"not a zip", |_, _| panic!(
            "corrupt ZIP must not execute"
        ))
        .is_err());
        spec.sha256 = expected;
        assert!(install(&manager, &spec, &bytes, |_, spec| validate_version(
            "ffmpeg version 9.0.20",
            spec
        ))
        .is_err());
        assert!(!group_directory(root.path(), &spec)
            .join("active.json")
            .exists());
        Ok(())
    }

    #[test]
    fn failed_repair_preserves_old_installation_and_success_replaces_only_marker() -> Result<()> {
        let root = tempfile::tempdir()?;
        let manager = EngineManager::new(root.path().to_owned())?;
        let (spec, bytes) = fixture()?;
        install(&manager, &spec, &bytes, |_, _| Ok(()))?;
        let marker = group_directory(root.path(), &spec).join("active.json");
        let original = fs::read(&marker)?;
        let old: ActiveInstallation = serde_json::from_slice(&original)?;
        assert!(install(&manager, &spec, &bytes, |_, _| bail!("validation failed")).is_err());
        assert_eq!(fs::read(&marker)?, original);
        install(&manager, &spec, &bytes, |_, _| Ok(()))?;
        assert_ne!(fs::read(&marker)?, original);
        assert!(marker
            .parent()
            .unwrap()
            .join(old.directory)
            .join("ffmpeg.exe")
            .exists());
        // A broken regular app marker can itself be repaired.
        fs::write(&marker, b"interrupted marker")?;
        install(&manager, &spec, &bytes, |_, _| Ok(()))?;
        let _: ActiveInstallation = serde_json::from_slice(&fs::read(marker)?)?;
        Ok(())
    }

    #[test]
    fn interrupted_download_and_cancellation_leave_no_active_or_partial_installation() -> Result<()>
    {
        let root = tempfile::tempdir()?;
        let manager = EngineManager::new(root.path().to_owned())?;
        let (spec, _) = fixture()?;
        let error = manager.install_with(
            &spec,
            &AtomicBool::new(false),
            &mut |_| {},
            |file, _, _| {
                file.write_all(b"partial")?;
                bail!("connection reset")
            },
            |_, _| panic!("must not validate"),
        );
        assert!(error.is_err());
        assert_eq!(
            fs::read_dir(group_directory(root.path(), &spec))?.count(),
            0
        );
        let error = manager.install_with(
            &spec,
            &AtomicBool::new(true),
            &mut |_| {},
            |file, cancel, notify| {
                copy_download(Cursor::new(b"data"), file, &spec, None, cancel, notify)
            },
            |_, _| panic!("cancelled"),
        );
        assert!(error.unwrap_err().to_string().contains("cancelled"));
        assert_eq!(
            fs::read_dir(group_directory(root.path(), &spec))?.count(),
            0
        );
        Ok(())
    }

    #[test]
    fn archive_paths_aliases_and_symlinks_are_rejected() -> Result<()> {
        let root = tempfile::tempdir()?;
        let manager = EngineManager::new(root.path().to_owned())?;
        let (mut spec, _) = fixture()?;
        for name in [
            "../escape",
            "/absolute",
            "C:/escape",
            "a/../b",
            "a\\b",
            "a/NUL.txt",
            "a/end.",
            "a/end ",
            "a//b",
        ] {
            assert!(safe_relative(name).is_err(), "{name}");
            let bytes = zip_bytes(&[(name, b"bad")])?;
            spec.sha256 = digest(&bytes);
            assert!(install(&manager, &spec, &bytes, |_, _| panic!("must reject paths")).is_err());
        }
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer.add_symlink("package/link", "../outside", SimpleFileOptions::default())?;
        let bytes = writer.finish()?.into_inner();
        spec.sha256 = digest(&bytes);
        assert!(install(&manager, &spec, &bytes, |_, _| panic!("must reject links")).is_err());
        let bytes = zip_bytes(&[("A", b"a"), ("a", b"b")])?;
        spec.sha256 = digest(&bytes);
        assert!(install(&manager, &spec, &bytes, |_, _| panic!(
            "must reject aliases"
        ))
        .is_err());
        assert!(!root.path().join("escape").exists());
        Ok(())
    }

    #[test]
    fn local_http_mock_streams_without_internet_and_detects_truncation() -> Result<()> {
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let server = std::thread::spawn(move || -> std::io::Result<()> {
            let (mut stream, _) = listener.accept()?;
            let mut request = [0; 4096];
            let count = stream.read(&mut request)?;
            assert!(count > 0);
            stream.write_all(
                b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\ndata",
            )?;
            Ok(())
        });
        let mut response = ureq::get(format!("http://{address}/archive")).call()?;
        let (spec, _) = fixture()?;
        let mut file = tempfile::tempfile()?;
        copy_download(
            response.body_mut().as_reader(),
            &mut file,
            &spec,
            Some(4),
            &AtomicBool::new(false),
            &mut |_| {},
        )?;
        server.join().unwrap()?;
        assert!(copy_download(
            Cursor::new(b"short"),
            &mut file,
            &spec,
            Some(10),
            &AtomicBool::new(false),
            &mut |_| {}
        )
        .is_err());
        Ok(())
    }

    #[test]
    fn version_boundaries_identity_and_architecture_are_checked() -> Result<()> {
        let spec = EngineManifest::pinned()?.tools.remove(0);
        for text in ["ffmpeg version 9.0.2-essentials", "ffprobe version 9.0.2"] {
            validate_version(text, &spec)?;
        }
        for text in [
            "ffmpeg version 9.0.20",
            "other version 9.0.2",
            "ffmpeg version 8.0",
        ] {
            assert!(validate_version(text, &spec).is_err());
        }
        let mut file = tempfile::NamedTempFile::new()?;
        file.write_all(&[0; 256])?;
        assert!(verify_pe64(file.path()).is_err());
        Ok(())
    }

    #[test]
    fn managed_integrity_cache_rechecks_changes_and_replacements() -> Result<()> {
        let root = tempfile::tempdir()?;
        let path = root.path().join("engine.exe");
        fs::write(&path, b"good")?;
        let expected = digest(b"good");
        verify_managed_hash(&path, &expected)?;
        verify_managed_hash(&path, &expected)?;
        let modified = fs::metadata(&path)?.modified()?;
        let mut replacement = tempfile::NamedTempFile::new_in(root.path())?;
        replacement.write_all(b"evil")?;
        replacement.as_file().set_modified(modified)?;
        replacement.persist(&path)?;
        assert!(verify_managed_hash(&path, &expected).is_err());
        fs::write(&path, b"changed length")?;
        assert!(verify_managed_hash(&path, &expected).is_err());
        Ok(())
    }

    #[test]
    fn typst_supplemental_notices_keep_official_bytes_and_locked_checksums() -> Result<()> {
        let root = tempfile::tempdir()?;
        let mut spec = EngineManifest::pinned()?.tools.remove(2);
        install_extra_notices(root.path(), &spec)?;
        for notice in &spec.source_notices {
            verify_hash(
                &root.path().join("licenses").join(&notice.path),
                &notice.sha256,
            )?;
        }
        spec.source_notices[0].sha256 = "0".repeat(64);
        let empty = tempfile::tempdir()?;
        assert!(install_extra_notices(empty.path(), &spec).is_err());
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct EngineProgress {
    pub engine: String,
    pub phase: String,
    pub downloaded: u64,
    pub total: Option<u64>,
}

fn progress(spec: &EngineSpec, phase: &str) -> EngineProgress {
    EngineProgress {
        engine: spec.name.clone(),
        phase: phase.into(),
        downloaded: 0,
        total: None,
    }
}

#[derive(Clone)]
pub struct EngineManager {
    root: PathBuf,
    manifest: EngineManifest,
}

impl EngineManager {
    pub fn from_environment() -> Result<Self> {
        ensure!(cfg!(all(windows, target_arch = "x86_64")), "Automatic preparation is available on Windows x64; local tools remain supported on this platform");
        Self::new(managed_root().context("Cannot locate your local application data folder")?)
    }

    pub fn new(root: PathBuf) -> Result<Self> {
        ensure!(
            root.is_absolute(),
            "Engine storage must be an absolute path"
        );
        Ok(Self {
            root,
            manifest: EngineManifest::pinned()?,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Call only after explicit network consent, on a worker thread.
    pub fn prepare(
        &self,
        resolver: &ToolResolver,
        repair: bool,
        cancelled: &AtomicBool,
        mut notify: impl FnMut(EngineProgress),
    ) -> Result<()> {
        let agent = download_agent();
        // Check the supplied PDF engine first, before downloading other archives.
        for spec in self
            .manifest
            .tools
            .iter()
            .filter(|spec| !spec.automatic_download)
        {
            ensure!(resolver.resolve(Tool::Typst).is_ok(), "The supplied Typst PDF engine is missing or damaged. Extract the complete application ZIP again; no runtime installer is required.");
            notify(progress(spec, "Ready"));
        }
        for spec in &self.manifest.tools {
            check_cancel(cancelled)?;
            if !spec.automatic_download {
                continue;
            }
            let available = Tool::ALL
                .into_iter()
                .filter(|tool| {
                    spec.executables
                        .iter()
                        .any(|file| file == windows_filename(*tool))
                })
                .all(|tool| resolver.resolve(tool).is_ok());
            if !repair && available {
                notify(progress(spec, "Ready"));
                continue;
            }
            self.install_with(spec, cancelled, &mut notify,
                |file, cancelled, notify| {
                    let mut response = agent.get(&spec.url).header("User-Agent", "RustFileConverter/1.1.0").call()
                        .context("Download failed. Check your Internet connection and try again")?;
                    let total = response.headers().get("content-length").and_then(|header| header.to_str().ok()).and_then(|value| value.parse().ok());
                    copy_download(response.body_mut().as_reader(), file, spec, total, cancelled, notify)
                }, validate_installation)
                .with_context(|| format!("{} could not be prepared. Check your Internet connection and available disk space, then retry", spec.name))?;
        }
        Ok(())
    }

    fn install_with(
        &self,
        spec: &EngineSpec,
        cancelled: &AtomicBool,
        notify: &mut dyn FnMut(EngineProgress),
        download: impl FnOnce(&mut File, &AtomicBool, &mut dyn FnMut(EngineProgress)) -> Result<()>,
        validate: impl FnOnce(&Path, &EngineSpec) -> Result<()>,
    ) -> Result<()> {
        let parent = group_directory(&self.root, spec);
        fs::create_dir_all(&parent)
            .context("Cannot create engine storage; check permissions and disk space")?;
        let staging = tempfile::Builder::new()
            .prefix(".rust-file-converter-download-")
            .tempdir_in(&parent)?;
        let archive = staging.path().join("download.zip");
        let mut output = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&archive)?;
        notify(progress(spec, "Downloading"));
        download(&mut output, cancelled, notify)?;
        output.sync_all()?;
        drop(output);
        check_cancel(cancelled)?;
        notify(progress(spec, "Verifying SHA-256"));
        verify_hash(&archive, &spec.sha256)?;
        let extracted = staging.path().join("extracted");
        fs::create_dir(&extracted)?;
        notify(progress(spec, "Extracting"));
        extract_selected(&archive, &extracted, spec, cancelled)?;
        install_extra_notices(&extracted, spec)?;
        let manifest = File::create(extracted.join("installation-manifest.json"))?;
        serde_json::to_writer_pretty(&manifest, spec)?;
        manifest.sync_all()?;
        notify(progress(spec, "Checking engines"));
        validate(&extracted, spec)?;
        check_cancel(cancelled)?;
        // Keep the complete generation in the same filesystem. Only a complete
        // generation can be referenced by the atomically replaced active marker.
        let generation = tempfile::Builder::new()
            .prefix("install-")
            .tempdir_in(&parent)?;
        for entry in fs::read_dir(&extracted)? {
            let entry = entry?;
            fs::rename(entry.path(), generation.path().join(entry.file_name()))?;
        }
        let active = ActiveInstallation {
            version: spec.version.clone(),
            archive_sha256: spec.sha256.clone(),
            directory: generation
                .path()
                .file_name()
                .context("Missing generation name")?
                .to_string_lossy()
                .into_owned(),
        };
        let mut marker = tempfile::NamedTempFile::new_in(&parent)?;
        serde_json::to_writer(marker.as_file_mut(), &active)?;
        marker.as_file_mut().sync_all()?;
        // Never remove a user's file/symlink masquerading as the active marker.
        let active_path = parent.join("active.json");
        if let Ok(metadata) = fs::symlink_metadata(&active_path) {
            ensure!(
                metadata.is_file() && !metadata.is_symlink(),
                "Unsafe existing active marker"
            );
        }
        marker.persist(&active_path).map_err(|error| error.error)?;
        let _retained = generation.keep();
        notify(progress(spec, "Ready"));
        Ok(())
    }
}

fn windows_filename(tool: Tool) -> &'static str {
    match tool {
        Tool::Ffmpeg => "ffmpeg.exe",
        Tool::Ffprobe => "ffprobe.exe",
        Tool::Pandoc => "pandoc.exe",
        Tool::Typst => "typst.exe",
    }
}

fn install_extra_notices(folder: &Path, spec: &EngineSpec) -> Result<()> {
    for notice in &spec.source_notices {
        safe_relative(&notice.path)?;
        let bytes: &[u8] = match (spec.name.as_str(), notice.path.as_str()) {
            ("Typst", "assets/LICENSE") => include_bytes!("../licenses/Typst/assets/LICENSE"),
            ("Typst", "assets/NOTICE") => include_bytes!("../licenses/Typst/assets/NOTICE"),
            _ => bail!("Unrecognized supplemental engine notice"),
        };
        ensure!(
            format!("{:x}", Sha256::digest(bytes)) == notice.sha256,
            "Supplemental engine notice checksum mismatch"
        );
        let destination = folder.join("licenses").join(&notice.path);
        fs::create_dir_all(destination.parent().context("Missing notice directory")?)?;
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(destination)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    Ok(())
}

fn check_cancel(cancelled: &AtomicBool) -> Result<()> {
    ensure!(
        !cancelled.load(Ordering::Relaxed),
        "Engine preparation cancelled"
    );
    Ok(())
}

fn copy_download(
    mut reader: impl Read,
    output: &mut File,
    spec: &EngineSpec,
    total: Option<u64>,
    cancelled: &AtomicBool,
    notify: &mut dyn FnMut(EngineProgress),
) -> Result<()> {
    const MAX_ARCHIVE: u64 = 512 * 1024 * 1024;
    ensure!(
        total.is_none_or(|size| size <= MAX_ARCHIVE),
        "Download is unexpectedly large"
    );
    let mut count = 0;
    let mut buffer = [0; 64 * 1024];
    let mut last = Instant::now();
    loop {
        check_cancel(cancelled)?;
        let size = reader
            .read(&mut buffer)
            .context("Download interrupted or timed out; retry preparation")?;
        if size == 0 {
            break;
        }
        count += size as u64;
        ensure!(count <= MAX_ARCHIVE, "Download exceeds safety limit");
        output
            .write_all(&buffer[..size])
            .context("Cannot save download; check available disk space")?;
        if last.elapsed() >= Duration::from_millis(100) {
            notify(EngineProgress {
                downloaded: count,
                total,
                ..progress(spec, "Downloading")
            });
            last = Instant::now();
        }
    }
    ensure!(
        total.is_none_or(|expected| expected == count),
        "Download is incomplete"
    );
    notify(EngineProgress {
        downloaded: count,
        total,
        ..progress(spec, "Downloading")
    });
    Ok(())
}

pub fn file_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

fn verify_hash(path: &Path, expected: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Missing engine file: {}", path.display()))?;
    ensure!(
        metadata.is_file() && !metadata.is_symlink(),
        "Engine file must be a regular file"
    );
    ensure!(
        file_sha256(path)? == expected,
        "SHA-256 mismatch for {}; the file will not be executed",
        path.display()
    );
    Ok(())
}

struct VerifiedFile {
    identity: u64,
    size: u64,
    modified: std::time::SystemTime,
    expected: String,
}

// Managed generations are immutable. Rehash at each process launch and whenever
// identity, length, timestamp or pinned checksum changes; avoid reading hundreds
// of MB again for every file in a conversion batch.
fn verify_managed_hash(path: &Path, expected: &str) -> Result<()> {
    static VERIFIED: OnceLock<Mutex<HashMap<PathBuf, VerifiedFile>>> = OnceLock::new();
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.is_symlink(),
        "Invalid managed executable"
    );
    let modified = metadata.modified()?;
    let identity = {
        let handle = same_file::Handle::from_path(path)?;
        let mut hasher = DefaultHasher::new();
        handle.hash(&mut hasher);
        hasher.finish()
    };
    let cache = VERIFIED.get_or_init(Default::default);
    {
        let cache = cache
            .lock()
            .map_err(|_| anyhow::anyhow!("Engine integrity cache unavailable"))?;
        if cache.get(path).is_some_and(|entry| {
            entry.identity == identity
                && entry.size == metadata.len()
                && entry.modified == modified
                && entry.expected == expected
        }) {
            return Ok(());
        }
    }
    verify_hash(path, expected)?;
    let mut cache = cache
        .lock()
        .map_err(|_| anyhow::anyhow!("Engine integrity cache unavailable"))?;
    if cache.len() >= 32 {
        cache.clear();
    }
    cache.insert(
        path.to_owned(),
        VerifiedFile {
            identity,
            size: metadata.len(),
            modified,
            expected: expected.into(),
        },
    );
    Ok(())
}

fn safe_relative(name: &str) -> Result<PathBuf> {
    ensure!(
        !name
            .chars()
            .any(|ch| ch.is_ascii_control() || "<>\"|?*".contains(ch)),
        "Unsafe archive characters"
    );
    ensure!(
        !name.is_empty() && !name.starts_with('/') && !name.contains(['\\', ':', '\0']),
        "Unsafe archive path: {name}"
    );
    let trimmed = name.strip_suffix('/').unwrap_or(name);
    for part in trimmed.split('/') {
        ensure!(
            !part.is_empty() && part != "." && part != ".." && !part.ends_with(['.', ' ']),
            "Unsafe archive component: {name}"
        );
        let base = part.split('.').next().unwrap_or("").to_ascii_uppercase();
        ensure!(
            !["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"].contains(&base.as_str())
                && !(base.len() == 4
                    && (base.starts_with("COM") || base.starts_with("LPT"))
                    && base.as_bytes()[3].is_ascii_digit()),
            "Reserved archive path"
        );
    }
    Ok(PathBuf::from(trimmed))
}

fn extract_selected(
    archive: &Path,
    output: &Path,
    spec: &EngineSpec,
    cancelled: &AtomicBool,
) -> Result<()> {
    let mut zip =
        zip::ZipArchive::new(File::open(archive)?).context("Downloaded archive is corrupt")?;
    ensure!(zip.len() < 100_000, "Archive has too many entries");
    let mut selected = HashMap::new();
    for file in &spec.executables {
        let source = [
            spec.archive_root.as_str(),
            spec.binary_directory.as_str(),
            file.as_str(),
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
        selected.insert(source, (PathBuf::from(file), &spec.binary_sha256[file]));
    }
    for notice in &spec.notices {
        selected.insert(
            format!("{}/{notice}", spec.archive_root),
            (
                PathBuf::from("licenses").join(notice),
                &spec.notice_sha256[notice],
            ),
        );
    }
    let mut seen = HashSet::new();
    let mut found = HashSet::new();
    let mut total = 0_u64;
    for index in 0..zip.len() {
        check_cancel(cancelled)?;
        let mut entry = zip.by_index(index)?;
        safe_relative(entry.name())?;
        ensure!(
            seen.insert(entry.name().trim_end_matches('/').to_ascii_lowercase()),
            "Duplicate archive path"
        );
        let mode = entry.unix_mode().unwrap_or(0) & 0o170000;
        ensure!(
            mode == 0 || mode == 0o100000 || mode == 0o040000,
            "Archive contains a link or special file"
        );
        total = total
            .checked_add(entry.size())
            .context("Archive size overflow")?;
        ensure!(
            total <= 2 * 1024 * 1024 * 1024,
            "Extracted archive exceeds safety limit"
        );
        if let Some((relative, expected)) = selected.get(entry.name()) {
            ensure!(
                !entry.is_dir() && mode != 0o040000,
                "Expected a regular archive file"
            );
            found.insert(entry.name().to_owned());
            let destination = output.join(relative);
            fs::create_dir_all(destination.parent().context("Missing extraction parent")?)?;
            let mut target = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&destination)?;
            let mut buffer = [0; 64 * 1024];
            loop {
                check_cancel(cancelled)?;
                let count = entry.read(&mut buffer)?;
                if count == 0 {
                    break;
                }
                target.write_all(&buffer[..count])?;
            }
            target.sync_all()?;
            drop(target);
            verify_hash(&destination, expected)?;
        }
    }
    ensure!(
        found.len() == selected.len(),
        "Archive is missing an expected executable or license"
    );
    Ok(())
}

fn verify_pe64(path: &Path) -> Result<()> {
    let mut file = File::open(path)?;
    let mut dos = [0; 64];
    file.read_exact(&mut dos)?;
    ensure!(&dos[..2] == b"MZ", "Not a Windows executable");
    let offset = u32::from_le_bytes(dos[60..64].try_into()?) as u64;
    file.seek(SeekFrom::Start(offset))?;
    let mut header = [0; 6];
    file.read_exact(&mut header)?;
    ensure!(
        &header == b"PE\0\0\x64\x86",
        "Engine is not a Windows x64 executable"
    );
    Ok(())
}

/// Bounded child execution without pipe deadlocks; output files are application-owned.
fn capture(path: &Path, arguments: &[&str]) -> Result<String> {
    let mut stdout = tempfile::tempfile()?;
    let stderr = tempfile::tempfile()?;
    let mut command = Command::new(path);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(stdout.try_clone()?)
        .stderr(stderr);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn().context("Cannot start downloaded engine")?;
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                ensure!(status.success(), "Engine validation failed ({status})");
                break;
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            result => {
                let _ = child.kill();
                let _ = child.wait();
                result.context("Cannot inspect engine")?;
                bail!("Engine validation timed out");
            }
        }
    }
    stdout.seek(SeekFrom::Start(0))?;
    let mut text = String::new();
    stdout.take(1024 * 1024).read_to_string(&mut text)?;
    Ok(text)
}

fn validate_version(text: &str, spec: &EngineSpec) -> Result<()> {
    let first = text.lines().next().unwrap_or("");
    let prefix = match spec.name.as_str() {
        "FFmpeg" => {
            if first.starts_with("ffprobe ") {
                "ffprobe version "
            } else {
                "ffmpeg version "
            }
        }
        "Pandoc" => "pandoc ",
        _ => "typst ",
    };
    let version = first
        .strip_prefix(prefix)
        .context("Unexpected engine identity")?;
    ensure!(
        version
            .strip_prefix(&spec.version)
            .is_some_and(|tail| tail.is_empty() || tail.starts_with([' ', '-'])),
        "Downloaded engine version does not match {}",
        spec.version
    );
    Ok(())
}

fn validate_installation(folder: &Path, spec: &EngineSpec) -> Result<()> {
    for filename in &spec.executables {
        let path = folder.join(filename);
        verify_hash(&path, &spec.binary_sha256[filename])?;
        verify_pe64(&path)?;
        let argument = if spec.name == "FFmpeg" {
            "-version"
        } else {
            "--version"
        };
        validate_version(&capture(&path, &[argument])?, spec)?;
        if filename == "ffmpeg.exe" {
            let encoders = capture(&path, &["-hide_banner", "-encoders"])?;
            for required in [
                "libmp3lame",
                "aac",
                "pcm_s16le",
                "flac",
                "libvorbis",
                "libx264",
                "libx265",
            ] {
                ensure!(
                    encoders
                        .lines()
                        .any(|line| line.split_whitespace().nth(1) == Some(required)),
                    "FFmpeg is missing the required {required} encoder"
                );
            }
        }
    }
    Ok(())
}

fn download_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .tls_config(
            ureq::tls::TlsConfig::builder()
                .provider(ureq::tls::TlsProvider::NativeTls)
                .build(),
        )
        .https_only(true)
        .max_redirects(5)
        .max_redirects_will_error(true)
        .timeout_global(Some(Duration::from_secs(20 * 60)))
        .timeout_connect(Some(Duration::from_secs(30)))
        .timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .build()
        .into()
}
