use anyhow::{bail, Context, Result};
use std::collections::VecDeque;
use std::io::Read;
use std::process::{Child, Command, Stdio};

fn configure(command: &mut Command) {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
}

// Also reap a child if reading stderr fails or the worker unwinds.
struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

pub(super) fn ffmpeg_encoders(ffmpeg: &std::path::Path) -> Result<String> {
    let mut command = Command::new(ffmpeg);
    configure(&mut command);
    let output = command
        .args(["-hide_banner", "-encoders"])
        .stdout(Stdio::piped())
        .output()
        .with_context(|| format!("Cannot inspect FFmpeg encoders: {}", ffmpeg.display()))?;
    if !output.status.success() {
        let diagnostics = read_diagnostics(output.stderr.as_slice())?;
        bail!(
            "{}",
            failure_message(
                "FFmpeg encoder detection",
                output.status,
                &diagnostics,
                "Requested capability: non-experimental libvorbis audio encoder"
            )
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn failure_message(
    label: &str,
    status: std::process::ExitStatus,
    bytes: &[u8],
    details: &str,
) -> String {
    let diagnostics = String::from_utf8_lossy(bytes);
    let cause = diagnostics.lines().find(|line| {
        let lower = line.to_ascii_lowercase();
        [
            "error",
            "failed",
            "invalid",
            "not found",
            "not supported",
            "unknown encoder",
        ]
        .iter()
        .any(|word| lower.contains(word))
    });
    let summary = cause.map(|line| line.trim().chars().take(160).collect::<String>());
    let cause = summary.map_or_else(String::new, |summary| format!(": {summary}"));
    format!("{label} conversion failed ({status}){cause}\n{details}\nstderr (up to the last 64 KiB):\n{diagnostics}")
}

pub(super) fn run_with_details(command: &mut Command, label: &str, details: &str) -> Result<()> {
    configure(command);
    let child = command.spawn().with_context(|| {
        format!("{label} could not be started. Open Conversion engines and choose Repair engines. Offline/source builds may use tools/, RFC_TOOLS_DIR or PATH.\n{details}")
    })?;
    let mut child = ChildGuard(child);
    let mut stderr = child
        .0
        .stderr
        .take()
        .context("Cannot capture conversion diagnostics")?;
    let bytes = read_diagnostics(&mut stderr).context("Cannot read conversion diagnostics")?;
    let status = child
        .0
        .wait()
        .context("Cannot wait for conversion process")?;
    if !status.success() {
        bail!("{}", failure_message(label, status, &bytes, details));
    }
    Ok(())
}

fn read_diagnostics(mut reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut tail = VecDeque::with_capacity(64 * 1024);
    let mut buffer = [0; 8192];
    loop {
        let count = match reader.read(&mut buffer) {
            Ok(count) => count,
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        if count == 0 {
            break;
        }
        for byte in &buffer[..count] {
            if tail.len() == 64 * 1024 {
                tail.pop_front();
            }
            tail.push_back(*byte);
        }
    }
    Ok(tail.into_iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failure_keeps_exit_code_stderr_and_requested_conversion() {
        #[cfg(windows)]
        let status = {
            use std::os::windows::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(0xffffffea)
        };
        #[cfg(unix)]
        let status = {
            use std::os::unix::process::ExitStatusExt;
            std::process::ExitStatus::from_raw(22 << 8)
        };
        let stderr =
            b"Guessed Channel Layout: mono\n[libvorbis] encoder setup failed\nInvalid argument\n";
        let details = "Requested output: ogg; audio codec: libvorbis; rate control: VBR quality 5";
        let error = failure_message("FFmpeg", status, stderr, details);
        assert!(error.contains(&status.to_string()));
        assert!(error
            .lines()
            .next()
            .unwrap()
            .contains("encoder setup failed"));
        assert!(error.contains(details));
        assert!(error.ends_with(std::str::from_utf8(stderr).unwrap()));
    }

    #[test]
    fn diagnostics_are_bounded_and_keep_the_final_error() -> Result<()> {
        let mut log = vec![b'x'; 100_000];
        log.extend_from_slice(b"final error");
        let tail = read_diagnostics(std::io::Cursor::new(log))?;
        assert_eq!(tail.len(), 64 * 1024);
        assert!(tail.ends_with(b"final error"));
        Ok(())
    }

    #[test]
    fn missing_program_is_an_actionable_error() {
        let error = run_with_details(
            &mut Command::new("rust-file-converter-nonexistent-test-program"),
            "Test engine",
            "",
        )
        .unwrap_err();
        assert!(error.to_string().contains("PATH"));
        assert!(error.chain().count() > 1);
    }
}
