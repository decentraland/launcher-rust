use anyhow::{anyhow, Context, Ok, Result};
use log::info;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use crate::{analytics::{event::Event, Analytics}, attempts::Attempts, environment::AppEnvironment, installs::{self, InstallsHub}, s3::{self, ReleaseResponse}, types::{BuildType, FlowError, Status, Step, StepError}};
use crate::channel::EventChannel;
use regex::Regex;
use sentry_anyhow::capture_anyhow;

pub trait LaunchStep {
    async fn is_complete(&self, state: Arc<Mutex<LaunchFlowState>>) -> Result<bool>;

    fn start_label(&self) -> Result<Status>;

    fn user_error_message(&self) -> &str;
    
    async fn execute<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> Result<()>;

    async fn execute_if_needed<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>, label: &str) -> std::result::Result<(), StepError> {
        let result = self.execute_if_needed_inner(channel, state, label).await;

        if let Err(e) = result {
            let error = StepError {
                inner_error: e,
                user_message: self.user_error_message().to_owned()
            };
            return std::result::Result::Err(error);
        }

        std::result::Result::Ok(())
    }

    async fn execute_if_needed_inner<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>, label: &str) -> Result<()> {
        let complete = self.is_complete(state.clone()).await?;
        if complete {
            info!("Step {} is already complete", label);
            return Ok(());
        }


        let status = self.start_label()?;
        channel.send(status)?;

        info!("Step {} is started", label);
        self.execute(channel, state).await?;
        info!("Step {} is finished", label);
        Ok(())
    }
}

pub struct LaunchFlowState {
    latest_release: Option<ReleaseResponse>,
    recent_download: Option<RecentDownload>,
    attempts: Attempts,
}

#[derive(Clone)]
struct RecentDownload {
    version: String,
    downloaded_path: PathBuf,
}

impl Default for LaunchFlowState {
    fn default() -> Self {
        LaunchFlowState {
            latest_release: None,
            recent_download: None,
            attempts: Attempts::default(),
        }
    }
}

pub struct LaunchFlow {
    fetch_step: FetchStep,
    download_step: DownloadStep,
    install_step: InstallStep,
    app_launch_step: AppLaunchStep,
}

impl LaunchFlow {

    pub fn new(installs_hub: Arc<Mutex<InstallsHub>>, analytics: Arc<Mutex<Analytics>>) -> Self {
        LaunchFlow {
            fetch_step: FetchStep{},
            download_step: DownloadStep {
                analytics: analytics.clone(),
            },
            install_step: InstallStep{
                analytics: analytics.clone(),
            },
            app_launch_step: AppLaunchStep {
                installs_hub,
            },
        }
    }

    pub async fn launch<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> std::result::Result<(), FlowError> {
        let result = self.launch_internal(channel, state.clone()).await;
        if let Err(e) = result {
            log::error!("Error during the flow {} {:#}", e.user_message, e.inner_error);
            capture_anyhow(&e.inner_error);
            let can_retry = Self::can_retry(state).await;
            let error = FlowError {
                user_message: e.user_message,
                can_retry 
            };
            return std::result::Result::Err(error);
        }
         
        std::result::Result::Ok(())
    }


    async fn launch_internal<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> std::result::Result<(), StepError> {
        Self::validate_attempt_and_increase(state.clone()).await?;
        self.fetch_step.execute_if_needed(channel, state.clone(), "fetch").await?;
        self.download_step.execute_if_needed(channel, state.clone(), "download").await?;
        self.install_step.execute_if_needed(channel, state.clone(), "install").await?;
        self.app_launch_step.execute_if_needed(channel, state.clone(), "launch").await?;
        std::result::Result::Ok(())
    }

