//! Crash-report flow.
//!
//! The launcher was reopened by `client-crash-watchdog` with `--crash-report-with-attachment
//! <path>` and drives the three-screen dialog (Something went wrong → Bug Report form → Submitted).
//!
//! `ReportFlow` is stateless and holds services only; everything the UI renders lives in
//! `ReportFlowState` and is broadcast as a whole `ReportStep` after every mutation.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use log::info;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::analytics::Analytics;
use crate::analytics::event::Event;
use crate::channel::EventChannel;
use crate::config;
use crate::crash_attachment::CrashAttachment;
use crate::errors::FlowError;
use crate::types::{IssueType, ReportStep, Status};
use crate::utils::{app_version, get_os_name};

/// Options of the Intercom "Bug Report" ticket type, copied from the Explorer's
/// `BugReportIssueTypes.cs` so tickets from both clients are shaped identically.
pub const ISSUE_TYPES: [IssueType; 16] = [
    IssueType {
        label: "Performance (Lag/FPS)",
        option_id: "84d3e47f-396f-40be-bb93-a8b36196cf97",
    },
    CRASH_FREEZE_ISSUE_TYPE,
    IssueType {
        label: "Chat",
        option_id: "b2db7b2e-3634-4c9d-9f55-b732bfe41319",
    },
    IssueType {
        label: "Voice Chat",
        option_id: "4395e4a3-7eb8-4bd1-a82e-546250d5c16d",
    },
    IssueType {
        label: "Streaming / Video Player",
        option_id: "dbceaef0-c69c-409b-afb3-5e9523a4dec5",
    },
    IssueType {
        label: "Hangouts / Events",
        option_id: "591a11e5-b440-4acb-9461-7d89e9ae4303",
    },
    IssueType {
        label: "Friends",
        option_id: "4d3a9289-da5a-47a6-a3e0-772effdd78f0",
    },
    IssueType {
        label: "Outfits",
        option_id: "ee7aadd6-6b28-4605-b7b9-908a0788b92c",
    },
    IssueType {
        label: "Wearables / Emotes",
        option_id: "e4e9abb6-8304-48eb-a622-b2516e0a1719",
    },
    IssueType {
        label: "Profile",
        option_id: "30f4a7a7-6366-4e05-9393-d1419a5b4008",
    },
    IssueType {
        label: "Communities",
        option_id: "cf4e335e-98fe-4638-a95a-cc6468de00c3",
    },
    IssueType {
        label: "Map & Minimap",
        option_id: "0c02dc0f-ed5e-4c57-adf1-eac0c91866f6",
    },
    IssueType {
        label: "Rewards",
        option_id: "6c896e12-f790-4971-bb06-a10967589a4c",
    },
    IssueType {
        label: "Gifting",
        option_id: "c602cf8d-a617-4ebf-95be-722f3e4b12c9",
    },
    IssueType {
        label: "Scene",
        option_id: "291a3bee-10f7-4fd3-8019-8d6d9c992f29",
    },
    IssueType {
        label: "Other",
        option_id: "30b90385-7138-4d42-99aa-87eeb1c85619",
    },
];

/// Preselected on the form: the dialog only ever opens after a crash.
pub const CRASH_FREEZE_ISSUE_TYPE: IssueType = IssueType {
    label: "Crash / Freeze",
    option_id: "10ab00f9-e944-4a7f-8b75-c8bf4e4ff270",
};

pub fn issue_type_by_option_id(option_id: &str) -> Option<IssueType> {
    ISSUE_TYPES
        .iter()
        .copied()
        .find(|t| t.option_id == option_id)
}

/// Screens of the dialog. `Submitted` is reached only through [`ReportFlow::submit`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Screen {
    Prompt,
    Form,
    Submitted,
}

/// The only way the UI mutates the draft: one field, its whole new value.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", tag = "event", content = "data")]
pub enum CrashReportField {
    #[serde(rename_all = "camelCase")]
    IssueType { option_id: String },
    #[serde(rename_all = "camelCase")]
    Description { text: String },
    #[serde(rename_all = "camelCase")]
    ShareLogs { value: bool },
    #[serde(rename_all = "camelCase")]
    DontShowAgain { value: bool },
}

