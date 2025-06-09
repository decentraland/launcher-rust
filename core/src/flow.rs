use crate::channel::EventChannel;
use crate::instances::RunningInstances;
use crate::{
    analytics::{Analytics, event::Event},
    attempts::Attempts,
    environment::AppEnvironment,
    installs::{self, InstallsHub},
    s3::{self, ReleaseResponse},
    types::{BuildType, FlowError, Status, Step, StepError},
};
use anyhow::{Context, Ok, Result, anyhow};
use log::info;
use regex::Regex;
use sentry_anyhow::capture_anyhow;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

pub trait LaunchStep<TIntent, TState> {
    async fn is_complete(&self, intent: &TIntent, state: Arc<Mutex<TState>>) -> Result<bool>;

    fn start_label(&self) -> Result<Status>;

    fn user_error_message(&self) -> &str;

    async fn execute<T: EventChannel>(
        &self,
        channel: &T,
        intent: &TIntent,
        state: Arc<Mutex<TState>>,
    ) -> Result<()>;

    async fn execute_if_needed<T: EventChannel>(
        &self,
        channel: &T,
        intent: &TIntent,
        state: Arc<Mutex<TState>>,
        label: &str,
    ) -> std::result::Result<(), StepError> {
        let result = self
            .execute_if_needed_inner(channel, intent, state, label)
            .await;

        if let Err(e) = result {
            let error = StepError {
                inner_error: e,
                user_message: self.user_error_message().to_owned(),
            };
            return std::result::Result::Err(error);
        }

        std::result::Result::Ok(())
    }

    async fn execute_if_needed_inner<T: EventChannel>(
        &self,
        channel: &T,
        intent: &TIntent,
        state: Arc<Mutex<TState>>,
        label: &str,
    ) -> Result<()> {
        let complete = self.is_complete(intent, state.clone()).await?;
        if complete {
            info!("Step {} is already complete", label);
            return Ok(());
        }

        let status = self.start_label()?;
        channel.send(status)?;

        info!("Step {} is started", label);
        self.execute(channel, intent, state).await?;
        info!("Step {} is finished", label);
        Ok(())
    }
}

type FlowVariantResult = std::result::Result<(), StepError>;

trait FlowVariant<TIntent, TState> {
    async fn launch<T: EventChannel>(
        &self,
        channel: &T,
        intent: &TIntent,
        state: Arc<Mutex<TState>>,
    ) -> FlowVariantResult;
}

pub struct LaunchFlowState {
    intent_state: IntentState,
    attempts: Attempts,
}

impl Default for LaunchFlowState {
    //TODO resolve intent
    fn default() -> Self {
        Self {
            intent_state: IntentState::OpenAppInstance(OpenAppInstanceState::default()),
            attempts: Attempts::default(),
        }
    }
}

enum IntentState {
    OpenAppInstance(OpenAppInstanceState),
    OpenDeepLinkInExistingInstance(OpenDeepLinkInExistingInstanceState),
}

#[derive(Default)]
struct OpenAppInstanceState {
    latest_release: Option<ReleaseResponse>,
    recent_download: Option<RecentDownload>,
}

#[derive(Default)]
struct OpenDeepLinkInExistingInstanceState;

pub enum LaunchIntent {
    OpenAppInstance(OpenAppInstanceIntent),
    OpenDeepLinkInExistingInstance(DeepLinkIntent),
}

#[derive(Default)]
pub struct OpenAppInstanceIntent;

pub struct DeepLinkIntent {
    deeplink: String,
}

#[derive(Clone)]
struct RecentDownload {
    version: String,
    downloaded_path: PathBuf,
}

pub struct LaunchFlow {
    open_app_instance: OpenAppInstanceFlow,
    open_deeplink_in_existing_instance: OpenDeepLinkInExistingInstanceFlow,
}

