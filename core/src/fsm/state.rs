use crate::protocols::DeepLink;

use crate::version::Version;

use super::values::{Attempt, Installations, Manifest};

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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum NetworkResource<T> {
    #[default]
    Unknown,
    Fetching,
    Fetched(T),
    Error {
        attempt: Attempt,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LaunchState {
    pub config: Config,
    pub user_intention: UserIntention,
    pub latest_version: NetworkResource<Version>,
    pub delta_manifest: NetworkResource<Option<Manifest>>,
    pub installations: Installations,
}

impl LaunchState {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            ..Self::default()
        }
    }
}