#[derive(Clone, Debug, Serialize)]
pub struct CrashReportDraft {
    pub issue_type_option_id: String,
    pub description: String,
    pub share_logs: bool,
}

impl Default for CrashReportDraft {
    fn default() -> Self {
        Self {
            issue_type_option_id: CRASH_FREEZE_ISSUE_TYPE.option_id.to_owned(),
            description: String::new(),
            share_logs: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    Submitting,
    Failed { message: String },
}

/// Single source of truth for the dialog. The UI keeps nothing; it renders [`Self::step`].
pub struct ReportFlowState {
    attachment: CrashAttachment,
    attachment_path: PathBuf,
    wallet: Option<String>,
    screen: Screen,
    draft: CrashReportDraft,
    dont_show_again: bool,
    phase: Phase,
    submitted: bool,
}

impl ReportFlowState {
    pub fn new(
        attachment: CrashAttachment,
        attachment_path: PathBuf,
        wallet: Option<String>,
    ) -> Self {
        Self {
            attachment,
            attachment_path,
            wallet,
            screen: Screen::Prompt,
            draft: CrashReportDraft::default(),
            dont_show_again: false,
            phase: Phase::Idle,
            submitted: false,
        }
    }

    pub const fn attachment(&self) -> &CrashAttachment {
        &self.attachment
    }

    pub const fn submitted(&self) -> bool {
        self.submitted
    }

    pub const fn dont_show_again(&self) -> bool {
        self.dont_show_again
    }

    /// The screen the UI must render right now.
    pub fn step(&self) -> ReportStep {
        match self.screen {
            Screen::Prompt => ReportStep::InfoSomethingWrongStep {
                do_not_show_again: self.dont_show_again,
            },
            Screen::Form => ReportStep::BugReportFormStep {
                issue_type: self.draft.issue_type_option_id.clone(),
                issue_types: ISSUE_TYPES.to_vec(),
                description: self.draft.description.clone(),
                share_logs: self.draft.share_logs,
                submitting: self.phase == Phase::Submitting,
                error: match &self.phase {
                    Phase::Failed { message } => Some(message.clone()),
                    Phase::Idle | Phase::Submitting => None,
                },
            },
            Screen::Submitted => ReportStep::BugReportSubmittedStep,
        }
    }

    fn apply(&mut self, field: CrashReportField) -> Result<()> {
        match field {
            CrashReportField::IssueType { option_id } => {
                issue_type_by_option_id(&option_id)
                    .ok_or_else(|| anyhow!("Unknown issue type option id: {option_id}"))?;
                self.draft.issue_type_option_id = option_id;
            }
            CrashReportField::Description { text } => {
                self.draft.description = text;
                // A stale failure message would sit under a draft the user is already fixing.
                if matches!(self.phase, Phase::Failed { .. }) {
                    self.phase = Phase::Idle;
                }
            }
            CrashReportField::ShareLogs { value } => self.draft.share_logs = value,
            CrashReportField::DontShowAgain { value } => self.dont_show_again = value,
        }
        Ok(())
    }

    fn validated_draft(&self) -> Result<CrashReportDraft, String> {
        if self.draft.description.trim().is_empty() {
            return Err("Please describe what happened before submitting.".to_owned());
        }
        if issue_type_by_option_id(&self.draft.issue_type_option_id).is_none() {
            return Err("Please select an issue type.".to_owned());
        }
        Ok(self.draft.clone())
    }
}

/// The payload a sink delivers. Shaped after the Explorer's Intercom ticket so the proxy route,
/// once it exists, is a drop-in.
#[derive(Clone, Debug, Serialize)]
pub struct CrashReport {
    pub issue_type: IssueType,
    pub description: String,
    pub share_logs: bool,
    pub wallet: Option<String>,
    pub attachment: CrashAttachment,
    pub launcher_version: String,
    pub os: String,
}

impl CrashReport {
    fn new(draft: CrashReportDraft, state: &ReportFlowState) -> Self {
        Self {
            issue_type: issue_type_by_option_id(&draft.issue_type_option_id)
                .unwrap_or(CRASH_FREEZE_ISSUE_TYPE),
            description: draft.description,
            share_logs: draft.share_logs,
            wallet: state.wallet.clone(),
            attachment: state.attachment.clone(),
            launcher_version: app_version().to_owned(),
            os: get_os_name().to_owned(),
        }
    }
}

/// Where a submitted report goes. Same enum-dispatch shape as `Analytics::{Client, Null}`.
pub enum ReportSink {
    /// Stub until the intercom-proxy exposes a launcher-callable route: logs the payload.
    Log(LogReportSink),
}

impl ReportSink {
    pub const fn new_from_env() -> Self {
        Self::Log(LogReportSink)
    }

    // Sync for now: the only sink logs. Becomes async together with the first network sink.
    pub fn submit(&self, report: &CrashReport) -> Result<()> {
        match self {
            Self::Log(_) => LogReportSink::submit(report),
        }
    }
}

pub struct LogReportSink;

impl LogReportSink {
    fn submit(report: &CrashReport) -> Result<()> {
        let json = serde_json::to_string_pretty(report)?;
        info!("Crash report (log sink, not delivered anywhere):\n{json}");
        Ok(())
    }
}

/// Stateless: holds services only. Every method takes the shared state and ends by
/// broadcasting the current step, so the UI always renders the latest state.
///
/// Analytics calls happen with the state lock released: they flush over the network and must
/// not stall the next keystroke.
pub struct ReportFlow {
    sink: ReportSink,
    analytics: Arc<Mutex<Analytics>>,
}

impl ReportFlow {
    pub const fn new(sink: ReportSink, analytics: Arc<Mutex<Analytics>>) -> Self {
        Self { sink, analytics }
    }

    /// First broadcast after the window opened on the "Something went wrong" screen.
    pub async fn open<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<ReportFlowState>>,
    ) -> Result<()> {
        let event = {
            let guard = state.lock().await;
            info!(
                "Crash report dialog opened for session {} (exit {})",
                guard.attachment.session_id, guard.attachment.exit_code
            );
            Self::broadcast(channel, &guard)?;
            Event::CRASH_REPORT_DIALOG_SHOWN {
                session_id: guard.attachment.session_id.clone(),
                explorer_version: guard.attachment.explorer_version.clone(),
                exit_code: guard.attachment.exit_code.clone(),
            }
        };
        self.track(event).await;
        Ok(())
    }

    pub async fn set_field<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<ReportFlowState>>,
        field: CrashReportField,
    ) -> Result<()> {
        let mut guard = state.lock().await;
        guard.apply(field)?;
        Self::broadcast(channel, &guard)
    }

