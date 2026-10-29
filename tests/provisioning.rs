//! Explicit network test. Unit tests never download vendor archives.
#![cfg(all(windows, target_arch = "x86_64"))]
mod common;
use anyhow::{ensure, Context, Result};
use file_converter::engines::{file_sha256, EngineManager};
use file_converter::tools::{Tool, ToolOrigin, ToolResolver};
use std::{collections::HashMap, fs, path::PathBuf, process::Command, sync::atomic::AtomicBool};

#[test]
#[ignore = "downloads FFmpeg/Pandoc into isolated storage beside supplied static Typst"]
fn automatic_provisioning_with_empty_path() -> Result<()> {
    let temporary = tempfile::tempdir()?;
    let root = std::env::var_os("RFC_PROVISIONING_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|| temporary.path().to_owned());
    let app = root.join("Application été 日本");
    let user_data = root.join("Données utilisateur");
    fs::create_dir_all(&app)?;
    let executable = app.join("provision-test.exe");
    fs::copy(std::env::current_exe()?, &executable)?;
    ensure!(
        !app.join("tools").exists(),
        "This test needs a fresh application directory"
    );
    fs::create_dir(app.join("tools"))?;
    let typst = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".cache/typst-static/typst.exe");
    file_converter::engines::verify_bundled_typst(&typst)?;
    fs::copy(typst, app.join("tools/typst.exe"))?;
    ensure!(
        !user_data.join("RustFileConverter").exists(),
        "This test needs empty user storage"
    );
    fs::write(
        root.join(".rust-file-converter-provisioning-test"),
        b"owned test workspace",
    )?;
    let elsewhere = tempfile::tempdir()?;
    for mode in ["online", "offline", "repair"] {
        let mut command = Command::new(&executable);
        command
            .args(["--ignored", "--exact", "provisioning_child", "--nocapture"])
            .env("RFC_PROVISIONING_CHILD", mode)
            .env("RFC_PROVISIONING_WORKSPACE", &root)
            .env("PATH", "")
            .env_remove("RFC_TOOLS_DIR")
            .env("LOCALAPPDATA", &user_data)
            .env("TYPST_PACKAGE_CACHE_PATH", root.join("empty-typst-cache"))
            .current_dir(elsewhere.path());
        if mode == "offline" {
            // A mistaken network request must fail; the offline pass must not download.
            command
                .env("HTTPS_PROXY", "http://127.0.0.1:9")
                .env("HTTP_PROXY", "http://127.0.0.1:9")
                .env("ALL_PROXY", "http://127.0.0.1:9");
        }
        let output = command.output()?;
        let text = String::from_utf8_lossy(&output.stdout).to_string()
            + &String::from_utf8_lossy(&output.stderr);
        println!("{mode}:\n{text}");
        ensure!(
            output.status.success(),
            "Provisioning child failed ({})",
            output.status
        );
    }
    Ok(())
}

