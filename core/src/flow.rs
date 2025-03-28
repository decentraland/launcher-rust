use anyhow::{anyhow, Context, Ok, Result};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use crate::{installs::{self, InstallsHub}, s3::{self, ReleaseResponse}};
use crate::types::Status;
use crate::channel::EventChannel;
use regex::Regex;

pub trait LaunchStep {
    async fn is_complete(&self, state: Arc<Mutex<LaunchFlowState>>) -> Result<bool>;
    
    async fn execute<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> Result<()>;

    async fn execute_if_needed<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> Result<()> {
        let complete = self.is_complete(state.clone()).await?;
        if complete {
            return Ok(());
        }

        self.execute(channel, state).await
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
        self.fetch_step.execute_if_needed(channel, state.clone()).await?;
        self.download_step.execute_if_needed(channel, state.clone()).await?;
        self.install_step.execute_if_needed(channel, state.clone()).await?;
        self.app_launch_step.execute_if_needed(channel, state.clone()).await?;
        Ok(())
    }
}

struct FetchStep {}

impl LaunchStep for FetchStep {
    async fn is_complete(&self, _state: Arc<Mutex<LaunchFlowState>>) -> Result<bool> {
        // always refetch the origin
        Ok(false)
    }
    
    async fn execute<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> Result<()> {
        let mut guard = state.lock().await;
        let latest_release = crate::s3::get_latest_explorer_release().await?;
        guard.latest_release = Some(latest_release);
        Ok(())
    }

}

