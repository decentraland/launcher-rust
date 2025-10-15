pub mod auth_token_storage;

use anyhow::{Result, anyhow};

use auth_token_storage::AuthTokenStorage;

pub struct AutoAuth {}

impl AutoAuth {
    pub fn try_obtain_auth_token() {
        if AuthTokenStorage::has_token() {
            log::info!("Token already obtained");
            return;
        }

        #[cfg(target_os = "macos")]
        match Self::obtain_token_internal() {
            Ok(token) => {
                let Some(token) = token else {
                    log::warn!("Token value is empty");
                    return;
                };

                log::info!("Token obtained");
                if let Err(e) = AuthTokenStorage::write_token(token.as_str()) {
                    log::error!("Cannot write token: {e}");
                }
            }
            Err(e) => {
                log::error!("Obtain auth token error: {e}");
            }
        }
    }

    #[cfg(target_os = "macos")]
    pub fn try_auto_copy_from_dmg() {}

    #[cfg(target_os = "macos")]
    fn obtain_token_internal() -> Result<Option<String>> {
        use anyhow::Context;
        use std::borrow::ToOwned;

        use crate::environment::macos::{dmg_backing_file, dmg_mount_path, where_from_attr};

        let path = std::env::current_exe()?;
        log::info!("Exe path: {}", path.display());
        let dmg_mount_path = dmg_mount_path(&path)?;
        log::info!("Exe is running from dmg: {dmg_mount_path:?}");

        let Some(dmg_mount_path) = dmg_mount_path else {
            return Ok(None);
        };

        let Some(dmg_dir) = dmg_mount_path.parent() else {
            return Err(anyhow!("Dmg doesn't have a parent"));
        };
        log::info!("Dmg parent: {}", dmg_dir.display());

        let dmg_file_path = dmg_backing_file(&dmg_dir.to_string_lossy())
            .with_context(|| "Cannot resolve mount path: {dmg_dir}")?
            .ok_or_else(|| anyhow!("Dmg original file not found"))?;
        let where_from = where_from_attr(dmg_file_path.as_path())
            .with_context(|| "Cannot read where from attr: {dmg_file_path}")?;

        log::info!("Where from attr: {where_from:?}");

        let Some(where_from) = where_from else {
            return Err(anyhow!("Dmg does not have where from data"));
        };

        // TODO trim redundant data and purify token
        let token = where_from.first().map(ToOwned::to_owned);

        Ok(token)
    }
}
