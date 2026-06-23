mod commands;
mod state;

use state::AppState;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::list_devices,
            commands::start_mirror,
            commands::stop_mirror,
            commands::list_sessions,
            commands::discover_wireless,
            commands::pair_device,
            commands::connect_device,
            commands::disconnect_device,
        ])
        .on_window_event(|window, event| {
            // Kill any running scrcpy children when the main window closes,
            // so we never leave orphan processes behind.
            if let tauri::WindowEvent::Destroyed = event {
                let app = window.app_handle();
                if let Some(state) = app.try_state::<AppState>() {
                    let mut sessions = state.sessions.lock().unwrap();
                    for (_, session) in sessions.drain() {
                        let _ = session.child.kill();
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
