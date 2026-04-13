/// Extracts the `anon_user_id` query parameter from a URL.
///
/// This deliberately searches by key name (`anon_user_id`) rather than by UUID
/// regex so it cannot collide with `token_from_url()`, which matches any
/// UUID-shaped value in query params or path segments.
pub fn anon_user_id_from_url(url_str: &str) -> Option<String> {
    let url = url::Url::parse(url_str).ok()?;

    for (key, value) in url.query_pairs() {
        if key == "anon_user_id" {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }

    None
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
}
