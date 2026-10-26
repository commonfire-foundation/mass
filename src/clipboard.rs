//! Explicit local helper integration. Never use a shell, OSC52, or an installer.
use anyhow::{Result, bail};
use std::{
    io::{Seek, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[derive(Clone, Debug)]
pub struct Helper {
    executable: PathBuf,
    wayland: bool,
}
fn executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.is_file()
    }
}
pub fn detect() -> Option<Helper> {
    for (name, wayland, display) in [
        ("wl-copy", true, "WAYLAND_DISPLAY"),
        ("xclip", false, "DISPLAY"),
    ] {
        if std::env::var_os(display).is_none_or(|v| v.is_empty()) {
            continue;
        }
        if let Some(paths) = std::env::var_os("PATH") {
            for directory in std::env::split_paths(&paths) {
                let path = directory.join(name);
                if executable(&path) {
                    return Some(Helper {
                        executable: path.canonicalize().ok()?,
                        wayland,
                    });
                }
            }
        }
    }
    None
}
pub fn copy(helper: &Helper, text: &str, scratch: &Path) -> Result<()> {
    if text.is_empty() {
        bail!("Nothing to copy");
    }
    // An anonymous owner-private file avoids secrets in argv and blocked pipe writes.
    let mut input = tempfile::tempfile_in(scratch)?;
    input.write_all(text.as_bytes())?;
    input.rewind()?;
    let mut command = Command::new(&helper.executable);
    if helper.wayland {
        command.args(["--type", "text/plain;charset=utf-8"]);
    } else {
        command.args(["-selection", "clipboard", "-in"]);
    }
    let mut child = command
        .stdin(Stdio::from(input))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                bail!("Clipboard helper failed; check your desktop clipboard connection");
            }
            return Ok(());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            bail!("Clipboard helper timed out");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    fn fails(script: &str, expected: &str) {
        let directory =
            tempfile::tempdir_in(Path::new(env!("CARGO_MANIFEST_DIR")).join("target")).unwrap();
        let executable = directory.path().join("clipboard-helper");
        std::fs::write(&executable, format!("#!/bin/sh\n{script}\n")).unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
        let helper = Helper {
            executable,
            wayland: true,
        };
        let error = copy(&helper, "synthetic clipboard text", directory.path()).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
    #[test]
    fn helper_failure_is_reported() {
        fails("exit 3", "failed");
    }
    #[test]
    fn helper_timeout_is_bounded() {
        fails("exec /bin/sleep 30", "timed out");
    }
}
