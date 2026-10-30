use std::fmt;

/// Decentraland environment inferred from the installer's download origin.
///
/// The gateway serves the same binaries on every domain, so the TLD of the
/// `decentraland.*` host the installer came from is the only signal of which
/// environment the user meant to install.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DclEnv {
    Org,
    Zone,
}

impl DclEnv {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "org" => Some(Self::Org),
            "zone" => Some(Self::Zone),
            _ => None,
        }
    }

    /// Infer from a download-origin URL host: the TLD of a `decentraland.*`
    /// domain names the environment, so both `decentraland.zone` and
    /// `download-gateway.decentraland.zone` resolve to `Zone`.
    ///
    /// The `decentraland` label is matched as the registrable domain (second to
    /// last), so a lookalike host like `decentraland.zone.example.com` yields
    /// `None` rather than `Zone`.
    pub fn from_url(url_str: &str) -> Option<Self> {
        let url = url::Url::parse(url_str).ok()?;
        let host = url.host_str()?.to_ascii_lowercase();

        let mut labels = host.rsplit('.');
        let tld = labels.next()?;
        if labels.next()? != "decentraland" {
            return None;
        }

        Self::parse(tld)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Org => "org",
            Self::Zone => "zone",
        }
    }
}

impl fmt::Display for DclEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case(
        "https://download-gateway.decentraland.zone/e6075daf-9842-4380-b954-4a240ecbf585/decentraland.dmg?anon_user_id=740ed2fb-2892-447c-90c6-941a24cac235",
        Some(DclEnv::Zone)
    )]
    #[case("https://decentraland.zone/", Some(DclEnv::Zone))]
    #[case(
        "https://explorer-artifacts.decentraland.zone/dry-run-launcher-rust/pr-196/run-855-19672401394/Decentraland_installer.exe",
        Some(DclEnv::Zone)
    )]
    #[case(
        "https://download-gateway.decentraland.org/e6075daf-9842-4380-b954-4a240ecbf585/decentraland.dmg",
        Some(DclEnv::Org)
    )]
    #[case("https://decentraland.org/download", Some(DclEnv::Org))]
    fn infers_environment_from_host(#[case] url: &str, #[case] expected: Option<DclEnv>) {
        assert_eq!(DclEnv::from_url(url), expected);
    }

    #[rstest]
    // Not a decentraland domain: no environment signal.
    #[case("https://example.com/decentraland.dmg")]
    // Lookalike host: `decentraland` is not the registrable domain here.
    #[case("https://decentraland.zone.example.com/decentraland.dmg")]
    // Decentraland domain the launcher does not map to an environment.
    #[case("https://decentraland.today/")]
    #[case("not-a-url")]
    fn returns_none_without_environment_signal(#[case] url: &str) {
        assert_eq!(DclEnv::from_url(url), None);
    }

    #[test]
    fn parses_stored_values_case_insensitively() {
        assert_eq!(DclEnv::parse(" ZONE\n"), Some(DclEnv::Zone));
        assert_eq!(DclEnv::parse("org"), Some(DclEnv::Org));
        assert_eq!(DclEnv::parse("prod"), None);
        assert_eq!(DclEnv::parse(""), None);
    }
}
