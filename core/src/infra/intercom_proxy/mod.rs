//! Ticket creation through `intercom-proxy`, the same route the Explorer clients use
//! (`POST /intercom/tickets`, signed fetch, `Origin` allow-listed per environment).

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use log::info;
use serde_json::{Map, Value, json};
use url::Url;

use crate::download_origin_metadata::dcl_env::DclEnv;
use crate::download_origin_metadata::dcl_env_storage::DclEnvStorage;
use crate::environment::{AppEnvironment, LauncherEnvironment};
use crate::infra::signed_fetch::{EphemeralIdentity, now_unix_ms, sign_request};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// The proxy signs `{}` and leaves the body unsigned.
const SIGNATURE_METADATA: &str = "{}";
const MAX_LOGGED_BODY: usize = 500;

pub struct IntercomProxyClient {
    tickets_url: Url,
    origin: String,
    http: reqwest::Client,
}

impl IntercomProxyClient {
    pub fn new(env: DclEnv) -> Result<Self> {
        let tld = env.as_str();
        let tickets_url = Url::parse(&format!(
            "https://intercom-proxy.decentraland.{tld}/intercom/tickets"
        ))?;
        let http = reqwest::Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .build()
            .context("Cannot build the HTTP client")?;
        Ok(Self {
            tickets_url,
            origin: format!("https://play.decentraland.{tld}"),
            http,
        })
    }

    /// Environment from the install's download origin when known; otherwise production builds
    /// target `org` and everything else `zone`.
    pub fn new_from_env() -> Result<Self> {
        Self::new(DclEnvStorage::read().unwrap_or_else(default_env))
    }

    pub const fn tickets_url(&self) -> &Url {
        &self.tickets_url
    }

    /// Returns the id of the created ticket.
    pub async fn create_ticket(
        &self,
        identity: &EphemeralIdentity,
        ticket_attributes: Map<String, Value>,
    ) -> Result<String> {
        let headers = sign_request(
            identity,
            "POST",
            &self.tickets_url,
            SIGNATURE_METADATA,
            now_unix_ms(),
        )?;

        let mut request = self
            .http
            .post(self.tickets_url.clone())
            .header("Origin", &self.origin)
            .json(&json!({ "ticket_attributes": ticket_attributes }));
        for (name, value) in headers {
            request = request.header(name, value);
        }

        info!("Creating Intercom ticket via {}", self.tickets_url);
        let response = request
            .send()
            .await
            .context("Intercom proxy request failed")?;
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!(
                "Intercom proxy answered {status}: {}",
                truncate(&body, MAX_LOGGED_BODY)
            ));
        }

        ticket_id(&body)
    }
}

fn default_env() -> DclEnv {
    match AppEnvironment::launcher_environment() {
        LauncherEnvironment::Production => DclEnv::Org,
        LauncherEnvironment::Development | LauncherEnvironment::Unknown => DclEnv::Zone,
    }
}

/// The proxy echoes Intercom's ticket object; only `id` matters and its absence means no ticket
/// was created even though the transport succeeded.
fn ticket_id(body: &str) -> Result<String> {
    let value: Value = serde_json::from_str(body).context("Intercom proxy answered non-JSON")?;
    match value.get("id") {
        Some(Value::String(id)) if !id.is_empty() => Ok(id.clone()),
        Some(Value::Number(id)) => Ok(id.to_string()),
        _ => Err(anyhow!(
            "Intercom proxy response has no ticket id: {}",
            truncate(body, MAX_LOGGED_BODY)
        )),
    }
}

fn truncate(text: &str, max: usize) -> &str {
    let end = text
        .char_indices()
        .nth(max)
        .map_or(text.len(), |(index, _)| index);
    text.get(..end).unwrap_or(text)
}

#[cfg(test)]
#[allow(clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn urls_follow_the_environment() {
        let zone = IntercomProxyClient::new(DclEnv::Zone).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            zone.tickets_url().as_str(),
            "https://intercom-proxy.decentraland.zone/intercom/tickets"
        );
        assert_eq!(zone.origin, "https://play.decentraland.zone");

        let org = IntercomProxyClient::new(DclEnv::Org).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(org.origin, "https://play.decentraland.org");
    }

    #[test]
    fn ticket_id_accepts_string_or_number_and_rejects_missing() {
        assert_eq!(ticket_id(r#"{"id":"123"}"#).unwrap_or_default(), "123");
        assert_eq!(ticket_id(r#"{"id":123}"#).unwrap_or_default(), "123");
        assert!(ticket_id(r#"{"error":"nope"}"#).is_err());
        assert!(ticket_id("not json").is_err());
    }

    #[test]
    fn truncate_is_char_safe() {
        assert_eq!(truncate("héllo", 2), "hé");
        assert_eq!(truncate("abc", 10), "abc");
    }
}
