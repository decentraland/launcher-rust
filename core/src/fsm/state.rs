use std::collections::BTreeMap;
use std::sync::Arc;

use crate::delta_manifest::DeltaManifest;
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
    Launch {
        deeplink: Option<DeepLink>,
        step: LaunchStep,
    },
}

// UI mirror, basically can be united with the protocal bridge in the future supposedly
#[derive(Clone, Debug)]
pub enum LaunchStep {
    DeeplinkOpening {
        attempt: Attempt,
    },
    Fetching {
        scanned: bool,
    },
    Downloading,
    Installing,
    Launching,
    DeepLinkPass,
    Launched,
    Failed {
        error: Arc<DCLError>,
        attempt: Attempt,
    },
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
pub struct State {
    pub config: Config,
    pub user_intention: UserIntention,
    pub latest_version: NetworkResource<Version>,
    pub delta_manifest: NetworkResource<Option<Arc<DeltaManifest>>>,
    pub installations: BTreeMap<Version, Installation>,
}

impl State {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }
}