struct OpenAppInstanceFlow {
    fetch_step: FetchStep,
    download_step: DownloadStep,
    install_step: InstallStep,
    app_launch_step: AppLaunchStep,
}

impl FlowVariant<OpenAppInstanceIntent, OpenAppInstanceState> for OpenAppInstanceFlow {
    async fn launch<T: EventChannel>(
        &self,
        channel: &T,
        intent: &OpenAppInstanceIntent,
        state: Arc<Mutex<OpenAppInstanceState>>,
    ) -> FlowVariantResult {
        self.fetch_step
            .execute_if_needed(channel, intent, state.clone(), "fetch")
            .await?;
        self.download_step
            .execute_if_needed(channel, intent, state.clone(), "download")
            .await?;
        self.install_step
            .execute_if_needed(channel, intent, state.clone(), "install")
            .await?;
        self.app_launch_step
            .execute_if_needed(channel, intent, state.clone(), "launch")
            .await?;
        FlowVariantResult::Ok(())
    }
}

struct OpenDeepLinkInExistingInstanceFlow {
    deeplink_step: HandleDeepLinkStep,
}

impl FlowVariant<DeepLinkIntent, OpenDeepLinkInExistingInstanceState>
    for OpenDeepLinkInExistingInstanceFlow
{
    async fn launch<T: EventChannel>(
        &self,
        channel: &T,
        intent: &DeepLinkIntent,
        state: Arc<Mutex<OpenDeepLinkInExistingInstanceState>>,
    ) -> FlowVariantResult {
        self.deeplink_step
            .execute_if_needed(channel, intent, state.clone(), "deeplink")
            .await?;
        FlowVariantResult::Ok(())
    }
}

impl LaunchFlow {
    pub fn new(
        installs_hub: Arc<Mutex<InstallsHub>>,
        analytics: Arc<Mutex<Analytics>>,
        running_instances: Arc<Mutex<RunningInstances>>,
    ) -> Self {
        Self {
            open_app_instance: OpenAppInstanceFlow {
                fetch_step: FetchStep {},
                download_step: DownloadStep {
                    analytics: analytics.clone(),
                },
                install_step: InstallStep {
                    analytics: analytics.clone(),
                },
                app_launch_step: AppLaunchStep { installs_hub },
            },
            open_deeplink_in_existing_instance: OpenDeepLinkInExistingInstanceFlow {
                deeplink_step: HandleDeepLinkStep { running_instances },
            },
        }
    }

    pub async fn launch<T: EventChannel>(
        &self,
        channel: &T,
        intent: LaunchIntent,
        state: Arc<Mutex<LaunchFlowState>>,
    ) -> std::result::Result<(), FlowError> {
        let result = self.launch_internal(channel, intent, state.clone()).await;
        if let Err(e) = result {
            log::error!(
                "Error during the flow {} {:#}",
                e.user_message,
                e.inner_error
            );
            capture_anyhow(&e.inner_error);
            let can_retry = Self::can_retry(state).await;
            let error = FlowError {
                user_message: e.user_message,
                can_retry,
            };
            return std::result::Result::Err(error);
        }

        std::result::Result::Ok(())
    }

    async fn launch_internal<T: EventChannel>(
        &self,
        channel: &T,
        intent: LaunchIntent,
        state: Arc<Mutex<LaunchFlowState>>,
    ) -> std::result::Result<(), StepError> {
        Self::validate_attempt_and_increase(state.clone()).await?;

        match intent {
            LaunchIntent::OpenAppInstance(intent) => {
                info!("Open a new app instance");
                //TODO pass state
                let state = Arc::new(Mutex::new(OpenAppInstanceState::default()));
                self.open_app_instance
                    .launch(channel, &intent, state)
                    .await?;
            }
            LaunchIntent::OpenDeepLinkInExistingInstance(deeplink) => {
                info!(
                    "Open deep link in existing instance: {:?}",
                    deeplink.deeplink
                );
                //TODO pass state
                let state = Arc::new(Mutex::new(OpenDeepLinkInExistingInstanceState::default()));
                self.open_deeplink_in_existing_instance
                    .launch(channel, &deeplink, state)
                    .await?;
            }
        }

        std::result::Result::Ok(())
    }

