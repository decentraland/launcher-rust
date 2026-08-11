#![warn(clippy::all, clippy::pedantic, clippy::nursery)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::todo,
    clippy::dbg_macro
)]
#![allow(clippy::uninlined_format_args)]

//! Fake Explorer for e2e runs. The launcher installs and launches it exactly
//! like the real client. It records every launch (pid + argv) to
//! `stub-launches.jsonl` in the install dir, consumes the deeplink bridge
//! file unless told not to, and exits when `stub-exit-all.txt` appears
//! (its content is the exit code) or after a safety timeout.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const SAFETY_TIMEOUT: Duration = Duration::from_secs(120);
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// The install ("latest") directory, resolved from the executable location:
/// windows `latest/Decentraland.exe`, macOS
/// `latest/Decentraland.app/Contents/MacOS/Explorer`.
fn install_dir() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    #[cfg(windows)]
    {
        exe.parent().map(Path::to_path_buf).unwrap_or_default()
    }
    #[cfg(unix)]
    {
        exe.ancestors()
            .nth(4)
            .map(Path::to_path_buf)
            .unwrap_or_default()
    }
}

fn append_line(path: &Path, line: &str) {
    let file = OpenOptions::new().create(true).append(true).open(path);
    if let Ok(mut file) = file {
        let _ = writeln!(file, "{line}");
    }
}

fn consume_bridge(dir: &Path) {
    if dir.join("stub-ignore-bridge.txt").exists() {
        return;
    }
    let Some(app_dir) = dir.parent() else {
        return;
    };
    let bridge = app_dir.join("deeplink-bridge.json");
    if !bridge.exists() {
        return;
    }
    let content = fs::read_to_string(&bridge).unwrap_or_default();
    append_line(&dir.join("stub-bridge-log.jsonl"), content.trim());
    let _ = fs::remove_file(&bridge);
}

fn requested_exit_code(dir: &Path) -> Option<i32> {
    let raw = fs::read_to_string(dir.join("stub-exit-all.txt")).ok()?;
    Some(raw.trim().parse().unwrap_or(0))
}

fn main() {
    let dir = install_dir();
    let argv: Vec<String> = std::env::args().collect();
    let record = serde_json::json!({
        "pid": std::process::id(),
        "argv": argv,
    });
    append_line(&dir.join("stub-launches.jsonl"), &record.to_string());

    let Some(deadline) = Instant::now().checked_add(SAFETY_TIMEOUT) else {
        return;
    };

    loop {
        // The sandbox was deleted (test over): disappear immediately instead
        // of lingering until the safety timeout.
        if !dir.exists() {
            return;
        }
        if let Some(code) = requested_exit_code(&dir) {
            std::process::exit(code);
        }
        consume_bridge(&dir);
        if Instant::now() >= deadline {
            return;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}
