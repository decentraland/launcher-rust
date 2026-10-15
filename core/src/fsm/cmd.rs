use std::sync::Arc;

use crate::analytics::event::Event;
use crate::errors::DCLError;
use crate::protocols::DeepLink;
use crate::version::Version;

use super::values::{Attempt, UpdateStrategy};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchRequest {
    LatestVersion,
    DeltaManifest { current_version: Version },
}

#[derive(Debug)]
pub enum Cmd {
    None,
    /// A message that changes nothing right now; the reason is logged.
    Ignore(&'static str),

    EnsureLocalInstallations,

    Fetch(FetchRequest),

    DownloadVersion(UpdateStrategy),
    InstallVersion(UpdateStrategy),
    LaunchVersion(Version, Option<DeepLink>),
    HandoffDeeplink(DeepLink),

    CheckRunningInstances,

    FireAnalyticsEvent(Event),
    /// Sentry and the file log: a breadcrumb unless final, then a capture. The UI reads errors from State.
    ReportToSentry {
        error: Arc<DCLError>,
        attempt: Attempt,
        is_final: bool,
    },

    Multi(Vec<Cmd>),
}
