use std::time::SystemTime;

use log::warn;
use tokio::sync::mpsc::UnboundedSender;

use crate::channel::EventChannel;
use crate::deeplink_bridge;
use crate::fsm::msg::Msg;
use crate::protocols::DeepLink;
use crate::types::Status;
use crate::version::Version;

use super::{Executor, deliver};

impl Executor {
    /// A failed check counts as no instance: the launch proceeds as it does today.
    pub(super) async fn check_running_instances(&self, inbox: &UnboundedSender<Msg>) {
        let checked = self.running_instances.lock().await.any_is_running();
        let any = match checked {
            Ok(any) => any,
            Err(e) => {
                warn!("[fsm] running instances check failed, assuming none: {e:#}");
                false
            }
        };
        deliver(
            inbox,
            Msg::RunningInstancesChecked {
                any,
                checked_at: SystemTime::now(),
            },
        );
    }

    pub(super) async fn handoff_deeplink(&self, deeplink: DeepLink, inbox: &UnboundedSender<Msg>) {
        let msg = match deeplink_bridge::execute_passthrough(&StatusSink, &deeplink).await {
            Ok(()) => Msg::DeeplinkHandedOff,
            Err(error) => Msg::DeeplinkHandoffFailed(error),
        };
        deliver(inbox, msg);
    }

    pub(super) async fn launch_version(
        &self,
        version: Version,
        deeplink: Option<DeepLink>,
        inbox: &UnboundedSender<Msg>,
    ) {
        let dev = version.to_string();
        let preferred_version = version.is_dev().then_some(dev.as_str());
        let result = self
            .installs_hub
            .lock()
            .await
            .launch_explorer(deeplink, preferred_version)
            .await;
        let msg = match result {
            Ok(()) => Msg::VersionLaunched(version),
            Err(error) => Msg::VersionLaunchFailed(version, error),
        };
        deliver(inbox, msg);
    }
}

/// The passthrough's own status event is dropped; the projection renders the handoff from State.
struct StatusSink;

impl EventChannel for StatusSink {
    fn send(&self, _: Status) -> anyhow::Result<()> {
        Ok(())
    }
}
