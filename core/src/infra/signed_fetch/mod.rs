//! Decentraland Signed Fetch.
//!
//! The request is authorized by an auth chain whose last link signs
//! `method:path:timestamp:metadata` with the ephemeral key. Mirrors `createPayload` in
//! `@dcl/crypto-middleware` (method and path lower-cased, metadata verbatim).

mod identity;
mod signer;

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Result, anyhow};
use url::Url;

pub use identity::{AuthLink, EphemeralIdentity, SIGNED_ENTITY_LINK_TYPE};
pub use signer::{address_of, recover_address};

pub const AUTH_CHAIN_HEADER_PREFIX: &str = "x-identity-auth-chain-";
pub const TIMESTAMP_HEADER: &str = "x-identity-timestamp";
pub const METADATA_HEADER: &str = "x-identity-metadata";

pub fn signed_fetch_payload(method: &str, path: &str, unix_ms: u128, metadata: &str) -> String {
    format!(
        "{}:{}:{unix_ms}:{metadata}",
        method.to_lowercase(),
        path.to_lowercase()
    )
}

/// Headers authorizing `method url` with `identity`. `metadata` must be the exact string the
/// caller also sends as the request metadata, since verifiers rebuild the payload from it.
pub fn sign_request(
    identity: &EphemeralIdentity,
    method: &str,
    url: &Url,
    metadata: &str,
    unix_ms: u128,
) -> Result<Vec<(String, String)>> {
    if identity.is_expired_now() {
        return Err(anyhow!("The ephemeral identity has expired"));
    }

    let payload = signed_fetch_payload(method, url.path(), unix_ms, metadata);
    let signature = signer::personal_sign(&identity.private_key_bytes()?, &payload)?;

    let mut links = identity.auth_chain.clone();
    links.push(AuthLink {
        kind: SIGNED_ENTITY_LINK_TYPE.to_owned(),
        payload,
        signature,
    });

    let mut headers = Vec::with_capacity(links.len().saturating_add(2));
    for (index, link) in links.iter().enumerate() {
        headers.push((
            format!("{AUTH_CHAIN_HEADER_PREFIX}{index}"),
            serde_json::to_string(link)?,
        ));
    }
    headers.push((TIMESTAMP_HEADER.to_owned(), unix_ms.to_string()));
    headers.push((METADATA_HEADER.to_owned(), metadata.to_owned()));
    Ok(headers)
}

pub fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

#[cfg(test)]
#[allow(clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;

    const KEY_ONE_HEX: &str = "0x0000000000000000000000000000000000000000000000000000000000000001";
    const KEY_ONE_ADDRESS: &str = "0x7e5f4552091a69125d5dfcb7b8c2659029395bdf";

    fn identity(expiration: &str) -> EphemeralIdentity {
        EphemeralIdentity::new(
            KEY_ONE_ADDRESS.to_owned(),
            KEY_ONE_HEX.to_owned(),
            expiration.to_owned(),
            vec![
                AuthLink {
                    kind: "SIGNER".to_owned(),
                    payload: "0xsigner".to_owned(),
                    signature: String::new(),
                },
                AuthLink {
                    kind: "ECDSA_EPHEMERAL".to_owned(),
                    payload: "Decentraland Login".to_owned(),
                    signature: "0xephemeral".to_owned(),
                },
            ],
        )
    }

    fn url() -> Url {
        Url::parse("https://intercom-proxy.decentraland.zone/intercom/tickets?x=1")
            .unwrap_or_else(|e| panic!("{e}"))
    }

    #[test]
    fn payload_lowercases_method_and_path_only() {
        assert_eq!(
            signed_fetch_payload("POST", "/Intercom/Tickets", 42, r#"{"A":"B"}"#),
            r#"post:/intercom/tickets:42:{"A":"B"}"#
        );
    }

    #[test]
    fn headers_carry_the_chain_plus_signed_entity_timestamp_and_metadata() {
        let headers = sign_request(
            &identity("2999-01-01T00:00:00Z"),
            "POST",
            &url(),
            "{}",
            1000,
        )
        .unwrap_or_else(|e| panic!("{e}"));

        assert_eq!(headers.len(), 5);
        assert_eq!(headers[0].0, "x-identity-auth-chain-0");
        assert_eq!(headers[1].0, "x-identity-auth-chain-1");
        assert_eq!(headers[2].0, "x-identity-auth-chain-2");
        assert_eq!(
            headers[3],
            ("x-identity-timestamp".to_owned(), "1000".to_owned())
        );
        assert_eq!(
            headers[4],
            ("x-identity-metadata".to_owned(), "{}".to_owned())
        );

        let signed: AuthLink =
            serde_json::from_str(&headers[2].1).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(signed.kind, SIGNED_ENTITY_LINK_TYPE);
        // The query string is not part of the signed path.
        assert_eq!(signed.payload, "post:/intercom/tickets:1000:{}");
        assert_eq!(
            recover_address(&signed.payload, &signed.signature).unwrap_or_default(),
            KEY_ONE_ADDRESS
        );
    }

    #[test]
    fn chain_links_serialize_with_the_type_key() {
        let headers = sign_request(&identity("2999-01-01T00:00:00Z"), "POST", &url(), "{}", 1)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            headers[0].1,
            r#"{"type":"SIGNER","payload":"0xsigner","signature":""}"#
        );
    }

    #[test]
    fn expired_identity_is_refused() {
        assert!(sign_request(&identity("2000-01-01T00:00:00Z"), "POST", &url(), "{}", 1).is_err());
    }
}
