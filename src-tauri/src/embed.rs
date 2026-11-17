use tauri::AppHandle;

/// Smallest side (px) the embedded scrcpy window may get. Smaller sizes make the
/// Flex virtual display request a size no encoder supports (scrcpy 5.0 crash).
pub const MIN_EMBED_SIDE: i32 = 320;
/// Largest allowed long:short aspect ratio.
pub const MAX_EMBED_ASPECT: i32 = 4;

/// Single choke point for sizing the embedded child window. `None` = skip resize
/// (keep the last good size): non-positive size or minimized wrapper.
pub fn safe_embed_size(w: i32, h: i32, minimized: bool) -> Option<(i32, i32)> {
    if minimized || w <= 0 || h <= 0 {
        return None;
    }
    let mut w = w.max(MIN_EMBED_SIDE);
    let mut h = h.max(MIN_EMBED_SIDE);
    if w > h.saturating_mul(MAX_EMBED_ASPECT) {
        h = (w + MAX_EMBED_ASPECT - 1) / MAX_EMBED_ASPECT;
    } else if h > w.saturating_mul(MAX_EMBED_ASPECT) {
        w = (h + MAX_EMBED_ASPECT - 1) / MAX_EMBED_ASPECT;
    }
    Some((w, h))
}

#[cfg(windows)]
mod win_embed {
    use crate::state::AppState;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Duration;
    use tauri::{AppHandle, Manager};
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, RECT};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClientRect, GetWindowLongPtrW, GetWindowThreadProcessId, IsIconic,
        IsWindowVisible, SetParent, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, HWND_TOP,
        SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOSIZE, SWP_SHOWWINDOW, WS_CAPTION, WS_CHILD,
        WS_CLIPCHILDREN, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
    };

    static EMBEDDED_HWNDS: OnceLock<Arc<Mutex<HashMap<String, isize>>>> = OnceLock::new();

    fn get_embedded_map() -> &'static Arc<Mutex<HashMap<String, isize>>> {
        EMBEDDED_HWNDS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
    }

    pub fn get_embedded_hwnd_for_serial(serial: &str) -> Option<isize> {
        get_embedded_map()
            .lock()
            .ok()
            .and_then(|map| map.get(serial).copied())
    }
    pub fn remove_embedded_hwnd_for_serial(serial: &str) {
        if let Ok(mut map) = get_embedded_map().lock() {
            map.remove(serial);
        }
    }

    struct EnumData {
        target_pid: u32,
        tauri_hwnd_raw: isize,
        hwnd: Option<isize>,
    }

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = &mut *(lparam.0 as *mut EnumData);
        // Ensure we never match or reparent AnyLeap's own Tauri window into itself
        if (hwnd.0 as isize) == data.tauri_hwnd_raw {
            return BOOL(1);
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == data.target_pid && IsWindowVisible(hwnd).as_bool() {
            data.hwnd = Some(hwnd.0 as isize);
            return BOOL(0);
        }
        BOOL(1)
    }

    fn find_hwnd_by_pid(target_pid: u32, tauri_hwnd_raw: isize) -> Option<isize> {
        let mut data = EnumData {
            target_pid,
            tauri_hwnd_raw,
            hwnd: None,
        };
        let _ =
            unsafe { EnumWindows(Some(enum_proc), LPARAM(&mut data as *mut EnumData as isize)) };
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

        // scrcpy may take a brief moment (especially on wireless) to create its window; retry for up to 12 seconds.
        let mut scrcpy_hwnd_raw: Option<isize> = None;
        for _ in 0..120 {
            if let Some(h) = find_hwnd_by_pid(target_pid, tauri_hwnd_raw) {
                scrcpy_hwnd_raw = Some(h);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        let scrcpy_hwnd_raw = scrcpy_hwnd_raw
            .ok_or_else(|| format!("scrcpy HWND not found for pid {}", target_pid))?;

        if scrcpy_hwnd_raw == tauri_hwnd_raw {
            return Err("Cannot embed host Tauri window into itself".to_string());
        }

        let scrcpy_hwnd = HWND(scrcpy_hwnd_raw as *mut _);
        let tauri_hwnd = HWND(tauri_hwnd_raw as *mut _);

        unsafe {
            // Reparent window
            let _ = SetParent(scrcpy_hwnd, Some(tauri_hwnd));
            let parent_style = GetWindowLongPtrW(tauri_hwnd, GWL_STYLE);
            SetWindowLongPtrW(
                tauri_hwnd,
                GWL_STYLE,
                parent_style | WS_CLIPCHILDREN.0 as isize,
            );
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
            let width = rect.right - rect.left - sidebar_w;
            let height = rect.bottom - rect.top - title_h;

            let minimized = IsIconic(tauri_hwnd).as_bool();
            let (flags, size) = match super::safe_embed_size(width, height, minimized) {
                Some(sz) => (SWP_FRAMECHANGED | SWP_SHOWWINDOW, sz),
                None => (SWP_FRAMECHANGED | SWP_SHOWWINDOW | SWP_NOSIZE, (0, 0)),
            };
            let _ = SetWindowPos(
                scrcpy_hwnd,
                Some(HWND_TOP),
                0,
                title_h,
                size.0,
                size.1,
                flags,
            );
        }

        if let Ok(mut map) = get_embedded_map().lock() {
            map.insert(serial.to_string(), scrcpy_hwnd_raw);
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
        let tauri_hwnd_raw = win.hwnd().map_err(|e| e.to_string())?.0 as isize;

        let cached_hwnd = get_embedded_map()
            .lock()
            .ok()
            .and_then(|map| map.get(serial).copied());

        let scrcpy_hwnd_raw = match cached_hwnd {
            Some(h) => h,
            None => {
                let state = app.state::<AppState>();
                let pid = {
                    let map = state.sessions.lock().map_err(|e| e.to_string())?;
                    map.values().find(|s| s.serial == serial).map(|s| s.pid)
                }
                .ok_or_else(|| format!("No active session found for serial '{}'", serial))?;
                find_hwnd_by_pid(pid, tauri_hwnd_raw)
                    .ok_or_else(|| format!("scrcpy HWND not found for pid {}", pid))?
            }
        };

        if scrcpy_hwnd_raw == tauri_hwnd_raw {
            return Err("Target window is host window".to_string());
        }

        let scrcpy_hwnd = HWND(scrcpy_hwnd_raw as *mut _);

        let scale = win.scale_factor().unwrap_or(1.0);
        let pos_x = x.unwrap_or(0);
        let pos_y = y.unwrap_or((36.0 * scale) as i32);
        let minimized = unsafe { IsIconic(HWND(tauri_hwnd_raw as *mut _)) }.as_bool();
        let Some((w, h)) = super::safe_embed_size(
            i32::try_from(width).unwrap_or(i32::MAX),
            i32::try_from(height).unwrap_or(i32::MAX),
            minimized,
        ) else {
            return Ok(());
        };

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

pub fn get_embedded_scrcpy_hwnd(serial: &str) -> Option<isize> {
    #[cfg(windows)]
    {
        win_embed::get_embedded_hwnd_for_serial(serial)
    }
    #[cfg(not(windows))]
    {
        let _ = serial;
        None
    }
}

pub fn remove_embedded_scrcpy_hwnd(serial: &str) {
    #[cfg(windows)]
    {
        win_embed::remove_embedded_hwnd_for_serial(serial);
    }
    #[cfg(not(windows))]
    {
        let _ = serial;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_zero_negative_and_minimized() {
        assert_eq!(safe_embed_size(0, 500, false), None);
        assert_eq!(safe_embed_size(500, 0, false), None);
        assert_eq!(safe_embed_size(-5, 500, false), None);
        assert_eq!(safe_embed_size(800, 600, true), None);
    }

    #[test]
    fn clamps_tiny_to_min() {
        assert_eq!(safe_embed_size(150, 150, false), Some((320, 320)));
        assert_eq!(safe_embed_size(1, 1, false), Some((320, 320)));
    }

    #[test]
    fn clamps_extreme_aspect_by_growing_short_side() {
        assert_eq!(safe_embed_size(1919, 99, false), Some((1919, 480)));
        assert_eq!(safe_embed_size(333, 1777, false), Some((445, 1777)));
        assert_eq!(safe_embed_size(2000, 500, false), Some((2000, 500)));
    }

    #[test]
    fn normal_unchanged() {
        assert_eq!(safe_embed_size(1001, 601, false), Some((1001, 601)));
        assert_eq!(safe_embed_size(1859, 1043, false), Some((1859, 1043)));
    }
}
