use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::SystemTime;

use crate::delta::manifest::Manifest as DeltaManifest;
use crate::environment::Args;
use crate::errors::DCLError;
use crate::protocols::DeepLink;
use crate::version::Version;

use super::values::{Attempt, Installation};

#[derive(Clone, Debug, Default)]
pub struct Config {
    pub use_delta_mode: bool,
    pub args: Args,
}

#[derive(Clone, Debug, Default)]
pub enum UserIntention {
    #[default]
    Idle,
    Launch(Option<DeepLink>),
}

#[derive(Clone, Debug, Default)]
pub enum NetworkResource<T> {
    #[default]
    Unknown,
    Fetching {
        attempt: Attempt,
    },
    Fetched(T),
    Error {
        error: Arc<DCLError>,
        attempt: Attempt,
    },
}

#[derive(Clone, Debug, Default)]
pub enum DeeplinkHandoff {
    #[default]
    Idle,
    HandingOff {
        attempt: Attempt,
    },
    HandedOff,
    Failed {
        error: Arc<DCLError>,
        attempt: Attempt,
    },
}

#[derive(Clone, Debug, Default)]
pub enum RunningInstances {
    #[default]
    Unknown,
    Checking,
    Checked {
        any: bool,
        checked_at: SystemTime,
    },
}

#[derive(Clone, Debug, Default)]
pub enum LocalInstallations {
    #[default]
    Unknown,
    Scanning,
    Scanned(BTreeMap<Version, Installation>),
}

impl LocalInstallations {
    pub const fn scanned(&self) -> Option<&BTreeMap<Version, Installation>> {
        match self {
            Self::Scanned(installations) => Some(installations),
            Self::Unknown | Self::Scanning => None,
        }
    }

    pub const fn scanned_mut(&mut self) -> Option<&mut BTreeMap<Version, Installation>> {
        match self {
            Self::Scanned(installations) => Some(installations),
            Self::Unknown | Self::Scanning => None,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub enum ExplorerLaunch {
    #[default]
    Idle,
    AwaitingRunningInstancesCheck(Version),
    Launching(Version),
    Launched(Version),
    Failed {
        error: Arc<DCLError>,
    },
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub config: Config,
    pub user_intention: UserIntention,
    pub deeplink_handoff: DeeplinkHandoff,
    pub local_installations: LocalInstallations,
    pub latest_version: NetworkResource<Version>,
    pub delta_manifest: NetworkResource<Option<Arc<DeltaManifest>>>,
    pub launch: ExplorerLaunch,
    pub running_instances: RunningInstances,
}

impl State {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }
}