    async fn validate_attempt_and_increase(
        state: Arc<Mutex<LaunchFlowState>>,
    ) -> std::result::Result<(), StepError> {
        let mut guard = state.lock().await;

        if guard.attempts.try_consume_attempt() {
            return std::result::Result::Ok(());
        }

        let message = "Out of attempts";
        let inner_error = anyhow!(message);
        let error = StepError {
            inner_error,
            user_message: message.to_owned(),
        };
        std::result::Result::Err(error)
    }

    async fn can_retry(state: Arc<Mutex<LaunchFlowState>>) -> bool {
        let guard = state.lock().await;
        guard.attempts.can_retry()
    }
}

struct HandleDeepLinkStep {
    running_instances: Arc<Mutex<RunningInstances>>,
}

impl LaunchStep<DeepLinkIntent, OpenDeepLinkInExistingInstanceState> for HandleDeepLinkStep {
    async fn is_complete(
        &self,
        _intent: &DeepLinkIntent,
        _state: Arc<Mutex<OpenDeepLinkInExistingInstanceState>>,
    ) -> Result<bool> {
        // always handle deeplink
        Ok(false)
    }

    fn user_error_message(&self) -> &str {
        "Cannot open deeplink"
    }

    fn start_label(&self) -> Result<Status> {
        let status = Status::State {
            step: Step::DeeplinkOpening,
        };
        Ok(status)
    }

    async fn execute<T: EventChannel>(
        &self,
        _channel: &T,
        _intent: &DeepLinkIntent,
        _state: Arc<Mutex<OpenDeepLinkInExistingInstanceState>>,
    ) -> Result<()> {
        let instances = self.running_instances.lock().await;
        if instances
            .any_is_running()
            .context("Cannot define if any client isntance is running")?
        {

            //TODO deeplink server
            //update if consumed
        }

        Ok(())
    }
}

struct FetchStep {}

impl LaunchStep<OpenAppInstanceIntent, OpenAppInstanceState> for FetchStep {
    async fn is_complete(
        &self,
        _intent: &OpenAppInstanceIntent,
        _state: Arc<Mutex<OpenAppInstanceState>>,
    ) -> Result<bool> {
        // always refetch the origin
        Ok(false)
    }

    fn user_error_message(&self) -> &str {
        "Fetch the latest client version failed"
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
        _intent: &OpenAppInstanceIntent,
        state: Arc<Mutex<OpenAppInstanceState>>,
    ) -> Result<()> {
        let mut guard = state.lock().await;
        let latest_release = crate::s3::get_latest_explorer_release().await?;
        guard.latest_release = Some(latest_release);
        Ok(())
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
                let mut guard = self.analytics.lock().await;
                guard
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

impl LaunchStep<OpenAppInstanceIntent, OpenAppInstanceState> for DownloadStep {
    async fn is_complete(
        &self,
        _intent: &OpenAppInstanceIntent,
        state: Arc<Mutex<OpenAppInstanceState>>,
    ) -> Result<bool> {
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

    fn user_error_message(&self) -> &str {
        "Failed to download"
    }

    fn start_label(&self) -> Result<Status> {
        let mode = DownloadStep::mode();
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
        _intent: &OpenAppInstanceIntent,
        state: Arc<Mutex<OpenAppInstanceState>>,
    ) -> Result<()> {
        let mode = DownloadStep::mode();

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
                    let mut analytics = self.analytics.lock().await;
                    analytics
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

                let mut analytics = self.analytics.lock().await;
                if let Err(e) = result {
                    analytics
                        .track_and_flush_silent(Event::DOWNLOAD_VERSION_ERROR {
                            version: Some(version.clone()),
                            error: e.to_string(),
                        })
                        .await;
                } else {
                    analytics
                        .track_and_flush_silent(Event::DOWNLOAD_VERSION_SUCCESS {
                            version: version.clone(),
                        })
                        .await;
                }

                guard.recent_download = Some(RecentDownload {
                    version,
                    downloaded_path: target_path,
                });

                Ok(())
            }
            None => Err(anyhow!("Latest release is not fetched")),
        }
    }
}

struct InstallStep {
    analytics: Arc<Mutex<Analytics>>,
}

impl InstallStep {
    async fn execute_internal(recent_download: RecentDownload) -> Result<()> {
        installs::install_explorer(
            &recent_download.version,
            Some(recent_download.downloaded_path),
        )
        .await
    }

