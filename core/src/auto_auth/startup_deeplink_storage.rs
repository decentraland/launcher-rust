use std::fs;

use anyhow::Result;

use crate::installs::startup_deeplink_path;

pub struct StartupDeeplinkStorage {}

impl StartupDeeplinkStorage {
    pub fn has() -> bool {
        startup_deeplink_path().exists()
    }

    pub fn write(deeplink: &str) -> Result<()> {
        fs::write(startup_deeplink_path(), deeplink)?;
        Ok(())
    }

    /// Read and delete (one-time use). Returns None if file absent or empty.
    pub fn consume() -> Option<String> {
        let path = startup_deeplink_path();
        let value = fs::read_to_string(&path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())?;
        if let Err(e) = fs::remove_file(&path) {
            log::warn!("Cannot delete startup deeplink file: {e}");
        }
        Some(value)
    }
}
