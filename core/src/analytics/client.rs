use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use std::collections::HashSet;

use anyhow::{Context, Result};
use log::error;
use segment::HttpClient;
use segment::message::{Track, User};
use serde_json::{Map, Value, json};
use time::OffsetDateTime;

use get_if_addrs::get_if_addrs;
use tokio::sync::Mutex;

use crate::analytics::infrastructure::event_queue::CombinedAnalyticsEventQueue;
use crate::analytics::infrastructure::event_send_daemon::AnalyticsEventSendDaemon;
use crate::analytics::infrastructure::queued_batcher::QueuedBatcher;

use super::event::Event;
use super::session::SessionId;

const APP_ID: &str = "decentraland-launcher-rust";

pub struct AnalyticsClient {
    anonymous_id: String,
    os: String,
    launcher_version: String,
    session_id: SessionId,
    batcher: QueuedBatcher,
    send_daemon: AnalyticsEventSendDaemon<HttpClient>,
}

impl AnalyticsClient {
    pub fn new(
        write_key: String,
        anonymous_id: String,
        os: String,
        launcher_version: String,
    ) -> Self {
        let queue = CombinedAnalyticsEventQueue::default();
        let queue = Arc::new(Mutex::new(queue));

        let context = json!({"direct": true});
        let batcher = QueuedBatcher::new(queue.clone(), Some(context));
        let session_id = SessionId::random();

        let client = HttpClient::default();
        let mut send_daemon = AnalyticsEventSendDaemon::new(queue, None, write_key, client);

        send_daemon.start();

        Self {
            anonymous_id,
            os,
            launcher_version,
            session_id,
            batcher,
            send_daemon,
        }
    }

