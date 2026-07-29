use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::installs::{referrer_bridge_path, referrer_storage_path};

use super::referrer::Referrer;

/// Attribution window. Bounds how long a stored referrer stays valid so a stale
/// attribution can't apply to an unrelated account created much later on the same
/// machine (e.g. a shared computer). Product-tunable; 30 days matches typical
/// referral attribution windows and comfortably covers download → first login.
const ATTRIBUTION_TTL_SECS: u64 = 30 * 24 * 60 * 60;

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

const fn is_expired(stored_at: u64, now: u64) -> bool {
    now.saturating_sub(stored_at) > ATTRIBUTION_TTL_SECS
}

pub struct ReferrerStorage {}

impl ReferrerStorage {
    pub fn read() -> Option<Referrer> {
        let path = referrer_storage_path();
        let content = fs::read_to_string(&path).ok()?;
        let mut lines = content.lines();

        let referrer = Referrer::parse(lines.next()?.trim())?;

        // A stored timestamp older than the attribution window invalidates the
        // referrer. A missing/unparsable timestamp is treated as expired so a
        // malformed file can't grant an unbounded attribution.
        let stored_at: u64 = lines.next()?.trim().parse().ok()?;
        if is_expired(stored_at, now_secs()) {
            return None;
        }

        Some(referrer)
    }

    pub fn has() -> bool {
        Self::read().is_some()
    }

    /// First-wins: an already stored (non-expired) referrer is never overwritten,
    /// so the earliest attribution on the machine is preserved within the window.
    pub fn write(referrer: &Referrer) -> Result<()> {
        if Self::has() {
            return Ok(());
        }
        fs::write(
            referrer_storage_path(),
            format!("{}\n{}", referrer.as_str(), now_secs()),
        )?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_timestamp_is_not_expired() {
        let now = 1_000_000_000;
        assert!(!is_expired(now, now));
        assert!(!is_expired(now - ATTRIBUTION_TTL_SECS, now));
    }

    #[test]
    fn timestamp_past_the_window_is_expired() {
        let now = 1_000_000_000;
        assert!(is_expired(now - ATTRIBUTION_TTL_SECS - 1, now));
    }

    #[test]
    fn clock_skew_into_the_past_does_not_expire() {
        // stored_at in the "future" relative to now: saturating_sub yields 0, not expired.
        let now = 1_000_000_000;
        assert!(!is_expired(now + 5_000, now));
    }
}
