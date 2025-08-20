use std::collections::HashSet;

use serde::Serialize;
use serde_json::{Map, Value, json};

use get_if_addrs::get_if_addrs;

#[derive(Default, Serialize)]
struct NetworkInfo {
    cellular: bool,
    wifi: bool,
}

pub fn network_context() -> Value {
    let info: NetworkInfo = network_context_internal();
    let internals = serde_json::to_value(&info).unwrap_or_else(|e| {
        log::error!("Cannot serialize network info, fallback to empty: {}", e);
        json!("{}")
    });
    let mut map = Map::new();
    map.insert("network".to_owned(), internals);
    Value::Object(map)
}

#[cfg(target_os = "macos")]
fn network_context_internal() -> NetworkInfo {
    use system_configuration::network_configuration::get_interfaces;

    // Collect active (non-loopback, non-link-local v4) interface BSD names
    let active_ifaces: HashSet<String> = match get_if_addrs() {
        Ok(addrs) => addrs
            .into_iter()
            .filter(|iface| {
                if iface.is_loopback() {
                    return false;
                }
                match iface.ip() {
                    std::net::IpAddr::V4(ip) => !ip.is_link_local(), // skip 169.254/16
                    std::net::IpAddr::V6(ip) => !ip.is_loopback(),   // keep non-loopback v6
                }
            })
            .map(|iface| iface.name)
            .collect(),
        Err(_) => return NetworkInfo::default(),
    };

    // Inspect SystemConfiguration interfaces and mark types for active interfaces
    let ifaces = get_interfaces();
    let mut info = NetworkInfo::default();
    for iface in ifaces.iter() {
        let Some(bsd) = iface.bsd_name().map(|s| s.to_string()) else {
            continue;
        };
        if !active_ifaces.contains(&bsd) {
            continue;
        }

        // Prefer the typed API if available; otherwise fall back to the type string
        // (The exact enum names can vary by crate version; the string fallback is robust)
        let kind_str = iface
            .interface_type_string()
            .map(|s| s.to_string())
            .unwrap_or_default();

        // Common macOS identifiers
        // Wi-Fi: "IEEE80211", sometimes shown as "AirPort" or "Wi-Fi"
        // Cellular: "WWAN", sometimes "Cellular"
        let is_wifi = kind_str.contains("IEEE80211")
            || kind_str.contains("Wi-Fi")
            || kind_str.contains("AirPort");
        let is_cell = kind_str.contains("WWAN") || kind_str.contains("Cellular");

        if is_wifi {
            info.wifi = true;
        }
        if is_cell {
            info.cellular = true;
        }

        if info.wifi && info.cellular {
            break;
        }
    }

    info
}

#[cfg(target_os = "windows")]
fn network_context_internal() -> Value {
    let mut available_network_types: HashSet<String> = HashSet::new();

    if let Ok(addrs) = get_if_addrs() {
        for iface in addrs {
            if iface.is_loopback() {
                continue;
            }

            let ip = iface.ip();
            let is_link_local = match ip {
                std::net::IpAddr::V4(ipv4) => ipv4.is_link_local(),
                std::net::IpAddr::V6(ipv6) => ipv6.is_loopback(),
            };
            if is_link_local {
                continue;
            }

            // Windows interface names can be long and friendly:
            // e.g. "Ethernet", "Wi-Fi", "vEthernet (WSL)"
            let name = iface.name;
            let lower_name = name.to_lowercase();

            let kind = if lower_name.contains("wifi")
                || lower_name.contains("wi-fi")
                || lower_name.contains("wlan")
            {
                "Wi-Fi"
            } else if lower_name.contains("ethernet") {
                "Ethernet"
            } else if lower_name.contains("ppp") {
                "Mobile"
            } else {
                "Unknown"
            };

            available_network_types.insert(format!("{name} - {kind}"));
        }

        let values: Vec<Value> = available_network_types
            .into_iter()
            .map(Value::String)
            .collect();
        Value::Array(values)
    } else {
        Value::Array(Vec::new())
    }
}
