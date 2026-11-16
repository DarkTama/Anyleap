use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Notify;

use crate::commands::{launch_app_window, validate_package_name, SessionInfo};
use crate::phone_hub::poller::PhoneMessagesUpdatedEvent;
use crate::phone_hub::{resolve_device_identity, scrcpy, validate_serial, PhoneHubState};

pub const UNKNOWN_PKG_REFRESH_COOLDOWN_MS: u64 = 60_000;
const SCRCPY_LIST_APPS_TIMEOUT_SECS: u64 = 15;

#[derive(Default)]
pub struct AppsSyncState {
    pub prefetched_serials: HashSet<String>,
    pub in_flight_identities: HashSet<String>,
    pub last_refresh_ms: HashMap<String, u64>,
    pub in_flight_notify: HashMap<String, Arc<Notify>>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PhoneApp {
    pub package: String,
    pub label: String,
    pub system: bool,
}

fn is_package_token(tok: &str) -> bool {
    !tok.is_empty()
        && tok.contains('.')
        && !tok.starts_with('.')
        && !tok.ends_with('.')
        && !tok.contains("..")
        && tok
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
}

fn flush_pending_app_line(out: &mut Vec<PhoneApp>, pending: Option<(bool, String)>) {
    let Some((system, rest)) = pending else {
        return;
    };
    let rest = rest.trim();
    let Some((label_part, pkg_part)) = rest.rsplit_once(char::is_whitespace) else {
        return;
    };
    let package = pkg_part.trim();
    if !is_package_token(package) {
        return;
    }
    let label_trimmed = label_part.trim();
    let label = if label_trimmed.is_empty() {
        package.to_string()
    } else {
        label_trimmed.to_string()
    };
    out.push(PhoneApp {
        package: package.to_string(),
        label,
        system,
    });
}

pub fn parse_scrcpy_app_list(output: &str) -> Vec<PhoneApp> {
    let mut out = Vec::new();
    let mut pending: Option<(bool, String)> = None;

    for raw in output.lines() {
        let trimmed = raw.trim_start();
        if let Some(r) = trimmed.strip_prefix("* ") {
            flush_pending_app_line(&mut out, pending.take());
            pending = Some((true, r.to_string()));
            continue;
        }
        if let Some(r) = trimmed.strip_prefix("- ") {
            flush_pending_app_line(&mut out, pending.take());
            pending = Some((false, r.to_string()));
            continue;
        }

        // scrcpy 4.0+ (verified on 5.0.1) wraps package names onto an indented continuation line when label length >= 30.
        if let Some((system, ref label_buf)) = pending {
            let cont = trimmed.trim();
            if raw.starts_with(char::is_whitespace) && is_package_token(cont) {
                let label_trimmed = label_buf.trim();
                let label = if label_trimmed.is_empty() {
                    cont.to_string()
                } else {
                    label_trimmed.to_string()
                };
                out.push(PhoneApp {
                    package: cont.to_string(),
                    label,
                    system,
                });
                pending = None;
                continue;
            }
        }

        flush_pending_app_line(&mut out, pending.take());
    }
    flush_pending_app_line(&mut out, pending);
    out
}

pub fn validate_app_package(package: &str) -> Result<(), String> {
    if package.is_empty()
        || !package
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
    {
        return Err("Invalid Android package name".to_string());
    }
    validate_package_name(package)
}

#[derive(Default, Serialize, Deserialize)]
pub struct AppLabelCache {
    #[serde(default)]
    devices: HashMap<String, Vec<PhoneApp>>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl AppLabelCache {
    pub fn load_from_path(path: PathBuf) -> Self {
        let devices = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<HashMap<String, Vec<PhoneApp>>>(&s).ok())
            .unwrap_or_default();
        Self {
            devices,
            path: Some(path),
        }
    }

    pub fn set_apps(&mut self, identity: &str, apps: Vec<PhoneApp>) {
        self.devices.insert(identity.to_string(), apps);
        if let Some(ref path) = self.path {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string(&self.devices) {
                let _ = std::fs::write(path, json);
            }
        }
    }

    pub fn get_apps(&self, identity: &str) -> Option<&[PhoneApp]> {
        self.devices.get(identity).map(|v| v.as_slice())
    }

