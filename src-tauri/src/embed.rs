use tauri::AppHandle;

#[cfg(windows)]
mod win_embed {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Duration;
    use tauri::{AppHandle, Manager};
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClientRect, GetWindowLongPtrW, GetWindowThreadProcessId,
        IsWindowVisible, SetParent, SetWindowLongPtrW, SetWindowPos,
        GWL_STYLE, HWND_TOP, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_SHOWWINDOW,
        WS_CAPTION, WS_CHILD, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
    };
    use crate::state::AppState;

    static EMBEDDED_HWNDS: OnceLock<Arc<Mutex<HashMap<String, isize>>>> = OnceLock::new();

    fn get_embedded_map() -> &'static Arc<Mutex<HashMap<String, isize>>> {
        EMBEDDED_HWNDS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
    }

    struct EnumData {
        target_pid: u32,
        hwnd: Option<HWND>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let data = &mut *(lparam.0 as *mut EnumData);
        if pid == data.target_pid && IsWindowVisible(hwnd).as_bool() {
            data.hwnd = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    fn find_hwnd_by_pid(target_pid: u32) -> Option<HWND> {
        let mut data = EnumData {
            target_pid,
            hwnd: None,
        };
        let _ = unsafe { EnumWindows(Some(enum_proc), LPARAM(&mut data as *mut EnumData as isize)) };
        data.hwnd
    }

    pub async fn embed_mirror_impl(
        app: &AppHandle,
        window_label: &str,
        serial: &str,
    ) -> Result<(), String> {
        let win = app
            .get_webview_window(window_label)
            .ok_or_else(|| format!("Window '{}' not found", window_label))?;
        let tauri_hwnd_raw = win.hwnd().map_err(|e| e.to_string())?.0 as isize;

        let state = app.state::<AppState>();
        let target_pid = {
            let map = state.sessions.lock().map_err(|e| e.to_string())?;
            map.values().find(|s| s.serial == serial).map(|s| s.pid)
        }
        .ok_or_else(|| format!("No active session found for serial '{}'", serial))?;

        // scrcpy may take a brief moment to create its window; retry for up to 3 seconds.
        let mut scrcpy_hwnd_raw: Option<isize> = None;
        for _ in 0..60 {
            if let Some(h) = find_hwnd_by_pid(target_pid) {
                scrcpy_hwnd_raw = Some(h.0 as isize);
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        let scrcpy_hwnd_raw = scrcpy_hwnd_raw
            .ok_or_else(|| format!("scrcpy HWND not found for pid {}", target_pid))?;
        let scrcpy_hwnd = HWND(scrcpy_hwnd_raw as *mut _);

        let tauri_hwnd = HWND(tauri_hwnd_raw as *mut _);
        unsafe {
            // Reparent window
            let _ = SetParent(scrcpy_hwnd, Some(tauri_hwnd));

            // Update window styles: remove popups/captions/thickframe, make it child + visible
            let mut style = GetWindowLongPtrW(scrcpy_hwnd, GWL_STYLE);
            style &= !(WS_POPUP.0 as isize | WS_CAPTION.0 as isize | WS_THICKFRAME.0 as isize);
            style |= WS_CHILD.0 as isize | WS_VISIBLE.0 as isize;
            SetWindowLongPtrW(scrcpy_hwnd, GWL_STYLE, style);

            // Initial bounds: full client area minus right sidebar (44px * scale)
            let mut rect = RECT::default();
            let _ = GetClientRect(tauri_hwnd, &mut rect);
            let scale = win.scale_factor().unwrap_or(1.0);
            let title_h = (36.0 * scale) as i32;
            let sidebar_w = (44.0 * scale) as i32;
            let width = (rect.right - rect.left - sidebar_w).max(1);
            let height = (rect.bottom - rect.top - title_h).max(1);

            let _ = SetWindowPos(
                scrcpy_hwnd,
                Some(HWND_TOP),
                0,
                title_h,
                width,
                height,
                SWP_FRAMECHANGED | SWP_SHOWWINDOW,
            );
        }

        if let Ok(mut map) = get_embedded_map().lock() {
            map.insert(serial.to_string(), scrcpy_hwnd.0 as isize);
        }

        Ok(())
    }

    pub fn resize_embedded_mirror_impl(
        app: &AppHandle,
        window_label: &str,
        serial: &str,
        width: u32,
        height: u32,
        x: Option<i32>,
        y: Option<i32>,
    ) -> Result<(), String> {
        let win = app
            .get_webview_window(window_label)
            .ok_or_else(|| format!("Window '{}' not found", window_label))?;

        let cached_hwnd = get_embedded_map()
            .lock()
            .ok()
            .and_then(|map| map.get(serial).copied());

        let scrcpy_hwnd = match cached_hwnd {
            Some(h) => HWND(h as *mut _),
            None => {
                let state = app.state::<AppState>();
                let pid = {
                    let map = state.sessions.lock().map_err(|e| e.to_string())?;
                    map.values().find(|s| s.serial == serial).map(|s| s.pid)
                }
                .ok_or_else(|| format!("No active session found for serial '{}'", serial))?;
                find_hwnd_by_pid(pid)
                    .ok_or_else(|| format!("scrcpy HWND not found for pid {}", pid))?
            }
        };

        let scale = win.scale_factor().unwrap_or(1.0);
        let pos_x = x.unwrap_or(0);
        let pos_y = y.unwrap_or((36.0 * scale) as i32);
        let w = (width as i32).max(1);
        let h = (height as i32).max(1);

        unsafe {
            let _ = SetWindowPos(
                scrcpy_hwnd,
                Some(HWND_TOP),
                pos_x,
                pos_y,
                w,
                h,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }

        Ok(())
    }
}

#[tauri::command]
pub async fn embed_mirror(
    app: AppHandle,
    window_label: String,
    serial: String,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        win_embed::embed_mirror_impl(&app, &window_label, &serial).await
    }
    #[cfg(not(windows))]
    {
        let _ = (app, window_label, serial);
        Err("Embedded mirror is only supported on Windows".to_string())
    }
}

#[tauri::command]
pub fn resize_embedded_mirror(
    app: AppHandle,
    window_label: String,
    serial: String,
    width: u32,
    height: u32,
    x: Option<i32>,
    y: Option<i32>,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        win_embed::resize_embedded_mirror_impl(&app, &window_label, &serial, width, height, x, y)
    }
    #[cfg(not(windows))]
    {
        let _ = (app, window_label, serial, width, height, x, y);
        Err("Embedded mirror is only supported on Windows".to_string())
    }
}
