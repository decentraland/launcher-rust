use std::fmt;

/// Validated campaign anonymous user ID for attribution tracking.
///
/// Guarantees the value is safe to pass as a CLI argument (no flag injection)
/// and conforms to a reasonable format (alphanumeric + hyphens/underscores, max 128 chars).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnonUserId(String);

impl AnonUserId {
    /// Parse from a raw string. Returns `None` if invalid.
    pub fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if is_valid(trimmed) {
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

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AnonUserId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Only allow alphanumeric chars, hyphens, and underscores, up to 128 chars.
/// Must not start with `-` to prevent CLI flag injection.
fn is_valid(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('-')
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn rejects_value_with_cli_flag_injection() {
        let url = "https://example.com/file.dmg?anon_user_id=--some-explorer-flag";
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
        assert_eq!(AnonUserId::parse("abc-123"), Some(AnonUserId("abc-123".to_string())));
    }

    #[test]
    fn parse_rejects_empty() {
        assert_eq!(AnonUserId::parse(""), None);
    }

    #[test]
    fn parse_rejects_flag_injection() {
        assert_eq!(AnonUserId::parse("--flag"), None);
    }
}
