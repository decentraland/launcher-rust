use crate::channel::EventChannel;
use crate::deeplink_bridge::{execute_passthrough, should_use_deeplink_bridge_for};
use crate::errors::{AttemptError, DCLError, DCLErrorTyped};
use crate::instances::RunningInstances;
use crate::logs::LogDestination;
use crate::protocols::{DeepLink, Protocol};
use crate::{
    analytics::{Analytics, event::Event},
    errors::{DCLErrorResult, FlowError},
    installs::{self, InstallsHub},
    s3::{self, ReleaseResponse},
};
use anyhow::{Context, Ok, Result, anyhow};
use dcl_launcher_shared::environment::AppEnvironment;
use dcl_launcher_shared::types::{BuildType, Status, Step};
use log::info;
use regex::Regex;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

const SILENT_ATTEMPTS_COUNT: u8 = 3;

/// Retrying the whole preparation cannot help a deeplink consume-wait timeout: the consumer is
/// booting, hung, or deferring, and every retry just re-waits the same budget. All other errors
/// (network, disk) stay retryable.
const fn is_retryable_error(error: &DCLError) -> bool {
    !matches!(error, DCLError::E3001_OPEN_DEEPLINK_TIMEOUT)
}

/// An attempt is final when the silent-retry budget is exhausted or the error is not retryable.
/// Only the final attempt is captured as a Sentry event; earlier ones become breadcrumbs.
const fn is_final_attempt(attempt: u8, error: &DCLError) -> bool {
    attempt >= SILENT_ATTEMPTS_COUNT || !is_retryable_error(error)
}

trait WorkflowStep<TState, TOutput> {
    async fn is_complete(&self, state: Arc<Mutex<TState>>) -> Result<bool>;

    fn start_label(&self) -> Result<Status>;

    async fn execute<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<TState>>,
    ) -> DCLErrorTyped<TOutput>;

    fn on_skipped(&self, _state: Arc<Mutex<TState>>) -> impl std::future::Future<Output = ()> {
        std::future::ready(())
    }

    async fn execute_if_needed<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<TState>>,
        label: &str,
    ) -> DCLErrorTyped<Option<TOutput>> {
        let complete = self.is_complete(state.clone()).await?;
        if complete {
            info!("Step {} is already complete", label);
            self.on_skipped(state).await;
            return DCLErrorTyped::Ok(None);
        }

        let status = self.start_label()?;
        channel.send(status)?;

        info!("Step {} is started", label);
        let result = self.execute(channel, state).await?;
        info!("Step {} is finished", label);
        DCLErrorTyped::Ok(Some(result))
    }
}

#[derive(Default)]
pub struct LaunchFlowState {
    latest_release: Option<ReleaseResponse>,
    recent_download: Option<RecentDownload>,
}

#[derive(Clone)]
struct RecentDownload {
    version: String,
    downloaded_path: PathBuf,
}

#[allow(clippy::struct_field_names)]
pub struct LaunchFlow {
    fetch_step: FetchStep,
    download_step: DownloadStep,
    install_step: InstallStep,
    deeplink_passthrough_step: DeeplinkPassthroughStep,
    app_launch_step: AppLaunchStep,

    analytics: Arc<Mutex<Analytics>>,
}

impl LaunchFlow {
    pub fn new(
        installs_hub: Arc<Mutex<InstallsHub>>,
        analytics: Arc<Mutex<Analytics>>,
        running_instances: Arc<Mutex<RunningInstances>>,
    ) -> Self {
        let app_launch_step = AppLaunchStep {
            installs_hub,
            running_instances: running_instances.clone(),
        };

        Self {
            fetch_step: FetchStep {
                analytics: analytics.clone(),
            },
            download_step: DownloadStep {
                analytics: analytics.clone(),
            },
            install_step: InstallStep {
                analytics: analytics.clone(),
                running_instances: running_instances.clone(),
            },
            deeplink_passthrough_step: DeeplinkPassthroughStep { running_instances },
            app_launch_step,
            analytics,
        }
    }

