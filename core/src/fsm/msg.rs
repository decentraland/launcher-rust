use crate::errors::DCLError;
use crate::protocols::DeepLink;

use super::values::{FailureReason, Manifest, Percent, ReadyInstallation, UpdateStrategy, Version};

#[derive(Debug)]
pub enum LaunchMsg {
    LaunchIntentionReceived(Option<DeepLink>),
    DeeplinkReceived(DeepLink),
    RetryIntentionReceived,

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
