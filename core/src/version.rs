use std::cmp::Ordering;
use std::fmt::{self, Display};
use std::hash::{Hash, Hasher};

const DEV: &str = "dev";
const V_PREFIX: char = 'v';

/// `v1.2.3`, `1.2.3`, with an optional prerelease suffix, or `dev`. Equality and order ignore the prefix.
#[derive(Clone, Debug)]
pub enum Version {
    Release {
        version: semver::Version,
        v_prefixed: bool,
    },
    Dev,
}

impl Version {
    pub fn parse(value: &str) -> Option<Self> {
        if value == DEV {
            return Some(Self::Dev);
        }
        let unprefixed = value.strip_prefix(V_PREFIX);
        let version = semver::Version::parse(unprefixed.unwrap_or(value)).ok()?;
        Some(Self::Release {
            version,
            v_prefixed: unprefixed.is_some(),
        })
    }

    pub const fn is_dev(&self) -> bool {
        matches!(self, Self::Dev)
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dev => f.write_str(DEV),
            Self::Release {
                version,
                v_prefixed: true,
            } => write!(f, "{V_PREFIX}{version}"),
            Self::Release {
                version,
                v_prefixed: false,
            } => Display::fmt(version, f),
        }
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (Self::Dev, Self::Dev) => Ordering::Equal,
            (Self::Dev, Self::Release { .. }) => Ordering::Greater,
            (Self::Release { .. }, Self::Dev) => Ordering::Less,
            (Self::Release { version: a, .. }, Self::Release { version: b, .. }) => a.cmp(b),
        }
    }
}

impl Hash for Version {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Dev => DEV.hash(state),
            Self::Release { version, .. } => version.hash(state),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_prefix_spelling() {
        assert_eq!(
            Version::parse("v1.2.3").map(|v| v.to_string()),
            Some("v1.2.3".to_owned())
        );
        assert_eq!(
            Version::parse("1.2.3-rc.1").map(|v| v.to_string()),
            Some("1.2.3-rc.1".to_owned())
        );
        assert_eq!(
            Version::parse("dev").map(|v| v.to_string()),
            Some("dev".to_owned())
        );
    }

    #[test]
    fn rejects_non_versions() {
        assert!(Version::parse("").is_none());
        assert!(Version::parse("v1.2.3/..").is_none());
        assert!(Version::parse("latest").is_none());
    }

    #[test]
    fn orders_numerically_and_ignores_the_prefix() {
        assert!(Version::parse("v1.10.0") > Version::parse("v1.9.0"));
        assert_eq!(Version::parse("v1.2.3"), Version::parse("1.2.3"));
        assert!(Version::parse("dev") > Version::parse("v99.0.0"));
    }
}
