use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

pub const SVC_PAIRING: &str = "_adb-tls-pairing._tcp";
pub const SVC_CONNECT: &str = "_adb-tls-connect._tcp";
pub const SVC_LEGACY: &str = "_adb._tcp";

pub const HW_SERIAL_PROP_SEP: &str = "__ANYLEAP_SEP__";
pub const MSG_NEEDS_REPAIR: &str = "This phone needs to be paired again";

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceTransports {
    pub usb: Option<String>,
    pub wifi: Option<String>,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Preferred active adb `-s` serial (USB if live, else Wi-Fi).
    pub serial: String,
    /// Stable hardware serial (`ro.serialno` / `ro.boot.serialno`, or parsed from mDNS).
    pub hw_serial: Option<String>,
    pub state: String,
    pub model: Option<String>,
    pub product: Option<String>,
    pub transports: DeviceTransports,
}

/// An adb mDNS service entry, as listed by `adb mdns services`.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MdnsService {
    pub name: String,
    pub service_type: String,
    pub host: String,
    pub port: u16,
    pub hw_serial: Option<String>,
}

#[derive(Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReconnectTarget {
    pub hw_serial: Option<String>,
    pub host: String,
    pub port: u16,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReconnectResult {
    /// "already_connected" | "reconnected" | "needs_repair"
    pub status: String,
    pub serial: Option<String>,
    pub hw_serial: Option<String>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReconnectDecision {
    AlreadyConnected {
        serial: String,
        hw_serial: Option<String>,
        host: Option<String>,
        port: Option<u16>,
    },
    ConnectMdns {
        host: String,
        port: u16,
        hw_serial: Option<String>,
        stale_serial: Option<String>,
    },
    TryStored {
        host: String,
        port: u16,
    },
    NeedsRepair {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MergeResult {
    pub devices: Vec<DeviceInfo>,
    pub dead_serials: Vec<String>,
}

pub fn is_valid_device_state(state: &str) -> bool {
    matches!(
        state,
        "device"
            | "unauthorized"
            | "offline"
            | "authorizing"
            | "connecting"
            | "no permissions"
            | "bootloader"
            | "recovery"
            | "sideload"
            | "host"
    )
}

pub fn is_wifi_serial(serial: &str) -> bool {
    split_wifi_serial(serial).is_some()
}

pub fn split_wifi_serial(serial: &str) -> Option<(String, u16)> {
    let (host, port_str) = serial.rsplit_once(':')?;
    if host.is_empty() {
        return None;
    }
    let port = port_str.parse::<u16>().ok()?;
    Some((host.to_string(), port))
}

pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim();
    h == "127.0.0.1" || h.eq_ignore_ascii_case("localhost") || h == "::1" || h == "[::1]"
}

/// Extract the hardware serial from an adb mDNS instance name.
///
/// Android Wireless Debugging advertises `_adb-tls-connect._tcp` and
/// `_adb-tls-pairing._tcp` with instance names shaped `adb-<serial>-<suffix>`
/// (e.g. `adb-140892554J008528-Exhc24`), and legacy `_adb._tcp` as `adb-<serial>`.
/// QR pairing challenges use `adbqr-<hex>` and return `None`.
pub fn parse_mdns_instance_serial(name: &str) -> Option<String> {
    let trimmed = name.trim();
    let rest = trimmed.strip_prefix("adb-")?;
    if rest.is_empty() {
        return None;
    }
    if let Some((serial, suffix)) = rest.rsplit_once('-') {
        if serial.is_empty() || suffix.is_empty() {
            return None;
        }
        Some(serial.to_string())
    } else {
        Some(rest.to_string())
    }
}

/// Resolve hardware serial from `ro.serialno` with fallback to `ro.boot.serialno`.
pub fn parse_hw_serial_props(ro_serialno: &str, ro_boot_serialno: &str) -> Option<String> {
    for candidate in [ro_serialno, ro_boot_serialno] {
        let trimmed = candidate.trim();
        if !trimmed.is_empty() && !trimmed.eq_ignore_ascii_case("unknown") {
            return Some(trimmed.to_string());
        }
    }
    None
}

/// Parse combined `getprop ro.serialno; echo __ANYLEAP_SEP__; getprop ro.boot.serialno` output.
pub fn parse_hw_serial_shell_output(output: &str) -> Option<String> {
    if let Some((first, second)) = output.split_once(HW_SERIAL_PROP_SEP) {
        parse_hw_serial_props(first, second)
    } else {
        parse_hw_serial_props(output, "")
    }
}

/// Parse `adb devices -l` output into structured device records.
///
/// Skips the header line and any `* daemon ...` startup noise, keeps non-`device`
/// states (e.g. `unauthorized`, `offline`) so the UI can guide the user.
pub fn parse_adb_devices(text: &str) -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty()
            || line.starts_with("List of devices")
            || line.starts_with('*')
            || line.starts_with("adb ")
            || line.starts_with("error:")
            || line.starts_with("cannot ")
        {
            continue;
        }
        let mut it = line.split_whitespace();
        let serial = match it.next() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let mut state = match it.next() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let rest: Vec<&str> = it.collect();
        // "no permissions" is the only multi-word state we expect.
        if state == "no" && rest.first() == Some(&"permissions") {
            state = "no permissions".to_string();
        }
        if !is_valid_device_state(&state) {
            continue;
        }
        let mut model = None;
        let mut product = None;
        for tok in &rest {
            if let Some(v) = tok.strip_prefix("model:") {
                model = Some(v.to_string());
            }
            if let Some(v) = tok.strip_prefix("product:") {
                product = Some(v.to_string());
            }
        }
        let wifi = is_wifi_serial(&serial);
        out.push(DeviceInfo {
            hw_serial: if wifi { None } else { Some(serial.clone()) },
            transports: DeviceTransports {
                usb: if wifi { None } else { Some(serial.clone()) },
                wifi: if wifi { Some(serial.clone()) } else { None },
            },
            serial,
            state,
            model,
            product,
        });
    }
    out
}

