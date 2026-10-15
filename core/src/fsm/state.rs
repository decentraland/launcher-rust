use std::sync::Arc;

use crate::delta_manifest::DeltaManifest;
use crate::errors::DCLError;
use crate::protocols::DeepLink;
use crate::version::Version;

use super::values::{Attempt, Installations};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Config {
    pub use_delta_mode: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum UserIntention {
    #[default]
    Idle,
    Launch(Option<DeepLink>),
}

#[derive(Clone, Debug, Default)]
pub enum NetworkResource<T> {
    #[default]
    Unknown,
    Fetching,
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
    pub installations: Installations,
}

impl State {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }
}