    pub fn get(&self, identity: &str, package: &str) -> Option<String> {
        self.devices
            .get(identity)?
            .iter()
            .find(|a| a.package == package)
            .map(|a| a.label.clone())
    }
}

pub(crate) fn with_apps_cache<R>(
    app: &AppHandle,
    state: &PhoneHubState,
    f: impl FnOnce(&mut AppLabelCache) -> R,
) -> Result<R, String> {
    let mut guard = state
        .apps_cache
        .lock()
        .map_err(crate::state::poison_error)?;
    if guard.is_none() {
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        let path = dir.join("phone_hub_apps.json");
        *guard = Some(AppLabelCache::load_from_path(path));
    }
    Ok(f(guard.as_mut().expect("cache initialized")))
}

pub(crate) fn resolve_label_from_cache(
    cache: &AppLabelCache,
    identity: &str,
    package: &str,
    fallback_label: Option<&str>,
) -> String {
    if let Some(label) = cache.get(identity, package) {
        return label;
    }
    if let Some(fb) = fallback_label {
        let trimmed = fb.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    package.to_string()
}

pub(crate) fn resolve_app_label_for_identity(
    app: &AppHandle,
    state: &PhoneHubState,
    identity: &str,
    package: &str,
    fallback_label: Option<&str>,
) -> String {
    if let Ok(label) = with_apps_cache(app, state, |c| {
        resolve_label_from_cache(c, identity, package, fallback_label)
    }) {
        return label;
    }
    fallback_label
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(package)
        .to_string()
}

pub(crate) fn resolve_app_label_cached(
    app: &AppHandle,
    state: &PhoneHubState,
    serial: &str,
    package: &str,
    fallback_label: Option<&str>,
) -> String {
    let identity = state
        .serial_identities
        .lock()
        .ok()
        .and_then(|m| m.get(serial).cloned())
        .unwrap_or_else(|| serial.to_string());

    resolve_app_label_for_identity(app, state, &identity, package, fallback_label)
}

pub(crate) fn should_refresh_apps_for_unknown(
    now_ms: u64,
    last_refresh_ms: Option<u64>,
    has_unknown_pkg: bool,
    in_flight: bool,
    cooldown_ms: u64,
) -> bool {
    if !has_unknown_pkg || in_flight {
        return false;
    }
    match last_refresh_ms {
        None => true,
        Some(last) => now_ms.saturating_sub(last) >= cooldown_ms,
    }
}

struct InFlightGuard<'a> {
    state: &'a PhoneHubState,
    identity: String,
    notify: Arc<Notify>,
}

impl Drop for InFlightGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut sync) = self.state.apps_sync.lock() {
            sync.in_flight_identities.remove(&self.identity);
            sync.in_flight_notify.remove(&self.identity);
        }
        self.notify.notify_waiters();
    }
}

async fn wait_for_in_flight(state: &PhoneHubState, identity: &str) -> bool {
    let notify = state
        .apps_sync
        .lock()
        .ok()
        .and_then(|sync| sync.in_flight_notify.get(identity).cloned());
    if let Some(n) = notify {
        let _ = tokio::time::timeout(
            Duration::from_secs(SCRCPY_LIST_APPS_TIMEOUT_SECS),
            n.notified(),
        )
        .await;
        true
    } else {
        false
    }
}

pub(crate) fn build_list_apps_args(serial: &str) -> [&str; 3] {
    ["--serial", serial, "--list-apps"]
}

pub(crate) async fn refresh_apps_for_identity(
    app: &AppHandle,
    state: &PhoneHubState,
    serial: &str,
    identity: &str,
) -> Result<Vec<PhoneApp>, String> {
    let guard = loop {
        let existing_notify = {
            let mut sync = state.apps_sync.lock().map_err(crate::state::poison_error)?;
            if sync.in_flight_identities.contains(identity) {
                sync.in_flight_notify.get(identity).cloned()
            } else {
                let notify = Arc::new(Notify::new());
                sync.in_flight_identities.insert(identity.to_string());
                sync.in_flight_notify
                    .insert(identity.to_string(), Arc::clone(&notify));
                break InFlightGuard {
                    state,
                    identity: identity.to_string(),
                    notify,
                };
            }
        };
        if let Some(n) = existing_notify {
            let _ = tokio::time::timeout(
                Duration::from_secs(SCRCPY_LIST_APPS_TIMEOUT_SECS),
                n.notified(),
            )
            .await;
            if let Ok(Some(cached)) =
                with_apps_cache(app, state, |c| c.get_apps(identity).map(|s| s.to_vec()))
            {
                if !cached.is_empty() {
                    return Ok(cached);
                }
            }
        }
    };

    let cmd = scrcpy(app).await?;
    let output = tokio::time::timeout(
        Duration::from_secs(SCRCPY_LIST_APPS_TIMEOUT_SECS),
        cmd.args(build_list_apps_args(serial)).output(),
    )
    .await
    .map_err(|_| "Timed out listing device apps".to_string())?
    .map_err(|e| e.to_string())?;

    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let apps = parse_scrcpy_app_list(&combined);

    if let Ok(mut sync) = state.apps_sync.lock() {
        sync.last_refresh_ms.insert(identity.to_string(), now_ms());
    }

    if apps.is_empty() && !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if err.is_empty() {
            "Failed to list device apps".to_string()
        } else {
            err
        });
    }

    if !apps.is_empty() {
        with_apps_cache(app, state, |c| c.set_apps(identity, apps.clone()))?;
    }
    drop(guard);
    Ok(apps)
}