#[test]
#[ignore = "internal isolated subprocess of automatic_provisioning_with_empty_path"]
fn provisioning_child() -> Result<()> {
    let mode =
        std::env::var("RFC_PROVISIONING_CHILD").context("Run the provisioning parent test")?;
    ensure!(
        ["online", "offline", "repair"].contains(&mode.as_str()),
        "Invalid child mode"
    );
    ensure!(
        std::env::var_os("PATH").unwrap_or_default().is_empty(),
        "PATH must be empty"
    );
    ensure!(
        std::env::var_os("RFC_TOOLS_DIR").is_none(),
        "Configured tools must be absent"
    );
    let resolver = ToolResolver::from_environment()?;
    let manager = EngineManager::from_environment()?;
    let workspace = PathBuf::from(
        std::env::var_os("RFC_PROVISIONING_WORKSPACE").context("Missing isolated workspace")?,
    );
    ensure!(
        workspace
            .join(".rust-file-converter-provisioning-test")
            .is_file(),
        "Run the parent test"
    );
    ensure!(
        manager.root() == workspace.join("Données utilisateur/RustFileConverter/tools"),
        "Refusing to modify real user storage"
    );
    if mode == "online" {
        ensure!(
            resolver
                .diagnostics()
                .iter()
                .all(|diagnostic| if diagnostic.tool == Tool::Typst {
                    diagnostic.result.is_ok()
                } else {
                    diagnostic.result.is_err()
                }),
            "Expected only supplied Typst to be ready"
        );
        // Native image decoding works before consent/preparation as well.
        let folder = tempfile::tempdir()?;
        let picture = folder.path().join("native.png");
        image::RgbImage::new(16, 12).save(&picture)?;
        ensure!(image::open(picture).is_ok());
    }
    let mut downloaded = HashMap::new();
    let mut phases = HashMap::new();
    let repair = mode == "repair";
    let mut old_markers = HashMap::new();
    let mut old_tools = Vec::new();
    if repair {
        for name in ["FFmpeg", "Pandoc"] {
            old_markers.insert(
                name.to_owned(),
                fs::read(
                    manager
                        .root()
                        .join(name.to_ascii_lowercase())
                        .join("active.json"),
                )?,
            );
        }
        for tool in Tool::ALL {
            let path = resolver.resolve(tool)?.path;
            old_tools.push((tool, path.clone(), file_sha256(&path)?));
        }
        let ffmpeg = &old_tools[0].1;
        ensure!(
            ffmpeg.starts_with(manager.root()),
            "Only corrupt an isolated test engine"
        );
        fs::write(ffmpeg, b"intentionally corrupted isolated test engine")?;
        ensure!(
            resolver.resolve(Tool::Ffmpeg).is_err(),
            "Corruption was not detected"
        );
        println!("Corrupt engine rejected before repair");
    }
    let before = if mode == "offline" {
        Some(engine_hashes(&resolver)?)
    } else {
        None
    };
    manager.prepare(&resolver, repair, &AtomicBool::new(false), |progress| {
        if repair && progress.engine != "Typst" {
            let marker = manager
                .root()
                .join(progress.engine.to_ascii_lowercase())
                .join("active.json");
            let current = fs::read(marker).expect("old marker must remain available");
            if progress.phase == "Ready" {
                assert_ne!(current, old_markers[&progress.engine]);
            } else {
                assert_eq!(current, old_markers[&progress.engine]);
            }
        }
        if phases.get(&progress.engine) != Some(&progress.phase) {
            println!("{}: {}", progress.engine, progress.phase);
            phases.insert(progress.engine.clone(), progress.phase.clone());
        }
        if progress.downloaded > 0 {
            downloaded.insert(progress.engine, progress.downloaded);
        }
    })?;
    if repair {
        for (tool, path, hash) in old_tools {
            ensure!(path.is_file(), "Repair deleted an older generation");
            if tool != Tool::Ffmpeg {
                ensure!(
                    file_sha256(&path)? == hash,
                    "Repair changed a healthy old engine"
                );
            }
            if tool == Tool::Typst {
                ensure!(
                    resolver.resolve(tool)?.path == path,
                    "Repair changed supplied Typst"
                );
            } else {
                ensure!(
                    resolver.resolve(tool)?.path != path,
                    "Repair did not activate a new generation"
                );
            }
        }
        println!("Repair validated before activation; old generations preserved");
    }
    if mode == "offline" {
        ensure!(downloaded.is_empty(), "Offline mode attempted a download");
        ensure!(
            before.as_ref() == Some(&engine_hashes(&resolver)?),
            "Offline mode changed engines"
        );
    } else {
        ensure!(
            downloaded.len() == 2 && !downloaded.contains_key("Typst"),
            "Expected only FFmpeg and Pandoc downloads"
        );
        println!(
            "Downloaded archive bytes: {}",
            downloaded.values().sum::<u64>()
        );
    }
    for tool in Tool::ALL {
        let selected = resolver.resolve(tool)?;
        ensure!(
            selected.origin
                == if tool == Tool::Typst {
                    ToolOrigin::Bundled
                } else {
                    ToolOrigin::Managed
                },
            "Engine did not use expected hybrid origin"
        );
        println!("{}: {}", tool.name(), selected.path.display());
    }
    common::verify_conversions(&resolver)?;
    Ok(())
}

fn engine_hashes(resolver: &ToolResolver) -> Result<Vec<String>> {
    Tool::ALL
        .into_iter()
        .map(|tool| file_sha256(&resolver.resolve(tool)?.path))
        .collect()
}
