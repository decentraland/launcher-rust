use std::sync::LazyLock;

use log::debug;
use serde::Serialize;
use serde_json::{Map, Value};

// Cross-source attribution join with landing-site Segment events relies on
// these field names and shapes matching the landing-site fingerprint module
// exactly. See:
//   https://github.com/decentraland/landing-site/blob/main/src/modules/fingerprint.ts
//
// `timezone_offset_minutes` (sent by the browser) is intentionally not
// mirrored here: `time::OffsetDateTime::now_local()` requires the
// `local-offset` feature and returns `Err` from inside the tokio runtime
// for soundness reasons. The IANA `fp_timezone` field is more useful anyway
// — the data warehouse can derive the offset from the timezone + event
// timestamp at join time.
// `fp_` prefix keeps these fields from colliding with caller-supplied
// property names when merged into a Segment event payload.
#[allow(clippy::struct_field_names)]
#[derive(Debug, Clone, Serialize)]
pub struct ClientFingerprint {
    fp_screen_width: u32,
    fp_screen_height: u32,
    fp_device_pixel_ratio: f64,
    fp_hardware_concurrency: Option<u32>,
    fp_timezone: Option<String>,
    fp_language: Option<String>,
    fp_platform: &'static str,
}

impl ClientFingerprint {
    pub fn current() -> Self {
        let display = primary_display_info();
        Self {
            fp_screen_width: display.width,
            fp_screen_height: display.height,
            fp_device_pixel_ratio: display.scale_factor,
            fp_hardware_concurrency: hardware_concurrency(),
            fp_timezone: iana_time_zone::get_timezone().ok(),
            fp_language: sys_locale::get_locale(),
            fp_platform: PLATFORM.as_str(),
        }
    }
}

#[allow(clippy::fallible_impl_from)]
impl From<ClientFingerprint> for Map<String, Value> {
    #[allow(clippy::unwrap_used)]
    fn from(value: ClientFingerprint) -> Self {
        // Infallible by construction: a `#[derive(Serialize)]` struct of
        // plain fields always serializes to a JSON Object. Anything else
        // would be a compile-time bug in this module.
        match serde_json::to_value(value).unwrap() {
            Value::Object(map) => map,
            _ => unreachable!(),
        }
    }
}

struct DisplaySnapshot {
    width: u32,
    height: u32,
    scale_factor: f64,
}

impl Default for DisplaySnapshot {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            scale_factor: 1.0,
        }
    }
}

fn primary_display_info() -> DisplaySnapshot {
    let displays = match display_info::DisplayInfo::all() {
        Ok(d) => d,
        Err(e) => {
            // Real desktops shouldn't hit this; CI and headless RDP do.
            // Debug level keeps it visible without noisy warnings.
            debug!("display-info probe failed, defaulting screen fields: {e}");
            return DisplaySnapshot::default();
        }
    };

    let chosen = displays
        .iter()
        .find(|d| d.is_primary)
        .or_else(|| displays.first());

    match chosen {
        Some(d) => DisplaySnapshot {
            width: d.width,
            height: d.height,
            scale_factor: f64::from(d.scale_factor),
        },
        None => DisplaySnapshot::default(),
    }
}

// `available_parallelism` returns `NonZero<usize>`; map to `u32` and surface
// `None` if the probe fails (locked-down CI / containers) rather than
// emitting a misleading zero or a fabricated default.
fn hardware_concurrency() -> Option<u32> {
    std::thread::available_parallelism()
        .ok()
        .and_then(|n| u32::try_from(n.get()).ok())
}

// `concat!` only accepts literal tokens, so we resolve the constants via
// `LazyLock` once on first access instead. Includes the arch so multi-arch
// builds (Apple Silicon vs Intel, ARM Windows) are distinguishable:
// e.g. "macos/aarch64", "windows/x86_64".
static PLATFORM: LazyLock<String> = LazyLock::new(|| {
    format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH)
});

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::{Result, anyhow};

    #[test]
    fn current_produces_a_serializable_snapshot_with_expected_fields() -> Result<()> {
        let fp = ClientFingerprint::current();
        let value = serde_json::to_value(&fp)?;
        let obj = value.as_object().ok_or_else(|| anyhow!("not an object"))?;

        for key in [
            "fp_screen_width",
            "fp_screen_height",
            "fp_device_pixel_ratio",
            "fp_hardware_concurrency",
            "fp_timezone",
            "fp_language",
            "fp_platform",
        ] {
            assert!(obj.contains_key(key), "missing field: {key}");
        }
        Ok(())
    }

    #[test]
    fn platform_includes_os_and_arch_separated_by_slash() -> Result<()> {
        let platform = PLATFORM.as_str();
        let parts: Vec<&str> = platform.split('/').collect();
        assert_eq!(parts.len(), 2, "expected os/arch, got {platform}");
        let os = parts.first().ok_or_else(|| anyhow!("missing os part"))?;
        let arch = parts.get(1).ok_or_else(|| anyhow!("missing arch part"))?;
        assert!(!os.is_empty());
        assert!(!arch.is_empty());
        Ok(())
    }

    #[test]
    fn hardware_concurrency_is_either_absent_or_plausible() {
        // Guards against accidental overflow / wrap-around in the
        // `u32::try_from` path. On hosts where the probe fails (some CI
        // sandboxes) the value is `None` and that's fine.
        if let Some(n) = hardware_concurrency() {
            assert!(n < 4096, "implausibly high hardware concurrency: {n}");
        }
    }

    #[test]
    fn fingerprint_converts_into_property_map() -> Result<()> {
        let map: Map<String, Value> = ClientFingerprint::current().into();
        assert!(map.contains_key("fp_platform"));
        assert!(map.contains_key("fp_screen_width"));
        Ok(())
    }
}
