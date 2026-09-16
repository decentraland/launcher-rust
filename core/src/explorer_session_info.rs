use std::fs;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde::Deserialize;

use crate::infra::signed_fetch::EphemeralIdentity;
use crate::installs::{session_info_files, session_info_path};

/// Session files older than this are leftovers of sessions that never reached the report flow.
pub const STALE_AFTER: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Contents of `session-info-<session_id>.json`, written by the Explorer.
///
/// `{ "version": 1, "session_id": "…", "wallet": "0x…", "explorer_version": "…",
///    "identity": { "address", "key", "expiration", "ephemeralAuthChain" } }`.
/// Not `Serialize` on purpose: the identity must never travel further than this process.
#[derive(Clone, Debug, Deserialize)]
pub struct ExplorerSessionInfo {
    #[serde(default)]
    pub version: u32,
    pub session_id: String,
    pub wallet: String,
    #[serde(default)]
    pub explorer_version: Option<String>,
    #[serde(default)]
    pub identity: Option<EphemeralIdentity>,
}

impl ExplorerSessionInfo {
    /// The stored info only if it belongs to `session_id`.
    pub fn read_for(session_id: &str) -> Option<Self> {
        let info = Self::read_from(&session_info_path(session_id))?;
        Self::matching(info, session_id)
    }

    /// Identity usable for signed fetch right now.
    pub fn valid_identity(&self) -> Option<&EphemeralIdentity> {
        let identity = self.identity.as_ref()?;
        if identity.is_expired_now() {
            log::info!(
                "Ephemeral identity for session {} has expired",
                self.session_id
            );
            return None;
        }
        Some(identity)
    }

    pub fn delete_for(session_id: &str) {
        let path = session_info_path(session_id);
        match fs::remove_file(&path) {
            Ok(()) => log::info!("Session info consumed: {}", path.display()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("Cannot remove session info {}: {e}", path.display()),
        }
    }

    /// Removes session files nobody consumed within [`STALE_AFTER`].
    pub fn sweep_stale() {
        let now = SystemTime::now();
        for path in session_info_files() {
            if is_stale(&path, now) {
                match fs::remove_file(&path) {
                    Ok(()) => log::info!("Stale session info removed: {}", path.display()),
                    Err(e) => log::warn!("Cannot remove {}: {e}", path.display()),
                }
            }
        }
    }

    fn read_from(path: &Path) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        match serde_json::from_str::<Self>(&content) {
            Ok(info) => Some(info),
            Err(e) => {
                log::warn!("{} is malformed, ignoring: {e}", path.display());
                None
            }
        }
    }

    fn matching(info: Self, session_id: &str) -> Option<Self> {
        if info.session_id == session_id {
            Some(info)
        } else {
            log::info!(
                "Session info belongs to session {}, not {session_id}; ignoring",
                info.session_id
            );
            None
        }
    }
}

fn is_stale(path: &Path, now: SystemTime) -> bool {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|modified| now.duration_since(modified).ok())
        .is_some_and(|age| age > STALE_AFTER)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn info(session_id: &str) -> ExplorerSessionInfo {
        ExplorerSessionInfo {
            version: 1,
            session_id: session_id.to_owned(),
            wallet: "0xabc".to_owned(),
            explorer_version: None,
            identity: None,
        }
    }

    fn temp_file() -> PathBuf {
        std::env::temp_dir().join(format!("dcl-session-{}.json", uuid::Uuid::new_v4()))
    }

    #[test]
    fn matching_session_keeps_the_info_other_session_is_discarded() {
        assert!(ExplorerSessionInfo::matching(info("s1"), "s1").is_some());
        assert!(ExplorerSessionInfo::matching(info("s1"), "s2").is_none());
    }

    #[test]
    fn parses_the_contract_with_and_without_identity() {
        let minimal: ExplorerSessionInfo =
            serde_json::from_str(r#"{"session_id":"s1","wallet":"0xabc"}"#)
                .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(minimal.session_id, "s1");
        assert!(minimal.identity.is_none());

        let full: ExplorerSessionInfo = serde_json::from_str(
            r#"{"version":1,"session_id":"s1","wallet":"0xabc","explorer_version":"v1",
                "identity":{"address":"0xabc","key":"0x01","expiration":"2999-01-01T00:00:00Z",
                "ephemeralAuthChain":[{"type":"SIGNER","payload":"0xabc","signature":""}]}}"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(full.explorer_version.as_deref(), Some("v1"));
        assert!(full.valid_identity().is_some());
    }

    #[test]
    fn expired_identity_is_not_offered() {
        let expired: ExplorerSessionInfo = serde_json::from_str(
            r#"{"session_id":"s1","wallet":"0xabc",
                "identity":{"address":"0xabc","key":"0x01","expiration":"2000-01-01T00:00:00Z",
                "ephemeralAuthChain":[]}}"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(expired.identity.is_some());
        assert!(expired.valid_identity().is_none());
    }

    #[test]
    fn debug_output_redacts_the_key() {
        let full: ExplorerSessionInfo = serde_json::from_str(
            r#"{"session_id":"s1","wallet":"0xabc",
                "identity":{"address":"0xabc","key":"0xdeadbeef","expiration":"2999-01-01T00:00:00Z",
                "ephemeralAuthChain":[]}}"#,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let debug = format!("{full:?}");
        assert!(!debug.contains("deadbeef"));
    }

    #[test]
    fn missing_file_is_none_and_fresh_file_is_not_stale() {
        let missing = temp_file();
        assert!(ExplorerSessionInfo::read_from(&missing).is_none());

        let fresh = temp_file();
        fs::write(&fresh, "{}").unwrap_or_else(|e| panic!("{e}"));
        assert!(!is_stale(&fresh, SystemTime::now()));
        assert!(is_stale(
            &fresh,
            SystemTime::now() + STALE_AFTER + Duration::from_secs(1)
        ));
        let _ = fs::remove_file(fresh);
    }
}