    pub async fn launch<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<LaunchFlowState>>,
    ) -> std::result::Result<(), FlowError> {
        let handled_by_passthrough = self.prepare_with_retries(channel, state.clone()).await?;
        if handled_by_passthrough {
            return std::result::Result::Ok(());
        }

        self.launch_once(channel, state).await
    }

    async fn prepare_with_retries<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<LaunchFlowState>>,
    ) -> std::result::Result<bool, FlowError> {
        let mut last_error: Option<AttemptError> = None;

        for attempt in 1..=SILENT_ATTEMPTS_COUNT {
            match self.prepare_internal(channel, state.clone()).await {
                std::result::Result::Ok(handled_by_passthrough) => {
                    return std::result::Result::Ok(handled_by_passthrough);
                }
                std::result::Result::Err(e) => {
                    let final_attempt = is_final_attempt(attempt, &e);
                    last_error = Some(self.report_attempt_error(e, attempt, final_attempt).await);
                    if final_attempt {
                        break;
                    }
                }
            }
        }

        std::result::Result::Err(FlowError {
            user_message: last_error
                .map(|e| e.error.user_message().to_owned())
                .unwrap_or_default(),
        })
    }

    async fn launch_once<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<LaunchFlowState>>,
    ) -> std::result::Result<(), FlowError> {
        match self
            .app_launch_step
            .execute_if_needed(channel, state, "launch")
            .await
        {
            std::result::Result::Ok(_) => std::result::Result::Ok(()),
            std::result::Result::Err(e) => {
                log::error!(
                    target: LogDestination::File.as_target(),
                    "Error launching Explorer. Cause {} {:#?}",
                    e,
                    e
                );
                let code = e.code();
                sentry::with_scope(
                    |scope| {
                        scope.set_tag("error_code", code);
                        scope.set_fingerprint(Some(&[code]));
                    },
                    || {
                        sentry::capture_error(&e);
                    },
                );
                std::result::Result::Err(FlowError {
                    user_message: e.user_message().to_owned(),
                })
            }
        }
    }

    async fn report_attempt_error(
        &self,
        error: DCLError,
        attempt: u8,
        is_final: bool,
    ) -> AttemptError {
        log::error!(
            target: LogDestination::File.as_target(),
            "Error during the flow. Attempt: {}, Cause {} {:#?}",
            attempt,
            error,
            error
        );

        let code = error.code();
        let attempt_error = AttemptError { error, attempt };

        if is_final {
            sentry::with_scope(
                |scope| {
                    scope.set_tag("error_code", code);
                    scope.set_fingerprint(Some(&[code]));
                },
                || {
                    sentry::capture_error(&attempt_error);
                },
            );
        } else {
            sentry::add_breadcrumb(sentry::protocol::Breadcrumb {
                category: Some("flow.attempt".to_owned()),
                message: Some(attempt_error.to_string()),
                level: sentry::Level::Warning,
                data: std::iter::once(("error_code".to_owned(), code.into())).collect(),
                ..Default::default()
            });
        }
        self.analytics
            .lock()
            .await
            .track_and_flush_silent((&attempt_error).into())
            .await;

        attempt_error
    }

    async fn prepare_internal<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<LaunchFlowState>>,
    ) -> DCLErrorTyped<bool> {
        let handled_by_passthrough = self
            .deeplink_passthrough_step
            .execute_if_needed(channel, state.clone(), "deeplink_passthrough")
            .await?;
        // If another Explorer instance is already running, treat this as a deeplink-only
        // handoff: update the deeplink bridge file and stop here instead of running the
        // fetch/download/install flow again.
        if handled_by_passthrough.unwrap_or(false) {
            info!(
                "Deeplink handled by passthrough (an Explorer instance is already running); skipping further steps"
            );
            return DCLErrorTyped::Ok(true);
        }

        self.fetch_step
            .execute_if_needed(channel, state.clone(), "fetch")
            .await?;
        self.download_step
            .execute_if_needed(channel, state.clone(), "download")
            .await?;
        self.install_step
            .execute_if_needed(channel, state.clone(), "install")
            .await?;

        DCLErrorTyped::Ok(false)
    }
}

