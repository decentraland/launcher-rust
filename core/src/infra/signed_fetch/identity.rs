use std::fmt;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// One link of a Decentraland auth chain.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthLink {
    #[serde(rename = "type")]
    pub kind: String,
    pub payload: String,
    #[serde(default)]
    pub signature: String,
}

pub const SIGNED_ENTITY_LINK_TYPE: &str = "ECDSA_SIGNED_ENTITY";

/// Ephemeral identity as the Explorer serializes it.
///
/// Fields `address`, `key`, `expiration`, `ephemeralAuthChain`. The private key is a bearer
/// credential: `Debug` redacts it and the type is deliberately not `Serialize`.
#[derive(Clone, Deserialize)]
pub struct EphemeralIdentity {
    pub address: String,
    #[serde(rename = "key")]
    private_key: String,
    /// ISO 8601 / RFC 3339 timestamp.
    pub expiration: String,
    #[serde(rename = "ephemeralAuthChain")]
    pub auth_chain: Vec<AuthLink>,
}

impl fmt::Debug for EphemeralIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EphemeralIdentity")
            .field("address", &self.address)
            .field("private_key", &"<redacted>")
            .field("expiration", &self.expiration)
            .field("auth_chain_links", &self.auth_chain.len())
            .finish()
    }
}

impl EphemeralIdentity {
    #[cfg(test)]
    pub(crate) const fn new(
        address: String,
        private_key: String,
        expiration: String,
        auth_chain: Vec<AuthLink>,
    ) -> Self {
        Self {
            address,
            private_key,
            expiration,
            auth_chain,
        }
    }

    /// An unparsable expiration counts as expired: a credential of unknown validity is not used.
    pub fn is_expired(&self, now: OffsetDateTime) -> bool {
        parse_expiration(&self.expiration).is_none_or(|expiration| expiration <= now)
    }

    pub fn is_expired_now(&self) -> bool {
        self.is_expired(OffsetDateTime::now_utc())
    }

    pub(super) fn private_key_bytes(&self) -> Result<[u8; 32]> {
        let stripped = self
            .private_key
            .strip_prefix("0x")
            .unwrap_or(&self.private_key);
        let bytes = hex::decode(stripped).context("Ephemeral private key is not hex")?;
        <[u8; 32]>::try_from(bytes.as_slice())
            .map_err(|_| anyhow!("Ephemeral private key must be 32 bytes"))
    }
}

/// Accepts RFC 3339 and the .NET round-trip form without an offset (treated as UTC).
fn parse_expiration(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &Rfc3339)
        .or_else(|_| OffsetDateTime::parse(&format!("{value}Z"), &Rfc3339))
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity(expiration: &str) -> EphemeralIdentity {
        EphemeralIdentity::new(
            "0xabc".to_owned(),
            "0x01".to_owned(),
            expiration.to_owned(),
            Vec::new(),
        )
    }

    #[test]
    fn expiration_formats_from_dotnet_are_accepted() {
        let now = OffsetDateTime::parse("2026-09-10T00:00:00Z", &Rfc3339)
            .unwrap_or(OffsetDateTime::UNIX_EPOCH);
        assert!(!identity("2026-09-17T12:34:56.1234567Z").is_expired(now));
        assert!(!identity("2026-09-17T12:34:56.1234567+00:00").is_expired(now));
        assert!(!identity("2026-09-17T12:34:56").is_expired(now));
        assert!(identity("2026-09-09T12:34:56Z").is_expired(now));
        assert!(identity("not a date").is_expired(now));
    }

    #[test]
    fn debug_never_prints_the_key() {
        let debug = format!("{:?}", identity("2026-09-17T00:00:00Z"));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("0x01"));
    }

    #[test]
    fn deserializes_the_explorer_shape() {
        let json = r#"{"address":"0xA","key":"0x01","expiration":"2026-09-17T00:00:00Z",
            "ephemeralAuthChain":[{"type":"SIGNER","payload":"0xA","signature":""},
            {"type":"ECDSA_EPHEMERAL","payload":"Decentraland Login","signature":"0xsig"}]}"#;
        let parsed: EphemeralIdentity = serde_json::from_str(json).unwrap_or_else(|e| {
            #[allow(clippy::panic)]
            {
                panic!("{e}")
            }
        });
        assert_eq!(parsed.address, "0xA");
        assert_eq!(parsed.auth_chain.len(), 2);
        assert_eq!(
            parsed.auth_chain.first().map(|l| l.kind.as_str()),
            Some("SIGNER")
        );
    }
}
