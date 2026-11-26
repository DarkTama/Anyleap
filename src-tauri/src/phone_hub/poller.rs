use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::async_runtime::JoinHandle;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Notify;

use crate::phone_files::parse_dumpsys_notifications_rich;
use crate::phone_hub::apps::{
    ensure_labels_for_packages, resolve_app_label_for_identity, trigger_apps_prefetch_for_serial,
};
use crate::phone_hub::overview::{
    parse_df_data, parse_getprop, parse_meminfo, parse_overview_battery, parse_screen,
    parse_telephony, parse_uptime, parse_wifi, PhoneOverview,
};
use crate::phone_hub::{adb, resolve_device_identity, validate_serial, PhoneHubState};

pub const DEFAULT_NOTIF_POLL_SECS: u32 = 10;
pub const MIN_NOTIF_POLL_SECS: u32 = 5;
pub const FAST_POLL_MS: u64 = 30_000;
pub const SLOW_POLL_MS: u64 = 300_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollTier {
    Static,
    Fast,
    Slow,
    Notifications(u32),
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PhoneOverviewEvent {
    pub serial: String,
    pub overview: PhoneOverview,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PhoneHubNotification {
    pub key: String,
    pub package: String,
    pub app_label: String,
    pub title: String,
    pub text: String,
    pub posted_unix: i64,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PhoneNotificationsEvent {
    pub serial: String,
    pub items: Vec<PhoneHubNotification>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PhoneMessagesUpdatedEvent {
    pub serial: String,
    pub conversation_key: String,
}

#[derive(Default)]
pub struct PollerEntry {
    pub sections: HashSet<String>,
    pub notify: Arc<Notify>,
    pub handle: Option<JoinHandle<()>>,
    pub generation: u64,
    pub last_static_ms: Option<u64>,
    pub last_fast_ms: Option<u64>,
    pub last_slow_ms: Option<u64>,
    pub last_notif_ms: Option<u64>,
}

pub struct PollerState {
    pub entries: HashMap<String, PollerEntry>,
    pub overviews: HashMap<String, PhoneOverview>,
    pub notif_poll_secs: u32,
    pub poll_counts: HashMap<String, u64>,
    pub next_gen: u64,
}

impl Default for PollerState {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            overviews: HashMap::new(),
            notif_poll_secs: DEFAULT_NOTIF_POLL_SECS,
            poll_counts: HashMap::new(),
            next_gen: 1,
        }
    }
}

impl PollerState {
    pub fn watch_sections(&mut self, serial: &str, sections: &[String]) {
        let valid: HashSet<String> = sections
            .iter()
            .filter(|s| s.as_str() == "overview" || s.as_str() == "notifications")
            .cloned()
            .collect();
        if valid.is_empty() {
            self.unwatch(serial);
            return;
        }
        let entry = self.entries.entry(serial.to_string()).or_insert_with(|| {
            let gen = self.next_gen;
            self.next_gen = self.next_gen.saturating_add(1);
            PollerEntry {
                notify: Arc::new(Notify::new()),
                generation: gen,
                ..Default::default()
            }
        });
        entry.sections = valid;
        entry.notify.notify_one();
    }

    pub fn unwatch(&mut self, serial: &str) {
        if let Some(mut entry) = self.entries.remove(serial) {
            if let Some(handle) = entry.handle.take() {
                handle.abort();
            }
            entry.notify.notify_waiters();
        }
    }

    pub fn is_watched(&self, serial: &str, section: &str) -> bool {
        self.entries
            .get(serial)
            .is_some_and(|e| e.sections.contains(section))
    }

    pub fn record_poll(&mut self, serial: &str) -> u64 {
        let count = self.poll_counts.entry(serial.to_string()).or_insert(0);
        *count = count.saturating_add(1);
        let val = *count;
        #[cfg(debug_assertions)]
        eprintln!("[phone_hub] poll serial={serial} count={val}");
        val
    }

    pub fn poll_count(&self, serial: &str) -> u64 {
        self.poll_counts.get(serial).copied().unwrap_or(0)
    }

    pub fn set_notif_poll_secs(&mut self, secs: u32) -> u32 {
        let clamped = secs.max(MIN_NOTIF_POLL_SECS);
        self.notif_poll_secs = clamped;
        for entry in self.entries.values() {
            entry.notify.notify_one();
        }
        clamped
    }
}

pub fn next_due(now_ms: u64, last_ms: Option<u64>, tier: PollTier, watched: bool) -> bool {
    if !watched {
        return false;
    }
    match tier {
        PollTier::Static => last_ms.is_none(),
        PollTier::Fast => last_ms.is_none_or(|l| now_ms.saturating_sub(l) >= FAST_POLL_MS),
        PollTier::Slow => last_ms.is_none_or(|l| now_ms.saturating_sub(l) >= SLOW_POLL_MS),
        PollTier::Notifications(secs) => {
            let interval_ms = (secs.max(MIN_NOTIF_POLL_SECS) as u64).saturating_mul(1000);
            last_ms.is_none_or(|l| now_ms.saturating_sub(l) >= interval_ms)
        }
    }
}

pub fn disconnect_cleanup_state(state: &PhoneHubState, serial: &str) {
    if let Ok(mut poller) = state.poller.lock() {
        poller.unwatch(serial);
    }
    if let Ok(mut msgs) = state.messages.lock() {
        msgs.clear(serial);
    }
    if let Ok(mut media) = state.media_cache.lock() {
        media.remove(serial);
    }
    if let Ok(mut sync) = state.apps_sync.lock() {
        sync.prefetched_serials.remove(serial);
    }
    if let Ok(mut ids) = state.serial_identities.lock() {
        ids.remove(serial);
    }
}

pub fn on_disconnect(app: &AppHandle, serial: &str) {
    if let Some(state) = app.try_state::<PhoneHubState>() {
        disconnect_cleanup_state(&state, serial);
    }
    crate::phone_hub::audio_only::stop_for_serial(app, serial);
}

pub(crate) fn reconcile_connected_serials(app: &AppHandle, live_serials: &[String]) {
    let live: HashSet<&str> = live_serials.iter().map(|s| s.as_str()).collect();
    let Some(state) = app.try_state::<PhoneHubState>() else {
        return;
    };
    let mut tracked: HashSet<String> = HashSet::new();
    if let Ok(poller) = state.poller.lock() {
        tracked.extend(poller.entries.keys().cloned());
    }
    if let Ok(msgs) = state.messages.lock() {
        tracked.extend(msgs.tracked_serials());
    }
    if let Ok(sync) = state.apps_sync.lock() {
        tracked.extend(sync.prefetched_serials.iter().cloned());
    }
    for serial in tracked {
        if !live.contains(serial.as_str()) {
            on_disconnect(app, &serial);
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn is_adb_disconnect_error(stderr: &str) -> bool {
    let low = stderr.to_ascii_lowercase();
    low.contains("not found")
        || low.contains("device offline")
        || low.contains("unauthorized")
        || low.contains("protocol fault")
        || low.contains("closed")
}

async fn run_adb_shell_checked(
    app: &AppHandle,
    serial: &str,
    script: &str,
) -> Result<String, bool> {
    let Ok(cmd) = adb(app).await else {
        return Err(false);
    };
    let Ok(Ok(out)) = tokio::time::timeout(
        Duration::from_secs(10),
        cmd.args(["-s", serial, "shell", script]).output(),
    )
    .await
    else {
        return Err(false);
    };
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !out.status.success() && is_adb_disconnect_error(&stderr) {
        return Err(true);
    }
    if !out.status.success() && out.stdout.is_empty() {
        return Err(false);
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

fn record_poll_count(app: &AppHandle, serial: &str) {
    if let Some(state) = app.try_state::<PhoneHubState>() {
        if let Ok(mut poller) = state.poller.lock() {
            poller.record_poll(serial);
        }
    }
}

pub(crate) const OVERVIEW_STATIC_SCRIPT: &str = "getprop";
pub(crate) const OVERVIEW_FAST_SCRIPT: &str = "dumpsys battery; echo __PH_WIFI__; cmd wifi status 2>/dev/null || dumpsys wifi; echo __PH_CELL__; dumpsys telephony.registry; echo __PH_PWR__; dumpsys power; echo __PH_WIN__; dumpsys window policy 2>/dev/null || dumpsys window";
pub(crate) const OVERVIEW_SLOW_SCRIPT: &str =
    "df -k /data; echo __PH_MEM__; cat /proc/meminfo; echo __PH_UP__; cat /proc/uptime";
pub(crate) const NOTIFICATIONS_DUMPSYS_SCRIPT: &str = "dumpsys notification --noredact";

pub(crate) fn apply_static_overview(stdout: &str, ov: &mut PhoneOverview) {
    let info = parse_getprop(stdout);
    ov.model = info.model;
    ov.brand = info.brand;
    ov.android_version = info.android_version;
    ov.security_patch = info.security_patch;
    ov.updated_at_ms = now_ms() as i64;
}

pub(crate) fn apply_fast_overview(stdout: &str, ov: &mut PhoneOverview) {
    let (batt_raw, rest) = stdout.split_once("__PH_WIFI__").unwrap_or((stdout, ""));
    let (wifi_raw, rest) = rest.split_once("__PH_CELL__").unwrap_or((rest, ""));
    let (cell_raw, rest) = rest.split_once("__PH_PWR__").unwrap_or((rest, ""));
    let (pwr_raw, win_raw) = rest.split_once("__PH_WIN__").unwrap_or((rest, ""));

    ov.battery = parse_overview_battery(batt_raw);
    ov.wifi = parse_wifi(wifi_raw);
    ov.cellular = parse_telephony(cell_raw);
    ov.screen = parse_screen(pwr_raw, win_raw);
    ov.updated_at_ms = now_ms() as i64;
}

pub(crate) fn apply_slow_overview(stdout: &str, ov: &mut PhoneOverview) {
    let (df_raw, rest) = stdout.split_once("__PH_MEM__").unwrap_or((stdout, ""));
    let (mem_raw, up_raw) = rest.split_once("__PH_UP__").unwrap_or((rest, ""));

    ov.storage = parse_df_data(df_raw);
    ov.ram = parse_meminfo(mem_raw);
    ov.uptime_secs = parse_uptime(up_raw);
    ov.updated_at_ms = now_ms() as i64;
}

async fn poll_static_into(
    app: &AppHandle,
    serial: &str,
    ov: &mut PhoneOverview,
) -> Result<(), bool> {
    record_poll_count(app, serial);
    let stdout = run_adb_shell_checked(app, serial, OVERVIEW_STATIC_SCRIPT).await?;
    apply_static_overview(&stdout, ov);
    Ok(())
}

async fn poll_fast_into(app: &AppHandle, serial: &str, ov: &mut PhoneOverview) -> Result<(), bool> {
    record_poll_count(app, serial);
    let stdout = run_adb_shell_checked(app, serial, OVERVIEW_FAST_SCRIPT).await?;
    apply_fast_overview(&stdout, ov);
    Ok(())
}

async fn poll_slow_into(app: &AppHandle, serial: &str, ov: &mut PhoneOverview) -> Result<(), bool> {
    record_poll_count(app, serial);
    let stdout = run_adb_shell_checked(app, serial, OVERVIEW_SLOW_SCRIPT).await?;
    apply_slow_overview(&stdout, ov);
    Ok(())
}

async fn poll_notifications_once(app: &AppHandle, serial: &str) -> Result<(), bool> {
    record_poll_count(app, serial);
    let stdout = run_adb_shell_checked(app, serial, NOTIFICATIONS_DUMPSYS_SCRIPT).await?;
    let records = parse_dumpsys_notifications_rich(&stdout);
    let Some(state) = app.try_state::<PhoneHubState>() else {
        return Ok(());
    };

    let identity = resolve_device_identity(app, &state, serial).await;
    let unique_pkgs: Vec<&str> = {
        let mut seen = HashSet::new();
        records
            .iter()
            .filter_map(|r| {
                seen.insert(r.base.package.as_str())
                    .then_some(r.base.package.as_str())
            })
            .collect()
    };
    ensure_labels_for_packages(app, &state, serial, &identity, &unique_pkgs).await;

    let mut labels_map: HashMap<String, String> = HashMap::new();
    let mut items = Vec::with_capacity(records.len());
    for rec in &records {
        let label = resolve_app_label_for_identity(
            app,
            &state,
            &identity,
            &rec.base.package,
            rec.base.app_label.as_deref(),
        );
        labels_map.insert(rec.base.package.clone(), label.clone());
        items.push(PhoneHubNotification {
            key: rec.base.key.clone(),
            package: rec.base.package.clone(),
            app_label: label,
            title: rec.base.title.clone(),
            text: rec.base.text.clone(),
            posted_unix: rec.base.posted_unix,
        });
    }

    let changed_keys = if let Ok(mut msgs) = state.messages.lock() {
        msgs.ingest(serial, &records, &labels_map)
    } else {
        Vec::new()
    };

    crate::phone_hub::vault::ingest_notifications_into_vault(app, &state, &identity, &items);

    let _ = app.emit(
        "phone-notifications",
        PhoneNotificationsEvent {
            serial: serial.to_string(),
            items,
        },
    );

    for conversation_key in changed_keys {
        let _ = app.emit(
            "phone-messages-updated",
            PhoneMessagesUpdatedEvent {
                serial: serial.to_string(),
                conversation_key,
            },
        );
    }

    Ok(())
}

async fn run_poller_loop(app: AppHandle, serial: String, generation: u64, notify: Arc<Notify>) {
    loop {
        let snapshot = {
            let Some(state) = app.try_state::<PhoneHubState>() else {
                break;
            };
            let Ok(poller) = state.poller.lock() else {
                break;
            };
            let Some(entry) = poller.entries.get(&serial) else {
                break;
            };
            if entry.generation != generation || entry.sections.is_empty() {
                break;
            }
            let cached_ov = poller.overviews.get(&serial).cloned().unwrap_or_default();
            (
                entry.sections.contains("overview"),
                entry.sections.contains("notifications"),
                entry.last_static_ms,
                entry.last_fast_ms,
                entry.last_slow_ms,
                entry.last_notif_ms,
                poller.notif_poll_secs,
                cached_ov,
            )
        };

        let (
            watch_overview,
            watch_notif,
            last_static,
            last_fast,
            last_slow,
            last_notif,
            notif_secs,
            mut ov,
        ) = snapshot;

        let now = now_ms();
        let need_static = next_due(now, last_static, PollTier::Static, watch_overview);
        let need_fast = next_due(now, last_fast, PollTier::Fast, watch_overview);
        let need_slow = next_due(now, last_slow, PollTier::Slow, watch_overview);
        let need_notif = next_due(
            now,
            last_notif,
            PollTier::Notifications(notif_secs),
            watch_notif,
        );

        let mut ov_updated = false;
        let mut new_static = last_static;
        let mut new_fast = last_fast;
        let mut new_slow = last_slow;
        let mut new_notif = last_notif;

        if need_static {
            match poll_static_into(&app, &serial, &mut ov).await {
                Ok(()) => {
                    new_static = Some(now_ms());
                    ov_updated = true;
                }
                Err(true) => {
                    on_disconnect(&app, &serial);
                    break;
                }
                Err(false) => {
                    new_static = Some(now_ms());
                }
            }
        }
        if need_fast {
            match poll_fast_into(&app, &serial, &mut ov).await {
                Ok(()) => {
                    new_fast = Some(now_ms());
                    ov_updated = true;
                }
                Err(true) => {
                    on_disconnect(&app, &serial);
                    break;
                }
                Err(false) => {
                    new_fast = Some(now_ms());
                }
            }
        }
        if need_slow {
            match poll_slow_into(&app, &serial, &mut ov).await {
                Ok(()) => {
                    new_slow = Some(now_ms());
                    ov_updated = true;
                }
                Err(true) => {
                    on_disconnect(&app, &serial);
                    break;
                }
                Err(false) => {
                    new_slow = Some(now_ms());
                }
            }
        }

        if ov_updated {
            if let Some(state) = app.try_state::<PhoneHubState>() {
                if let Ok(mut poller) = state.poller.lock() {
                    poller.overviews.insert(serial.clone(), ov.clone());
                }
            }
            let _ = app.emit(
                "phone-overview",
                PhoneOverviewEvent {
                    serial: serial.clone(),
                    overview: ov,
                },
            );
        }

        if need_notif {
            match poll_notifications_once(&app, &serial).await {
                Ok(()) => {
                    new_notif = Some(now_ms());
                }
                Err(true) => {
                    on_disconnect(&app, &serial);
                    break;
                }
                Err(false) => {
                    new_notif = Some(now_ms());
                }
            }
        }

        // Persist updated timestamps and compute next sleep duration.
        let sleep_ms = {
            let Some(state) = app.try_state::<PhoneHubState>() else {
                break;
            };
            let Ok(mut poller) = state.poller.lock() else {
                break;
            };
            let notif_interval_ms =
                (poller.notif_poll_secs.max(MIN_NOTIF_POLL_SECS) as u64).saturating_mul(1000);
            let Some(entry) = poller.entries.get_mut(&serial) else {
                break;
            };
            if entry.generation != generation || entry.sections.is_empty() {
                break;
            }
            entry.last_static_ms = new_static;
            entry.last_fast_ms = new_fast;
            entry.last_slow_ms = new_slow;
            entry.last_notif_ms = new_notif;

            let cur = now_ms();
            let mut wait_ms = 30_000u64;
            if entry.sections.contains("overview") {
                let fast_rem = entry
                    .last_fast_ms
                    .map_or(0, |l| FAST_POLL_MS.saturating_sub(cur.saturating_sub(l)));
                let slow_rem = entry
                    .last_slow_ms
                    .map_or(0, |l| SLOW_POLL_MS.saturating_sub(cur.saturating_sub(l)));
                wait_ms = wait_ms.min(fast_rem).min(slow_rem);
            }
            if entry.sections.contains("notifications") {
                let notif_rem = entry.last_notif_ms.map_or(0, |l| {
                    notif_interval_ms.saturating_sub(cur.saturating_sub(l))
                });
                wait_ms = wait_ms.min(notif_rem);
            }
            wait_ms.clamp(250, 30_000)
        };

        tokio::select! {
            _ = tokio::time::sleep(Duration::from_millis(sleep_ms)) => {}
            _ = notify.notified() => {}
        }
    }
}

#[tauri::command]
pub async fn phone_hub_watch(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    serial: String,
    sections: Vec<String>,
) -> Result<(), String> {
    validate_serial(&serial)?;
    trigger_apps_prefetch_for_serial(&app, &serial);
    let spawn_info = {
        let mut poller = state.poller.lock().map_err(crate::state::poison_error)?;
        poller.watch_sections(&serial, &sections);
        if let Some(entry) = poller.entries.get_mut(&serial) {
            if entry.handle.is_none() {
                let gen = entry.generation;
                let notify = Arc::clone(&entry.notify);
                Some((gen, notify))
            } else {
                None
            }
        } else {
            None
        }
    };

    if let Some((gen, notify)) = spawn_info {
        let app_c = app.clone();
        let serial_c = serial.clone();
        let notify_c = Arc::clone(&notify);
        let handle = tauri::async_runtime::spawn(async move {
            run_poller_loop(app_c, serial_c, gen, notify_c).await;
        });
        if let Ok(mut poller) = state.poller.lock() {
            if let Some(entry) = poller.entries.get_mut(&serial) {
                if entry.generation == gen {
                    entry.handle = Some(handle);
                } else {
                    handle.abort();
                }
            } else {
                handle.abort();
            }
        }
    }
    Ok(())
}

#[tauri::command]
pub fn phone_hub_unwatch(state: State<'_, PhoneHubState>, serial: String) -> Result<(), String> {
    validate_serial(&serial)?;
    let mut poller = state.poller.lock().map_err(crate::state::poison_error)?;
    poller.unwatch(&serial);
    Ok(())
}

#[tauri::command]
pub fn phone_hub_set_settings(
    state: State<'_, PhoneHubState>,
    notification_poll_secs: u32,
) -> Result<(), String> {
    let mut poller = state.poller.lock().map_err(crate::state::poison_error)?;
    poller.set_notif_poll_secs(notification_poll_secs);
    Ok(())
}

#[tauri::command]
pub async fn phone_get_overview(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    serial: String,
    refresh: bool,
) -> Result<PhoneOverview, String> {
    validate_serial(&serial)?;
    let cached = {
        let poller = state.poller.lock().map_err(crate::state::poison_error)?;
        poller.overviews.get(&serial).cloned()
    };

    if !refresh {
        if let Some(ov) = cached {
            return Ok(ov);
        }
    }

    let mut ov = cached.unwrap_or_default();
    let need_static = ov.model.is_none() && ov.android_version.is_none();
    if need_static {
        if let Err(true) = poll_static_into(&app, &serial, &mut ov).await {
            on_disconnect(&app, &serial);
            return Err("Device disconnected".to_string());
        }
    }
    if let Err(true) = poll_fast_into(&app, &serial, &mut ov).await {
        on_disconnect(&app, &serial);
        return Err("Device disconnected".to_string());
    }
    if let Err(true) = poll_slow_into(&app, &serial, &mut ov).await {
        on_disconnect(&app, &serial);
        return Err("Device disconnected".to_string());
    }

    let now = now_ms();
    {
        let mut poller = state.poller.lock().map_err(crate::state::poison_error)?;
        poller.overviews.insert(serial.clone(), ov.clone());
        if let Some(entry) = poller.entries.get_mut(&serial) {
            if need_static {
                entry.last_static_ms = Some(now);
            }
            entry.last_fast_ms = Some(now);
            entry.last_slow_ms = Some(now);
        }
    }

    let _ = app.emit(
        "phone-overview",
        PhoneOverviewEvent {
            serial,
            overview: ov.clone(),
        },
    );
    Ok(ov)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::phone_files::{ParsedChatEntry, ParsedNotificationRecord, PhoneNotification};

    #[test]
    fn tier_schedule_static_fast_slow_notifications() {
        assert!(!next_due(1000, None, PollTier::Fast, false));
        assert!(!next_due(1000, None, PollTier::Static, false));

        // Static: once per connection.
        assert!(next_due(1000, None, PollTier::Static, true));
        assert!(!next_due(999_999, Some(1000), PollTier::Static, true));

        // Fast: 30s while watched.
        assert!(next_due(1000, None, PollTier::Fast, true));
        assert!(!next_due(30_999, Some(1000), PollTier::Fast, true));
        assert!(next_due(31_000, Some(1000), PollTier::Fast, true));

        // Slow: 5 min (300_000ms) while watched.
        assert!(next_due(1000, None, PollTier::Slow, true));
        assert!(!next_due(300_999, Some(1000), PollTier::Slow, true));
        assert!(next_due(301_000, Some(1000), PollTier::Slow, true));

        // Notifications: default 10s, min 5s.
        assert!(next_due(1000, None, PollTier::Notifications(10), true));
        assert!(!next_due(
            10_999,
            Some(1000),
            PollTier::Notifications(10),
            true
        ));
        assert!(next_due(
            11_000,
            Some(1000),
            PollTier::Notifications(10),
            true
        ));
        // Clamped to min 5s even if 1s passed.
        assert!(!next_due(
            5_999,
            Some(1000),
            PollTier::Notifications(1),
            true
        ));
        assert!(next_due(
            6_000,
            Some(1000),
            PollTier::Notifications(1),
            true
        ));
    }

    #[test]
    fn watch_is_per_serial() {
        let mut poller = PollerState::default();
        poller.watch_sections("SER_A", &["overview".into(), "notifications".into()]);
        poller.watch_sections("SER_B", &["notifications".into()]);

        assert!(poller.is_watched("SER_A", "overview"));
        assert!(poller.is_watched("SER_A", "notifications"));
        assert!(!poller.is_watched("SER_B", "overview"));
        assert!(poller.is_watched("SER_B", "notifications"));

        poller.unwatch("SER_A");
        assert!(!poller.is_watched("SER_A", "overview"));
        assert!(!poller.is_watched("SER_A", "notifications"));
        assert!(poller.is_watched("SER_B", "notifications"));
    }

    #[test]
    fn poller_stops_and_clears_on_disconnect() {
        let state = PhoneHubState::default();
        {
            let mut poller = state.poller.lock().unwrap();
            poller.watch_sections("SER_A", &["overview".into(), "notifications".into()]);
            poller.watch_sections("SER_B", &["notifications".into()]);
        }
        {
            let mut msgs = state.messages.lock().unwrap();
            let rec = ParsedNotificationRecord {
                base: PhoneNotification {
                    key: "k1".into(),
                    package: "com.example.chat".into(),
                    app_label: None,
                    title: "Alex".into(),
                    text: "hi".into(),
                    posted_unix: 1_700_000_000,
                },
                shortcut: Some("alex".into()),
                conversation_title: None,
                is_group: false,
                is_messaging: true,
                messages: vec![ParsedChatEntry {
                    sender: "Alex".into(),
                    text: "hi".into(),
                    timestamp_ms: 1_700_000_000_000,
                }],
            };
            msgs.ingest("SER_A", std::slice::from_ref(&rec), &HashMap::new());
            msgs.ingest("SER_B", &[rec], &HashMap::new());
        }

        disconnect_cleanup_state(&state, "SER_A");

        let poller = state.poller.lock().unwrap();
        assert!(!poller.is_watched("SER_A", "overview"));
        assert!(!poller.is_watched("SER_A", "notifications"));
        assert!(poller.is_watched("SER_B", "notifications"));

        let msgs = state.messages.lock().unwrap();
        assert!(msgs.conversations("SER_A").is_empty());
        assert_eq!(msgs.conversations("SER_B").len(), 1);
    }

    #[test]
    fn debug_mode_poll_counter_increments() {
        let mut poller = PollerState::default();
        assert_eq!(poller.poll_count("SER_A"), 0);
        assert_eq!(poller.record_poll("SER_A"), 1);
        assert_eq!(poller.record_poll("SER_A"), 2);
        assert_eq!(poller.poll_count("SER_A"), 2);
        assert_eq!(poller.poll_count("SER_B"), 0);

        assert_eq!(poller.set_notif_poll_secs(2), 5);
        assert_eq!(poller.set_notif_poll_secs(15), 15);
    }
}
