//! Starts `client-crash-watchdog` next to a freshly launched Explorer.
//!
//! The watchdog is a Tauri sidecar (`bundle.externalBin`), so it sits in the same directory as
//! the launcher executable on both platforms. Failing to start it must never fail the launch.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, anyhow};

pub const WATCHDOG_BINARY_NAME: &str = "client-crash-watchdog";

pub const ARG_PID: &str = "--pid";
pub const ARG_SESSION_ID: &str = "--session-id";
pub const ARG_EXPLORER_VERSION: &str = "--explorer-version";
pub const ARG_LAUNCHER_EXE: &str = "--launcher-exe";

pub fn spawn(pid: u32, session_id: &str, explorer_version: &str) {
    match spawn_internal(pid, session_id, explorer_version) {
        Ok(path) => log::info!(
            "Crash watchdog {} attached to Explorer pid {pid}",
            path.display()
        ),
        Err(e) => log::warn!("Crash watchdog not started, crashes will go unreported: {e:#}"),
    }
}

fn spawn_internal(pid: u32, session_id: &str, explorer_version: &str) -> Result<PathBuf> {
    let launcher_exe = std::env::current_exe().context("Cannot resolve the launcher executable")?;
    let watchdog = binary_path(&launcher_exe)?;
    if !watchdog.exists() {
        return Err(anyhow!("{} does not exist", watchdog.display()));
    }

    let mut command = Command::new(&watchdog);
    command
        .args(args_for(pid, session_id, explorer_version, &launcher_exe))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
        .spawn()
        .with_context(|| format!("Cannot spawn {}", watchdog.display()))?;
    Ok(watchdog)
}

fn binary_path(launcher_exe: &std::path::Path) -> Result<PathBuf> {
    let dir = launcher_exe
        .parent()
        .ok_or_else(|| anyhow!("The launcher executable has no parent directory"))?;
    Ok(dir.join(format!(
        "{WATCHDOG_BINARY_NAME}{}",
        std::env::consts::EXE_SUFFIX
    )))
}

fn args_for(
    pid: u32,
    session_id: &str,
    explorer_version: &str,
    launcher_exe: &std::path::Path,
) -> Vec<String> {
    vec![
        ARG_PID.to_owned(),
        pid.to_string(),
        ARG_SESSION_ID.to_owned(),
        session_id.to_owned(),
        ARG_EXPLORER_VERSION.to_owned(),
        explorer_version.to_owned(),
        ARG_LAUNCHER_EXE.to_owned(),
        launcher_exe.to_string_lossy().into_owned(),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn watchdog_sits_next_to_the_launcher() {
        let path = binary_path(Path::new("/apps/Launcher.app/Contents/MacOS/launcher"))
            .unwrap_or_default();
        assert_eq!(
            path,
            Path::new("/apps/Launcher.app/Contents/MacOS").join(format!(
                "{WATCHDOG_BINARY_NAME}{}",
                std::env::consts::EXE_SUFFIX
            ))
        );
    }

    #[test]
    fn args_are_flag_value_pairs() {
        let args = args_for(42, "sid", "v1.2.3", Path::new("/x/launcher"));
        assert_eq!(
            args,
            vec![
                "--pid",
                "42",
                "--session-id",
                "sid",
                "--explorer-version",
                "v1.2.3",
                "--launcher-exe",
                "/x/launcher"
            ]
        );
    }
}
