use std::fs;

use anyhow::Result;

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
        fs::write(auth_token_marker_path(), token)?;
        fs::write(auth_token_bridge_path(), token)?;
        Ok(())
    }
}
