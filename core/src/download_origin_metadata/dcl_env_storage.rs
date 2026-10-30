use std::fs;

use anyhow::Result;

use crate::installs::{dcl_env_bridge_path, dcl_env_storage_path};

use super::dcl_env::DclEnv;

fn parse_bridge_content(content: &str) -> Option<DclEnv> {
    DclEnv::parse(content.trim_start_matches('\u{feff}').trim())
}

pub struct DclEnvStorage {}

impl DclEnvStorage {
    pub fn read() -> Option<DclEnv> {
        let content = fs::read_to_string(dcl_env_storage_path()).ok()?;
        DclEnv::parse(content.trim())
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

    pub fn delete() {
        let path = dcl_env_storage_path();

        match fs::remove_file(&path) {
            Ok(()) => log::info!("Dcl environment consumed"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("Cannot remove dcl environment storage file: {e}"),
        }
    }

    pub fn ingest_bridge_file() {
        let bridge = dcl_env_bridge_path();
        let Ok(content) = fs::read_to_string(&bridge) else {
            return;
        };

        if let Some(env) = parse_bridge_content(&content) {
            if let Err(e) = Self::write(env) {
                log::error!("Cannot write dcl environment from bridge file: {e}");
                return;
            }
            log::info!("Environment ingested from bridge file: {env}");
        } else {
            log::warn!("Dcl environment bridge file content is invalid, discarding");
        }

        if let Err(e) = fs::remove_file(&bridge) {
            log::warn!("Cannot remove dcl environment bridge file: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case("zone", Some(DclEnv::Zone))]
    #[case("org", Some(DclEnv::Org))]
    #[case("zone\r\n", Some(DclEnv::Zone))]
    #[case("  ZONE  ", Some(DclEnv::Zone))]
    #[case("\u{feff}zone", Some(DclEnv::Zone))]
    #[case("\u{feff}zone\r\n", Some(DclEnv::Zone))]
    #[case("", None)]
    #[case("\u{feff}", None)]
    #[case("prod", None)]
    fn parses_bridge_content(#[case] content: &str, #[case] expected: Option<DclEnv>) {
        assert_eq!(parse_bridge_content(content), expected);
    }
}