    /// REPORT BUG (`Form`) and CANCEL (`Prompt`). The draft survives going back.
    pub async fn navigate<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<ReportFlowState>>,
        screen: Screen,
    ) -> Result<()> {
        if screen == Screen::Submitted {
            return Err(anyhow!(
                "The submitted screen is reached only by submitting"
            ));
        }
        let form_opened = {
            let mut guard = state.lock().await;
            if guard.phase == Phase::Submitting {
                return Err(anyhow!("Cannot navigate while a submission is in flight"));
            }
            let form_opened = guard.screen == Screen::Prompt && screen == Screen::Form;
            guard.screen = screen;
            Self::broadcast(channel, &guard)?;
            form_opened.then(|| Event::CRASH_REPORT_FORM_OPENED {
                session_id: guard.attachment.session_id.clone(),
            })
        };
        if let Some(event) = form_opened {
            self.track(event).await;
        }
        Ok(())
    }

    /// SUBMIT. On failure the draft stays untouched and the form shows the message inline.
    pub async fn submit<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<ReportFlowState>>,
    ) -> std::result::Result<(), FlowError> {
        let report = {
            let mut guard = state.lock().await;
            let draft = match guard.validated_draft() {
                Ok(draft) => draft,
                Err(message) => {
                    guard.phase = Phase::Failed {
                        message: message.clone(),
                    };
                    Self::broadcast_silent(channel, &guard);
                    return Err(FlowError {
                        user_message: message,
                    });
                }
            };
            guard.phase = Phase::Submitting;
            Self::broadcast_silent(channel, &guard);
            CrashReport::new(draft, &guard)
        };

        self.track(Event::CRASH_REPORT_SUBMIT {
            session_id: report.attachment.session_id.clone(),
            issue_type: report.issue_type.label.to_owned(),
            share_logs: report.share_logs,
        })
        .await;

        let result = self.sink.submit(&report);
        let session_id = report.attachment.session_id.clone();

        let mut guard = state.lock().await;
        match result {
            Ok(()) => {
                guard.submitted = true;
                guard.screen = Screen::Submitted;
                guard.phase = Phase::Idle;
                CrashAttachment::delete(&guard.attachment_path);
                Self::broadcast_silent(channel, &guard);
                drop(guard);
                self.track(Event::CRASH_REPORT_SUBMIT_SUCCESS { session_id })
                    .await;
                Ok(())
            }
            Err(e) => {
                log::error!("Crash report submission failed: {e:#}");
                let message =
                    "We couldn't send your report. Please check your connection and try again."
                        .to_owned();
                guard.phase = Phase::Failed {
                    message: message.clone(),
                };
                Self::broadcast_silent(channel, &guard);
                drop(guard);
                self.track(Event::CRASH_REPORT_SUBMIT_ERROR {
                    session_id,
                    error: format!("{e:#}"),
                })
                .await;
                Err(FlowError {
                    user_message: message,
                })
            }
        }
    }

    /// RELAUNCH or X. The process exits or restarts right after, so nothing is broadcast.
    /// A checked "Don't show this again" is persisted here, since this is the last chance.
    pub async fn dismiss(&self, state: Arc<Mutex<ReportFlowState>>, relaunch: bool) {
        let event = {
            let guard = state.lock().await;
            info!(
                "Crash report dialog dismissed: relaunch={relaunch} submitted={} dont_show_again={}",
                guard.submitted, guard.dont_show_again
            );
            if guard.dont_show_again {
                if let Err(e) = config::set_crash_report_dialog_disabled(true) {
                    log::error!("Cannot persist crash-report-dialog-disabled: {e:#}");
                }
            }
            CrashAttachment::delete(&guard.attachment_path);
            Event::CRASH_REPORT_DISMISSED {
                session_id: guard.attachment.session_id.clone(),
                submitted: guard.submitted,
                relaunch,
                dont_show_again: guard.dont_show_again,
            }
        };
        self.track(event).await;
    }

    async fn track(&self, event: Event) {
        self.analytics
            .lock()
            .await
            .track_and_flush_silent(event)
            .await;
    }

    fn broadcast<T: EventChannel>(channel: &T, state: &ReportFlowState) -> Result<()> {
        channel.send(Status::Report(state.step()))
    }

    fn broadcast_silent<T: EventChannel>(channel: &T, state: &ReportFlowState) {
        if let Err(e) = Self::broadcast(channel, state) {
            log::error!("Cannot send crash report step: {e}");
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;
    use std::sync::{Mutex as StdMutex, PoisonError};

    #[derive(Default)]
    struct RecordingChannel {
        sent: StdMutex<Vec<Status>>,
    }

    impl RecordingChannel {
        fn last_step(&self) -> ReportStep {
            let sent = self.sent.lock().unwrap_or_else(PoisonError::into_inner);
            match sent.last() {
                Some(Status::Report(step)) => step.clone(),
                _ => panic!("expected a report step"),
            }
        }

        fn count(&self) -> usize {
            self.sent
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .len()
        }
    }

    impl EventChannel for RecordingChannel {
        fn send(&self, status: Status) -> Result<()> {
            self.sent
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(status);
            Ok(())
        }
    }

    fn attachment() -> CrashAttachment {
        CrashAttachment {
            session_id: "session".to_owned(),
            explorer_version: "v1.0.0".to_owned(),
            exit_code: "exit 1".to_owned(),
            crashed_at: 1,
            pid: 1,
        }
    }

    fn state() -> Arc<Mutex<ReportFlowState>> {
        let path = std::env::temp_dir().join(format!("dcl-report-{}.json", uuid::Uuid::new_v4()));
        Arc::new(Mutex::new(ReportFlowState::new(attachment(), path, None)))
    }

    fn flow() -> ReportFlow {
        let analytics = Arc::new(Mutex::new(Analytics::new(None)));
        ReportFlow::new(ReportSink::new_from_env(), analytics)
    }

    #[tokio::test]
    async fn open_broadcasts_the_prompt() {
        let channel = RecordingChannel::default();
        flow()
            .open(&channel, state())
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        assert!(matches!(
            channel.last_step(),
            ReportStep::InfoSomethingWrongStep {
                do_not_show_again: false
            }
        ));
    }

    #[tokio::test]
    async fn form_defaults_to_crash_freeze_and_shares_logs() {
        let channel = RecordingChannel::default();
        flow()
            .navigate(&channel, state(), Screen::Form)
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        match channel.last_step() {
            ReportStep::BugReportFormStep {
                issue_type,
                issue_types,
                description,
                share_logs,
                submitting,
                error,
            } => {
                assert_eq!(issue_type, CRASH_FREEZE_ISSUE_TYPE.option_id);
                assert_eq!(issue_types.len(), ISSUE_TYPES.len());
                assert!(description.is_empty());
                assert!(share_logs);
                assert!(!submitting);
                assert!(error.is_none());
            }
            _ => panic!("expected the form"),
        }
    }

    #[tokio::test]
    async fn every_field_edit_is_echoed_and_survives_cancel() {
        let channel = RecordingChannel::default();
        let state = state();
        let flow = flow();

        flow.navigate(&channel, state.clone(), Screen::Form)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        flow.set_field(
            &channel,
            state.clone(),
            CrashReportField::Description {
                text: "it froze".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{e}"));
        flow.set_field(
            &channel,
            state.clone(),
            CrashReportField::ShareLogs { value: false },
        )
        .await
        .unwrap_or_else(|e| panic!("{e}"));
        flow.navigate(&channel, state.clone(), Screen::Prompt)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        flow.navigate(&channel, state, Screen::Form)
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        match channel.last_step() {
            ReportStep::BugReportFormStep {
                description,
                share_logs,
                ..
            } => {
                assert_eq!(description, "it froze");
                assert!(!share_logs);
            }
            _ => panic!("expected the form"),
        }
        assert_eq!(channel.count(), 5);
    }

    #[tokio::test]
    async fn unknown_issue_type_is_rejected() {
        let channel = RecordingChannel::default();
        let result = flow()
            .set_field(
                &channel,
                state(),
                CrashReportField::IssueType {
                    option_id: "nope".to_owned(),
                },
            )
            .await;
        assert!(result.is_err());
        assert_eq!(channel.count(), 0);
    }

    #[tokio::test]
    async fn empty_description_fails_inline_and_clears_on_edit() {
        let channel = RecordingChannel::default();
        let state = state();
        let flow = flow();
        flow.navigate(&channel, state.clone(), Screen::Form)
            .await
            .unwrap_or_else(|e| panic!("{e}"));

        assert!(flow.submit(&channel, state.clone()).await.is_err());
        assert!(matches!(
            channel.last_step(),
            ReportStep::BugReportFormStep { error: Some(_), .. }
        ));

        flow.set_field(
            &channel,
            state,
            CrashReportField::Description {
                text: "x".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(
            channel.last_step(),
            ReportStep::BugReportFormStep { error: None, .. }
        ));
    }

    #[tokio::test]
    async fn successful_submit_reaches_the_submitted_screen() {
        let channel = RecordingChannel::default();
        let state = state();
        let flow = flow();
        flow.navigate(&channel, state.clone(), Screen::Form)
            .await
            .unwrap_or_else(|e| panic!("{e}"));
        flow.set_field(
            &channel,
            state.clone(),
            CrashReportField::Description {
                text: "it froze".to_owned(),
            },
        )
        .await
        .unwrap_or_else(|e| panic!("{e}"));

        flow.submit(&channel, state.clone())
            .await
            .unwrap_or_else(|e| panic!("{}", e.user_message));

        assert!(matches!(
            channel.last_step(),
            ReportStep::BugReportSubmittedStep
        ));
        assert!(state.lock().await.submitted());
    }

    #[tokio::test]
    async fn submitted_screen_cannot_be_navigated_to() {
        let channel = RecordingChannel::default();
        assert!(
            flow()
                .navigate(&channel, state(), Screen::Submitted)
                .await
                .is_err()
        );
    }

    #[test]
    fn issue_type_lookup() {
        assert_eq!(
            issue_type_by_option_id(CRASH_FREEZE_ISSUE_TYPE.option_id),
            Some(CRASH_FREEZE_ISSUE_TYPE)
        );
        assert!(issue_type_by_option_id("missing").is_none());
    }
}
