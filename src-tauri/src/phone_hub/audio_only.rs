use std::collections::HashMap;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};

use crate::phone_hub::{scrcpy, validate_serial, PhoneHubState};
use crate::state::AppState;

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct PhoneAudioStateEvent {
    pub serial: String,
    pub playing: bool,
}

pub struct AudioOnlyEntry {
    pub generation: u64,
    pub child: CommandChild,
}

#[derive(Default)]
pub struct AudioOnlyState {
    pub active: HashMap<String, AudioOnlyEntry>,
    pub next_gen: u64,
}

pub fn can_start_audio_only(
    mirror_audio_active: bool,
    _already_playing: bool,
) -> Result<(), &'static str> {
    if mirror_audio_active {
        return Err("Mirror session with audio is already active for this device");
    }
    Ok(())
}

pub fn build_audio_only_args(serial: &str) -> Vec<String> {
    vec![
        "--serial".to_string(),
        serial.to_string(),
        "--no-video".to_string(),
        "--no-window".to_string(),
    ]
}

fn is_mirror_audio_active(app: &AppHandle, serial: &str) -> bool {
    let Some(app_state) = app.try_state::<AppState>() else {
        return false;
    };
    let Ok(sessions) = app_state.sessions.lock() else {
        return false;
    };
    sessions.values().any(|s| {
        s.serial == serial
            && !s
                .args
                .iter()
                .any(|a| a == "--no-audio" || a == "--no-audio-playback")
    })
}

pub fn stop_for_serial(app: &AppHandle, serial: &str) {
    let removed = if let Some(state) = app.try_state::<PhoneHubState>() {
        if let Ok(mut guard) = state.audio_only.lock() {
            guard.active.remove(serial)
        } else {
            None
        }
    } else {
        None
    };

    if let Some(entry) = removed {
        let _ = entry.child.kill();
        let _ = app.emit(
            "phone-audio-state",
            PhoneAudioStateEvent {
                serial: serial.to_string(),
                playing: false,
            },
        );
    }
}

pub fn stop_all(app: &AppHandle) {
    let drained: Vec<(String, AudioOnlyEntry)> =
        if let Some(state) = app.try_state::<PhoneHubState>() {
            if let Ok(mut guard) = state.audio_only.lock() {
                guard.active.drain().collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };

    for (serial, entry) in drained {
        let _ = entry.child.kill();
        let _ = app.emit(
            "phone-audio-state",
            PhoneAudioStateEvent {
                serial,
                playing: false,
            },
        );
    }
}

#[tauri::command]
pub async fn phone_audio_start(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    serial: String,
) -> Result<(), String> {
    validate_serial(&serial)?;
    let mirror_audio_active = is_mirror_audio_active(&app, &serial);
    let already_playing = {
        let guard = state
            .audio_only
            .lock()
            .map_err(crate::state::poison_error)?;
        guard.active.contains_key(&serial)
    };

    can_start_audio_only(mirror_audio_active, already_playing).map_err(|e| e.to_string())?;
    if already_playing {
        let _ = app.emit(
            "phone-audio-state",
            PhoneAudioStateEvent {
                serial,
                playing: true,
            },
        );
        return Ok(());
    }

    let args = build_audio_only_args(&serial);
    let (mut rx, child) = scrcpy(&app)
        .await?
        .args(args)
        .spawn()
        .map_err(|e| e.to_string())?;

    let gen = {
        let mut guard = state
            .audio_only
            .lock()
            .map_err(crate::state::poison_error)?;
        if guard.active.contains_key(&serial) {
            let _ = child.kill();
            return Ok(());
        }
        guard.next_gen = guard.next_gen.saturating_add(1);
        let gen = guard.next_gen;
        guard.active.insert(
            serial.clone(),
            AudioOnlyEntry {
                generation: gen,
                child,
            },
        );
        gen
    };

    let _ = app.emit(
        "phone-audio-state",
        PhoneAudioStateEvent {
            serial: serial.clone(),
            playing: true,
        },
    );

    let app_c = app.clone();
    let serial_c = serial.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(ev) = rx.recv().await {
            if let CommandEvent::Terminated(_) = ev {
                let mut should_emit = false;
                if let Some(hub) = app_c.try_state::<PhoneHubState>() {
                    if let Ok(mut guard) = hub.audio_only.lock() {
                        if guard
                            .active
                            .get(&serial_c)
                            .is_some_and(|e| e.generation == gen)
                        {
                            guard.active.remove(&serial_c);
                            should_emit = true;
                        }
                    }
                }
                if should_emit {
                    let _ = app_c.emit(
                        "phone-audio-state",
                        PhoneAudioStateEvent {
                            serial: serial_c,
                            playing: false,
                        },
                    );
                }
                break;
            }
        }
    });

    Ok(())
}

#[tauri::command]
pub fn phone_audio_stop(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    serial: String,
) -> Result<(), String> {
    validate_serial(&serial)?;
    let removed = {
        let mut guard = state
            .audio_only
            .lock()
            .map_err(crate::state::poison_error)?;
        guard.active.remove(&serial)
    };
    if let Some(entry) = removed {
        let _ = entry.child.kill();
    }
    let _ = app.emit(
        "phone-audio-state",
        PhoneAudioStateEvent {
            serial,
            playing: false,
        },
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_only_refused_when_mirror_audio_active() {
        assert!(can_start_audio_only(true, false).is_err());
        assert!(can_start_audio_only(true, true).is_err());
    }

    #[test]
    fn idempotent_start_and_verified_scrcpy_args() {
        assert!(can_start_audio_only(false, false).is_ok());
        assert!(can_start_audio_only(false, true).is_ok());

        let args = build_audio_only_args("SER123");
        assert_eq!(
            args,
            vec!["--serial", "SER123", "--no-video", "--no-window"]
        );
    }
}