    async fn validate_attempt_and_increase(state: Arc<Mutex<LaunchFlowState>>) -> std::result::Result<(), StepError> {
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

struct FetchStep {}

impl LaunchStep for FetchStep {
    async fn is_complete(&self, _state: Arc<Mutex<LaunchFlowState>>) -> Result<bool> {
        // always refetch the origin
        Ok(false)
    }

    fn user_error_message(&self) -> &str {
        "Fetch the latest client version failed"
    }

    fn start_label(&self) -> Result<Status> {
        let status = Status::State { step: Step::Fetching };
        Ok(status)
    }
    
    async fn execute<T: EventChannel>(&self, _channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> Result<()> {
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
        let mode = if any_installed { BuildType::Update } else { BuildType::New };
        mode
    }

    async fn version_from_url(&self, url: &str) -> Result<String> {
        let pattern = format!(r"(^{}\/{}\/(v?\d+\.\d+\.\d+-?\w*)\/(\w+.zip))", AppEnvironment::bucket_url(), s3::RELEASE_PREFIX);
        let re = Regex::new(&pattern)?;

        let captures = re.captures(url).context(format!("cannot find matches in the url: {}", url))?;
        let version = captures.get(2).map(|m| m.as_str());

        match version {
            Some(v) => {
                Ok(v.to_owned())
            },
            None => {
                let mut guard = self.analytics.lock().await;
                guard
                    .track_and_flush_silent(Event::DOWNLOAD_VERSION_ERROR { version: None, error: "No version provided".to_owned() })
                    .await;
                Err(anyhow!("url doesn't contain version"))
            },
        }
    }
}

impl LaunchStep for DownloadStep {
    async fn is_complete(&self, state: Arc<Mutex<LaunchFlowState>>) -> Result<bool> {
        let guard = state.lock().await;
        match &guard.latest_release {
            Some(release) => {

                let version = release.version.as_str();
                let updated = crate::installs::is_explorer_updated(version);
                Ok(updated)
            },
            None => {
                Err(anyhow!("Latest release is not found in the state"))
            },
        }
    }

    fn user_error_message(&self) -> &str {
        "Failed to download"
    }

    fn start_label(&self) -> Result<Status> {
        let mode = DownloadStep::mode();
        let status = Status::State { step: Step::Downloading { progress: 0, build_type: mode } };
        Ok(status)
    }
    
    async fn execute<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> Result<()> {
        let mode = DownloadStep::mode();

        let mut guard = state.lock().await;

        let release = &guard.latest_release;
        match release {
            Some(r) => {
                let url = &r.browser_download_url;
                let version = self.version_from_url(url).await?;

                let target_path = installs::target_download_path();
                let path: &str = target_path.to_str().context("Cannot convert target download path")?;

                {
                    let mut analytics = self.analytics.lock().await;
                    analytics.track_and_flush_silent(Event::DOWNLOAD_VERSION { version: version.clone() }).await;
                }

                let result = installs::downloads::download_file(url, path, channel, &mode, self.analytics.clone()).await;

                let mut analytics = self.analytics.lock().await;
                if let Err(e) = result {
                    analytics.track_and_flush_silent(Event::DOWNLOAD_VERSION_ERROR { version: Some(version.clone()), error: e.to_string() }).await;
                }
                else {
                    analytics.track_and_flush_silent(Event::DOWNLOAD_VERSION_SUCCESS { version: version.clone() }).await;
                }

                guard.recent_download = Some(
                    RecentDownload {
                        version, 
                        downloaded_path: target_path,
                    }
                );

                Ok(())

            },
            None => {
                Err(anyhow!("Latest release is not fetched"))
            },
        }
    }
}

struct InstallStep {
    analytics: Arc<Mutex<Analytics>>,
}

impl InstallStep {
    async fn execute_internal(recent_download: RecentDownload) -> Result<()> {
        installs::install_explorer(&recent_download.version, Some(recent_download.downloaded_path)).await?;
        Ok(())
    }

    async fn recent_download_and_update_state(state: Arc<Mutex<LaunchFlowState>>) -> Option<RecentDownload> {
        let mut guard = state.lock().await;
        let recent_download = guard.recent_download.clone();
        if recent_download.is_none() {
            return None;
        }
        guard.recent_download = None;
        recent_download
    }
}

impl LaunchStep for InstallStep {
    async fn is_complete(&self, state: Arc<Mutex<LaunchFlowState>>) -> Result<bool> {
        let guard = state.lock().await;
        Ok(guard.recent_download.is_none())
    }

    fn user_error_message(&self) -> &str {
        "Failed to install"
    }

    fn start_label(&self) -> Result<Status> {
        let mode = DownloadStep::mode();
        let status = Status::State { step: Step::Installing { build_type: mode } };
        Ok(status)
    }
    
    async fn execute<T: EventChannel>(&self, _channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> Result<()> {
        let mut analytics = self.analytics.lock().await;

        let recent_download = InstallStep::recent_download_and_update_state(state).await;
        match recent_download {
            Some(download) => {
                let version = download.version.clone();
                analytics.track_and_flush_silent(Event::INSTALL_VERSION_START { version: version.clone() }).await;
                let result = InstallStep::execute_internal(download).await;
                if let Err(e) = &result {
                    analytics.track_and_flush_silent(Event::INSTALL_VERSION_ERROR { version: Some(version), error: e.to_string() }).await;
                }
                else {
                    analytics.track_and_flush_silent(Event::INSTALL_VERSION_SUCCESS { version }).await;
                }
                result
            },
            None => {
                const ERROR_MESSAGE: &str = "Downloaded archive not found";
                analytics.track_and_flush_silent(Event::INSTALL_VERSION_ERROR { version: None, error: ERROR_MESSAGE.to_owned() }).await;
                Err(anyhow!(ERROR_MESSAGE))
            },
        }
    }
}

struct AppLaunchStep {
    installs_hub: Arc<Mutex<InstallsHub>>,
}

impl LaunchStep for AppLaunchStep {
    async fn is_complete(&self, _: Arc<Mutex<LaunchFlowState>>) -> Result<bool> {
        // Always launch explorer
        Ok(false)
    }

    fn user_error_message(&self) -> &str {
        "Failed to launch"
    }

    fn start_label(&self) -> Result<Status> {
        let status = Status::State { step: Step::Launching };
        Ok(status)
    }
    
    async fn execute<T: EventChannel>(&self, _channel: &T, _state: Arc<Mutex<LaunchFlowState>>) -> Result<()> {
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