struct FetchStep {
    analytics: Arc<Mutex<Analytics>>,
}

impl WorkflowStep<LaunchFlowState, ()> for FetchStep {
    async fn is_complete(&self, _state: Arc<Mutex<LaunchFlowState>>) -> Result<bool> {
        // always refetch the origin
        Ok(false)
    }

    fn start_label(&self) -> Result<Status> {
        let status = Status::State {
            step: Step::Fetching,
        };
        Ok(status)
    }

    async fn execute<T: EventChannel>(
        &self,
        _channel: &T,
        state: Arc<Mutex<LaunchFlowState>>,
    ) -> DCLErrorResult {
        self.analytics
            .lock()
            .await
            .track_and_flush_silent(Event::FETCH_VERSION_START)
            .await;

        let fetch_result = crate::s3::get_latest_explorer_release().await;
        if let Err(e) = &fetch_result {
            self.analytics
                .lock()
                .await
                .track_and_flush_silent(Event::FETCH_VERSION_ERROR {
                    error: e.to_string(),
                })
                .await;
        }
        let latest_release = fetch_result?;
        let version = latest_release.version.clone();
        state.lock().await.latest_release = Some(latest_release);

        self.analytics
            .lock()
            .await
            .track_and_flush_silent(Event::FETCH_VERSION_SUCCESS { version })
            .await;

        DCLErrorResult::Ok(())
    }
}

struct DownloadStep {
    analytics: Arc<Mutex<Analytics>>,
}

impl DownloadStep {
    pub fn mode() -> BuildType {
        let any_installed = crate::installs::is_explorer_installed(None);
        if any_installed {
            BuildType::Update
        } else {
            BuildType::New
        }
    }

    async fn version_from_url(&self, url: &str) -> Result<String> {
        let pattern = format!(
            r"(^{}\/{}\/(v?\d+\.\d+\.\d+-?\w*)\/(\w+.zip))",
            AppEnvironment::bucket_url(),
            s3::RELEASE_PREFIX
        );
        let re = Regex::new(&pattern)?;

        let captures = re
            .captures(url)
            .context(format!("cannot find matches in the url: {}", url))?;
        let version = captures.get(2).map(|m| m.as_str());

        match version {
            Some(v) => Ok(v.to_owned()),
            None => {
                self.analytics
                    .lock()
                    .await
                    .track_and_flush_silent(Event::DOWNLOAD_VERSION_ERROR {
                        version: None,
                        error: "No version provided".to_owned(),
                    })
                    .await;
                Err(anyhow!("url doesn't contain version"))
            }
        }
    }
}

impl WorkflowStep<LaunchFlowState, ()> for DownloadStep {
    async fn is_complete(&self, state: Arc<Mutex<LaunchFlowState>>) -> Result<bool> {
        let guard = state.lock().await;
        match &guard.latest_release {
            Some(release) => {
                let version = release.version.as_str();
                let updated = crate::installs::is_explorer_updated(version);
                Ok(updated)
            }
            None => Err(anyhow!("Latest release is not found in the state")),
        }
    }

    async fn on_skipped(&self, state: Arc<Mutex<LaunchFlowState>>) {
        let version = state
            .lock()
            .await
            .latest_release
            .as_ref()
            .map(|r| r.version.clone());
        if let Some(version) = version {
            self.analytics
                .lock()
                .await
                .track_and_flush_silent(Event::DOWNLOAD_VERSION_SKIPPED { version })
                .await;
        }
    }

    fn start_label(&self) -> Result<Status> {
        let mode = Self::mode();
        let status = Status::State {
            step: Step::Downloading {
                progress: 0,
                build_type: mode,
            },
        };
        Ok(status)
    }

