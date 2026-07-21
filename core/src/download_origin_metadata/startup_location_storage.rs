use std::fs;

use anyhow::Result;

use crate::installs::startup_deeplink_path;

pub struct StartupDeeplinkStorage {}

impl StartupDeeplinkStorage {
    pub fn has() -> bool {
        let path = startup_deeplink_path();
        let exists = path.exists();
        log::info!(
            "Startup deeplink file {} exists: {}",
            path.display(),
            exists
        );
        exists
    }

    pub fn write(deeplink: &str) -> Result<()> {
        let path = startup_deeplink_path();
        log::info!("Writing startup deeplink to {}: {}", path.display(), deeplink);
        fs::write(path, deeplink)?;
        Ok(())
    }

    pub fn read() -> Option<String> {
        let path = startup_deeplink_path();
        match fs::read_to_string(&path) {
            Ok(raw) => {
                let trimmed = raw.trim().to_string();
                if trimmed.is_empty() {
                    log::info!("Startup deeplink file {} is empty", path.display());
                    None
                } else {
                    log::info!(
                        "Read startup deeplink from {}: {}",
                        path.display(),
                        trimmed
                    );
                    Some(trimmed)
                }
            }
            Err(e) => {
                log::info!(
                    "No startup deeplink file at {} ({})",
                    path.display(),
                    e.kind()
                );
                None
            }
        }
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
