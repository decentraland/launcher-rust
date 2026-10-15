mod fetch;
mod installations;
mod launch;
mod reporting;

use std::future::{Future, ready};
use std::sync::Arc;

use log::{info, warn};
use tokio::sync::Mutex;
use tokio::sync::mpsc::UnboundedSender;

use crate::analytics::Analytics;
use crate::infra::fsm::CmdExecutor;
use crate::installs::InstallsHub;
use crate::instances::RunningInstances;

use super::cmd::{Cmd, FetchRequest};
use super::msg::Msg;

/// Stateless: every command is spawned as its own task that answers through the inbox.
#[derive(Clone)]
pub struct Executor {
    analytics: Arc<Mutex<Analytics>>,
    running_instances: Arc<Mutex<RunningInstances>>,
    installs_hub: Arc<Mutex<InstallsHub>>,
}

impl Executor {
    pub const fn new(
        analytics: Arc<Mutex<Analytics>>,
        running_instances: Arc<Mutex<RunningInstances>>,
        installs_hub: Arc<Mutex<InstallsHub>>,
    ) -> Self {
        Self {
            analytics,
            running_instances,
            installs_hub,
        }
    }

    fn dispatch(&self, cmd: Cmd, inbox: &UnboundedSender<Msg>) {
        match cmd {
            Cmd::None => {}
            Cmd::Ignore(reason) => info!("[fsm] ignored: {reason}"),
            Cmd::Multi(cmds) => {
                for cmd in cmds {
                    self.dispatch(cmd, inbox);
                }
            }
            Cmd::EnsureLocalInstallations => {
                self.spawn(inbox, |this, inbox| async move {
                    this.scan_local_installations(&inbox).await;
                });
            }
            Cmd::Fetch(FetchRequest::LatestVersion) => {
                self.spawn(inbox, |this, inbox| async move {
                    this.fetch_latest_version(&inbox).await;
                });
            }
            Cmd::Fetch(FetchRequest::DeltaManifest { target, .. }) => {
                self.spawn(inbox, |this, inbox| async move {
                    this.fetch_delta_manifest(&target, &inbox).await;
                });
            }
            Cmd::DownloadVersion(strategy) => {
                self.spawn(inbox, |this, inbox| async move {
                    this.download_version(strategy, &inbox).await;
                });
            }
            Cmd::InstallVersion(strategy) => {
                self.spawn(inbox, |this, inbox| async move {
                    this.install_version(strategy, &inbox).await;
                });
            }
            Cmd::LaunchVersion(version, deeplink) => {
                self.spawn(inbox, |this, inbox| async move {
                    this.launch_version(version, deeplink, &inbox).await;
                });
            }
            Cmd::HandoffDeeplink(deeplink) => {
                self.spawn(inbox, |this, inbox| async move {
                    this.handoff_deeplink(deeplink, &inbox).await;
                });
            }
            Cmd::CheckRunningInstances => {
                self.spawn(inbox, |this, inbox| async move {
                    this.check_running_instances(&inbox).await;
                });
            }
            Cmd::FireAnalyticsEvent(event) => {
                self.spawn(inbox, |this, _| async move {
                    this.fire_analytics_event(event).await;
                });
            }
            Cmd::ReportToSentry {
                error,
                attempt,
                is_final,
            } => reporting::report_to_sentry(&error, attempt, is_final),
        }
    }

    fn spawn<F, Fut>(&self, inbox: &UnboundedSender<Msg>, task: F)
    where
        F: FnOnce(Self, UnboundedSender<Msg>) -> Fut,
        Fut: Future<Output = ()> + Send + 'static,
    {
        drop(tokio::spawn(task(self.clone(), inbox.clone())));
    }
}

impl CmdExecutor<Cmd, Msg> for Executor {
    fn execute(
        &mut self,
        cmd: Cmd,
        inbox: &UnboundedSender<Msg>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send {
        self.dispatch(cmd, inbox);
        ready(Ok(()))
    }
}

fn deliver(inbox: &UnboundedSender<Msg>, msg: Msg) {
    if inbox.send(msg).is_err() {
        warn!("[fsm] inbox is closed; the result is dropped");
    }
}
