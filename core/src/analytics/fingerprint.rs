use serde::Serialize;

// Cross-source attribution join with landing-site Segment events relies on
// these field names and shapes matching `landing-site/src/modules/fingerprint.ts`
// exactly. `timezone_offset_minutes` (sent by the browser) is intentionally
// not mirrored here: `time::OffsetDateTime::now_local()` requires the
// `local-offset` feature and returns `Err` from inside the tokio runtime
// for soundness reasons. The IANA `timezone` field is more useful anyway —
// the data warehouse can derive the offset from the timezone + event
// timestamp at join time.
#[derive(Debug, Clone, Serialize)]
pub struct ClientFingerprint {
    screen_width: u32,
    screen_height: u32,
    device_pixel_ratio: f64,
    color_depth: u32,
    hardware_concurrency: u32,
    timezone: String,
    language: String,
    platform: String,
}

impl ClientFingerprint {
    pub fn collect() -> Self {
        let display = primary_display_info();
        Self {
            screen_width: display.width,
            screen_height: display.height,
            device_pixel_ratio: display.scale_factor,
            color_depth: display.color_depth,
            hardware_concurrency: hardware_concurrency(),
            timezone: timezone_name(),
            language: locale(),
            platform: platform_string(),
        }
    }
}

struct DisplaySnapshot {
    width: u32,
    height: u32,
    scale_factor: f64,
    color_depth: u32,
}

impl Default for DisplaySnapshot {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            scale_factor: 1.0,
            color_depth: 0,
        }
    }
}

fn primary_display_info() -> DisplaySnapshot {
    let Ok(displays) = display_info::DisplayInfo::all() else {
        return DisplaySnapshot::default();
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
            // `display-info` doesn't expose bit depth on every platform.
            // Modern displays are 24- or 32-bit; pinning to 24 keeps the
            // schema parity with the browser-side payload.
            color_depth: 24,
        },
        None => DisplaySnapshot::default(),
    }
}

fn hardware_concurrency() -> u32 {
    std::thread::available_parallelism()
        .map(|n| u32::try_from(n.get()).unwrap_or(u32::MAX))
        .unwrap_or(0)
}

fn timezone_name() -> String {
    iana_time_zone::get_timezone().unwrap_or_default()
}

fn locale() -> String {
    sys_locale::get_locale().unwrap_or_default()
}

// Includes the arch so multi-arch builds (Apple Silicon vs Intel, ARM
// Windows) are distinguishable: e.g. "macos/aarch64", "windows/x86_64".
fn platform_string() -> String {
    format!("{}/{}", std::env::consts::OS, std::env::consts::ARCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collect_produces_a_serializable_snapshot_with_every_field() {
        let fp = ClientFingerprint::collect();
        let value = serde_json::to_value(&fp).expect("serializable");
        let obj = value.as_object().expect("object");

        for key in [
            "screen_width",
            "screen_height",
            "device_pixel_ratio",
            "color_depth",
            "hardware_concurrency",
            "timezone",
            "language",
            "platform",
        ] {
            assert!(obj.contains_key(key), "missing field: {key}");
        }
    }

    #[test]
    fn platform_string_includes_os_and_arch_separated_by_slash() {
        let platform = platform_string();
        let parts: Vec<&str> = platform.split('/').collect();
        assert_eq!(parts.len(), 2, "expected os/arch, got {platform}");
        assert!(!parts[0].is_empty());
        assert!(!parts[1].is_empty());
    }

    #[test]
    fn hardware_concurrency_returns_a_plausible_count() {
        // Guards against accidental overflow / wrap-around in the
        // `u32::try_from` path. Real machines won't realistically expose
        // thousands of logical cores.
        let n = hardware_concurrency();
        assert!(n < 4096, "implausibly high hardware concurrency: {n}");
    }
}
