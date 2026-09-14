//! `config.json` inside the launcher data dir.
//!
//! Both the argument merge in [`crate::environment`] and core's analytics
//! user id read this same file, so the read/write primitives live here.

use std::path::PathBuf;

use anyhow::{Context, Result};
use serde_json::{Map, Value};

pub fn path() -> PathBuf {
    crate::app_dir().join("config.json")
}

pub fn content() -> Result<Map<String, Value>> {
    let path = path();
    if path.exists() {
        let data = std::fs::read_to_string(path).context("Failed to read config.json")?;
        return serde_json::from_str::<Map<String, Value>>(&data).context("Failed to parse JSON");
    }

    Ok(Map::new())
}

pub fn write(value: &Map<String, Value>) -> Result<()> {
    let file = std::fs::File::create(path())?;
    serde_json::to_writer_pretty(file, &value)?;
    Ok(())
}

pub fn arguments_from_key(key: &str) -> Vec<String> {
    let config = match content() {
        Ok(config) => config,
        Err(e) => {
            log::error!("Error on reading config content: {}", e);
            return Vec::new();
        }
    };

    let Some(raw) = config.get(key).and_then(Value::as_str) else {
        return Vec::new();
    };

    raw.split(' ').map(ToOwned::to_owned).collect()
}

pub fn cmd_arguments() -> Vec<String> {
    const KEY: &str = "cmd-arguments";
    arguments_from_key(KEY)
}