    async fn execute<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<LaunchFlowState>>,
    ) -> DCLErrorResult {
        let mode = Self::mode();

        let mut guard = state.lock().await;

        let release = &guard.latest_release;
        match release {
            Some(r) => {
                let url = &r.browser_download_url;
                let version = self.version_from_url(url).await?;

                let target_path = installs::target_download_path();
                let path: &str = target_path
                    .to_str()
                    .context("Cannot convert target download path")?;

                {
                    self.analytics
                        .lock()
                        .await
                        .track_and_flush_silent(Event::DOWNLOAD_VERSION {
                            version: version.clone(),
                        })
                        .await;
                }

                let result = installs::downloads::download_file(
                    url,
                    path,
                    channel,
                    &mode,
                    self.analytics.clone(),
                )
                .await;

                if let Err(e) = &result {
                    self.analytics
                        .lock()
                        .await
                        .track_and_flush_silent(Event::DOWNLOAD_VERSION_ERROR {
                            version: Some(version.clone()),
                            error: e.to_string(),
                        })
                        .await;
                } else {
                    self.analytics
                        .lock()
                        .await
                        .track_and_flush_silent(Event::DOWNLOAD_VERSION_SUCCESS {
                            version: version.clone(),
                        })
                        .await;
                }
                result?;

                guard.recent_download = Some(RecentDownload {
                    version,
                    downloaded_path: target_path,
                });

                DCLErrorResult::Ok(())
            }
            None => DCLErrorResult::Err(anyhow!("Latest release is not fetched").into()),
        }
    }
}

struct InstallStep {
    analytics: Arc<Mutex<Analytics>>,
    running_instances: Arc<Mutex<RunningInstances>>,
}

impl InstallStep {
    async fn execute_internal(&self, recent_download: RecentDownload) -> DCLErrorResult {
        self.check_explorer_not_running().await?;
        installs::install_explorer(
            &recent_download.version,
            Some(recent_download.downloaded_path),
        )
        .and_then(|()| installs::rename_explorer_to_latest())
    }

    async fn check_explorer_not_running(&self) -> DCLErrorResult {
        let running = self
            .running_instances
            .lock()
            .await
            .explorer_processes_by_path();
        if running.is_empty() {
            // `Ok`/`Err` are shadowed by `anyhow::Ok` (imported at the top),
            // so qualify with `DCLErrorResult` to stay on `DCLError`.
            return DCLErrorResult::Ok(());
        }
        log::warn!(
            "Explorer is still running; refusing to install. Blocking processes: {:?}",
            running
        );
        DCLErrorResult::Err(DCLError::E3008_EXPLORER_ALREADY_RUNNING { processes: running })
    }

    async fn recent_download_and_update_state(
        state: Arc<Mutex<LaunchFlowState>>,
    ) -> Option<RecentDownload> {
        let mut guard = state.lock().await;
        let recent_download = guard.recent_download.clone()?;
        guard.recent_download = None;
        drop(guard);
        Some(recent_download)
    }
}

impl WorkflowStep<LaunchFlowState, ()> for InstallStep {
    async fn is_complete(&self, state: Arc<Mutex<LaunchFlowState>>) -> Result<bool> {
        let guard = state.lock().await;

        Ok(guard.recent_download.is_none() && installs::explorer_latest_version_path().exists())
    }

    async fn on_skipped(&self, state: Arc<Mutex<LaunchFlowState>>) {
        let version = state
            .lock()
            .await
            .latest_release
            .as_ref()
            .map(|r| r.version.clone());
        if let Some(version) = version {
            self.analytics
                .lock()
                .await
                .track_and_flush_silent(Event::INSTALL_VERSION_SKIPPED { version })
                .await;
        }
    }

    fn start_label(&self) -> Result<Status> {
        let mode = DownloadStep::mode();
        let status = Status::State {
            step: Step::Installing { build_type: mode },
        };
        Ok(status)
    }