    async fn track(&mut self, event: String, mut properties: Map<String, Value>) -> Result<()> {
        properties.insert("os".to_owned(), Value::String(self.os.clone()));
        properties.insert(
            "launcherVersion".to_owned(),
            Value::String(self.launcher_version.clone()),
        );
        properties.insert(
            "sessionId".to_owned(),
            Value::String(self.session_id.value().to_owned()),
        );
        properties.insert("appId".to_owned(), Value::String(APP_ID.to_owned()));

        let user = User::AnonymousId {
            anonymous_id: self.anonymous_id.clone(),
        };

        let properties: Value = Value::Object(properties);
        let context: Option<Value> = Some(network_context());

        let msg = Track {
            user,
            event,
            properties,
            context,
            timestamp: Some(OffsetDateTime::now_utc()),
            ..Default::default()
        };

        match self.batcher.push(msg) {
            Ok(option) => {
                // if something returned then it has not been enqued
                if let Some(msg) = option {
                    self.batcher.flush().await?;
                    if let Err(e) = self.batcher.push(msg) {
                        Err(anyhow!("Cannot push message even after flush: {e}"))
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(anyhow!("Cannot push message to batcher: {e}")),
        }
    }

    async fn flush(&mut self) -> Result<()> {
        self.batcher.flush().await
    }

    pub async fn track_and_flush(&mut self, event: Event) -> Result<()> {
        let properties = properties_from_event(&event);
        let event_name = format!("{}", event);
        self.track(event_name, properties)
            .await
            .context("Cannot track")?;
        self.flush().await.context("Cannot flush")?;
        Ok(())
    }

    pub const fn anonymous_id(&self) -> &str {
        self.anonymous_id.as_str()
    }

    pub const fn session_id(&self) -> &SessionId {
        &self.session_id
    }

    pub async fn cleanup(&self) {
        self.send_daemon
            .wait_until_empty_queue_or_abandon(None)
            .await;
    }
}

fn properties_from_event(event: &Event) -> Map<String, Value> {
    let result = serde_json::to_value(event);
    match result {
        Ok(json) => match json.as_object() {
            Some(map) => match map.get("data") {
                Some(data) => match data {
                    Value::Object(map) => map.to_owned(),
                    _ => {
                        error!("serialized event is not a json object: {:#?}", data);
                        Map::new()
                    }
                },
                None => {
                    error!("serialized event doesn't have data property");
                    Map::new()
                }
            },
            None => {
                error!("serialized event is not an object");
                Map::new()
            }
        },
        Err(error) => {
            error!("Cannot serialize event; {}", error);
            Map::new()
        }
    }
}

fn network_context() -> Value {
    let internals = network_context_internal();
    let mut map = Map::new();
    map.insert("network".to_owned(), internals);
    Value::Object(map)
}

#[cfg(target_os = "macos")]
fn network_context_internal() -> Value {
    use system_configuration::network_configuration::get_interfaces;

    let mut available_network_types: HashSet<String> = HashSet::new();

    if let Ok(addrs) = get_if_addrs() {
        // Active
        let active_ifaces: HashSet<String> = addrs
            .into_iter()
            .filter(|iface| {
                // Skip loopbacks
                if iface.is_loopback() {
                    return false;
                }
                // Skip link-local
                match iface.ip() {
                    std::net::IpAddr::V4(ip) => !ip.is_link_local(),
                    std::net::IpAddr::V6(ip) => !ip.is_loopback(),
                }
            })
            .map(|iface| iface.name)
            .collect();

        // Interfaces
        let ifaces = get_interfaces();
        for iface in ifaces.iter() {
            if let Some(name) = iface.bsd_name() {
                let name = name.to_string();
                if active_ifaces.contains(&name) {
                    let display_name = iface
                        .display_name()
                        .map(|e| e.to_string())
                        .unwrap_or_default();
                    let kind = iface
                        .interface_type_string()
                        .map(|e| e.to_string())
                        .unwrap_or_default();
                    available_network_types.insert(format!("{display_name} - {kind}"));
                }
            }
        }

        let values: Vec<Value> = available_network_types
            .into_iter()
            .map(Value::String)
            .collect();
        Value::Array(values)
    } else {
        Value::Array(Vec::new())
    }
}

#[cfg(target_os = "windows")]
fn network_context_internal() -> Value {
    let mut available_network_types: HashSet<String> = HashSet::new();

    if let Ok(addrs) = get_if_addrs() {
        for iface in addrs {
            if iface.is_loopback() {
                continue;
            }

            let ip = iface.ip();
            let is_link_local = match ip {
                std::net::IpAddr::V4(ipv4) => ipv4.is_link_local(),
                std::net::IpAddr::V6(ipv6) => ipv6.is_loopback(),
            };
            if is_link_local {
                continue;
            }

            // Windows interface names can be long and friendly:
            // e.g. "Ethernet", "Wi-Fi", "vEthernet (WSL)"
            let name = iface.name;
            let lower_name = name.to_lowercase();

            let kind = if lower_name.contains("wifi")
                || lower_name.contains("wi-fi")
                || lower_name.contains("wlan")
            {
                "Wi-Fi"
            } else if lower_name.contains("ethernet") {
                "Ethernet"
            } else if lower_name.contains("ppp") {
                "Mobile"
            } else {
                "Unknown"
            };

            available_network_types.insert(format!("{name} - {kind}"));
        }

        let values: Vec<Value> = available_network_types
            .into_iter()
            .map(Value::String)
            .collect();
        Value::Array(values)
    } else {
        Value::Array(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_attachments() -> Result<()> {
        let track = Track {
            user: User::AnonymousId {
                anonymous_id: String::new(),
            },
            properties: Value::Null,
            event: "test".to_owned(),
            timestamp: None,
            context: Some(network_context()),
            extra: Map::new(),
            integrations: None,
        };
        let json_value = serde_json::to_value(track.clone())?;

        //TODO strict check
        println!("message: {}", json_value);


        let mut batcher = Batcher::new(Some(json!("{\"type\": \"default context\"}")));
        let _ = batcher.push(track);
        let message = batcher.into_message();
        let json_value = serde_json::to_value(message)?;

        println!("message: {}", json_value);

        Ok(())
    }
}
