use std::collections::HashSet;

use serde_json::{Map, Value};

use get_if_addrs::get_if_addrs;

struct NetworkInfo {
    cellular: bool,
    wifi: bool
}

pub fn network_context() -> Value {
    let internals = network_context_internal();
    let mut map = Map::new();
    map.insert("network".to_owned(), internals);
    Value::Object(map)
}

#[cfg(target_os = "macos")]
fn network_context_internal() -> Value {
    use system_configuration::network_configuration::get_interfaces;

    let mut available_network_types: HashSet<String> = HashSet::new();

    if let Ok(addrs) = get_if_addrs() {
        // Active
        let active_ifaces: HashSet<String> = addrs
            .into_iter()
            .filter(|iface| {
                // Skip loopbacks
                if iface.is_loopback() {
                    return false;
                }
                // Skip link-local
                match iface.ip() {
                    std::net::IpAddr::V4(ip) => !ip.is_link_local(),
                    std::net::IpAddr::V6(ip) => !ip.is_loopback(),
                }
            })
            .map(|iface| iface.name)
            .collect();

        // Interfaces
        let ifaces = get_interfaces();
        for iface in ifaces.iter() {
            if let Some(name) = iface.bsd_name() {
                let name = name.to_string();
                if active_ifaces.contains(&name) {
                    let display_name = iface
                        .display_name()
                        .map(|e| e.to_string())
                        .unwrap_or_default();
                    let kind = iface
                        .interface_type_string()
                        .map(|e| e.to_string())
                        .unwrap_or_default();
                    available_network_types.insert(format!("{display_name} - {kind}"));
                }
            }
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