    async fn recent_download_and_update_state(
        state: Arc<Mutex<OpenAppInstanceState>>,
    ) -> Option<RecentDownload> {
        let mut guard = state.lock().await;
        let recent_download = guard.recent_download.clone();
        if recent_download.is_none() {
            return None;
        }
        guard.recent_download = None;
        recent_download
    }
}

impl LaunchStep<OpenAppInstanceIntent, OpenAppInstanceState> for InstallStep {
    async fn is_complete(
        &self,
        _intent: &OpenAppInstanceIntent,
        state: Arc<Mutex<OpenAppInstanceState>>,
    ) -> Result<bool> {
        let guard = state.lock().await;
        Ok(guard.recent_download.is_none())
    }

    fn user_error_message(&self) -> &str {
        "Failed to install"
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
        _intent: &OpenAppInstanceIntent,
        state: Arc<Mutex<OpenAppInstanceState>>,
    ) -> Result<()> {
        let mut analytics = self.analytics.lock().await;

        let recent_download = InstallStep::recent_download_and_update_state(state).await;
        match recent_download {
            Some(download) => {
                let version = download.version.clone();
                analytics
                    .track_and_flush_silent(Event::INSTALL_VERSION_START {
                        version: version.clone(),
                    })
                    .await;
                let result = InstallStep::execute_internal(download).await;
                if let Err(e) = &result {
                    analytics
                        .track_and_flush_silent(Event::INSTALL_VERSION_ERROR {
                            version: Some(version),
                            error: e.to_string(),
                        })
                        .await;
                } else {
                    analytics
                        .track_and_flush_silent(Event::INSTALL_VERSION_SUCCESS { version })
                        .await;
                }
                result
            }
            None => {
                const ERROR_MESSAGE: &str = "Downloaded archive not found";
                analytics
                    .track_and_flush_silent(Event::INSTALL_VERSION_ERROR {
                        version: None,
                        error: ERROR_MESSAGE.to_owned(),
                    })
                    .await;
                Err(anyhow!(ERROR_MESSAGE))
            }
        }
    }
}

struct AppLaunchStep {
    installs_hub: Arc<Mutex<InstallsHub>>,
}

impl LaunchStep<OpenAppInstanceIntent, OpenAppInstanceState> for AppLaunchStep {
    async fn is_complete(
        &self,
        _intent: &OpenAppInstanceIntent,
        _: Arc<Mutex<OpenAppInstanceState>>,
    ) -> Result<bool> {
        // Always launch explorer
        Ok(false)
    }

    fn user_error_message(&self) -> &str {
        "Failed to launch"
    }

    fn start_label(&self) -> Result<Status> {
        let status = Status::State {
            step: Step::Launching,
        };
        Ok(status)
    }

    async fn execute<T: EventChannel>(
        &self,
        _channel: &T,
        _intent: &OpenAppInstanceIntent,
        _state: Arc<Mutex<OpenAppInstanceState>>,
    ) -> Result<()> {
        let guard = self.installs_hub.lock().await;

        //TODO passed version if specified manually from upper flow
        guard.launch_explorer(None).await?;
        Ok(())
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
