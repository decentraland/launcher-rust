use serde::{Deserialize, Serialize};

/// Mirror of `dcl_launcher_core::types::Status`.
///
/// Core's type derives `Serialize` only, so the UI side could never
/// deserialize it; these mirrors carry byte-identical serde attributes.
/// The wire shape is locked by `src/components/Home/types.ts` — the frontend
/// must keep receiving exactly the JSON it receives today.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum Status {
    #[serde(rename_all = "camelCase")]
    State { step: Step },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
}

/// Mirror of `dcl_launcher_core::types::Step`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum Step {
    #[serde(rename_all = "camelCase")]
    LauncherUpdate(LauncherUpdate),
    #[serde(rename_all = "camelCase")]
    DeeplinkOpening,
    #[serde(rename_all = "camelCase")]
    Fetching,
    #[serde(rename_all = "camelCase")]
    Downloading { progress: u8, build_type: BuildType },
    #[serde(rename_all = "camelCase")]
    Installing { build_type: BuildType },
    #[serde(rename_all = "camelCase")]
    Launching,
}

/// Mirror of `dcl_launcher_core::types::LauncherUpdate`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum LauncherUpdate {
    CheckingForUpdate,
    Downloading { progress: Option<u8> },
    DownloadFinished,
    InstallingUpdate,
    RestartingApp,
}

impl From<LauncherUpdate> for Status {
    fn from(update: LauncherUpdate) -> Self {
        Self::State {
            step: Step::LauncherUpdate(update),
        }
    }
}

/// Mirror of `dcl_launcher_core::types::BuildType`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum BuildType {
    #[serde(rename_all = "camelCase")]
    New,
    #[serde(rename_all = "camelCase")]
    Update,
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use serde_json::{Value, json};

    fn roundtrip(status: &Status) -> Result<Status> {
        let raw = serde_json::to_string(status)?;
        Ok(serde_json::from_str(&raw)?)
    }

    #[test]
    fn downloading_matches_frontend_contract() -> Result<()> {
        let status = Status::State {
            step: Step::Downloading {
                progress: 42,
                build_type: BuildType::New,
            },
        };

        let expected: Value = json!({
            "event": "state",
            "data": {
                "step": {
                    "event": "downloading",
                    "data": { "progress": 42, "buildType": { "event": "new" } }
                }
            }
        });

        assert_eq!(serde_json::to_value(&status)?, expected);
        assert_eq!(roundtrip(&status)?, status);
        Ok(())
    }

    #[test]
    fn unit_step_omits_data() -> Result<()> {
        let status = Status::State {
            step: Step::Fetching,
        };

        let expected: Value = json!({
            "event": "state",
            "data": { "step": { "event": "fetching" } }
        });

        assert_eq!(serde_json::to_value(&status)?, expected);
        assert_eq!(roundtrip(&status)?, status);
        Ok(())
    }

    #[test]
    fn launcher_update_progress_matches_contract() -> Result<()> {
        let status: Status = LauncherUpdate::Downloading { progress: Some(7) }.into();

        let expected: Value = json!({
            "event": "state",
            "data": {
                "step": {
                    "event": "launcherUpdate",
                    "data": { "event": "downloading", "data": { "progress": 7 } }
                }
            }
        });

        assert_eq!(serde_json::to_value(&status)?, expected);
        assert_eq!(roundtrip(&status)?, status);
        Ok(())
    }

    #[test]
    fn error_matches_frontend_contract() -> Result<()> {
        let status = Status::Error {
            message: "boom".to_owned(),
        };

        let expected: Value = json!({
            "event": "error",
            "data": { "message": "boom" }
        });

        assert_eq!(serde_json::to_value(&status)?, expected);
        assert_eq!(roundtrip(&status)?, status);
        Ok(())
    }
}