pub(crate) async fn ensure_labels_for_packages(
    app: &AppHandle,
    state: &PhoneHubState,
    serial: &str,
    identity: &str,
    packages: &[&str],
) {
    if packages.is_empty() {
        return;
    }

    let has_missing = |st: &PhoneHubState| -> bool {
        with_apps_cache(app, st, |c| {
            packages.iter().any(|pkg| c.get(identity, pkg).is_none())
        })
        .unwrap_or(false)
    };

    if !has_missing(state) {
        return;
    }

    if wait_for_in_flight(state, identity).await && !has_missing(state) {
        return;
    }

    let should_refresh = state.apps_sync.lock().ok().is_some_and(|sync| {
        should_refresh_apps_for_unknown(
            now_ms(),
            sync.last_refresh_ms.get(identity).copied(),
            true,
            sync.in_flight_identities.contains(identity),
            UNKNOWN_PKG_REFRESH_COOLDOWN_MS,
        )
    });

    if should_refresh {
        let _ = refresh_apps_for_identity(app, state, serial, identity).await;
    }
}

pub(crate) fn trigger_apps_prefetch_for_serial(app: &AppHandle, serial: &str) {
    let Some(state) = app.try_state::<PhoneHubState>() else {
        return;
    };
    let should_spawn = state
        .apps_sync
        .lock()
        .ok()
        .is_some_and(|mut sync| sync.prefetched_serials.insert(serial.to_string()));
    if !should_spawn {
        return;
    }

    let app_c = app.clone();
    let serial_c = serial.to_string();
    tauri::async_runtime::spawn(async move {
        let Some(state) = app_c.try_state::<PhoneHubState>() else {
            return;
        };
        let identity = resolve_device_identity(&app_c, &state, &serial_c).await;
        if refresh_apps_for_identity(&app_c, &state, &serial_c, &identity)
            .await
            .is_ok()
        {
            let mut label_map = HashMap::new();
            let _ = with_apps_cache(&app_c, &state, |c| {
                if let Some(list) = c.get_apps(&identity) {
                    for a in list {
                        label_map.insert(a.package.clone(), a.label.clone());
                    }
                }
            });
            if !label_map.is_empty() {
                let updated_keys = if let Ok(mut msgs) = state.messages.lock() {
                    msgs.refresh_app_labels(&serial_c, &label_map)
                } else {
                    Vec::new()
                };
                for conversation_key in updated_keys {
                    let _ = app_c.emit(
                        "phone-messages-updated",
                        PhoneMessagesUpdatedEvent {
                            serial: serial_c.clone(),
                            conversation_key,
                        },
                    );
                }
            }
        }
    });
}

pub(crate) fn trigger_apps_prefetch_for_serials(app: &AppHandle, serials: &[String]) {
    for serial in serials {
        trigger_apps_prefetch_for_serial(app, serial);
    }
}

#[tauri::command]
pub async fn phone_list_apps(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    serial: String,
    refresh: bool,
) -> Result<Vec<PhoneApp>, String> {
    validate_serial(&serial)?;
    let identity = resolve_device_identity(&app, &state, &serial).await;

    if !refresh {
        if let Some(cached) =
            with_apps_cache(&app, &state, |c| c.get_apps(&identity).map(|s| s.to_vec()))?
        {
            if !cached.is_empty() {
                return Ok(cached);
            }
        }
    }

    refresh_apps_for_identity(&app, &state, &serial, &identity).await
}