/// Parse `adb mdns services` output into structured entries.
///
/// Anchors on the known service-type token (instance names can contain spaces),
/// then reads the following token as `host:port`. Skips the header and any
/// `* daemon ...` noise.
pub fn parse_mdns_services(text: &str) -> Vec<MdnsService> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("List of discovered") || line.starts_with('*') {
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        let svc_idx = match toks
            .iter()
            .position(|t| matches!(*t, SVC_PAIRING | SVC_CONNECT | SVC_LEGACY))
        {
            Some(i) if i > 0 && i + 1 < toks.len() => i,
            _ => continue,
        };
        let name = toks[..svc_idx].join(" ");
        let service_type = toks[svc_idx].to_string();
        let (host, port_str) = match toks[svc_idx + 1].rsplit_once(':') {
            Some(hp) => hp,
            None => continue,
        };
        let port = match port_str.parse::<u16>() {
            Ok(p) => p,
            Err(_) => continue,
        };
        if host.is_empty() {
            continue;
        }
        let hw_serial = parse_mdns_instance_serial(&name);
        out.push(MdnsService {
            name,
            service_type,
            host: host.to_string(),
            port,
            hw_serial,
        });
    }
    out
}

fn mdns_hw_for_endpoint(mdns: &[MdnsService], serial: &str) -> Option<String> {
    let (host, port) = split_wifi_serial(serial)?;
    let mut best: Option<&MdnsService> = None;
    for svc in mdns {
        if svc.host == host && svc.port == port && svc.hw_serial.is_some() {
            match best {
                None => best = Some(svc),
                Some(prev) => {
                    if svc.service_type == SVC_CONNECT && prev.service_type != SVC_CONNECT {
                        best = Some(svc);
                    }
                }
            }
        }
    }
    best.and_then(|s| s.hw_serial.clone())
}

