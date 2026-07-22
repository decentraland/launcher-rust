use std::fs;

use anyhow::Result;

use crate::installs::{referrer_bridge_path, referrer_storage_path};

use super::referrer::Referrer;

pub struct ReferrerStorage {}

impl ReferrerStorage {
    pub fn read() -> Option<Referrer> {
        let path = referrer_storage_path();
        let content = fs::read_to_string(&path).ok()?;
        Referrer::parse(content.trim())
    }

    pub fn has() -> bool {
        Self::read().is_some()
    }

    /// First-wins: an already stored referrer is never overwritten,
    /// so the earliest attribution on the machine is preserved.
    pub fn write(referrer: &Referrer) -> Result<()> {
        if Self::has() {
            return Ok(());
        }
        fs::write(referrer_storage_path(), referrer.as_str())?;
        Ok(())
    }

    /// Windows: the download gateway's NSIS wrapper drops `referrer-bridge.txt`
    /// at install time. Move its content into the canonical storage and remove
    /// the bridge file. No-op when the bridge file does not exist (macOS, or
    /// installs without referral attribution).
    pub fn ingest_bridge_file() {
        let bridge = referrer_bridge_path();
        let Ok(content) = fs::read_to_string(&bridge) else {
            return;
        };

        if let Some(referrer) = Referrer::parse(content.trim()) {
            if let Err(e) = Self::write(&referrer) {
                log::error!("Cannot write referrer from bridge file: {e}");
                return;
            }
            log::info!("Referrer ingested from bridge file");
        } else {
            log::warn!("Referrer bridge file content is invalid, discarding");
        }

        if let Err(e) = fs::remove_file(&bridge) {
            log::warn!("Cannot remove referrer bridge file: {e}");
        }
    }
}
