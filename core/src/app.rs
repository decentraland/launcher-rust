use anyhow::{Context, Result};

use crate::analytics::Analytics;
use crate::analytics::event::Event;
use crate::crash_attachment::CrashAttachment;
#[cfg(target_os = "macos")]
use crate::download_origin_metadata::DownloadOrigin;
use crate::download_origin_metadata::campaign_anon_user_id_storage::CampaignAnonUserIdStorage;
use crate::download_origin_metadata::campaign_attribution_marker::CampaignAttributionMarker;
use crate::download_origin_metadata::dcl_env_storage::DclEnvStorage;
use crate::download_origin_metadata::referrer_storage::ReferrerStorage;
use crate::environment::{AppEnvironment, Args};
use crate::explorer_session_info::ExplorerSessionInfo;
use crate::installs;
use crate::instances::RunningInstances;
use crate::launch_flow::{LaunchFlow, LaunchFlowState};
use crate::monitoring::Monitoring;
use crate::protocols::Protocol;
use crate::report_flow::{ReportFlow, ReportFlowState, ReportSink};
use crate::{analytics, logs, utils};
use log::{error, info};
use std::sync::Arc;
use tokio::sync::Mutex;
use utils::{BUILD_COMMIT, BUILD_PR, app_version};

pub struct LaunchContext {
    pub flow: LaunchFlow,
    pub state: Arc<Mutex<LaunchFlowState>>,
}

pub struct ReportContext {
    pub flow: Arc<ReportFlow>,
    pub state: Arc<Mutex<ReportFlowState>>,
}

/// Which flow this process runs, selected once at startup from the command line.
pub enum FlowContext {
    Launch(LaunchContext),
    Report(ReportContext),
}

pub struct AppState {
    pub context: FlowContext,
    pub protocol: Protocol,
    pub analytics: Arc<Mutex<Analytics>>,
}

impl AppState {
    pub async fn setup() -> Result<Self> {
        logs::dispath_logs()?;

        info!(
            "Application setup start. Version: {} commit: {} pr: {}",
            app_version(),
            BUILD_COMMIT,
            BUILD_PR
        );

        std::panic::set_hook(Box::new(|info| error!("Panic occurred: {:?}", info)));

        Monitoring::try_setup_sentry().context("Cannot setup monitoring")?;

        #[cfg(target_os = "macos")]
        {
            DownloadOrigin::try_extract_origin_data();
            DownloadOrigin::try_install_to_app_dir_if_from_dmg();
        }

        ReferrerStorage::ingest_bridge_file();
        DclEnvStorage::ingest_bridge_file();

        let campaign_anon_user_id = CampaignAnonUserIdStorage::read();

        let mut analytics = {
            let analytics = analytics::Analytics::new_from_env();
            match &campaign_anon_user_id {
                Some(id) => analytics.with_campaign_anon_user_id(id.as_str()),
                None => analytics,
            }
        };

        analytics
            .track_and_flush_silent(Event::LAUNCHER_OPEN {
                version: utils::app_version().to_owned(),
            })
            .await;

        if let Some(anon_id) = &campaign_anon_user_id {
            if !CampaignAttributionMarker::is_reported() {
                // Mark before sending (at-most-once) to avoid duplicates on crash
                if let Err(e) = CampaignAttributionMarker::mark_reported() {
                    log::warn!("Cannot write attribution marker: {e}");
                }
                info!("Firing Campaign Attribution Detected event");
                analytics
                    .track_and_flush_silent(Event::CAMPAIGN_ATTRIBUTION_DETECTED {
                        anon_user_id: anon_id.as_str().to_owned(),
                    })
                    .await;
            }
        }

        let analytics = Arc::new(Mutex::new(analytics));

        let args = AppEnvironment::cmd_args();
        let context = match report_context_from_args(&args, analytics.clone()) {
            Some(report) => FlowContext::Report(report),
            None => FlowContext::Launch(new_launch_context(analytics.clone())),
        };

        let app_state = Self {
            context,
            protocol: Protocol {},
            analytics,
        };

        info!("Application setup complete");

        Ok(app_state)
    }

    pub async fn cleanup(&self) {
        let mut analytics = self.analytics.lock().await;
        analytics
            .track_and_flush_silent(Event::LAUNCHER_CLOSE {
                version: utils::app_version().to_owned(),
            })
            .await;
        analytics.cleanup().await;
    }
}

fn new_launch_context(analytics: Arc<Mutex<Analytics>>) -> LaunchContext {
    let running_instances = Arc::new(Mutex::new(RunningInstances::default()));
    let installs_hub = Arc::new(Mutex::new(installs::InstallsHub::new(
        analytics.clone(),
        running_instances.clone(),
    )));

    let flow = LaunchFlow::new(installs_hub, analytics, running_instances);
    LaunchContext {
        flow,
        state: Arc::new(Mutex::new(LaunchFlowState::default())),
    }
}

/// `Some` only when the watchdog handed over a readable attachment. An unreadable file is logged
/// and the launcher falls back to the regular launch flow rather than showing an empty dialog.
fn report_context_from_args(
    args: &Args,
    analytics: Arc<Mutex<Analytics>>,
) -> Option<ReportContext> {
    let path = args.crash_report_attachment.as_ref()?;

    let attachment = match CrashAttachment::read(path) {
        Ok(attachment) => attachment,
        Err(e) => {
            error!("Cannot read crash attachment, falling back to the launch flow: {e:#}");
            return None;
        }
    };

    info!(
        "Crash report flow selected for session {} (explorer {}, {})",
        attachment.session_id, attachment.explorer_version, attachment.exit_code
    );

    ExplorerSessionInfo::sweep_stale();
    let session_info = ExplorerSessionInfo::read_for(&attachment.session_id);

    let state = ReportFlowState::new(attachment, path.clone(), session_info);
    Some(ReportContext {
        flow: Arc::new(ReportFlow::new(ReportSink::new_from_env(), analytics)),
        state: Arc::new(Mutex::new(state)),
    })
}