struct DownloadStep {}

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
    
    async fn execute<T: EventChannel>(&self, channel: &T, state: Arc<Mutex<LaunchFlowState>>) -> Result<()> {
        let any_installed = crate::installs::is_explorer_installed(None);
        let mode = if any_installed { crate::types::BuildType::Update } else { crate::types::BuildType::New };

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
        Ok(guard.recent_download.is_some())
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



  const initialized = useRef(false);
  const [state, setState] = useState<AppState | undefined>(undefined);
  const [isInstalled, setIsInstalled] = useState(false);
  const [isUpdated, setIsUpdated] = useState(false);
  const [downloadUrl, setDownloadUrl] = useState<string | undefined>(undefined);
  const [downloadingProgress, setDownloadingProgress] = useState(0);
  const [downloadedVersion, setDownloadedVersion] = useState<string | undefined>(undefined);
  const [retry, setRetry] = useState(0);
  const [error, setError] = useState<string | undefined>(undefined);

// TODO catch these 2 params
  const shouldRunDevVersion = getRunDevVersion();
  const customDownloadedFilePath = getDownloadedFilePath();

  const handleFetch = useCallback(async () => {
    try {
      + const { browser_download_url: url, version } = await getLatestRelease();
      + setDownloadUrl(url);
      // If there is any Explorer version installed, set isInstalled = true
      + setIsInstalled(await isExplorerInstalled());

      // Validates if the version fetched is installed or not to download the new version
      ++ const _isInstalled = await isExplorerInstalled(version);
      ++ if (!_isInstalled) {
      ++  handleDownload(url);
      ++  return;
      ++ }

      ++ setState(AppState.Installed);

      const _isUpdated = await isExplorerUpdated(version);
      if (!_isUpdated) {
        handleDownload(url);
        return;
      }
      setIsUpdated(true);
      setRetry(0);
      handleLaunch();
    } catch (error) {
      const errorMessage = getErrorMessage(error);
      setError(getErrorMessage(errorMessage));
      log.error('[Renderer][Home][GetLatestRelease]', errorMessage);
      handleRetryFetch();
    }
  }, [setDownloadUrl, setError, setIsInstalled, setIsUpdated, setState]);

  const handleRetryFetch = useCallback(
    (manualRetry: boolean = false) => {
      if (!manualRetry && retry >= 5) {
        return;
      }

      setRetry(retry + 1);
      setTimeout(() => {
        handleFetch();
      }, FIVE_SECONDS);
    },
    [retry],
  );

  const handleLaunch = useCallback((version?: string) => {
    const _version = shouldRunDevVersion ? 'dev' : version;
    setState(AppState.Launching);
    setTimeout(() => {
      launchExplorer(_version);
      launchState(handleLaunchState);
    }, ONE_SECOND);
  }, []);

  const handleLaunchState = useCallback(
    (_event: IpcRendererEvent, eventData: IpcRendererEventData) => {
      switch (eventData.type) {
        case IPC_EVENT_DATA_TYPE.LAUNCHED:
          setState(AppState.Launched);
          break;
        case IPC_EVENT_DATA_TYPE.ERROR:
          setError((eventData as IpcRendererEventDataError).error);
          log.error('[Renderer][Home][HandleLaunchState]', getErrorMessage((eventData as IpcRendererEventDataError).error));
          break;
      }
    },
    [setError, setState],
  );

  const handleRetryInstall = useCallback(
    (manualRetry: boolean = false) => {
      if (!manualRetry && retry >= 5) {
        return;
      }

      if (!downloadedVersion) {
        return;
      }

      setRetry(retry + 1);
      setTimeout(() => {
        handleInstall(downloadedVersion);
      }, FIVE_SECONDS);
    },
    [downloadedVersion, retry],
  );

  const handleInstallState = useCallback(
    (_event: IpcRendererEvent, eventData: IpcRendererEventData) => {
      switch (eventData.type) {
        case IPC_EVENT_DATA_TYPE.START:
          setState(AppState.Installing);
          break;
        case IPC_EVENT_DATA_TYPE.COMPLETED:
          setState(AppState.Installed);
          setIsUpdated(true);
          setRetry(0);
          handleLaunch();
          break;
        case IPC_EVENT_DATA_TYPE.ERROR:
          setError((eventData as IpcRendererEventDataError).error);
          log.error('[Renderer][Home][HandleInstallState]', getErrorMessage((eventData as IpcRendererEventDataError).error));
          handleRetryInstall();
          break;
      }
    },
    [handleLaunch, handleRetryInstall, setError, setIsUpdated, setRetry, setState],
  );

  const handleInstall = useCallback((version: string, downloadedFilePath?: string) => {
    installExplorer(version, downloadedFilePath);
    installState(handleInstallState);
  }, []);

  const handleRetryDownload = useCallback(
    (manualRetry: boolean = false) => {
      if (!downloadUrl) {
        throw new Error('Not available downloadable release found.');
      }

      if (!manualRetry && retry >= 5) {
        return;
      }

      setRetry(retry + 1);
      setTimeout(() => {
        handleDownload(downloadUrl);
      }, FIVE_SECONDS);
    },
    [retry, downloadUrl],
  );

  const handleDownloadState = useCallback(
    (_event: IpcRendererEvent, eventData: IpcRendererEventData) => {
      switch (eventData.type) {
        case IPC_EVENT_DATA_TYPE.START:
          setState(AppState.Downloading);
          break;
        case IPC_EVENT_DATA_TYPE.PROGRESS:
          setDownloadingProgress((eventData as IpcRendererDownloadProgressStateEventData).progress);
          break;
        case IPC_EVENT_DATA_TYPE.COMPLETED: {
          const downloadeVersion = (eventData as IpcRendererDownloadCompletedEventData).version;
          setState(AppState.Downloaded);
          setDownloadedVersion(downloadeVersion);
          setRetry(0);
          handleInstall(downloadeVersion);
          break;
        }
        case IPC_EVENT_DATA_TYPE.CANCELLED: {
          const downloadeVersion = (eventData as IpcRendererDownloadCompletedEventData)?.version;
          if (downloadeVersion) {
            handleLaunch(downloadeVersion);
          } else {
            setState(AppState.Cancelled);
          }
          break;
        }
        case IPC_EVENT_DATA_TYPE.ERROR:
          setError((eventData as IpcRendererEventDataError).error);
          log.error('[Renderer][Home][HandleDownloadState]', getErrorMessage((eventData as IpcRendererEventDataError).error));
          handleRetryDownload();
          break;
      }
    },
    [handleInstall, handleRetryDownload, setDownloadingProgress, setDownloadedVersion, setError, setRetry, setState],
  );

  const handleDownload = useCallback((url: string) => {
    downloadExplorer(url);
    downloadState(handleDownloadState);
  }, []);



*/
