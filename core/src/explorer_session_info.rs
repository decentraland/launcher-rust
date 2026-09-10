use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::installs::session_info_path;

/// Wallet handed over by the Explorer.
///
/// Written by the Explorer after login into the launcher app dir (`session-info.json`), keyed by
/// the `--session_id` the launcher passed on the command line. The launcher never learns the
/// wallet any other way.
///
/// Contract (Explorer side, follow-up):
/// `{ "session_id": "<--session_id arg>", "wallet": "0x…", "explorer_version": "…" }`
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct ExplorerSessionInfo {
    pub session_id: String,
    pub wallet: String,
    #[serde(default)]
    pub explorer_version: Option<String>,
}

impl ExplorerSessionInfo {
    /// The stored info only if it belongs to `session_id`: with several Explorer instances the
    /// file holds the most recent login, which may not be the one that crashed.
    pub fn read_for(session_id: &str) -> Option<Self> {
        let path = session_info_path();
        let info = Self::read_from(&path)?;
        Self::matching(info, session_id)
    }

    fn read_from(path: &Path) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        match serde_json::from_str::<Self>(&content) {
            Ok(info) => Some(info),
            Err(e) => {
                log::warn!("session-info.json is malformed, ignoring: {e}");
                None
            }
        }
    }

    fn matching(info: Self, session_id: &str) -> Option<Self> {
        if info.session_id == session_id {
            Some(info)
        } else {
            log::info!(
                "session-info.json belongs to session {}, not {session_id}; wallet unknown",
                info.session_id
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(session_id: &str) -> ExplorerSessionInfo {
        ExplorerSessionInfo {
            session_id: session_id.to_owned(),
            wallet: "0xabc".to_owned(),
            explorer_version: None,
        }
    }

    #[test]
    fn matching_session_keeps_the_wallet() {
        assert_eq!(
            ExplorerSessionInfo::matching(info("s1"), "s1"),
            Some(info("s1"))
        );
    }

    #[test]
    fn other_session_is_discarded() {
        assert_eq!(ExplorerSessionInfo::matching(info("s1"), "s2"), None);
    }

    #[test]
    fn parses_the_contract_with_optional_version() {
        let parsed: ExplorerSessionInfo =
            serde_json::from_str(r#"{"session_id":"s1","wallet":"0xabc"}"#).unwrap_or_else(|e| {
                #[allow(clippy::panic)]
                {
                    panic!("{e}")
                }
            });
        assert_eq!(parsed, info("s1"));

        let parsed: ExplorerSessionInfo =
            serde_json::from_str(r#"{"session_id":"s1","wallet":"0xabc","explorer_version":"v1"}"#)
                .unwrap_or_else(|e| {
                    #[allow(clippy::panic)]
                    {
                        panic!("{e}")
                    }
                });
        assert_eq!(parsed.explorer_version.as_deref(), Some("v1"));
    }

    #[test]
    fn missing_file_is_none() {
        let path = std::env::temp_dir().join(format!("dcl-missing-{}.json", uuid::Uuid::new_v4()));
        assert!(ExplorerSessionInfo::read_from(&path).is_none());
    }
}
