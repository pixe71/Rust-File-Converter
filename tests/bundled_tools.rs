mod common;
// Relocate this test executable beside tools/, then run it with an empty PATH.
use anyhow::{Context, Result};
use file_converter::tools::{Tool, ToolOrigin, ToolResolver};
use std::fs;
use std::path::Path;
use std::process::Command;

#[test]
#[ignore = "requires the pinned real engines in the development tools/ folder"]
fn portable_engines_work_without_path() -> Result<()> {
    let local = Path::new(env!("CARGO_MANIFEST_DIR")).join("tools");
    let directory = tempfile::tempdir()?;
    let package = directory.path().join("Portable avec espaces été 日本");
    fs::create_dir_all(package.join("tools"))?;
    for tool in Tool::ALL {
        fs::copy(
            local.join(tool.file_name()),
            package.join("tools").join(tool.file_name()),
        )
        .with_context(|| format!("Prepare {} in tools/ first", tool.name()))?;
    }
    let executable = package.join(if cfg!(windows) {
        "portable-test.exe"
    } else {
        "portable-test"
    });
    fs::copy(std::env::current_exe()?, &executable)?;
    let elsewhere = tempfile::tempdir()?;
    let output = Command::new(&executable)
        .args(["--ignored", "--exact", "portable_child", "--nocapture"])
        .env("RFC_PORTABLE_TEST_CHILD", "1")
        .env_remove("RFC_TOOLS_DIR")
        .env("PATH", "")
        .env(
            "TYPST_PACKAGE_CACHE_PATH",
            directory.path().join("empty-typst-cache"),
        )
        .current_dir(elsewhere.path())
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout).to_string()
            + &String::from_utf8_lossy(&output.stderr)
    );
    let cache = directory.path().join("empty-typst-cache");
    assert!(
        !cache.exists() || fs::read_dir(cache)?.next().is_none(),
        "The default PDF conversion must not need a downloaded Typst package"
    );
    println!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

#[test]
#[ignore = "internal child process of portable_engines_work_without_path"]
fn portable_child() -> Result<()> {
    anyhow::ensure!(
        std::env::var("RFC_PORTABLE_TEST_CHILD").as_deref() == Ok("1"),
        "Run the parent portable test"
    );
    assert_eq!(std::env::var_os("PATH").unwrap_or_default(), "");
    let resolver = ToolResolver::from_environment()?;
    for tool in Tool::ALL {
        let resolved = resolver.resolve(tool)?;
        assert_eq!(resolved.origin, ToolOrigin::Bundled);
        assert!(resolved.path.is_absolute());
        println!("{}: {}", tool.name(), resolved.path.display());
    }
    common::verify_conversions(&resolver)
}
