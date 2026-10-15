use crate::version::Version;

// Fields follow once the producer contract is specified.
#[derive(Debug, PartialEq, Eq)]
pub struct DeltaManifest;

impl DeltaManifest {
    #[allow(clippy::unused_self)]
    pub const fn lists_base(&self, _version: &Version) -> bool {
        true
    }
}
