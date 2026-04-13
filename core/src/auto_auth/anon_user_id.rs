/// Extracts the `anon_user_id` query parameter from a URL.
///
/// This deliberately searches by key name (`anon_user_id`) rather than by UUID
/// regex so it cannot collide with `token_from_url()`, which matches any
/// UUID-shaped value in query params or path segments.
///
/// The value is validated against an allowlist pattern to prevent injection of
/// CLI flags or excessively long strings.
pub fn anon_user_id_from_url(url_str: &str) -> Option<String> {
    let url = url::Url::parse(url_str).ok()?;

    for (key, value) in url.query_pairs() {
        if key == "anon_user_id" {
            let trimmed = value.trim();
            if is_valid_anon_user_id(trimmed) {
                return Some(trimmed.to_string());
            }
        }
    }

    None
}

/// Only allow alphanumeric chars, hyphens, and underscores, up to 128 chars.
fn is_valid_anon_user_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
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
            anon_user_id_from_url(url),
            Some("abc-123-def".to_string())
        );
    }

    #[test]
    fn returns_none_when_missing() {
        let url = "https://download-gateway.decentraland.zone/391a85da-a3bb-49e2-a45e-96c740c38424/decentraland.dmg";
        assert_eq!(anon_user_id_from_url(url), None);
    }

    #[test]
    fn returns_none_for_empty_value() {
        let url = "https://example.com/file.dmg?anon_user_id=";
        assert_eq!(anon_user_id_from_url(url), None);
    }

    #[test]
    fn ignores_other_uuid_params() {
        let url = "https://example.com/file.dmg?token=b5876cf1-9b6b-451e-b467-9700f754a8f7&anon_user_id=user-42";
        assert_eq!(
            anon_user_id_from_url(url),
            Some("user-42".to_string())
        );
    }

    #[test]
    fn does_not_match_on_different_key() {
        let url = "https://example.com/file.dmg?some_other_id=user-42";
        assert_eq!(anon_user_id_from_url(url), None);
    }

    #[test]
    fn handles_invalid_url() {
        assert_eq!(anon_user_id_from_url("not-a-url"), None);
    }

    #[test]
    fn rejects_value_with_cli_flag_injection() {
        let url = "https://example.com/file.dmg?anon_user_id=--some-explorer-flag";
        assert_eq!(anon_user_id_from_url(url), None);
    }

    #[test]
    fn rejects_value_too_long() {
        let long_id = "a".repeat(129);
        let url = format!("https://example.com/file.dmg?anon_user_id={long_id}");
        assert_eq!(anon_user_id_from_url(&url), None);
    }

    #[test]
    fn accepts_uuid_format() {
        let url = "https://example.com/file.dmg?anon_user_id=62792c33-59e3-4e7f-be42-289c053ecb37";
        assert_eq!(
            anon_user_id_from_url(url),
            Some("62792c33-59e3-4e7f-be42-289c053ecb37".to_string())
        );
    }
}