fn state_rank(state: &str) -> u8 {
    match state {
        "device" => 3,
        "unauthorized" | "authorizing" => 2,
        _ => 1,
    }
}

/// Merge raw `adb devices -l` entries by `hw_serial`, preferring live USB transport
/// when both USB and Wi-Fi are online, and identifying dead `IP:port` entries that
/// belong to a merged/live device.
pub fn merge_devices(
    raw: Vec<DeviceInfo>,
    hw_serials: &HashMap<String, String>,
    mdns: &[MdnsService],
) -> MergeResult {
    struct Enriched {
        info: DeviceInfo,
        hw: Option<String>,
        is_wifi: bool,
    }

    let mut enriched = Vec::with_capacity(raw.len());
    for mut dev in raw {
        let is_wifi = is_wifi_serial(&dev.serial);
        let hw = hw_serials.get(&dev.serial).cloned().or_else(|| {
            if is_wifi {
                mdns_hw_for_endpoint(mdns, &dev.serial)
            } else {
                Some(dev.serial.clone())
            }
        });
        dev.hw_serial = hw.clone();
        enriched.push(Enriched {
            info: dev,
            hw,
            is_wifi,
        });
    }

    // Track live Wi-Fi endpoints and active mDNS connect endpoints per non-loopback host
    // so stale `offline` IP:port entries on the same phone IP are detected and cleaned up.
    let mut live_wifi_ports_by_host: HashMap<String, HashSet<u16>> = HashMap::new();
    let mut live_hw_set: HashSet<String> = HashSet::new();
    for item in &enriched {
        if item.info.state == "device" {
            if let Some(ref hw) = item.hw {
                live_hw_set.insert(hw.clone());
            }
            if item.is_wifi {
                if let Some((host, port)) = split_wifi_serial(&item.info.serial) {
                    if !is_loopback_host(&host) {
                        live_wifi_ports_by_host
                            .entry(host)
                            .or_default()
                            .insert(port);
                    }
                }
            }
        }
    }

    let mut mdns_connect_ports_by_host: HashMap<String, HashSet<u16>> = HashMap::new();
    for svc in mdns {
        if svc.service_type == SVC_CONNECT && !is_loopback_host(&svc.host) {
            mdns_connect_ports_by_host
                .entry(svc.host.clone())
                .or_default()
                .insert(svc.port);
        }
    }

    let mut dead_serials = Vec::new();
    let mut group_order: Vec<String> = Vec::new();
    let mut groups: HashMap<String, Vec<Enriched>> = HashMap::new();

    for item in enriched {
        if item.is_wifi && item.info.state == "offline" {
            let dead_by_hw = item.hw.as_ref().is_some_and(|hw| live_hw_set.contains(hw));
            let dead_by_host = split_wifi_serial(&item.info.serial).is_some_and(|(host, port)| {
                !is_loopback_host(&host)
                    && (live_wifi_ports_by_host
                        .get(&host)
                        .is_some_and(|ports| !ports.contains(&port))
                        || mdns_connect_ports_by_host
                            .get(&host)
                            .is_some_and(|ports| !ports.contains(&port)))
            });
            if dead_by_hw || dead_by_host {
                dead_serials.push(item.info.serial);
                continue;
            }
        }

        let key = item.hw.clone().unwrap_or_else(|| item.info.serial.clone());
        if !groups.contains_key(&key) {
            group_order.push(key.clone());
        }
        groups.entry(key).or_default().push(item);
    }

    let mut devices = Vec::with_capacity(group_order.len());
    for key in group_order {
        let Some(items) = groups.remove(&key) else {
            continue;
        };

        let mut usb_items: Vec<DeviceInfo> = Vec::new();
        let mut wifi_items: Vec<DeviceInfo> = Vec::new();
        let mut resolved_hw: Option<String> = None;

        for item in items {
            if resolved_hw.is_none() {
                resolved_hw = item.hw;
            }
            if item.is_wifi {
                wifi_items.push(item.info);
            } else {
                usb_items.push(item.info);
            }
        }

        usb_items.sort_by_key(|b| std::cmp::Reverse(state_rank(&b.state)));
        wifi_items.sort_by_key(|b| std::cmp::Reverse(state_rank(&b.state)));

        let best_usb = usb_items.first().cloned();
        let best_wifi = wifi_items.first().cloned();

        // Any extra offline Wi-Fi entries in the same hw_serial group are dead.
        for extra in wifi_items.into_iter().skip(1) {
            if extra.state == "offline" {
                dead_serials.push(extra.serial);
            }
        }

        let usb_live = best_usb.as_ref().is_some_and(|d| d.state == "device");
        let wifi_live = best_wifi.as_ref().is_some_and(|d| d.state == "device");

        let primary = match (best_usb.as_ref(), best_wifi.as_ref()) {
            (Some(u), Some(_)) if usb_live => u.clone(),
            (Some(_), Some(w)) if wifi_live => w.clone(),
            (Some(u), _) => u.clone(),
            (None, Some(w)) => w.clone(),
            (None, None) => continue,
        };

        let model = best_usb
            .as_ref()
            .and_then(|u| u.model.clone())
            .or_else(|| best_wifi.as_ref().and_then(|w| w.model.clone()));
        let product = best_usb
            .as_ref()
            .and_then(|u| u.product.clone())
            .or_else(|| best_wifi.as_ref().and_then(|w| w.product.clone()));

        devices.push(DeviceInfo {
            serial: primary.serial,
            hw_serial: resolved_hw,
            state: primary.state,
            model,
            product,
            transports: DeviceTransports {
                usb: best_usb.map(|u| u.serial),
                wifi: best_wifi.map(|w| w.serial),
            },
        });
    }

    MergeResult {
        devices,
        dead_serials,
    }
}

