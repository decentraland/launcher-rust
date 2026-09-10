use serde::Serialize;

/// Everything the UI can render. The UI is a stateless renderer of the latest `Status` it
/// received: each flow owns a separate arm so their protocols never share variants.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum Status {
    Launch(LaunchStatus),
    Report(ReportStep),
}

impl From<LaunchStatus> for Status {
    fn from(status: LaunchStatus) -> Self {
        Self::Launch(status)
    }
}

impl From<Step> for Status {
    fn from(step: Step) -> Self {
        Self::Launch(LaunchStatus::State { step })
    }
}

impl From<ReportStep> for Status {
    fn from(step: ReportStep) -> Self {
        Self::Report(step)
    }
}

/// Download → install → launch funnel states.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum LaunchStatus {
    #[serde(rename_all = "camelCase")]
    State { step: Step },
    #[serde(rename_all = "camelCase")]
    Error { message: String },
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
        Self::from(Step::LauncherUpdate(update))
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

/// One option of the "Issue Type" list attribute on the Intercom "Bug Report" ticket type.
/// Intercom takes the option id, never the label.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueType {
    pub label: &'static str,
    pub option_id: &'static str,
}

/// Crash-report flow screens. The variant *is* the screen; each one carries exactly what its
/// screen renders, so the UI keeps no state of its own.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum ReportStep {
    #[serde(rename_all = "camelCase")]
    InfoSomethingWrongStep {
        do_not_show_again: bool,
    },
    #[serde(rename_all = "camelCase")]
    BugReportFormStep {
        /// Selected option id, always one of `issue_types`.
        issue_type: String,
        issue_types: Vec<IssueType>,
        description: String,
        share_logs: bool,
        /// SUBMIT is in flight: the UI disables the button and shows a spinner.
        submitting: bool,
        /// Last submission failure, rendered inline; the draft above is untouched.
        error: Option<String>,
    },
    BugReportSubmittedStep,
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn json(status: &Status) -> serde_json::Value {
        serde_json::to_value(status).unwrap_or_default()
    }

    #[test]
    fn launch_arm_keeps_the_pre_fork_shape_one_level_down() {
        let value = json(&Status::from(Step::Fetching));
        assert_eq!(value["event"], "launch");
        assert_eq!(value["data"]["event"], "state");
        assert_eq!(value["data"]["data"]["step"]["event"], "fetching");

        let value = json(&Status::from(LaunchStatus::Error {
            message: "boom".to_owned(),
        }));
        assert_eq!(value["data"]["event"], "error");
        assert_eq!(value["data"]["data"]["message"], "boom");
    }

    #[test]
    fn report_steps_encode_the_screen_in_the_event_name() {
        let value = json(&Status::from(ReportStep::InfoSomethingWrongStep {
            do_not_show_again: true,
        }));
        assert_eq!(value["event"], "report");
        assert_eq!(value["data"]["event"], "infoSomethingWrongStep");
        assert_eq!(value["data"]["data"]["doNotShowAgain"], true);

        let value = json(&Status::from(ReportStep::BugReportFormStep {
            issue_type: "id".to_owned(),
            issue_types: vec![IssueType {
                label: "Other",
                option_id: "id",
            }],
            description: "text".to_owned(),
            share_logs: true,
            submitting: false,
            error: None,
        }));
        assert_eq!(value["data"]["event"], "bugReportFormStep");
        let data = &value["data"]["data"];
        assert_eq!(data["issueType"], "id");
        assert_eq!(data["issueTypes"][0]["optionId"], "id");
        assert_eq!(data["description"], "text");
        assert_eq!(data["shareLogs"], true);
        assert_eq!(data["submitting"], false);
        assert!(data["error"].is_null());

        let value = json(&Status::from(ReportStep::BugReportSubmittedStep));
        assert_eq!(value["data"]["event"], "bugReportSubmittedStep");
    }
}
