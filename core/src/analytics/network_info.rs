use serde::Serialize;
use serde_json::{Map, Value, json};

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
    use get_if_addrs::get_if_addrs;
    use std::collections::HashSet;
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

#[allow(unsafe_code)]
#[cfg(target_os = "windows")]
fn network_context_internal() -> NetworkInfo {
    use windows::Win32::NetworkManagement::IpHelper::{
        GAA_FLAG_SKIP_ANYCAST, GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_FRIENDLY_NAME,
        GAA_FLAG_SKIP_MULTICAST, IF_TYPE_IEEE80211, IF_TYPE_WWANPP, IF_TYPE_WWANPP2,
        IP_ADAPTER_ADDRESSES_LH,
    };
    use windows::Win32::Networking::WinSock::{
        AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6,
    };

    // check if a sockaddr is usable (not loopback/link-local)
    fn addr_is_usable(sa: *const SOCKADDR) -> bool {
        if sa.is_null() {
            return false;
        }
        unsafe {
            match (*sa).sa_family {
                AF_INET => {
                    let v4 = *(sa as *const SOCKADDR_IN);
                    let octets = v4.sin_addr.S_un.S_addr.to_ne_bytes(); // IPv4 in LE
                    // Convert to standard order
                    let ip = [octets[0], octets[1], octets[2], octets[3]];
                    // 127.0.0.0/8 loopback, 169.254.0.0/16 link-local
                    !(ip[0] == 127 || (ip[0] == 169 && ip[1] == 254))
                }
                AF_INET6 => {
                    let v6 = *(sa as *const SOCKADDR_IN6);
                    let ip = v6.sin6_addr.u.Byte;
                    // ::1 loopback
                    let is_loopback = ip.iter().take(15).all(|&b| b == 0) && ip[15] == 1;
                    // fe80::/10 link-local -> first 10 bits 1111 1110 10xx xxxx
                    let is_link_local = ip[0] == 0xfe && (ip[1] & 0xc0) == 0x80;
                    !(is_loopback || is_link_local)
                }
                _ => false,
            }
        }
    }

    // consider an adapter "active" if
    // operational status is up
    // it has at least one usable (non-loopback, non-link-local) unicast address
    fn adapter_is_active(aa: *const IP_ADAPTER_ADDRESSES_LH) -> bool {
        unsafe {
            use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;

            if aa.is_null() {
                return false;
            }
            if (*aa).OperStatus != IfOperStatusUp {
                return false;
            }
            let mut ua = (*aa).FirstUnicastAddress;
            while !ua.is_null() {
                let sa = (*ua).Address.lpSockaddr;
                if addr_is_usable(sa) {
                    return true;
                }
                ua = (*ua).Next;
            }
            false
        }
    }

    unsafe {
        use windows::Win32::NetworkManagement::IpHelper::GetAdaptersAddresses;

        // call to get required buffer size
        let mut size: u32 = 0;
        let flags = GAA_FLAG_SKIP_ANYCAST
            | GAA_FLAG_SKIP_MULTICAST
            | GAA_FLAG_SKIP_DNS_SERVER
            | GAA_FLAG_SKIP_FRIENDLY_NAME;
        let mut ret = GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, None, &mut size);

        if ret != 0 && size == 0 {
            // unable to query
            return NetworkInfo::default();
        }

        // allocate buffer and fetch
        let mut buf: Vec<u8> = vec![0u8; size as usize];
        let aa_head = buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH;
        ret = GetAdaptersAddresses(AF_UNSPEC.0 as u32, flags, None, Some(aa_head), &mut size);
        if ret != 0 {
            return NetworkInfo::default();
        }

        // iterate linked list
        let mut cur = aa_head as *const IP_ADAPTER_ADDRESSES_LH;
        let mut info = NetworkInfo::default();
        while !cur.is_null() {
            if adapter_is_active(cur) {
                let if_type = (*cur).IfType;
                // wifi
                if if_type == IF_TYPE_IEEE80211 {
                    info.wifi = true;
                }
                // cellular (WWAN)
                if if_type == IF_TYPE_WWANPP || if_type == IF_TYPE_WWANPP2 {
                    info.cellular = true;
                }

                if info.wifi && info.cellular {
                    break;
                }
            }
            cur = (*cur).Next;
        }
        info
    }
}
