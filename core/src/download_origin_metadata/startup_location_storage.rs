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

    pub fn read() -> Option<String> {
        fs::read_to_string(startup_deeplink_path())
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn clear() {
        let path = startup_deeplink_path();
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("Cannot delete startup location file: {e}"),
        }
    }
}
