use crate::errors::DCLError;

use super::state::{DeepLinkValue, Manifest};
use super::strategy::{FailureReason, Percent, UpdateStrategy};
use super::version::{ReadyInstallation, Version};

#[derive(Debug)]
pub enum LaunchMsg {
    LaunchIntentionReceived(Option<DeepLinkValue>),
    DeeplinkReceived(DeepLinkValue),
    RetryRequested,

    LatestVersionFetched(Result<Version, DCLError>),
    DeltaManifestFetched(Result<Option<Manifest>, DCLError>),

    LocalInstallationPresenceConfirmed(Vec<ReadyInstallation>),

    VersionDownloadCompleted(UpdateStrategy),
    DownloadProgressed(UpdateStrategy, Percent),
    VersionDownloadFailed(UpdateStrategy, FailureReason),

    RunningInstancesChecked { any: bool },

    DeeplinkHandedOff,
    DeeplinkHandoffFailed(DCLError),

    VersionInstallationCompleted(Version, ReadyInstallation),
    VersionInstallationFailed(UpdateStrategy, FailureReason),

    VersionLaunched(Version),
    VersionLaunchFailed(Version, DCLError),
}
