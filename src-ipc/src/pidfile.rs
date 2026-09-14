use std::fs;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sysinfo::Pid;

#[cfg(windows)]
pub const SERVICE_PROCESS_NAME: &str = "dcl_launcher_service.exe";
#[cfg(unix)]
pub const SERVICE_PROCESS_NAME: &str = "dcl_launcher_service";

/// Contents of `APP_DIR/current-service-pid.txt`.
///
/// A bare pid is unsafe on Windows (pids get recycled), so liveness always
/// validates the process name too — same guard `RunningInstances` uses for
/// Explorer pids.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServicePid {
    pub pid: u32,
    pub process_name: String,
    pub version: String,
    pub started_at_unix_secs: u64,
}

pub fn path() -> PathBuf {
    dcl_launcher_shared::app_dir().join("current-service-pid.txt")
}

pub fn write_own(version: &str) -> Result<()> {
    let entry = ServicePid {
        pid: std::process::id(),
        process_name: SERVICE_PROCESS_NAME.to_owned(),
        version: version.to_owned(),
        started_at_unix_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or_default(),
    };
    let payload =
        serde_json::to_string_pretty(&entry).context("Cannot serialize the service pid entry")?;
    fs::write(path(), payload).context("Cannot write the service pid file")?;
    Ok(())
}

/// `None` when the file is absent or unreadable — both mean "not running".
pub fn read() -> Option<ServicePid> {
    let raw = fs::read_to_string(path()).ok()?;
    match serde_json::from_str(&raw) {
        Ok(entry) => Some(entry),
        Err(e) => {
            log::warn!("Cannot parse the service pid file, treating as absent: {e}");
            None
        }
    }
}

pub fn remove() {
    if let Err(e) = fs::remove_file(path()) {
        if e.kind() != std::io::ErrorKind::NotFound {
            log::warn!("Cannot remove the service pid file: {e}");
        }
    }
}

pub fn is_alive(entry: &ServicePid) -> bool {
    let system = sysinfo::System::new_all();
    let Some(process) = system.process(Pid::from_u32(entry.pid)) else {
        return false;
    };
    process
        .name()
        .to_str()
        .is_some_and(|name| name == entry.process_name)
}

/// Kills the recorded process after re-validating pid + name. Returns whether
/// a kill signal was actually sent.
pub fn kill(entry: &ServicePid) -> bool {
    let system = sysinfo::System::new_all();
    let Some(process) = system.process(Pid::from_u32(entry.pid)) else {
        return false;
    };
    let name_matches = process
        .name()
        .to_str()
        .is_some_and(|name| name == entry.process_name);
    if !name_matches {
        log::warn!(
            "Not killing pid {}: process name mismatch (recycled pid?)",
            entry.pid
        );
        return false;
    }
    process.kill()
}

pub async fn wait_dead(entry: &ServicePid, timeout: Duration) -> bool {
    const POLL_INTERVAL: Duration = Duration::from_millis(200);
    let Some(deadline) = tokio::time::Instant::now().checked_add(timeout) else {
        return !is_alive(entry);
    };
    loop {
        if !is_alive(entry) {
            return true;
        }
        if tokio::time::Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use serde_json::{Value, json};

    #[test]
    fn entry_roundtrip_and_shape() -> Result<()> {
        let entry = ServicePid {
            pid: 4242,
            process_name: SERVICE_PROCESS_NAME.to_owned(),
            version: "1.20.0".to_owned(),
            started_at_unix_secs: 1_754_000_000,
        };

        let expected: Value = json!({
            "pid": 4242,
            "processName": SERVICE_PROCESS_NAME,
            "version": "1.20.0",
            "startedAtUnixSecs": 1_754_000_000,
        });

        assert_eq!(serde_json::to_value(&entry)?, expected);

        let raw = serde_json::to_string(&entry)?;
        let parsed: ServicePid = serde_json::from_str(&raw)?;
        assert_eq!(parsed, entry);
        Ok(())
    }

    #[test]
    fn truncated_content_is_treated_as_absent() {
        let parsed: std::result::Result<ServicePid, _> = serde_json::from_str("{\"pid\": 42");
        assert!(parsed.is_err());
    }

    #[test]
    fn dead_pid_is_not_alive() {
        let entry = ServicePid {
            // Huge pid that cannot exist on either OS.
            pid: u32::MAX - 3,
            process_name: SERVICE_PROCESS_NAME.to_owned(),
            version: "1.20.0".to_owned(),
            started_at_unix_secs: 0,
        };
        assert!(!is_alive(&entry));
    }
}