    async fn execute<T: EventChannel>(
        &self,
        _channel: &T,
        state: Arc<Mutex<LaunchFlowState>>,
    ) -> DCLErrorResult {
        let recent_download = Self::recent_download_and_update_state(state).await;

        if let Some(download) = recent_download {
            let version = download.version.clone();
            self.analytics
                .lock()
                .await
                .track_and_flush_silent(Event::INSTALL_VERSION_START {
                    version: version.clone(),
                })
                .await;
            let result = self.execute_internal(download).await;
            if let Err(e) = &result {
                self.analytics
                    .lock()
                    .await
                    .track_and_flush_silent(Event::INSTALL_VERSION_ERROR {
                        version: Some(version),
                        error: e.to_string(),
                    })
                    .await;
            } else {
                self.analytics
                    .lock()
                    .await
                    .track_and_flush_silent(Event::INSTALL_VERSION_SUCCESS { version })
                    .await;
            }
            return result;
        }

        DCLErrorResult::Ok(())
    }
}

struct AppLaunchStep {
    installs_hub: Arc<Mutex<InstallsHub>>,
    running_instances: Arc<Mutex<RunningInstances>>,
}

struct DeeplinkPassthroughStep {
    running_instances: Arc<Mutex<RunningInstances>>,
}

impl DeeplinkPassthroughStep {
    async fn is_any_instance_running(&self) -> anyhow::Result<bool> {
        let guard = self.running_instances.lock().await;
        guard.any_is_running()
    }

    async fn should_use_deeplink_bridge_for(&self, deeplink: &DeepLink) -> anyhow::Result<bool> {
        let any_is_running = self.is_any_instance_running().await?;
        Ok(should_use_deeplink_bridge_for(deeplink, any_is_running))
    }
}

impl WorkflowStep<LaunchFlowState, bool> for DeeplinkPassthroughStep {
    async fn is_complete(&self, _: Arc<Mutex<LaunchFlowState>>) -> Result<bool> {
        let Some(deeplink) = Protocol::value() else {
            return Ok(true);
        };

        let use_bridge = self.should_use_deeplink_bridge_for(&deeplink).await?;
        Ok(!use_bridge)
    }

    fn start_label(&self) -> Result<Status> {
        Ok(Status::State {
            step: Step::Launching,
        })
    }

    async fn execute<T: EventChannel>(
        &self,
        channel: &T,
        _: Arc<Mutex<LaunchFlowState>>,
    ) -> DCLErrorTyped<bool> {
        let Some(deeplink) = Protocol::value() else {
            return DCLErrorTyped::Ok(false);
        };

        // Re-check the bridge policy against this snapshot: an open_url event may have
        // reassigned the protocol since `is_complete`, so decide and act on one value.
        if !self.should_use_deeplink_bridge_for(&deeplink).await? {
            return DCLErrorTyped::Ok(false);
        }

        execute_passthrough(channel, &deeplink).await?;
        DCLErrorTyped::Ok(true)
    }
}

impl AppLaunchStep {
    async fn is_any_instance_running(&self) -> anyhow::Result<bool> {
        let guard = self.running_instances.lock().await;
        guard.any_is_running()
    }

    async fn should_use_deeplink_bridge_for(&self, deeplink: &DeepLink) -> anyhow::Result<bool> {
        let any_is_running = self.is_any_instance_running().await?;
        Ok(should_use_deeplink_bridge_for(deeplink, any_is_running))
    }
}

impl WorkflowStep<LaunchFlowState, ()> for AppLaunchStep {
    async fn is_complete(&self, _: Arc<Mutex<LaunchFlowState>>) -> Result<bool> {
        // Always launch explorer
        Ok(false)
    }

    fn start_label(&self) -> Result<Status> {
        let status = Status::State {
            step: Step::Launching,
        };
        Ok(status)
    }

