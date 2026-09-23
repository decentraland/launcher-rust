use std::fmt;

/// Validated referral attribution address.
///
/// Format constraint: `0x` + 40 hex chars. Stored lowercase.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Referrer(String);

impl Referrer {
    pub fn parse(value: &str) -> Option<Self> {
        let trimmed = value.trim();
        if Self::is_valid(trimmed) {
            Some(Self(trimmed.to_lowercase()))
        } else {
            None
        }
    }

    /// Extract from a URL's `referrer` query parameter.
    ///
    /// Searches by key name (`referrer`) rather than by pattern
    /// so it cannot collide with auth token extraction.
    pub fn from_url(url_str: &str) -> Option<Self> {
        let url = url::Url::parse(url_str).ok()?;

        for (key, value) in url.query_pairs() {
            if key == "referrer" {
                return Self::parse(&value);
            }
        }

        None
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn is_valid(value: &str) -> bool {
        value.len() == 42
            && value.starts_with("0x")
            && value[2..].chars().all(|c| c.is_ascii_hexdigit())
    }
}

impl fmt::Display for Referrer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_referrer_from_query() {
        let url = "https://download-gateway.decentraland.zone/391a85da-a3bb-49e2-a45e-96c740c38424/decentraland.dmg?anon_user_id=abc-123&referrer=0x24e5f44999c151f08609f8e27b2238c773c4d020";
        assert_eq!(
            Referrer::from_url(url),
            Some(Referrer(
                "0x24e5f44999c151f08609f8e27b2238c773c4d020".to_string()
            ))
        );
    }

    #[test]
    fn lowercases_mixed_case_address() {
        assert_eq!(
            Referrer::parse("0x24E5F44999C151F08609F8E27B2238C773C4D020"),
            Some(Referrer(
                "0x24e5f44999c151f08609f8e27b2238c773c4d020".to_string()
            ))
        );
    }

    #[test]
    fn returns_none_when_missing() {
        let url = "https://download-gateway.decentraland.zone/391a85da-a3bb-49e2-a45e-96c740c38424/decentraland.dmg";
        assert_eq!(Referrer::from_url(url), None);
    }

    #[test]
    fn does_not_match_on_different_key() {
        let url = "https://example.com/file.dmg?ref=0x24e5f44999c151f08609f8e27b2238c773c4d020";
        assert_eq!(Referrer::from_url(url), None);
    }

    #[test]
    fn handles_invalid_url() {
        assert_eq!(Referrer::from_url("not-a-url"), None);
    }

    #[test]
    fn rejects_invalid_values() {
        for bad in [
            "",
            "0x123",
            "not-an-address",
            "javascript:alert(1)",
            "0xZZZ5f44999c151f08609f8e27b2238c773c4d020",
        ] {
            assert_eq!(Referrer::parse(bad), None, "should reject {bad}");
        }
    }

    #[test]
    fn rejects_invalid_value_in_url() {
        let url = "https://example.com/file.dmg?referrer=javascript:alert(1)";
        assert_eq!(Referrer::from_url(url), None);
    }

    #[test]
    fn accepts_value_with_surrounding_whitespace() {
        assert_eq!(
            Referrer::parse(" 0x24e5f44999c151f08609f8e27b2238c773c4d020 "),
            Some(Referrer(
                "0x24e5f44999c151f08609f8e27b2238c773c4d020".to_string()
            ))
        );
    }
}
