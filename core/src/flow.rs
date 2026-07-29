use crate::channel::EventChannel;
use crate::deeplink_bridge::{execute_passthrough, should_use_deeplink_bridge_for};
use crate::errors::{AttemptError, StepError, StepResultTyped};
use crate::instances::RunningInstances;
use crate::protocols::{DeepLink, Protocol};
use crate::{
    analytics::{Analytics, event::Event},
    environment::AppEnvironment,
    errors::{FlowError, StepResult},
    installs::{self, InstallsHub},
    s3::{self, ReleaseResponse},
    types::{BuildType, Status, Step},
};
use anyhow::{Context, Ok, Result, anyhow};
use log::info;
use regex::Regex;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

trait WorkflowStep<TState, TOutput> {
    async fn is_complete(&self, state: Arc<Mutex<TState>>) -> Result<bool>;

    fn start_label(&self) -> Result<Status>;

    async fn execute<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<TState>>,
    ) -> StepResultTyped<TOutput>;

    async fn execute_if_needed<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<TState>>,
        label: &str,
    ) -> StepResultTyped<Option<TOutput>> {
        let complete = self.is_complete(state.clone()).await?;
        if complete {
            info!("Step {} is already complete", label);
            return StepResultTyped::Ok(None);
        }

        let status = self.start_label()?;
        channel.send(status)?;

        info!("Step {} is started", label);
        let result = self.execute(channel, state).await?;
        info!("Step {} is finished", label);
        StepResultTyped::Ok(Some(result))
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
            fetch_step: FetchStep {},
            download_step: DownloadStep {
                analytics: analytics.clone(),
            },
            install_step: InstallStep {
                analytics: analytics.clone(),
                running_instances: running_instances.clone(),
            },
            deeplink_passthrough_step: DeeplinkPassthroughStep {
                running_instances,
            },
            app_launch_step,
            analytics,
        }
    }

    pub async fn launch<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<LaunchFlowState>>,
    ) -> std::result::Result<(), FlowError> {
        const SILENT_ATTEMPTS_COUNT: u8 = 3;

        let mut last_error: Option<AttemptError> = None;

        for attempt in 1..=SILENT_ATTEMPTS_COUNT {
            let result = self.launch_internal(channel, state.clone()).await;

            if let Err(e) = result {
                // Breadcrumb-only: the error itself is captured just below
                // with a per-code fingerprint; a plain error log would create
                // a second, message-grouped Sentry issue per attempt.
                log::error!(
                    target: crate::logs::SENTRY_BREADCRUMB_ONLY_TARGET,
                    "Error during the flow. Attempt: {}, Cause {} {:#?}",
                    attempt,
                    e,
                    e
                );
                let code = e.code();
                let e = AttemptError { error: e, attempt };

                sentry::with_scope(
                    |scope| {
                        scope.set_tag("error_code", code);
                        scope.set_fingerprint(Some(&[code]));
                    },
                    || {
                        sentry::capture_error(&e);
                    },
                );
                self.analytics
                    .lock()
                    .await
                    .track_and_flush_silent((&e).into())
                    .await;

                last_error = Some(e);
                continue;
            }

            return std::result::Result::Ok(());
        }

        if let Some(e) = last_error {
            let error = FlowError {
                user_message: e.error.user_message().to_owned(),
            };
            std::result::Result::Err(error)
        } else {
            std::result::Result::Ok(())
        }
    }

    async fn launch_internal<T: EventChannel>(
        &self,
        channel: &T,
        state: Arc<Mutex<LaunchFlowState>>,
    ) -> StepResult {
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
            return StepResult::Ok(());
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
        self.app_launch_step
            .execute_if_needed(channel, state.clone(), "launch")
            .await?;
        StepResult::Ok(())
    }
}

struct FetchStep {}

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
    ) -> StepResult {
        let mut guard = state.lock().await;
        let latest_release = crate::s3::get_latest_explorer_release().await?;
        guard.latest_release = Some(latest_release);
        StepResult::Ok(())
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
    ) -> StepResult {
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

                StepResult::Ok(())
            }
            None => StepResult::Err(anyhow!("Latest release is not fetched").into()),
        }
    }
}

struct InstallStep {
    analytics: Arc<Mutex<Analytics>>,
    running_instances: Arc<Mutex<RunningInstances>>,
}

impl InstallStep {
    async fn execute_internal(&self, recent_download: RecentDownload) -> StepResult {
        self.check_explorer_not_running().await?;
        installs::install_explorer(
            &recent_download.version,
            Some(recent_download.downloaded_path),
        )
        .and_then(|()| installs::rename_explorer_to_latest())
    }

    async fn check_explorer_not_running(&self) -> StepResult {
        let running = self
            .running_instances
            .lock()
            .await
            .explorer_processes_by_path();
        if running.is_empty() {
            // `Ok`/`Err` are shadowed by `anyhow::Ok` (imported at the top),
            // so qualify with `StepResult` to stay on `StepError`.
            return StepResult::Ok(());
        }
        log::warn!(
            "Explorer is still running; refusing to install. Blocking processes: {:?}",
            running
        );
        StepResult::Err(StepError::E3008_EXPLORER_ALREADY_RUNNING { processes: running })
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

        Ok(guard.recent_download.is_none()
            && installs::explorer_latest_version_path().exists())
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
    ) -> StepResult {
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

        StepResult::Ok(())
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
    ) -> StepResultTyped<bool> {
        let Some(deeplink) = Protocol::value() else {
            return StepResultTyped::Ok(false);
        };

        // Re-check the bridge policy against this snapshot: an open_url event may have
        // reassigned the protocol since `is_complete`, so decide and act on one value.
        if !self.should_use_deeplink_bridge_for(&deeplink).await? {
            return StepResultTyped::Ok(false);
        }

        execute_passthrough(channel, &deeplink).await?;
        StepResultTyped::Ok(true)
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
    ) -> StepResult {
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
                    StepResult::Ok(())
                }
            }
            None => {
                //TODO passed version if specified manually from upper flow
                self.installs_hub
                    .lock()
                    .await
                    .launch_explorer(None, None)
                    .await?;
                StepResult::Ok(())
            }
        }
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

