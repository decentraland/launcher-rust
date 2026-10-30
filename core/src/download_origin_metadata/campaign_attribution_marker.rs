use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::installs::campaign_attribution_reported_marker_path;

pub struct CampaignAttributionMarker {}

impl CampaignAttributionMarker {
    pub fn is_reported() -> bool {
        campaign_attribution_reported_marker_path().exists()
    }

    pub fn mark_reported() -> Result<()> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        fs::write(
            campaign_attribution_reported_marker_path(),
            timestamp.to_string(),
        )?;
        Ok(())
    }
}
