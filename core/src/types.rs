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

impl Into<Status> for LauncherUpdate {
    fn into(self) -> Status {
        Status::State {
            step: Step::LauncherUpdate(self),
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

pub struct FlowError {
    pub user_message: String,
    pub can_retry: bool,
}

impl From<&FlowError> for Status {
    fn from(err: &FlowError) -> Self {
        Status::Error {
            message: err.user_message.to_owned(),
            can_retry: err.can_retry,
        }
    }
}

pub struct StepError {
    pub user_message: String,
    pub inner_error: anyhow::Error,
}
