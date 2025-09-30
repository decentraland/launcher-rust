use std::fs;

use anyhow::Result;
use serde_json::json;

use crate::installs::auth_token_bridge_path;
use crate::installs::auth_token_marker_path;

pub struct AuthTokenStorage {}

impl AuthTokenStorage {
    pub fn has_token() -> bool {
        let path = auth_token_marker_path();

        match fs::exists(path) {
            Ok(has) => has,
            Err(e) => {
                log::error!("Cannot read token path: {e}");
                false
            }
        }
    }

    pub fn write_token(token: &str) -> Result<()> {
        let json = json!(
            {
                "token": token
            }
        );

        let marker_file = fs::File::create(auth_token_marker_path())?;
        serde_json::to_writer_pretty(marker_file, &json)?;

        let bridge_file = fs::File::create(auth_token_bridge_path())?;
        serde_json::to_writer_pretty(bridge_file, &json)?;

        Ok(())
    }
}
