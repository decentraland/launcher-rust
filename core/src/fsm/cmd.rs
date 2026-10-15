use crate::analytics::event::Event;
use crate::errors::DCLError;

use super::state::DeepLinkValue;
use super::strategy::UpdateStrategy;
use super::version::Version;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FetchRequest {
    LatestVersion,
    DeltaManifest { current_version: Version },
}

#[derive(Debug)]
pub enum LaunchCmd {
    None,

    EnsureLocalInstallations,

    Fetch(FetchRequest),

    DownloadVersion(UpdateStrategy),
    InstallVersion(UpdateStrategy),
    LaunchVersion(Version, Option<DeepLinkValue>),
    HandoffDeeplink(DeepLinkValue),

    CheckRunningInstances,

    FireAnalyticsEvent(Event),
    ReportError(DCLError),

    Multi(Vec<LaunchCmd>),
}
