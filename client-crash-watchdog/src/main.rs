//! `dcl_watchdog`: started by the launcher right after it spawned the Explorer. Waits for that
//! process to exit; a non-zero exit writes a `CrashAttachment` and reopens the launcher with
//! `--crash-report-with-attachment <path>` unless the user opted out of the dialog.

// Avoid popup terminal window
#![windows_subsystem = "windows"]

mod process_wait;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use dcl_launcher_core::{
    analytics::{Analytics, event::Event},
    anyhow::{Context, Result, anyhow},
    config,
    crash_attachment::CrashAttachment,
    crash_watchdog::{ARG_EXPLORER_VERSION, ARG_LAUNCHER_EXE, ARG_PID, ARG_SESSION_ID},
    environment::ARG_CRASH_REPORT_WITH_ATTACHMENT,
    log, logs,
};
use process_wait::ExitOutcome;

const EVENT_SEND_TIMEOUT: Duration = Duration::from_secs(5);

/// Guards against pid reuse between the launcher reading the pid and the watchdog attaching:
/// the Explorer's main executable is `Decentraland` / `Decentraland.exe`.
const EXPLORER_PROCESS_NAME_FRAGMENT: &str = "decentraland";

#[derive(Debug, PartialEq, Eq)]
struct WatchConfig {
    pid: u32,
    session_id: String,
    explorer_version: String,
    launcher_exe: PathBuf,
}

impl WatchConfig {
    fn parse(args: &[String]) -> Result<Self> {
        Ok(Self {
            pid: value_of(args, ARG_PID)?
                .parse()
                .context("--pid is not a number")?,
            session_id: value_of(args, ARG_SESSION_ID)?.to_owned(),
            explorer_version: value_of(args, ARG_EXPLORER_VERSION)?.to_owned(),
            launcher_exe: PathBuf::from(value_of(args, ARG_LAUNCHER_EXE)?),
        })
    }
}

fn value_of<'a>(args: &'a [String], flag: &str) -> Result<&'a str> {
    let position = args
        .iter()
        .position(|a| a == flag)
        .ok_or_else(|| anyhow!("{flag} is not provided"))?;
    args.get(position.saturating_add(1))
        .map(String::as_str)
        .filter(|value| !value.starts_with("--"))
        .ok_or_else(|| anyhow!("{flag} has no value"))
}

fn main() {
    if let Err(e) = logs::dispath_logs() {
        eprintln!("Cannot initialize logs: {e}");
        std::process::exit(1);
    }
    if let Err(e) = run() {
        log::error!("Crash watchdog stopped with an error: {e:#}");
    }
}

fn run() -> Result<()> {
    log::info!("Start dcl_watchdog v{}", std::env!("CARGO_PKG_VERSION"));

    let args: Vec<String> = std::env::args().collect();
    log::info!("Args: {args:?}");
    let config = WatchConfig::parse(&args)?;

    ensure_is_explorer(config.pid)?;

    log::info!(
        "Watching Explorer pid {} (session {}, version {})",
        config.pid,
        config.session_id,
        config.explorer_version
    );

    match process_wait::wait_for_exit(config.pid)? {
        ExitOutcome::Clean => {
            log::info!("Explorer pid {} exited cleanly", config.pid);
            Ok(())
        }
        ExitOutcome::Unexpected { code } => on_unexpected_exit(&config, code),
    }
}

fn ensure_is_explorer(pid: u32) -> Result<()> {
    let system = sysinfo::System::new_all();
    let process = system
        .process(sysinfo::Pid::from_u32(pid))
        .ok_or_else(|| anyhow!("Process {pid} is not running; nothing to watch"))?;
    let name = process.name().to_string_lossy().to_lowercase();
    if name.contains(EXPLORER_PROCESS_NAME_FRAGMENT) {
        return Ok(());
    }
    Err(anyhow!(
        "Process {pid} is `{name}`, not the Explorer; refusing to watch a reused pid"
    ))
}

fn on_unexpected_exit(config: &WatchConfig, exit_code: String) -> Result<()> {
    let dialog_suppressed = config::crash_report_dialog_disabled();
    log::warn!(
        "Explorer pid {} exited unexpectedly ({exit_code}); dialog suppressed: {dialog_suppressed}",
        config.pid
    );

    track_unexpected_exit(config, &exit_code, dialog_suppressed);

    let attachment = CrashAttachment {
        session_id: config.session_id.clone(),
        explorer_version: config.explorer_version.clone(),
        exit_code,
        crashed_at: now_secs(),
        pid: config.pid,
    };
    let path = attachment.write()?;
    log::info!("Crash attachment written to {}", path.display());

    if dialog_suppressed {
        log::info!("User opted out of the crash dialog; not reopening the launcher");
        return Ok(());
    }

    open_launcher(&config.launcher_exe, &path)
}

fn track_unexpected_exit(config: &WatchConfig, exit_code: &str, dialog_suppressed: bool) {
    let event = Event::EXPLORER_UNEXPECTED_EXIT {
        session_id: config.session_id.clone(),
        explorer_version: config.explorer_version.clone(),
        exit_code: exit_code.to_owned(),
        dialog_suppressed,
    };

    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            log::error!("Cannot build a runtime for analytics, event dropped: {e}");
            return;
        }
    };

    runtime.block_on(async move {
        let mut analytics = Analytics::new_from_env();
        analytics.track_and_flush_silent(event).await;
        analytics.cleanup_within(EVENT_SEND_TIMEOUT).await;
    });
}

fn open_launcher(launcher_exe: &Path, attachment: &Path) -> Result<()> {
    log::info!(
        "Reopening launcher {} for the crash report",
        launcher_exe.display()
    );
    Command::new(launcher_exe)
        .arg(format!("--{ARG_CRASH_REPORT_WITH_ATTACHMENT}"))
        .arg(attachment)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| format!("Cannot start {}", launcher_exe.display()))?;
    Ok(())
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn parses_all_four_flags_in_any_order() {
        let parsed = WatchConfig::parse(&args(&[
            "dcl_watchdog",
            "--launcher-exe",
            "/x/launcher",
            "--pid",
            "42",
            "--explorer-version",
            "v1.2.3",
            "--session-id",
            "sid",
        ]))
        .unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(
            parsed,
            WatchConfig {
                pid: 42,
                session_id: "sid".to_owned(),
                explorer_version: "v1.2.3".to_owned(),
                launcher_exe: PathBuf::from("/x/launcher"),
            }
        );
    }

    #[test]
    fn missing_or_invalid_flags_are_errors() {
        assert!(WatchConfig::parse(&args(&["dcl_watchdog"])).is_err());
        assert!(
            WatchConfig::parse(&args(&[
                "dcl_watchdog",
                "--pid",
                "abc",
                "--session-id",
                "s",
                "--explorer-version",
                "v",
                "--launcher-exe",
                "/l",
            ]))
            .is_err()
        );
        // A flag directly followed by another flag has no value.
        assert!(
            WatchConfig::parse(&args(&[
                "dcl_watchdog",
                "--pid",
                "--session-id",
                "s",
                "--explorer-version",
                "v",
                "--launcher-exe",
                "/l",
            ]))
            .is_err()
        );
    }
}