pub fn is_auth_connect_error(msg: &str) -> bool {
    let low = msg.to_ascii_lowercase();
    low.contains("failed to authenticate")
        || low.contains("unauthorized")
        || low.contains("authorizing")
        || low.contains("not paired")
        || low.contains("rejected")
}

/// Find the best `_adb-tls-connect._tcp` mDNS entry matching a target device.
pub fn find_mdns_connect_match<'a>(
    target: &ReconnectTarget,
    mdns: &'a [MdnsService],
) -> Option<&'a MdnsService> {
    if let Some(ref hw) = target.hw_serial {
        if let Some(svc) = mdns
            .iter()
            .find(|s| s.service_type == SVC_CONNECT && s.hw_serial.as_deref() == Some(hw.as_str()))
        {
            return Some(svc);
        }
    }
    if target.hw_serial.is_none() && !target.host.is_empty() {
        if let Some(svc) = mdns
            .iter()
            .find(|s| s.service_type == SVC_CONNECT && s.host == target.host)
        {
            return Some(svc);
        }
    }
    None
}

/// Decide how to reconnect a saved Wi-Fi device given current live devices and mDNS.
pub fn decide_reconnect(
    target: &ReconnectTarget,
    live_devices: &[DeviceInfo],
    mdns: &[MdnsService],
) -> ReconnectDecision {
    let stored_serial = if !target.host.is_empty() && target.port > 0 {
        Some(format!("{}:{}", target.host, target.port))
    } else {
        None
    };

    let matched_live = live_devices.iter().find(|d| {
        if let (Some(thw), Some(dhw)) = (target.hw_serial.as_deref(), d.hw_serial.as_deref()) {
            if thw == dhw {
                return true;
            }
        }
        if let Some(ref ss) = stored_serial {
            if d.transports.wifi.as_deref() == Some(ss.as_str()) || d.serial == *ss {
                return true;
            }
        }
        if !target.host.is_empty() && !is_loopback_host(&target.host) {
            let prefix = format!("{}:", target.host);
            if d.transports
                .wifi
                .as_deref()
                .is_some_and(|w| w.starts_with(&prefix))
            {
                return true;
            }
        }
        false
    });

    if let Some(dev) = matched_live {
        if dev.state == "device" {
            if let Some(ref wifi) = dev.transports.wifi {
                if let Some((h, p)) = split_wifi_serial(wifi) {
                    return ReconnectDecision::AlreadyConnected {
                        serial: dev.serial.clone(),
                        hw_serial: dev.hw_serial.clone(),
                        host: Some(h),
                        port: Some(p),
                    };
                }
            }
        } else if dev.state == "unauthorized" && dev.transports.usb.is_none() {
            return ReconnectDecision::NeedsRepair {
                message: MSG_NEEDS_REPAIR.to_string(),
            };
        }
    }

    if let Some(svc) = find_mdns_connect_match(target, mdns) {
        let new_ep = format!("{}:{}", svc.host, svc.port);
        let stale_serial = stored_serial.filter(|s| s != &new_ep);
        return ReconnectDecision::ConnectMdns {
            host: svc.host.clone(),
            port: svc.port,
            hw_serial: svc.hw_serial.clone().or_else(|| target.hw_serial.clone()),
            stale_serial,
        };
    }

    if !target.host.is_empty() && target.port > 0 {
        return ReconnectDecision::TryStored {
            host: target.host.clone(),
            port: target.port,
        };
    }

    ReconnectDecision::NeedsRepair {
        message: MSG_NEEDS_REPAIR.to_string(),
    }
}