    async fn execute<T: EventChannel>(
        &self,
        channel: &T,
        _state: Arc<Mutex<LaunchFlowState>>,
    ) -> DCLErrorResult {
        match Protocol::value() {
            Some(deeplink) => {
                if self.should_use_deeplink_bridge_for(&deeplink).await? {
                    execute_passthrough(channel, &deeplink).await
                } else {
                    self.installs_hub
                        .lock()
                        .await
                        .launch_explorer(Some(deeplink), None)
                        .await?;
                    DCLErrorResult::Ok(())
                }
            }
            None => {
                //TODO passed version if specified manually from upper flow
                self.installs_hub
                    .lock()
                    .await
                    .launch_explorer(None, None)
                    .await?;
                DCLErrorResult::Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // E3001 is the one code the client-side deferral window (and a booting/hung
    // Explorer) can never resolve by retrying the whole flow again -- report.md item 2's
    // "single 15s budget" replaces the prior 3x3s. Every other error keeps retrying.
    #[test]
    fn e3001_open_deeplink_timeout_is_not_retryable() {
        assert!(!is_retryable_error(&DCLError::E3001_OPEN_DEEPLINK_TIMEOUT));
    }

    #[rstest]
    #[case(DCLError::E3003_CANT_GET_VERSION)]
    #[case(DCLError::E3004_CANT_RENAME_LATEST)]
    #[case(DCLError::E2006_DOWNLOAD_FAILED_NETWORK_TIMEOUT)]
    fn other_errors_stay_retryable(#[case] error: DCLError) {
        assert!(is_retryable_error(&error));
    }

    // `report_attempt_error`'s capture-vs-breadcrumb split (item 3) is driven entirely by
    // `is_final_attempt`: attempts 1..SILENT_ATTEMPTS_COUNT-1 must stay non-final
    // (breadcrumb-only) for a retryable error, while a non-retryable error (E3001) is
    // final on its very first attempt instead of only at the exhausted budget.
    #[rstest]
    #[case(1, DCLError::E3001_OPEN_DEEPLINK_TIMEOUT, true)]
    #[case(2, DCLError::E3001_OPEN_DEEPLINK_TIMEOUT, true)]
    #[case(SILENT_ATTEMPTS_COUNT, DCLError::E3001_OPEN_DEEPLINK_TIMEOUT, true)]
    #[case(1, DCLError::E3003_CANT_GET_VERSION, false)]
    #[case(2, DCLError::E3003_CANT_GET_VERSION, false)]
    #[case(SILENT_ATTEMPTS_COUNT, DCLError::E3003_CANT_GET_VERSION, true)]
    fn final_attempt_classification(
        #[case] attempt: u8,
        #[case] error: DCLError,
        #[case] expected_final: bool,
    ) {
        assert_eq!(is_final_attempt(attempt, &error), expected_final);
    }

    // Pins the constant the two matrices above are computed against, so a silent bump of
    // the retry budget can't invalidate this test's coverage without also failing here.
    #[test]
    fn silent_attempts_budget_is_three() {
        assert_eq!(SILENT_ATTEMPTS_COUNT, 3);
    }
}

/*

//TODO handle fork flow:
//  useEffect(() => {
    const fetchReleaseData = async () => {
      if (!initialized.current) {
        initialized.current = true;
        // When running with the param --downloadedfilepath={{PATH}}, skip the download step and try to install the .zip provided
        if (customDownloadedFilePath) {
          handleInstall('dev', customDownloadedFilePath);
        }
        // When running with the param --version=dev, skip all the checks and launch the app
        else if (shouldRunDevVersion) {
          handleLaunch();
        }
        // Fetch the latest available version of Decentraland from the github repo releases
        else {
          await handleFetch();
        }
      }
    };

    fetchReleaseData();
  }, []);


  const [retry, setRetry] = useState(0);
  const [error, setError] = useState<string | undefined>(undefined);

// TODO catch these 2 params
  const shouldRunDevVersion = getRunDevVersion();
  const customDownloadedFilePath = getDownloadedFilePath();
*/
