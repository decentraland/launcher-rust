use std::fs;

use anyhow::Result;

use crate::installs::campaign_anon_user_id_storage_path;

use super::anon_user_id::AnonUserId;

pub struct CampaignAnonUserIdStorage {}

impl CampaignAnonUserIdStorage {
    pub fn read() -> Option<AnonUserId> {
        let path = campaign_anon_user_id_storage_path();
        let content = fs::read_to_string(&path).ok()?;
        AnonUserId::parse(content.trim())
    }

    pub fn has() -> bool {
        Self::read().is_some()
    }

    pub fn write(id: &AnonUserId) -> Result<()> {
        if Self::read().as_ref() == Some(id) {
            return Ok(());
        }
        fs::write(campaign_anon_user_id_storage_path(), id.as_str())?;
        Ok(())
    }
}