/// Convert a connect attempt outcome into the IPC `ReconnectResult`.
pub fn finalize_reconnect_result(
    connect_outcome: Result<(), String>,
    host: String,
    port: u16,
    hw_serial: Option<String>,
) -> ReconnectResult {
    match connect_outcome {
        Ok(()) => ReconnectResult {
            status: "reconnected".to_string(),
            serial: Some(format!("{host}:{port}")),
            hw_serial,
            host: Some(host),
            port: Some(port),
            message: None,
        },
        Err(err) => {
            let _ = is_auth_connect_error(&err);
            ReconnectResult {
                status: "needs_repair".to_string(),
                serial: None,
                hw_serial,
                host: Some(host),
                port: Some(port),
                message: Some(MSG_NEEDS_REPAIR.to_string()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mdns_instance_serial_variants() {
        assert_eq!(
            parse_mdns_instance_serial("adb-140892554J008528-Exhc24").as_deref(),
            Some("140892554J008528")
        );
        assert_eq!(
            parse_mdns_instance_serial("adb-ABC123-TnSdi9").as_deref(),
            Some("ABC123")
        );
        assert_eq!(
            parse_mdns_instance_serial("adb-MY-SER-123-QxJ99a").as_deref(),
            Some("MY-SER-123")
        );
        assert_eq!(
            parse_mdns_instance_serial("adb-ABC123").as_deref(),
            Some("ABC123")
        );
        assert_eq!(parse_mdns_instance_serial("adbqr-1a2b3c4d"), None);
        assert_eq!(parse_mdns_instance_serial("my phone"), None);
        assert_eq!(parse_mdns_instance_serial("adb-"), None);
        assert_eq!(parse_mdns_instance_serial("adb--Exhc24"), None);
        assert_eq!(parse_mdns_instance_serial("adb-ABC123-"), None);
    }

    #[test]
    fn parses_mdns_services_with_hw_serial_host_port() {
        let text = "\
List of discovered mdns services
adb-140892554J008528-Exhc24\t_adb-tls-connect._tcp\t192.168.0.175:46391
adb-140892554J008528-QXjCrW\t_adb-tls-pairing._tcp\t192.168.0.175:33861
adb-LEGACY99\t_adb._tcp\t192.168.0.180:5555
";
        let svcs = parse_mdns_services(text);
        assert_eq!(svcs.len(), 3);
        assert_eq!(svcs[0].hw_serial.as_deref(), Some("140892554J008528"));
        assert_eq!(svcs[0].host, "192.168.0.175");
        assert_eq!(svcs[0].port, 46391);
        assert_eq!(svcs[0].service_type, SVC_CONNECT);
        assert_eq!(svcs[1].hw_serial.as_deref(), Some("140892554J008528"));
        assert_eq!(svcs[2].hw_serial.as_deref(), Some("LEGACY99"));
    }

    #[test]
    fn parses_hw_serial_with_ro_boot_serialno_fallback() {
        assert_eq!(
            parse_hw_serial_props("140892554J008528\n", "fallback").as_deref(),
            Some("140892554J008528")
        );
        assert_eq!(
            parse_hw_serial_props("", "BOOTSER123\r\n").as_deref(),
            Some("BOOTSER123")
        );
        assert_eq!(
            parse_hw_serial_props("unknown\n", "BOOTSER123\n").as_deref(),
            Some("BOOTSER123")
        );
        assert_eq!(parse_hw_serial_props("", "unknown"), None);
        assert_eq!(
            parse_hw_serial_shell_output("\n__ANYLEAP_SEP__\n140892554J008528\n").as_deref(),
            Some("140892554J008528")
        );
    }

    #[test]
    fn merges_mixed_usb_wifi_and_dead_port_duplicates() {
        let adb_out = "\
List of devices attached
140892554J008528       device product:panther model:Pixel_7 device:panther transport_id:1
192.168.0.175:46391    device product:panther model:Pixel_7 device:panther transport_id:2
192.168.0.175:40921    offline transport_id:3
USBONLY999             unauthorized transport_id:4
192.168.0.200:38111    offline transport_id:5
";
        let raw = parse_adb_devices(adb_out);
        assert_eq!(raw.len(), 5);

        let mdns = parse_mdns_services(
            "adb-140892554J008528-Exhc24\t_adb-tls-connect._tcp\t192.168.0.175:46391\n\
             adb-OFFLINEHW88-AbCdEf\t_adb-tls-connect._tcp\t192.168.0.200:38111\n",
        );

        let mut hw_map = HashMap::new();
        hw_map.insert(
            "140892554J008528".to_string(),
            "140892554J008528".to_string(),
        );
        hw_map.insert(
            "192.168.0.175:46391".to_string(),
            "140892554J008528".to_string(),
        );

        let merged = merge_devices(raw, &hw_map, &mdns);
        assert_eq!(merged.dead_serials, vec!["192.168.0.175:40921"]);
        assert_eq!(merged.devices.len(), 3);

        // Merged USB + Wi-Fi prefers USB transport when both are live.
        let pixel = &merged.devices[0];
        assert_eq!(pixel.serial, "140892554J008528");
        assert_eq!(pixel.hw_serial.as_deref(), Some("140892554J008528"));
        assert_eq!(pixel.state, "device");
        assert_eq!(pixel.transports.usb.as_deref(), Some("140892554J008528"));
        assert_eq!(
            pixel.transports.wifi.as_deref(),
            Some("192.168.0.175:46391")
        );

        // Unauthorized USB keeps USB serial as hw_serial.
        let usb_unauth = &merged.devices[1];
        assert_eq!(usb_unauth.serial, "USBONLY999");
        assert_eq!(usb_unauth.hw_serial.as_deref(), Some("USBONLY999"));
        assert_eq!(usb_unauth.state, "unauthorized");

        // Offline Wi-Fi matching mDNS resolves hw_serial from mDNS instance name.
        let offline_wifi = &merged.devices[2];
        assert_eq!(offline_wifi.serial, "192.168.0.200:38111");
        assert_eq!(offline_wifi.hw_serial.as_deref(), Some("OFFLINEHW88"));
        assert_eq!(offline_wifi.state, "offline");
    }

    #[test]
    fn merge_prefers_live_wifi_when_usb_is_unauthorized() {
        let raw = parse_adb_devices(
            "140892554J008528 unauthorized\n192.168.0.175:46391 device model:Pixel_7\n",
        );
        let mut hw_map = HashMap::new();
        hw_map.insert(
            "192.168.0.175:46391".to_string(),
            "140892554J008528".to_string(),
        );
        let merged = merge_devices(raw, &hw_map, &[]);
        assert_eq!(merged.devices.len(), 1);
        assert_eq!(merged.devices[0].serial, "192.168.0.175:46391");
        assert_eq!(merged.devices[0].state, "device");
        assert_eq!(
            merged.devices[0].transports.usb.as_deref(),
            Some("140892554J008528")
        );
        assert_eq!(
            merged.devices[0].transports.wifi.as_deref(),
            Some("192.168.0.175:46391")
        );
    }

    #[test]
    fn reconnect_decision_stale_port_with_mdns_match_reconnects_without_repair() {
        let target = ReconnectTarget {
            hw_serial: Some("140892554J008528".into()),
            host: "192.168.0.175".into(),
            port: 40921,
        };
        let mdns = parse_mdns_services(
            "adb-140892554J008528-Exhc24\t_adb-tls-connect._tcp\t192.168.0.175:46391\n",
        );
        let decision = decide_reconnect(&target, &[], &mdns);
        assert_eq!(
            decision,
            ReconnectDecision::ConnectMdns {
                host: "192.168.0.175".into(),
                port: 46391,
                hw_serial: Some("140892554J008528".into()),
                stale_serial: Some("192.168.0.175:40921".into()),
            }
        );

        let res = finalize_reconnect_result(
            Ok(()),
            "192.168.0.175".into(),
            46391,
            Some("140892554J008528".into()),
        );
        assert_eq!(res.status, "reconnected");
        assert_eq!(res.serial.as_deref(), Some("192.168.0.175:46391"));
        assert_eq!(res.message, None);
    }

    #[test]
    fn reconnect_decision_matches_hw_serial_even_when_ip_changed() {
        let target = ReconnectTarget {
            hw_serial: Some("140892554J008528".into()),
            host: "192.168.0.175".into(),
            port: 40921,
        };
        let mdns = parse_mdns_services(
            "adb-140892554J008528-New999\t_adb-tls-connect._tcp\t192.168.1.88:41234\n",
        );
        let decision = decide_reconnect(&target, &[], &mdns);
        assert_eq!(
            decision,
            ReconnectDecision::ConnectMdns {
                host: "192.168.1.88".into(),
                port: 41234,
                hw_serial: Some("140892554J008528".into()),
                stale_serial: Some("192.168.0.175:40921".into()),
            }
        );
    }

    #[test]
    fn reconnect_decision_no_mdns_match_and_connect_failure_requests_repair() {
        let target = ReconnectTarget {
            hw_serial: Some("140892554J008528".into()),
            host: "192.168.0.175".into(),
            port: 40921,
        };
        let decision = decide_reconnect(&target, &[], &[]);
        assert_eq!(
            decision,
            ReconnectDecision::TryStored {
                host: "192.168.0.175".into(),
                port: 40921,
            }
        );

        let res = finalize_reconnect_result(
            Err("cannot connect to 192.168.0.175:40921: Connection refused".into()),
            "192.168.0.175".into(),
            40921,
            Some("140892554J008528".into()),
        );
        assert_eq!(res.status, "needs_repair");
        assert_eq!(res.message.as_deref(), Some(MSG_NEEDS_REPAIR));
    }

    #[test]
    fn reconnect_decision_auth_error_requests_repair() {
        let res = finalize_reconnect_result(
            Err("failed to authenticate to 192.168.0.175:46391".into()),
            "192.168.0.175".into(),
            46391,
            Some("140892554J008528".into()),
        );
        assert_eq!(res.status, "needs_repair");
        assert_eq!(res.message.as_deref(), Some(MSG_NEEDS_REPAIR));
    }
}
