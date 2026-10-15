use anyhow::anyhow;
use log::info;
use tokio::sync::mpsc::UnboundedSender;

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

    /// No manifest is published until the producer contract is specified: every launch takes the zip.
    pub(super) fn fetch_delta_manifest(current_version: &Version, inbox: &UnboundedSender<Msg>) {
        info!("[fsm] no delta manifest producer yet; base {current_version} falls back to the zip");
        deliver(inbox, Msg::DeltaManifestFetched(Ok(None)));
    }
}
