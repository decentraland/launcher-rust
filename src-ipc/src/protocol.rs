use serde::{Deserialize, Serialize};

use dcl_launcher_shared::environment::Args;
use dcl_launcher_shared::types::{Status, Step};

/// One JSON line on the wire.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Frame {
    #[serde(rename_all = "camelCase")]
    Req { id: u64, cmd: Command },
    #[serde(rename_all = "camelCase")]
    Res { id: u64, result: Response },
    #[serde(rename_all = "camelCase")]
    Event { status: Status },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "cmd")]
pub enum Command {
    #[serde(rename_all = "camelCase")]
    Hello {
        protocol_version: u32,
        app_version: String,
    },
    /// Flow commands carry the effective launcher arguments: the service
    /// never parses its own argv, it uses the [`Args`] attached to each
    /// invocation.
    #[serde(rename_all = "camelCase")]
    Launch {
        deeplink: Option<String>,
        #[serde(default)]
        args: Args,
    },
    #[serde(rename_all = "camelCase")]
    Retry {
        #[serde(default)]
        args: Args,
    },
    ViewCurrentState,
    #[serde(rename_all = "camelCase")]
    InjectDeeplink {
        url: String,
    },
    #[serde(rename_all = "camelCase")]
    Shutdown {
        reason: ShutdownReason,
    },
    #[serde(rename = "notifyUIClosed")]
    NotifyUiClosed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ShutdownReason {
    Update,
    Outdated,
    User,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<ResponseData>,
}

impl Response {
    #[must_use]
    pub const fn ok_empty() -> Self {
        Self {
            ok: true,
            user_message: None,
            data: None,
        }
    }

    #[must_use]
    pub const fn ok_with(data: ResponseData) -> Self {
        Self {
            ok: true,
            user_message: None,
            data: Some(data),
        }
    }

    #[must_use]
    pub const fn err(user_message: String) -> Self {
        Self {
            ok: false,
            user_message: Some(user_message),
            data: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum ResponseData {
    #[serde(rename_all = "camelCase")]
    Hello {
        service_version: String,
        protocol_version: u32,
    },
    #[serde(rename_all = "camelCase")]
    CurrentState { state: ServiceState },
}

/// Answer to `viewCurrentState`: what the service is doing right now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum ServiceState {
    Idle,
    #[serde(rename_all = "camelCase")]
    Busy {
        step: Step,
    },
    #[serde(rename_all = "camelCase")]
    Errored {
        message: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use serde_json::{Value, json};

    fn roundtrip(frame: &Frame) -> Result<Frame> {
        let raw = serde_json::to_string(frame)?;
        Ok(serde_json::from_str(&raw)?)
    }

    #[test]
    fn hello_request_shape() -> Result<()> {
        let frame = Frame::Req {
            id: 1,
            cmd: Command::Hello {
                protocol_version: 1,
                app_version: "1.20.0".to_owned(),
            },
        };

        let expected: Value = json!({
            "kind": "req",
            "id": 1,
            "cmd": { "cmd": "hello", "protocolVersion": 1, "appVersion": "1.20.0" }
        });

        assert_eq!(serde_json::to_value(&frame)?, expected);
        assert_eq!(roundtrip(&frame)?, frame);
        Ok(())
    }

    #[test]
    fn launch_request_carries_args() -> Result<()> {
        let frame = Frame::Req {
            id: 2,
            cmd: Command::Launch {
                deeplink: Some("decentraland://open".to_owned()),
                args: Args {
                    skip_analytics: true,
                    use_latest_json_url: Some("https://example.com/latest.json".to_owned()),
                    ..Args::default()
                },
            },
        };

        let expected: Value = json!({
            "kind": "req",
            "id": 2,
            "cmd": {
                "cmd": "launch",
                "deeplink": "decentraland://open",
                "args": {
                    "skipAnalytics": true,
                    "forceInMemoryAnalyticsQueue": false,
                    "openNewClientInstance": false,
                    "alwaysTriggerUpdater": false,
                    "neverTriggerUpdater": false,
                    "useLatestJsonUrl": "https://example.com/latest.json",
                    "localScene": false,
                    "bridgeOnly": false
                }
            }
        });

        assert_eq!(serde_json::to_value(&frame)?, expected);
        assert_eq!(roundtrip(&frame)?, frame);
        Ok(())
    }

    #[test]
    fn flow_requests_without_args_fall_back_to_defaults() -> Result<()> {
        let launch: Frame = serde_json::from_value(json!({
            "kind": "req",
            "id": 4,
            "cmd": { "cmd": "launch", "deeplink": null }
        }))?;
        assert_eq!(
            launch,
            Frame::Req {
                id: 4,
                cmd: Command::Launch {
                    deeplink: None,
                    args: Args::default(),
                },
            }
        );

        let retry: Frame = serde_json::from_value(json!({
            "kind": "req",
            "id": 5,
            "cmd": { "cmd": "retry" }
        }))?;
        assert_eq!(
            retry,
            Frame::Req {
                id: 5,
                cmd: Command::Retry {
                    args: Args::default(),
                },
            }
        );
        Ok(())
    }

    #[test]
    fn notify_ui_closed_uses_agreed_spelling() -> Result<()> {
        let frame = Frame::Req {
            id: 3,
            cmd: Command::NotifyUiClosed,
        };

        let expected: Value = json!({
            "kind": "req",
            "id": 3,
            "cmd": { "cmd": "notifyUIClosed" }
        });

        assert_eq!(serde_json::to_value(&frame)?, expected);
        assert_eq!(roundtrip(&frame)?, frame);
        Ok(())
    }

    #[test]
    fn busy_state_response_shape() -> Result<()> {
        let frame = Frame::Res {
            id: 7,
            result: Response::ok_with(ResponseData::CurrentState {
                state: ServiceState::Busy {
                    step: Step::Launching,
                },
            }),
        };

        let expected: Value = json!({
            "kind": "res",
            "id": 7,
            "result": {
                "ok": true,
                "data": {
                    "type": "currentState",
                    "state": { "state": "busy", "step": { "event": "launching" } }
                }
            }
        });

        assert_eq!(serde_json::to_value(&frame)?, expected);
        assert_eq!(roundtrip(&frame)?, frame);
        Ok(())
    }

    #[test]
    fn error_response_carries_user_message() -> Result<()> {
        let frame = Frame::Res {
            id: 9,
            result: Response::err("Cannot launch".to_owned()),
        };

        let expected: Value = json!({
            "kind": "res",
            "id": 9,
            "result": { "ok": false, "userMessage": "Cannot launch" }
        });

        assert_eq!(serde_json::to_value(&frame)?, expected);
        assert_eq!(roundtrip(&frame)?, frame);
        Ok(())
    }
}
