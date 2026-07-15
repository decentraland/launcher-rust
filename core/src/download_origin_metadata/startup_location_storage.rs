use std::fs;

use anyhow::Result;

use crate::installs::startup_location_path;

/// Persists the startup location (position/realm) for the first Explorer launch.
///
/// The value is extracted from the installer's download-origin metadata and
/// stored as a `decentraland://` deeplink, so the next launcher startup can seed
/// it into `Protocol` and pass it to the Explorer.
pub struct StartupLocationStorage {}

impl StartupLocationStorage {
    pub fn has() -> bool {
        startup_location_path().exists()
    }

    pub fn write(deeplink: &str) -> Result<()> {
        fs::write(startup_location_path(), deeplink)?;
        Ok(())
    }

    /// Read and delete (one-time use). Returns None if file absent or empty.
    pub fn consume() -> Option<String> {
        let path = startup_location_path();
        let value = fs::read_to_string(&path)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())?;
        if let Err(e) = fs::remove_file(&path) {
            log::warn!("Cannot delete startup location file: {e}");
        }
        Some(value)
    }
}
