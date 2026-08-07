use regex::Regex;
use std::fmt;
use std::path::Path;
use std::sync::LazyLock;

static UUID_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\b[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}\b")
        .expect("UUID regex is a valid literal pattern")
});

/// Validated campaign anonymous user ID for attribution tracking.
///
/// Format constraint: alphanumeric + hyphens/underscores, max 128 chars.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnonUserId(String);

impl AnonUserId {
    pub fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if Self::is_valid(trimmed) {
            Some(Self(trimmed.to_string()))
        } else {
            None
        }
    }

    /// Extract from a URL's `anon_user_id` query parameter.
    ///
    /// Searches by key name (`anon_user_id`) rather than by UUID regex
    /// so it cannot collide with auth token extraction.
    pub fn from_url(url_str: &str) -> Option<Self> {
        let url = url::Url::parse(url_str).ok()?;

        for (key, value) in url.query_pairs() {
            if key == "anon_user_id" {
                return Self::parse(&value);
            }
        }

        None
    }

    /// Extract from an installer's filename.
    ///
    /// The download gateway names anonymous EXE downloads
    /// `Decentraland-Installer-<UUID>.exe`. The regex matches any RFC 4122
    /// UUID embedded in the filename so we tolerate browser-added suffixes
    /// (e.g. `Decentraland-Installer-<UUID> (3).exe` for dedup) and don't lock
    /// the launcher to a specific gateway-side filename convention.
    ///
    /// This is the fallback path used when `Zone.Identifier` has been stripped
    /// by Windows' silent-unblock handling for trusted signed binaries — which
    /// is the steady-state for popular pre-signed installers and not an edge
    /// case.
    pub fn from_installer_filename(installer_path: &str) -> Option<Self> {
        let file_name = Path::new(installer_path).file_name()?.to_str()?;

        let matched = UUID_RE.find(file_name)?;
        Self::parse(matched.as_str())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_valid(value: &str) -> bool {
        !value.is_empty()
            && value.len() <= 128
            && value
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }
}

impl fmt::Display for AnonUserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    // Tests use forward-slash paths so they run on both Windows and Unix CI
    // hosts. `Path::file_name` uses the host OS's path semantics, but the
    // production code targets Windows where `\` and `/` both work as
    // separators — and we only need to exercise the parsing logic here, not
    // the OS path resolution.
    #[rstest]
    #[case(
        "Downloads/Decentraland-Installer-391a85da-a3bb-49e2-a45e-96c740c38424.exe",
        Some("391a85da-a3bb-49e2-a45e-96c740c38424")
    )]
    #[case(
        // Bare filename, no parent directory.
        "Decentraland-Installer-391a85da-a3bb-49e2-a45e-96c740c38424.exe",
        Some("391a85da-a3bb-49e2-a45e-96c740c38424")
    )]
    #[case(
        // Browser dedup suffix when the file already exists in Downloads.
        "Decentraland-Installer-391a85da-a3bb-49e2-a45e-96c740c38424 (3).exe",
        Some("391a85da-a3bb-49e2-a45e-96c740c38424")
    )]
    #[case(
        // Different valid UUID, slash-prefixed absolute path.
        "/tmp/Decentraland-Installer-62792c33-59e3-4e7f-be42-289c053ecb37.exe",
        Some("62792c33-59e3-4e7f-be42-289c053ecb37")
    )]
    #[case(
        // Old-style filename (no UUID) → no fallback match, the caller must
        // treat this as "no anon_user_id available".
        "Decentraland-Installer.exe",
        None
    )]
    #[case(
        // Different surrounding context — the regex finds the UUID anywhere
        // in the filename, so the parser stays decoupled from the gateway's
        // exact filename convention.
        "some-other-installer-391a85da-a3bb-49e2-a45e-96c740c38424.exe",
        Some("391a85da-a3bb-49e2-a45e-96c740c38424")
    )]
    #[case(
        // Prefix matches but the UUID part is malformed (raw space). The
        // RFC 4122 regex rejects it. Defends against attacker-controlled
        // filenames.
        "Decentraland-Installer-not a uuid.exe",
        None
    )]
    #[case(
        // Wrong variant bits (third group does not start with 1-5). RFC 4122
        // strict regex rejects, even though `AnonUserId::parse` alone would
        // accept the alphanumeric+hyphen string.
        "Decentraland-Installer-391a85da-a3bb-09e2-a45e-96c740c38424.exe",
        None
    )]
    #[case(
        // Uppercase hex still matches thanks to the (?i) flag.
        "Decentraland-Installer-391A85DA-A3BB-49E2-A45E-96C740C38424.exe",
        Some("391A85DA-A3BB-49E2-A45E-96C740C38424")
    )]
    #[case(
        // Empty stem (impossible in practice, but we should not panic).
        "",
        None
    )]
    fn extracts_anon_user_id_from_installer_filename(
        #[case] path: &str,
        #[case] expected: Option<&str>,
    ) {
        let actual = AnonUserId::from_installer_filename(path);
        assert_eq!(expected, actual.as_ref().map(AnonUserId::as_str));
    }

    #[test]
    fn extracts_anon_user_id_from_query() {
        let url = "https://download-gateway.decentraland.zone/391a85da-a3bb-49e2-a45e-96c740c38424/decentraland.dmg?anon_user_id=abc-123-def";
        assert_eq!(
            AnonUserId::from_url(url),
            Some(AnonUserId("abc-123-def".to_string()))
        );
    }

    #[test]
    fn returns_none_when_missing() {
        let url = "https://download-gateway.decentraland.zone/391a85da-a3bb-49e2-a45e-96c740c38424/decentraland.dmg";
        assert_eq!(AnonUserId::from_url(url), None);
    }

    #[test]
    fn returns_none_for_empty_value() {
        let url = "https://example.com/file.dmg?anon_user_id=";
        assert_eq!(AnonUserId::from_url(url), None);
    }

    #[test]
    fn ignores_other_uuid_params() {
        let url = "https://example.com/file.dmg?token=b5876cf1-9b6b-451e-b467-9700f754a8f7&anon_user_id=user-42";
        assert_eq!(
            AnonUserId::from_url(url),
            Some(AnonUserId("user-42".to_string()))
        );
    }

    #[test]
    fn does_not_match_on_different_key() {
        let url = "https://example.com/file.dmg?some_other_id=user-42";
        assert_eq!(AnonUserId::from_url(url), None);
    }

    #[test]
    fn handles_invalid_url() {
        assert_eq!(AnonUserId::from_url("not-a-url"), None);
    }

    #[test]
    fn rejects_value_with_disallowed_chars() {
        let url = "https://example.com/file.dmg?anon_user_id=a%20b";
        assert_eq!(AnonUserId::from_url(url), None);
    }

    #[test]
    fn rejects_value_too_long() {
        let long_id = "a".repeat(129);
        let url = format!("https://example.com/file.dmg?anon_user_id={long_id}");
        assert_eq!(AnonUserId::from_url(&url), None);
    }

    #[test]
    fn accepts_uuid_format() {
        let url = "https://example.com/file.dmg?anon_user_id=62792c33-59e3-4e7f-be42-289c053ecb37";
        assert_eq!(
            AnonUserId::from_url(url),
            Some(AnonUserId("62792c33-59e3-4e7f-be42-289c053ecb37".to_string()))
        );
    }

    #[test]
    fn parse_valid() {
        assert_eq!(
            AnonUserId::parse("abc-123"),
            Some(AnonUserId("abc-123".to_string()))
        );
    }

    #[test]
    fn parse_rejects_empty() {
        assert_eq!(AnonUserId::parse(""), None);
    }
}
