use std::sync::Arc;

use crate::delta_manifest::DeltaManifest;
use crate::errors::DCLError;
use crate::version::Version;

pub type Attempt = u8;
pub type Percent = u8;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReadyInstallation(pub Version);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateStrategy {
    ByZip {
        target: Version,
    },
    ByDelta {
        target: Version,
        from: Version,
        manifest: Arc<DeltaManifest>,
    },
}

impl UpdateStrategy {
    pub const fn target(&self) -> &Version {
        match self {
            Self::ByZip { target } | Self::ByDelta { target, .. } => target,
        }
    }

    pub const fn name(&self) -> &'static str {
        match self {
            Self::ByZip { .. } => "zip",
            Self::ByDelta { .. } => "delta",
        }
    }
}

#[derive(Clone, Debug)]
pub enum Installation {
    Ready(ReadyInstallation),
    Downloading {
        strategy: UpdateStrategy,
        percent: Percent,
        attempt: Attempt,
    },
    Installing {
        strategy: UpdateStrategy,
        attempt: Attempt,
    },
    Failed {
        error: Arc<DCLError>,
        attempt: Attempt,
    },
}
