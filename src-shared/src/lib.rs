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
#![allow(
    clippy::uninlined_format_args,
    clippy::missing_errors_doc,
    clippy::option_if_let_else,
    clippy::single_match_else,
    clippy::must_use_candidate,
    clippy::future_not_send,
    clippy::enum_glob_use
)]

//! Base crate for implementations shared across the launcher processes
//! (core/service, IPC, thin UI): filesystem locations, app version, and
//! macOS dmg detection. This crate depends on NO other launcher crate —
//! core references it, never the other way around.

pub mod macos;

use std::fs::create_dir_all;
use std::path::PathBuf;

use anyhow::Result;

const APP_NAME: &str = "DecentralandLauncherLight";

/// The launcher's data dir (installs, config, bridges, pid file).
///
/// Test-only escape hatch: hermetic e2e runs redirect all launcher state
/// away from the real user profile via `DCL_LAUNCHER_BASE_DIR`. Debug
/// builds only.
#[allow(clippy::expect_used, clippy::missing_panics_doc)]
pub fn app_dir() -> PathBuf {
    #[cfg(debug_assertions)]
    if let Ok(base) = std::env::var("DCL_LAUNCHER_BASE_DIR") {
        if !base.is_empty() {
            let path = PathBuf::from(base).join(APP_NAME);
            create_dir_all(&path).expect("Cannot create app directory");
            return path;
        }
    }
    let path = dirs::data_local_dir()
        .expect("Failed to get current directory")
        .join(APP_NAME);
    create_dir_all(&path).expect("Cannot create app directory");
    path
}

#[must_use]
pub const fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn log_dir() -> Result<PathBuf> {
    let mut path = PathBuf::new();
    if let Some(dir) = dirs::home_dir() {
        path.push(dir);
    }

    #[cfg(target_os = "macos")]
    {
        path.push("Library/Logs");
    }
    #[cfg(target_os = "windows")]
    {
        let dir = std::env::var("APPDATA")?;
        path.push(dir);
    }

    path.push(APP_NAME);
    create_dir_all(&path)?;
    Ok(path)
}

/// The service (via core) owns `output.log`.
pub fn log_file_path() -> Result<PathBuf> {
    Ok(log_dir()?.join("output.log"))
}

/// The thin UI logs to its own file — two processes appending one file
/// interleave.
pub fn ui_log_file_path() -> Result<PathBuf> {
    Ok(log_dir()?.join("output-ui.log"))
}

#[cfg(target_os = "macos")]
pub fn is_running_from_dmg() -> Result<bool> {
    let path = std::env::current_exe()?;
    let dmg_mount_path = macos::dmg_mount_path(&path)?;
    Ok(dmg_mount_path.is_some())
}
