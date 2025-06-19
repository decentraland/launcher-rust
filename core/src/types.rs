use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum Status {
    #[serde(rename_all = "camelCase")]
    State { step: Step },
    #[serde(rename_all = "camelCase")]
    Error { message: String, can_retry: bool },
}

#[derive(Clone, Serialize)]
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

#[derive(Clone, Serialize)]
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
        Status::State {
            step: Step::LauncherUpdate(update),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum BuildType {
    #[serde(rename_all = "camelCase")]
    New,
    #[serde(rename_all = "camelCase")]
    Update,
}
