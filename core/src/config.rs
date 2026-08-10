use anyhow::{Result, anyhow};
use dcl_launcher_shared::config::{arguments_from_key, content, write};
use log::error;
use serde_json::Value;

fn user_id() -> Result<String> {
    const KEY: &str = "analytics-user-id";
    let config = content()?;
    if let Some(id) = config.get(KEY) {
        let value = id.as_str();
        match value {
            Some(user) => {
                return Ok(user.to_owned());
            }
            None => {
                return Err(anyhow!("Value under key {} is in a wrong format", KEY));
            }
        }
    }

    let mut config = config;
    let id = uuid::Uuid::new_v4().to_string();
    config.insert(KEY.to_owned(), Value::String(id.clone()));
    write(&config)?;
    Ok(id)
}

pub fn user_id_or_none() -> String {
    user_id().unwrap_or_else(|e| {
        error!("Cannot get user id from config, fallback is used: {:#}", e);
        "none".to_owned()
    })
}

pub fn client_additional_arguments() -> Vec<String> {
    const KEY: &str = "client-additional-arguments";
    arguments_from_key(KEY)
}
