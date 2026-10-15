use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use anyhow::{Context, anyhow};
use log::{error, warn};
use tokio::sync::mpsc::UnboundedSender;

use crate::channel::EventChannel;
use crate::delta;
use crate::delta::manifest::Manifest;
use crate::delta::patcher::Patcher;
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
            UpdateStrategy::ByDelta {
                target,
                from,
                manifest,
            } => Self::download_delta(target, from, manifest, &strategy, inbox).await,
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
            last_percent: AtomicU8::new(u8::MAX),
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

    async fn download_delta(
        target: &Version,
        from: &Version,
        manifest: &Manifest,
        strategy: &UpdateStrategy,
        inbox: &UnboundedSender<Msg>,
    ) -> DCLErrorResult {
        let Some(delta) = manifest.base(&from.to_string()) else {
            return Err(anyhow!("the manifest lists no base {from}").into());
        };
        let progress = {
            let strategy = strategy.clone();
            let inbox = inbox.clone();
            move |percent| deliver(&inbox, Msg::DownloadProgressed(strategy.clone(), percent))
        };
        delta::download::download_patches(target, manifest, delta, progress).await
    }

    pub(super) async fn install_version(
        &self,
        strategy: UpdateStrategy,
        inbox: &UnboundedSender<Msg>,
    ) {
        let target = strategy.target().clone();
        let result = match &strategy {
            UpdateStrategy::ByZip { .. } => self.install_zip(&target).await,
            UpdateStrategy::ByDelta { from, manifest, .. } => {
                self.install_delta(&target, from, manifest.clone()).await
            }
        };
        let msg = match result {
            Ok(()) => Msg::VersionInstallationCompleted(target.clone(), ReadyInstallation(target)),
            Err(error) => Msg::VersionInstallationFailed(strategy, error),
        };
        deliver(inbox, msg);
    }

    /// Installing swaps `latest/`, which an Explorer holding files there would break.
    async fn refuse_while_explorer_runs(&self) -> DCLErrorResult {
        let running = self
            .running_instances
            .lock()
            .await
            .explorer_processes_by_path();
        if running.is_empty() {
            return Ok(());
        }
        warn!("[fsm] Explorer is still running; refusing to install: {running:?}");
        Err(DCLError::E3008_EXPLORER_ALREADY_RUNNING { processes: running })
    }

    async fn install_zip(&self, target: &Version) -> DCLErrorResult {
        self.refuse_while_explorer_runs().await?;
        let version = target.to_string();
        tokio::task::spawn_blocking(move || {
            installs::install_explorer(&version, None)
                .and_then(|()| installs::rename_explorer_to_latest())
        })
        .await
        .map_err(|e| DCLError::from(anyhow::Error::from(e)))?
    }

    /// Builds `<app>/<target>/` from `latest/` plus the patches, then commits it like a zip.
    async fn install_delta(
        &self,
        target: &Version,
        from: &Version,
        manifest: Arc<Manifest>,
    ) -> DCLErrorResult {
        self.refuse_while_explorer_runs().await?;
        let patcher = Patcher::locate()?;
        let (target, from) = (target.clone(), from.clone());
        tokio::task::spawn_blocking(move || {
            let delta = manifest
                .base(&from.to_string())
                .ok_or_else(|| DCLError::from(anyhow!("the manifest lists no base {from}")))?;
            delta::apply::build_version_from_delta(&target, &manifest, delta, &patcher)?;
            installs::commit_install(&target.to_string(), None)?;
            installs::rename_explorer_to_latest()
        })
        .await
        .map_err(|e| DCLError::from(anyhow::Error::from(e)))?
    }
}

/// Turns the download's status stream into one progress message per percent; the UI reads
/// progress from State.
struct DownloadProgress {
    strategy: UpdateStrategy,
    inbox: UnboundedSender<Msg>,
    last_percent: AtomicU8,
}

impl EventChannel for DownloadProgress {
    fn send(&self, status: Status) -> anyhow::Result<()> {
        if let Status::Launch(LaunchStatus::State {
            step: Step::Downloading { progress, .. },
        }) = status
            && self.last_percent.swap(progress, Ordering::Relaxed) != progress
        {
            deliver(
                &self.inbox,
                Msg::DownloadProgressed(self.strategy.clone(), progress),
            );
        }
        Ok(())
    }
}
