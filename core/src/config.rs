use anyhow::{Context, Result, anyhow};
use log::error;
use serde_json::{Map, Value};

use crate::installs::config_path;

fn config_content() -> Result<Map<String, Value>> {
    let path = config_path();
    if path.exists() {
        let data = std::fs::read_to_string(path).context("Failed to read config.json")?;
        return serde_json::from_str::<Map<String, Value>>(&data).context("Failed to parse JSON");
    }

    let map: Map<String, Value> = Map::new();
    Ok(map)
}

fn write_config(value: &Map<String, Value>) -> Result<()> {
    let path = config_path();
    let file = std::fs::File::create(path)?;
    serde_json::to_writer_pretty(file, &value)?;
    Ok(())
}

fn user_id() -> Result<String> {
    const KEY: &str = "analytics-user-id";
    let config = config_content()?;
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
    write_config(&config)?;
    Ok(id)
}

pub fn user_id_or_none() -> String {
    user_id().unwrap_or_else(|e| {
        error!("Cannot get user id from config, fallback is used: {:#}", e);
        "none".to_owned()
    })
}

pub fn arguments_from_key(key: &str) -> Vec<String> {
    let config = config_content();
    match config {
        Ok(config) => {
            if let Some(raw) = config.get(key) {
                let raw = raw.as_str();
                match raw {
                    Some(value) => value.split(' ').map(ToOwned::to_owned).collect(),
                    None => Vec::new(),
                }
            } else {
                Vec::new()
            }
        }
        Err(e) => {
            log::error!("Error on reading config content: {}", e);
            Vec::new()
        }
    }
}

pub fn cmd_arguments() -> Vec<String> {
    const KEY: &str = "cmd-arguments";
    arguments_from_key(KEY)
}

pub fn client_additional_arguments() -> Vec<String> {
    const KEY: &str = "client-additional-arguments";
    arguments_from_key(KEY)
}
