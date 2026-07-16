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

    pub fn read() -> Option<String> {
        fs::read_to_string(startup_location_path())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn clear() {
        let path = startup_location_path();
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("Cannot delete startup location file: {e}"),
        }
    }
}
