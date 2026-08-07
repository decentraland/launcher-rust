use std::fs;

use anyhow::Result;

use crate::installs::dcl_env_storage_path;

use super::dcl_env::DclEnv;

pub struct DclEnvStorage {}

impl DclEnvStorage {
    pub fn read() -> Option<DclEnv> {
        let content = fs::read_to_string(dcl_env_storage_path()).ok()?;
        DclEnv::parse(content.trim())
    }

    pub fn is_zone() -> bool {
        Self::read() == Some(DclEnv::Zone)
    }

    /// Last-wins, unlike the attribution storages: the environment is
    /// configuration, not attribution. Reinstalling from a production installer
    /// on a machine that once installed from `.zone` must move the client back
    /// to production.
    pub fn write(env: DclEnv) -> Result<()> {
        if Self::read() == Some(env) {
            return Ok(());
        }
        fs::write(dcl_env_storage_path(), env.as_str())?;
        Ok(())
    }
}
