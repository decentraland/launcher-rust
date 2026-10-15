use anyhow::anyhow;
use tokio::sync::mpsc::UnboundedSender;

use crate::delta;
use crate::errors::DCLError;
use crate::fsm::msg::Msg;
use crate::s3;
use crate::version::Version;

use super::{Executor, deliver};

impl Executor {
    pub(super) async fn fetch_latest_version(&self, inbox: &UnboundedSender<Msg>) {
        let result = s3::get_latest_explorer_release().await.and_then(|release| {
            Version::parse(&release.version).ok_or_else(|| {
                DCLError::from(anyhow!(
                    "latest.json version cannot be parsed: {}",
                    release.version
                ))
            })
        });
        deliver(inbox, Msg::LatestVersionFetched(result));
    }

    pub(super) async fn fetch_delta_manifest(
        &self,
        target: &Version,
        inbox: &UnboundedSender<Msg>,
    ) {
        let result = delta::fetch::fetch_manifest(target).await;
        deliver(inbox, Msg::DeltaManifestFetched(result));
    }
}
