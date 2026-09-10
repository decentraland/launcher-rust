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

const CRASH_REPORT_DIALOG_DISABLED_KEY: &str = "crash-report-dialog-disabled";

/// User opt-out of the crash-report dialog ("Don't show this again").
pub fn crash_report_dialog_disabled() -> bool {
    match config_content() {
        Ok(config) => bool_at(&config, CRASH_REPORT_DIALOG_DISABLED_KEY),
        Err(e) => {
            error!(
                "Cannot read config, crash report dialog stays enabled: {:#}",
                e
            );
            false
        }
    }
}

pub fn set_crash_report_dialog_disabled(disabled: bool) -> Result<()> {
    let mut config = config_content()?;
    config.insert(
        CRASH_REPORT_DIALOG_DISABLED_KEY.to_owned(),
        Value::Bool(disabled),
    );
    write_config(&config)
}

/// Missing or non-boolean values read as `false`, so a hand-edited config can't disable a feature
/// by accident.
fn bool_at(config: &Map<String, Value>, key: &str) -> bool {
    config.get(key).and_then(Value::as_bool).unwrap_or(false)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config_with(key: &str, value: Value) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert(key.to_owned(), value);
        map
    }

    #[test]
    fn bool_at_reads_only_real_booleans() {
        assert!(bool_at(
            &config_with(CRASH_REPORT_DIALOG_DISABLED_KEY, Value::Bool(true)),
            CRASH_REPORT_DIALOG_DISABLED_KEY
        ));
        assert!(!bool_at(
            &config_with(CRASH_REPORT_DIALOG_DISABLED_KEY, Value::Bool(false)),
            CRASH_REPORT_DIALOG_DISABLED_KEY
        ));
        assert!(!bool_at(
            &config_with(
                CRASH_REPORT_DIALOG_DISABLED_KEY,
                Value::String("true".to_owned())
            ),
            CRASH_REPORT_DIALOG_DISABLED_KEY
        ));
        assert!(!bool_at(&Map::new(), CRASH_REPORT_DIALOG_DISABLED_KEY));
    }
}