#[tauri::command]
pub async fn phone_open_app(
    app: AppHandle,
    serial: String,
    package: String,
) -> Result<SessionInfo, String> {
    validate_serial(&serial)?;
    validate_app_package(&package)?;
    launch_app_window(app, serial, package).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scrcpy_app_list_system_user_spaces_unicode() {
        let raw = include_str!("../../tests/fixtures/phone_hub/scrcpy_list_apps.txt");
        let apps = parse_scrcpy_app_list(raw);
        assert_eq!(apps.len(), 6);

        let settings = apps
            .iter()
            .find(|a| a.package == "com.android.settings")
            .unwrap();
        assert_eq!(settings.label, "Settings");
        assert!(settings.system);

        let chat = apps
            .iter()
            .find(|a| a.package == "com.example.chat")
            .unwrap();
        assert_eq!(chat.label, "Example Chat");
        assert!(!chat.system);

        // Long app name (>= 30 chars) wraps package onto indented next line in scrcpy 4.0+ / 5.0.1.
        let shop = apps
            .iter()
            .find(|a| a.package == "com.example.shop")
            .unwrap();
        assert_eq!(shop.label, "Example Marketplace Online Shopping");
        assert!(!shop.system);

        let jp = apps
            .iter()
            .find(|a| a.package == "com.example.notepad_jp")
            .unwrap();
        assert_eq!(jp.label, "NotePad 日本語");
        assert!(!jp.system);

        assert!(parse_scrcpy_app_list("random unrelated log output").is_empty());
    }

    #[test]
    fn notification_for_listed_package_resolves_app_label() {
        let apps_raw = include_str!("../../tests/fixtures/phone_hub/scrcpy_list_apps.txt");
        let apps = parse_scrcpy_app_list(apps_raw);
        let mut cache = AppLabelCache::default();
        cache.set_apps("HW_PHONE_1", apps);

        let notif_raw = r#"Current Notification Manager state:
  Notification List:
    NotificationRecord(0x0111aaaa: pkg=com.example.shop user=UserHandle{0} id=1 tag=null importance=4 key=0|com.example.shop|1|null|10400: Notification(channel=promo shortcut=null flags=AUTO_CANCEL vis=PRIVATE))
      key=0|com.example.shop|1|null|10400
      notification=
            when=1787287360000/1787287360000
            extras={
                android.title=String (Order Shipped)
                android.text=String (Your parcel is on the way)
            }
"#;
        let records = crate::phone_files::parse_dumpsys_notifications_rich(notif_raw);
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].base.app_label, None);

        let label = resolve_label_from_cache(
            &cache,
            "HW_PHONE_1",
            &records[0].base.package,
            records[0].base.app_label.as_deref(),
        );
        assert_eq!(label, "Example Marketplace Online Shopping");

        // Unknown package falls back to package ID when no fallback label exists.
        assert_eq!(
            resolve_label_from_cache(&cache, "HW_PHONE_1", "com.example.unknown", None),
            "com.example.unknown"
        );
    }

    #[test]
    fn unknown_package_refresh_is_rate_limited() {
        assert!(should_refresh_apps_for_unknown(
            100_000,
            None,
            true,
            false,
            UNKNOWN_PKG_REFRESH_COOLDOWN_MS
        ));
        assert!(!should_refresh_apps_for_unknown(
            100_000,
            Some(80_000),
            true,
            false,
            UNKNOWN_PKG_REFRESH_COOLDOWN_MS
        ));
        assert!(should_refresh_apps_for_unknown(
            160_000,
            Some(100_000),
            true,
            false,
            UNKNOWN_PKG_REFRESH_COOLDOWN_MS
        ));
        assert!(!should_refresh_apps_for_unknown(
            200_000,
            None,
            true,
            true,
            UNKNOWN_PKG_REFRESH_COOLDOWN_MS
        ));
        assert!(!should_refresh_apps_for_unknown(
            200_000,
            None,
            false,
            false,
            UNKNOWN_PKG_REFRESH_COOLDOWN_MS
        ));
    }

    #[test]
    fn app_label_cache_persists_and_hits_and_misses() {
        let dir = std::env::temp_dir().join(format!("anyleap_apps_test_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("phone_hub_apps.json");

        {
            let mut cache = AppLabelCache::load_from_path(path.clone());
            assert_eq!(cache.get("HW123", "com.example.chat"), None);
            cache.set_apps(
                "HW123",
                vec![PhoneApp {
                    package: "com.example.chat".into(),
                    label: "Example Chat".into(),
                    system: false,
                }],
            );
            assert_eq!(
                cache.get("HW123", "com.example.chat").as_deref(),
                Some("Example Chat")
            );
            assert_eq!(cache.get("HW999", "com.example.chat"), None);
        }

        let reloaded = AppLabelCache::load_from_path(path);
        assert_eq!(
            reloaded.get("HW123", "com.example.chat").as_deref(),
            Some("Example Chat")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn package_validation_rejects_semicolon_and_space() {
        assert!(validate_app_package("com.example.chat").is_ok());
        assert!(validate_app_package("org.example.app_2").is_ok());
        assert!(validate_app_package("com.example.chat;id").is_err());
        assert!(validate_app_package("com.example.chat rm").is_err());
        assert!(validate_app_package("").is_err());
        assert!(validate_app_package("single").is_err());
    }
}
