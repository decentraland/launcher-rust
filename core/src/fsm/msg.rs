use std::time::SystemTime;

use crate::delta_manifest::DeltaManifest;
use crate::errors::DCLError;
use crate::protocols::DeepLink;
use crate::version::Version;

use super::values::{Percent, ReadyInstallation, UpdateStrategy};

#[derive(Debug)]
pub enum Msg {
    LaunchIntentionReceived(Option<DeepLink>),
    DeeplinkReceived(DeepLink),
    RetryIntentionReceived,

    LatestVersionFetched(Result<Version, DCLError>),
    DeltaManifestFetched(Result<Option<DeltaManifest>, DCLError>),

    LocalInstallationPresenceConfirmed(Vec<ReadyInstallation>),

    VersionDownloadCompleted(UpdateStrategy),
    DownloadProgressed(UpdateStrategy, Percent),
    VersionDownloadFailed(UpdateStrategy, DCLError),

    RunningInstancesChecked { any: bool, checked_at: SystemTime },

    DeeplinkHandedOff,
    DeeplinkHandoffFailed(DCLError),

    VersionInstallationCompleted(Version, ReadyInstallation),
    VersionInstallationFailed(UpdateStrategy, DCLError),

    VersionLaunched(Version),
    VersionLaunchFailed(Version, DCLError),
}
