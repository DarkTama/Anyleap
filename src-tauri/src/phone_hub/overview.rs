use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct OverviewBattery {
    pub level: Option<u32>,
    pub charging: Option<bool>,
    pub temp_c: Option<f32>,
    pub health: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BytesUsage {
    pub used_bytes: u64,
    pub total_bytes: u64,
}

pub type Bytes2 = BytesUsage;

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WifiInfo {
    pub ssid: Option<String>,
    pub signal_level: Option<u8>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CellInfo {
    pub operator: Option<String>,
    pub signal_level: Option<u8>,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ScreenState {
    On,
    Off,
    Locked,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StaticInfo {
    pub model: Option<String>,
    pub brand: Option<String>,
    pub android_version: Option<String>,
    pub security_patch: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PhoneOverview {
    pub model: Option<String>,
    pub brand: Option<String>,
    pub android_version: Option<String>,
    pub security_patch: Option<String>,
    pub battery: Option<OverviewBattery>,
    pub storage: Option<BytesUsage>,
    pub ram: Option<BytesUsage>,
    pub wifi: Option<WifiInfo>,
    pub cellular: Option<CellInfo>,
    pub screen: Option<ScreenState>,
    pub uptime_secs: Option<u64>,
    pub updated_at_ms: i64,
}

pub fn parse_getprop(output: &str) -> StaticInfo {
    let mut model = None;
    let mut fallback_model = None;
    let mut brand = None;
    let mut manufacturer = None;
    let mut android_version = None;
    let mut security_patch = None;

    for raw_line in output.lines() {
        let line = raw_line.trim();
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some((key, val_part)) = rest.split_once("]: [") else {
            continue;
        };
        let Some(val) = val_part.strip_suffix(']') else {
            continue;
        };
        let v = val.trim();
        if v.is_empty() {
            continue;
        }
        match key {
            "ro.product.model" => model = Some(v.to_string()),
            "ro.product.vendor.model" | "ro.product.odm.model" => {
                if fallback_model.is_none() {
                    fallback_model = Some(v.to_string());
                }
            }
            "ro.product.brand" | "ro.product.vendor.brand" => {
                if brand.is_none() || key == "ro.product.brand" {
                    brand = Some(v.to_string());
                }
            }
            "ro.product.manufacturer" => manufacturer = Some(v.to_string()),
            "ro.build.version.release" => android_version = Some(v.to_string()),
            "ro.build.version.security_patch" => security_patch = Some(v.to_string()),
            _ => {}
        }
    }

    StaticInfo {
        model: model.or(fallback_model),
        brand: brand.or(manufacturer),
        android_version,
        security_patch,
    }
}

pub fn parse_df_data(output: &str) -> Option<BytesUsage> {
    let mut tokens: Vec<&str> = Vec::new();
    for raw in output.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("Filesystem") || line.starts_with("df:") {
            continue;
        }
        tokens.extend(line.split_whitespace());
    }

    // Look for `<fs> <1k-blocks> <used> <avail> <pct%> <mount>` windows in token stream.
    for i in 0..tokens.len() {
        if !tokens[i].ends_with('%') {
            continue;
        }
        if i < 3 {
            continue;
        }
        let Ok(total_kb) = tokens[i - 3].parse::<u64>() else {
            continue;
        };
        let Ok(used_kb) = tokens[i - 2].parse::<u64>() else {
            continue;
        };
        let Ok(_avail_kb) = tokens[i - 1].parse::<u64>() else {
            continue;
        };
        if total_kb == 0 {
            continue;
        }
        let mount = tokens.get(i + 1).copied().unwrap_or("");
        if mount == "/data" || mount.starts_with("/data") || tokens.len() <= 7 {
            return Some(BytesUsage {
                used_bytes: used_kb.saturating_mul(1024),
                total_bytes: total_kb.saturating_mul(1024),
            });
        }
    }
    None
}

pub fn parse_meminfo(output: &str) -> Option<BytesUsage> {
    let mut total_kb: Option<u64> = None;
    let mut avail_kb: Option<u64> = None;
    let mut free_kb: Option<u64> = None;
    let mut buffers_kb: u64 = 0;
    let mut cached_kb: u64 = 0;

    for raw in output.lines() {
        let line = raw.trim();
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let Some(num_tok) = rest.split_whitespace().next() else {
            continue;
        };
        let Ok(val) = num_tok.parse::<u64>() else {
            continue;
        };
        match key.trim() {
            "MemTotal" => total_kb = Some(val),
            "MemAvailable" => avail_kb = Some(val),
            "MemFree" => free_kb = Some(val),
            "Buffers" => buffers_kb = val,
            "Cached" => cached_kb = val,
            _ => {}
        }
    }

    let total = total_kb?;
    if total == 0 {
        return None;
    }
    let avail = avail_kb.or_else(|| {
        free_kb.map(|f| {
            f.saturating_add(buffers_kb)
                .saturating_add(cached_kb)
                .min(total)
        })
    })?;
    let used = total.saturating_sub(avail);
    Some(BytesUsage {
        used_bytes: used.saturating_mul(1024),
        total_bytes: total.saturating_mul(1024),
    })
}

fn rssi_to_signal_level(rssi: i32) -> u8 {
    if rssi >= -55 {
        4
    } else if rssi >= -66 {
        3
    } else if rssi >= -77 {
        2
    } else if rssi >= -88 {
        1
    } else {
        0
    }
}

fn clean_ssid(raw: &str) -> Option<String> {
    let mut s = raw.trim();
    if let Some(stripped) = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')) {
        s = stripped;
    }
    if s.is_empty()
        || s.eq_ignore_ascii_case("<unknown ssid>")
        || s.eq_ignore_ascii_case("<none>")
        || s.eq_ignore_ascii_case("null")
        || s == "0x"
    {
        None
    } else {
        Some(s.to_string())
    }
}

pub fn parse_wifi(output: &str) -> Option<WifiInfo> {
    let lower = output.to_ascii_lowercase();
    if lower.contains("wi-fi is disabled") || lower.contains("wifi is disabled") {
        return None;
    }

    let mut ssid: Option<String> = None;
    let mut saw_ssid_field = false;
    let mut rssi: Option<i32> = None;
    let mut connected =
        lower.contains("supplicant state: completed") || lower.contains("connected to \"");

    for raw in output.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("Connected to ") {
            connected = true;
            if let Some(cleaned) = clean_ssid(rest) {
                ssid = Some(cleaned);
            }
        }
        if let Some(idx) = line.find("SSID:") {
            saw_ssid_field = true;
            let after = line[idx + 5..].trim_start();
            let raw_val = if let Some(quoted) = after.strip_prefix('"') {
                if let Some(end_q) = quoted.find('"') {
                    &after[..end_q + 2]
                } else {
                    after
                }
            } else {
                after.split(',').next().unwrap_or(after)
            };
            if let Some(cleaned) = clean_ssid(raw_val) {
                ssid = Some(cleaned);
            } else {
                ssid = None;
            }
        }
        if let Some(idx) = line.find("RSSI:") {
            let after = line[idx + 5..].trim_start();
            let num_tok = after
                .split(|c: char| c == ',' || c.is_whitespace())
                .next()
                .unwrap_or("");
            if let Ok(val) = num_tok.parse::<i32>() {
                // -127 is Android's INVALID_RSSI sentinel.
                if val > -127 && val <= 0 {
                    rssi = Some(val);
                }
            }
        }
    }

    if !connected && !saw_ssid_field && rssi.is_none() {
        return None;
    }
    if lower.contains("supplicant state: disconnected") && rssi.is_none() {
        return None;
    }

    Some(WifiInfo {
        ssid,
        signal_level: rssi.map(rssi_to_signal_level),
    })
}

fn extract_telephony_field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let idx = line.find(key)?;
    let rest = &line[idx + key.len()..];
    let val = rest.split([',', '}']).next()?.trim();
    if val.is_empty() || val.eq_ignore_ascii_case("null") {
        None
    } else {
        Some(val)
    }
}

pub fn parse_telephony(output: &str) -> Option<CellInfo> {
    let mut operator: Option<String> = None;
    let mut signal_level: Option<u8> = None;
    let mut primary_tech: Option<String> = None;

    for raw in output.lines() {
        let line = raw.trim();
        if operator.is_none() {
            for key in [
                "mOperatorAlphaLong=",
                "operatorAlphaLong=",
                "mVoiceOperatorAlphaLong=",
                "mDataOperatorAlphaLong=",
            ] {
                if let Some(val) = extract_telephony_field(line, key) {
                    operator = Some(val.to_string());
                    break;
                }
            }
        }

        if let Some(p) = extract_telephony_field(line, "primary=") {
            primary_tech = Some(p.to_string());
        }

        if line.contains("mSignalStrength") || line.contains("SignalStrength:") {
            if let Some(ref tech) = primary_tech {
                if let Some(tech_idx) = line.find(&format!("{tech}:")) {
                    if let Some(lvl_str) = extract_telephony_field(&line[tech_idx..], "level=") {
                        if let Ok(lvl) = lvl_str
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .parse::<u8>()
                        {
                            signal_level = Some(lvl.min(4));
                        }
                    }
                }
            }
            if signal_level.is_none() {
                // Scan all `level=N` occurrences; prefer the highest valid 0..=4 level.
                let mut rest = line;
                let mut best: Option<u8> = None;
                while let Some(idx) = rest.find("level=") {
                    let after = &rest[idx + 6..];
                    let tok = after
                        .split(|c: char| !c.is_ascii_digit())
                        .next()
                        .unwrap_or("");
                    if let Ok(lvl) = tok.parse::<u8>() {
                        let clamped = lvl.min(4);
                        best = Some(best.map_or(clamped, |prev| prev.max(clamped)));
                    }
                    rest = after;
                }
                signal_level = best;
            }
        }
    }

    if operator.is_none() && signal_level.is_none() {
        None
    } else {
        Some(CellInfo {
            operator,
            signal_level,
        })
    }
}

pub fn parse_screen(power: &str, window: &str) -> Option<ScreenState> {
    let mut awake: Option<bool> = None;
    for raw in power.lines() {
        let line = raw.trim();
        if let Some(val) = line.strip_prefix("mWakefulness=") {
            let v = val.trim();
            if v.eq_ignore_ascii_case("Awake") {
                awake = Some(true);
            } else if v.eq_ignore_ascii_case("Asleep") || v.eq_ignore_ascii_case("Dozing") {
                awake = Some(false);
            }
        } else if line.contains("Display Power: state=OFF") {
            awake = Some(false);
        } else if line.contains("Display Power: state=ON") && awake.is_none() {
            awake = Some(true);
        }
    }

    match awake {
        Some(false) => Some(ScreenState::Off),
        Some(true) => {
            let combined = format!("{power}\n{window}");
            let locked = combined.contains("mDreamingLockscreen=true")
                || combined.contains("mShowingLockscreen=true")
                || combined.contains("isStatusBarKeyguard=true")
                || combined.contains("mKeyguardShowing=true")
                || combined.contains("KeyguardStateMonitor: mIsShowing=true");
            if locked {
                Some(ScreenState::Locked)
            } else {
                Some(ScreenState::On)
            }
        }
        None => None,
    }
}

pub fn parse_uptime(output: &str) -> Option<u64> {
    let tok = output.split_whitespace().next()?;
    let secs = tok.parse::<f64>().ok()?;
    if secs.is_finite() && secs >= 0.0 {
        Some(secs.floor() as u64)
    } else {
        None
    }
}

fn map_battery_health(code: &str) -> Option<String> {
    match code.trim() {
        "2" => Some("Good".to_string()),
        "3" => Some("Overheat".to_string()),
        "4" => Some("Dead".to_string()),
        "5" => Some("Over voltage".to_string()),
        "6" => Some("Unspecified failure".to_string()),
        "7" => Some("Cold".to_string()),
        _ => None,
    }
}

pub fn parse_overview_battery(output: &str) -> Option<OverviewBattery> {
    let base = crate::commands::parse_battery_info(output);
    let mut temp_c: Option<f32> = None;
    let mut health: Option<String> = None;

    for raw in output.lines() {
        let line = raw.trim();
        if let Some(rest) = line.strip_prefix("temperature:") {
            if let Ok(tenths) = rest.trim().parse::<i32>() {
                temp_c = Some(tenths as f32 / 10.0);
            }
        } else if let Some(rest) = line.strip_prefix("health:") {
            health = map_battery_health(rest);
        } else if temp_c.is_none()
            && line.starts_with("Temperature{")
            && line.contains("mName=battery")
        {
            if let Some(val_str) = extract_telephony_field(line, "mValue=") {
                if let Ok(val) = val_str.parse::<f32>() {
                    temp_c = Some(val);
                }
            }
        }
    }

    if base.level.is_none() && !base.charging && temp_c.is_none() && health.is_none() {
        return None;
    }

    Some(OverviewBattery {
        level: base.level,
        charging: Some(base.charging),
        temp_c,
        health,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_getprop_fixture_and_handles_garbage() {
        let raw = include_str!("../../tests/fixtures/phone_hub/getprop.txt");
        let info = parse_getprop(raw);
        assert_eq!(info.model.as_deref(), Some("Phone X1"));
        assert_eq!(info.brand.as_deref(), Some("GenericBrand"));
        assert_eq!(info.android_version.as_deref(), Some("15"));
        assert_eq!(info.security_patch.as_deref(), Some("2026-10-05"));

        let empty = parse_getprop("not valid getprop output");
        assert_eq!(empty, StaticInfo::default());
    }

    #[test]
    fn parses_df_data_fixture_and_wrapped_lines() {
        let raw = include_str!("../../tests/fixtures/phone_hub/df.txt");
        let usage = parse_df_data(raw).expect("df usage");
        assert_eq!(usage.total_bytes, 114_688_000 * 1024);
        assert_eq!(usage.used_bytes, 52_428_800 * 1024);

        let wrapped = "Filesystem 1K-blocks Used Available Use% Mounted on\n/dev/block/very/long/mapper/name\n  10000 4000 6000 40% /data\n";
        let w = parse_df_data(wrapped).expect("wrapped df");
        assert_eq!(w.total_bytes, 10_000 * 1024);
        assert_eq!(w.used_bytes, 4_000 * 1024);

        assert_eq!(parse_df_data("garbage"), None);
    }

    #[test]
    fn parses_meminfo_fixture_and_legacy_fallback() {
        let raw = include_str!("../../tests/fixtures/phone_hub/meminfo.txt");
        let ram = parse_meminfo(raw).expect("meminfo");
        assert_eq!(ram.total_bytes, 8_052_344 * 1024);
        assert_eq!(ram.used_bytes, (8_052_344 - 3_220_936) * 1024);

        let legacy =
            "MemTotal: 4000000 kB\nMemFree: 500000 kB\nBuffers: 100000 kB\nCached: 1400000 kB\n";
        let leg = parse_meminfo(legacy).expect("legacy meminfo");
        assert_eq!(leg.total_bytes, 4_000_000 * 1024);
        assert_eq!(leg.used_bytes, 2_000_000 * 1024);

        assert_eq!(parse_meminfo("no memory info"), None);
    }

    #[test]
    fn parses_wifi_status_and_unknown_ssid() {
        let raw = include_str!("../../tests/fixtures/phone_hub/wifi_status.txt");
        let wifi = parse_wifi(raw).expect("wifi");
        assert_eq!(wifi.ssid.as_deref(), Some("Home-5G"));
        assert_eq!(wifi.signal_level, Some(4));

        let hidden = include_str!("../../tests/fixtures/phone_hub/wifi_unknown_ssid.txt");
        let w2 = parse_wifi(hidden).expect("wifi unknown ssid");
        assert_eq!(w2.ssid, None);
        assert_eq!(w2.signal_level, Some(2));

        assert_eq!(parse_wifi("Wi-Fi is disabled"), None);
        assert_eq!(parse_wifi("garbage"), None);
    }

    #[test]
    fn parses_telephony_fixture_and_garbage() {
        let raw = include_str!("../../tests/fixtures/phone_hub/telephony.txt");
        let cell = parse_telephony(raw).expect("telephony");
        assert_eq!(cell.operator.as_deref(), Some("ExampleTel"));
        assert_eq!(cell.signal_level, Some(3));

        assert_eq!(parse_telephony("random garbage"), None);
    }

    #[test]
    fn parses_screen_states_on_off_locked() {
        let on = include_str!("../../tests/fixtures/phone_hub/power_on.txt");
        assert_eq!(parse_screen(on, on), Some(ScreenState::On));

        let off_locked = include_str!("../../tests/fixtures/phone_hub/power_off_locked.txt");
        let (power, window) = off_locked.split_once("---WINDOW---").unwrap();
        assert_eq!(parse_screen(power, window), Some(ScreenState::Off));

        // Awake + keyguard showing -> Locked
        assert_eq!(parse_screen(on, window), Some(ScreenState::Locked));
        assert_eq!(parse_screen("garbage", "garbage"), None);
    }

    #[test]
    fn parses_uptime_and_battery_thermal() {
        assert_eq!(parse_uptime("12345.67 23456.78\n"), Some(12345));
        assert_eq!(parse_uptime("not a number"), None);

        let raw = include_str!("../../tests/fixtures/phone_hub/thermal.txt");
        let (batt_part, _therm_part) = raw.split_once("---THERMAL---").unwrap();
        let batt = parse_overview_battery(batt_part).expect("battery");
        assert_eq!(batt.level, Some(82));
        assert_eq!(batt.charging, Some(true));
        assert_eq!(batt.temp_c, Some(32.5));
        assert_eq!(batt.health.as_deref(), Some("Good"));

        assert_eq!(parse_overview_battery("garbage"), None);
    }

    #[test]
    fn phone_overview_serializes_camel_case() {
        let ov = PhoneOverview {
            model: Some("Phone X1".into()),
            android_version: Some("15".into()),
            security_patch: Some("2026-10-05".into()),
            battery: Some(OverviewBattery {
                level: Some(82),
                charging: Some(true),
                temp_c: Some(32.5),
                health: Some("Good".into()),
            }),
            storage: Some(BytesUsage {
                used_bytes: 100,
                total_bytes: 200,
            }),
            wifi: Some(WifiInfo {
                ssid: Some("Home".into()),
                signal_level: Some(4),
            }),
            screen: Some(ScreenState::Locked),
            uptime_secs: Some(42),
            updated_at_ms: 1700000000000,
            ..Default::default()
        };
        let v = serde_json::to_value(&ov).unwrap();
        assert_eq!(v["androidVersion"], "15");
        assert_eq!(v["securityPatch"], "2026-10-05");
        assert_eq!(v["battery"]["tempC"], 32.5);
        assert_eq!(v["storage"]["usedBytes"], 100);
        assert_eq!(v["wifi"]["signalLevel"], 4);
        assert_eq!(v["screen"], "locked");
        assert_eq!(v["uptimeSecs"], 42);
        assert_eq!(v["updatedAtMs"], 1700000000000i64);
    }
}
