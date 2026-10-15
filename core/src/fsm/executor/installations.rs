use anyhow::{Context, anyhow};
use log::{error, warn};
use tokio::sync::mpsc::UnboundedSender;

use crate::channel::EventChannel;
use crate::errors::{DCLError, DCLErrorResult};
use crate::fsm::msg::Msg;
use crate::fsm::values::{ReadyInstallation, UpdateStrategy};
use crate::installs::{self, downloads};
use crate::s3;
use crate::types::{BuildType, LaunchStatus, Status, Step};
use crate::version::Version;

use super::{Executor, deliver};

impl Executor {
    pub(super) async fn scan_local_installations(&self, inbox: &UnboundedSender<Msg>) {
        let ready = match tokio::task::spawn_blocking(installs::ready_installations).await {
            Ok(versions) => versions.into_iter().map(ReadyInstallation).collect(),
            Err(e) => {
                error!("[fsm] local installations scan panicked: {e}");
                Vec::new()
            }
        };
        deliver(inbox, Msg::LocalInstallationPresenceConfirmed(ready));
    }

    pub(super) async fn download_version(
        &self,
        strategy: UpdateStrategy,
        inbox: &UnboundedSender<Msg>,
    ) {
        let result = match &strategy {
            UpdateStrategy::ByZip { target } => self.download_zip(target, &strategy, inbox).await,
            UpdateStrategy::ByDelta { .. } => {
                Err(anyhow!("delta download is not implemented yet").into())
            }
        };
        let msg = match result {
            Ok(()) => Msg::VersionDownloadCompleted(strategy),
            Err(error) => Msg::VersionDownloadFailed(strategy, error),
        };
        deliver(inbox, msg);
    }

    async fn download_zip(
        &self,
        target: &Version,
        strategy: &UpdateStrategy,
        inbox: &UnboundedSender<Msg>,
    ) -> DCLErrorResult {
        let url = s3::release_zip_url(&target.to_string());
        let path = installs::target_download_path();
        let path = path
            .to_str()
            .context("Cannot convert target download path")?;
        let progress = DownloadProgress {
            strategy: strategy.clone(),
            inbox: inbox.clone(),
        };
        downloads::download_file(
            &url,
            path,
            &progress,
            &BuildType::New,
            self.analytics.clone(),
        )
        .await?;
        Ok(())
    }

    pub(super) async fn install_version(
        &self,
        strategy: UpdateStrategy,
        inbox: &UnboundedSender<Msg>,
    ) {
        let target = strategy.target().clone();
        let result = match &strategy {
            UpdateStrategy::ByZip { .. } => self.install_zip(&target).await,
            UpdateStrategy::ByDelta { .. } => {
                Err(anyhow!("delta install is not implemented yet").into())
            }
        };
        let msg = match result {
            Ok(()) => Msg::VersionInstallationCompleted(target.clone(), ReadyInstallation(target)),
            Err(error) => Msg::VersionInstallationFailed(strategy, error),
        };
        deliver(inbox, msg);
    }

    /// Refuses while an Explorer holds files under `latest`, then installs on a blocking thread.
    async fn install_zip(&self, target: &Version) -> DCLErrorResult {
        let running = self
            .running_instances
            .lock()
            .await
            .explorer_processes_by_path();
        if !running.is_empty() {
            warn!("[fsm] Explorer is still running; refusing to install: {running:?}");
            return Err(DCLError::E3008_EXPLORER_ALREADY_RUNNING { processes: running });
        }
        let version = target.to_string();
        tokio::task::spawn_blocking(move || {
            installs::install_explorer(&version, None)
                .and_then(|()| installs::rename_explorer_to_latest())
        })
        .await
        .map_err(|e| DCLError::from(anyhow::Error::from(e)))?
    }
}

/// Turns the download's status stream into progress messages; the UI reads progress from State.
struct DownloadProgress {
    strategy: UpdateStrategy,
    inbox: UnboundedSender<Msg>,
}

impl EventChannel for DownloadProgress {
    fn send(&self, status: Status) -> anyhow::Result<()> {
        if let Status::Launch(LaunchStatus::State {
            step: Step::Downloading { progress, .. },
        }) = status
        {
            deliver(
                &self.inbox,
                Msg::DownloadProgressed(self.strategy.clone(), progress),
            );
        }
        Ok(())
    }
}
