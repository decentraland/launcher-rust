use crate::analytics::event::Event;
use crate::errors::DCLError;
use crate::protocols::DeepLink;
use crate::version::Version;

use super::values::UpdateStrategy;

#[derive(Clone, Debug, PartialEq, Eq)]
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
    LaunchVersion(Version, Option<DeepLink>),
    HandoffDeeplink(DeepLink),

    CheckRunningInstances,

    FireAnalyticsEvent(Event),
    ReportError(DCLError),

    Multi(Vec<LaunchCmd>),
}
