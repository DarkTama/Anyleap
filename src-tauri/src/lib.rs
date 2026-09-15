mod commands;
mod embed;
mod state;
mod wheel_swipe;
use state::AppState;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::Manager;

/// Bring the main window back to the foreground (from tray / minimized).
fn show_main(app: &tauri::AppHandle) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_http::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
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
            commands::send_keyevent,
            commands::open_notifications,
            commands::restart_with_screen_off,
            commands::mirror_rect,
            commands::toggle_device_orientation,
            commands::handle_dropped_files,
            commands::list_installed_apps,
            commands::launch_app,
            commands::take_screenshot,
            commands::get_system_resolution,
            wheel_swipe::set_wheel_swipe,
            embed::embed_mirror,
            embed::resize_embedded_mirror,
        ])
        .setup(|app| {
            // Wheel-to-swipe hook/worker/refresher threads (Windows no-ops elsewhere).
            wheel_swipe::init(app.handle());

            // System-tray icon: left-click restores the window; the menu offers
            // Show / Quit. Pairs with the frontend's close-to-tray handler.
            let show_i = MenuItem::with_id(app, "show", "Show AnyLeap", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show_i, &quit_i])?;
            let _tray = TrayIconBuilder::with_id("main-tray")
                .icon(app.default_window_icon().unwrap().clone())
                .tooltip("AnyLeap")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => show_main(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main(tray.app_handle());
                    }
                })
                .build(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // Kill any running scrcpy children when the *main* window is destroyed,
            // so we never leave orphan processes behind. Closing the floating
            // controls window (or hiding main to tray) must not touch sessions.
            if let tauri::WindowEvent::Destroyed = event {
                let label = window.label();
                if label.starts_with("mirror-") {
                    let serial = label.trim_start_matches("mirror-");
                    let app = window.app_handle();
                    if let Some(state) = app.try_state::<AppState>() {
                        if let Ok(mut sessions) = state.sessions.lock() {
                            let to_remove: Vec<String> = sessions
                                .iter()
                                .filter(|(_, s)| s.serial == serial)
                                .map(|(id, _)| id.clone())
                                .collect();
                            for id in to_remove {
                                if let Some(s) = sessions.remove(&id) {
                                    let _ = s.child.kill();
                                }
                            }
                        }
                    }
                    return;
                }
                if label != "main" {
                    return;
                }
                let app = window.app_handle();
                if let Some(state) = app.try_state::<AppState>() {
                    match state.sessions.lock() {
                        Ok(mut sessions) => {
                            for (_, session) in sessions.drain() {
                                let _ = session.child.kill();
                            }
                        }
                        Err(e) => {
                            eprintln!("Sessions mutex poisoned, could not kill child processes: {}", e);
                        }
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
