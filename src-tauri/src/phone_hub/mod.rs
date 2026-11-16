use std::collections::HashMap;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};

pub mod apps;
pub mod audio_only;
#[cfg(test)]
mod e2e_tests;
pub mod media;
pub mod messages;
pub mod overview;
pub mod poller;
pub mod thumbs;

#[derive(Default)]
pub struct PhoneHubState {
    pub apps_cache: Mutex<Option<apps::AppLabelCache>>,
    pub apps_sync: Mutex<apps::AppsSyncState>,
    pub serial_identities: Mutex<HashMap<String, String>>,
    pub media_cache: Mutex<HashMap<String, Vec<media::MediaItem>>>,
    pub messages: Mutex<messages::MessageStore>,
    pub poller: Mutex<poller::PollerState>,
    pub audio_only: Mutex<audio_only::AudioOnlyState>,
}

pub(crate) fn validate_serial(serial: &str) -> Result<(), String> {
    let trimmed = serial.trim();
    if trimmed.is_empty() || trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("Invalid device serial".to_string());
    }
    Ok(())
}

pub(crate) async fn adb(app: &AppHandle) -> Result<tauri_plugin_shell::process::Command, String> {
    crate::commands::spec27_adb_cmd(app).await
}

pub(crate) async fn scrcpy(
    app: &AppHandle,
) -> Result<tauri_plugin_shell::process::Command, String> {
    crate::commands::spec27_scrcpy_cmd(app).await
}

pub(crate) async fn resolve_device_identity(
    app: &AppHandle,
    state: &PhoneHubState,
    serial: &str,
) -> String {
    if let Ok(map) = state.serial_identities.lock() {
        if let Some(id) = map.get(serial) {
            return id.clone();
        }
    }
    if let Some(hw) = crate::commands::spec27_query_hw_serial(app, serial).await {
        if let Ok(mut map) = state.serial_identities.lock() {
            map.insert(serial.to_string(), hw.clone());
        }
        return hw;
    }
    serial.to_string()
}

pub fn on_app_exit(app: &AppHandle) {
    audio_only::stop_all(app);
    if let Some(state) = app.try_state::<PhoneHubState>() {
        if let Ok(mut poller) = state.poller.lock() {
            let serials: Vec<String> = poller.entries.keys().cloned().collect();
            for serial in serials {
                poller.unwatch(&serial);
            }
        }
        if let Ok(mut msgs) = state.messages.lock() {
            msgs.clear_all();
        }
    }
}
