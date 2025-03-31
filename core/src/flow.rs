use anyhow::{anyhow, Context, Ok, Result};
use log::info;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use crate::{installs::{self, InstallsHub}, s3::{self, ReleaseResponse}, types::{Status, Step, BuildType}};
use crate::channel::EventChannel;
use regex::Regex;

pub trait LaunchStep {
    async fn is_complete(&self, state: Arc<Mutex<LaunchFlowState>>) -> Result<bool>;

    fn start_label(&self) -> Result<Status>;
    
    async fn execute<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> Result<()>;

    async fn execute_if_needed<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>, label: &str) -> Result<()> {
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
    pub fn new(installs_hub: Arc<Mutex<InstallsHub>>) -> Self {
        LaunchFlow {
            fetch_step: FetchStep{},
            download_step: DownloadStep{},
            install_step: InstallStep{},
            app_launch_step: AppLaunchStep {
                installs_hub,
            },
        }
    }

    pub async fn launch<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> Result<()> {
        self.fetch_step.execute_if_needed(channel, state.clone(), "fetch").await?;
        self.download_step.execute_if_needed(channel, state.clone(), "download").await?;
        self.install_step.execute_if_needed(channel, state.clone(), "install").await?;
        self.app_launch_step.execute_if_needed(channel, state.clone(), "launch").await?;
        Ok(())
    }
}

struct FetchStep {}

impl LaunchStep for FetchStep {
    async fn is_complete(&self, _state: Arc<Mutex<LaunchFlowState>>) -> Result<bool> {
        // always refetch the origin
        Ok(false)
    }

    fn start_label(&self) -> Result<Status> {
        let status = Status::State { step: Step::Fetching };
        Ok(status)
    }
    
    async fn execute<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> Result<()> {
        let mut guard = state.lock().await;
        let latest_release = crate::s3::get_latest_explorer_release().await?;
        guard.latest_release = Some(latest_release);
        Ok(())
    }

}

struct DownloadStep {}

impl DownloadStep {
    pub fn mode() -> BuildType {
        let any_installed = crate::installs::is_explorer_installed(None);
        let mode = if any_installed { BuildType::Update } else { BuildType::New };
        mode
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

                let pattern = format!(r"(^{}\/{}\/(v?\d+\.\d+\.\d+-?\w*)\/(\w+.zip))", s3::bucket_url()?, s3::RELEASE_PREFIX);
                let re = Regex::new(&pattern)?;

                let captures = re.captures(url).context(format!("cannot find matches in the url: {}", url))?;
                // TODO preserved for analytics
                let version = captures.get(2).map(|m| m.as_str()).context(format!("url doesn't contain version"))?.to_string();

                let target_path = installs::target_download_path();
                let path: &str = target_path.to_str().context("Cannot convert target download path")?;

                installs::downloads::download_file(url, path, channel, &mode).await?;

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

struct InstallStep {}

impl LaunchStep for InstallStep {
    async fn is_complete(&self, state: Arc<Mutex<LaunchFlowState>>) -> Result<bool> {
        let guard = state.lock().await;
        Ok(guard.recent_download.is_none())
    }

    fn start_label(&self) -> Result<Status> {
        let mode = DownloadStep::mode();
        let status = Status::State { step: Step::Installing { build_type: mode } };
        Ok(status)
    }
    
    async fn execute<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> Result<()> {
        let mut guard = state.lock().await;
        let recent_download = guard.recent_download.clone().ok_or_else(|| anyhow!("Downloaded archive not found"))?;
        guard.recent_download = None;
        installs::install_explorer(&recent_download.version, Some(recent_download.downloaded_path)).await?;
        Ok(())
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

    fn start_label(&self) -> Result<Status> {
        let status = Status::State { step: Step::Launching };
        Ok(status)
    }
    
    async fn execute<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> Result<()> {
        let guard = self.installs_hub.lock().await;

        //TODO passed version if specified manually from upper flow
        guard.launch_explorer(None).await?;
        //TODO close launcher
                //close_window().await?;

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
