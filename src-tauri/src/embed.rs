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

/// Focus routing decision when the user clicks inside an embedded mirror host window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedFocusAction {
    None,
    FocusScrcpy,
    KeepWebview,
}

/// True when `(px, py)` lies inside the half-open screen/client rectangle `[left, right) x [top, bottom)`.
pub fn is_point_in_rect(px: i32, py: i32, left: i32, top: i32, right: i32, bottom: i32) -> bool {
    px >= left && px < right && py >= top && py < bottom
}

/// Decide whether a mouse activation/click on the embedded mirror host should move Win32
/// keyboard focus to the embedded scrcpy child or leave it with the WebView2 UI.
pub fn decide_click_focus(
    in_scrcpy_rect: bool,
    scrcpy_visible: bool,
    host_minimized: bool,
) -> EmbedFocusAction {
    if host_minimized || !scrcpy_visible {
        return EmbedFocusAction::None;
    }
    if in_scrcpy_rect {
        EmbedFocusAction::FocusScrcpy
    } else {
        EmbedFocusAction::KeepWebview
    }
}

/// Guard against stealing keyboard focus when another application holds the foreground.
/// When `fg_hwnd == 0` (isolated/headless desktop transitions), allow focusing only on an
/// explicit click inside the scrcpy child so background activation never grabs focus.
pub fn should_focus_scrcpy_for_foreground(
    fg_hwnd: isize,
    tauri_hwnd: isize,
    fg_is_child_of_tauri: bool,
    from_direct_click: bool,
) -> bool {
    if tauri_hwnd == 0 {
        return false;
    }
    if fg_hwnd == tauri_hwnd || fg_is_child_of_tauri {
        return true;
    }
    fg_hwnd == 0 && from_direct_click
}

/// Cached metadata for an embedded scrcpy child window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddedEntry {
    pub hwnd: isize,
    pub pid: u32,
    pub host_hwnd: isize,
}

/// Cache resolution decision for `embed_mirror`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbedCacheAction {
    /// No valid cache hit for this `(pid, host_hwnd)` -> evict if stale and embed fresh.
    EmbedFresh { evict_stale: bool },
    /// Cached child HWND already belongs to this live session PID and host window.
    AlreadyEmbedded(isize),
}

/// Decide whether a cached embed entry is reusable for `(current_pid, current_host_hwnd)`
/// or stale and in need of eviction.
pub fn decide_embed_cache(
    cached: Option<EmbeddedEntry>,
    current_pid: u32,
    current_host_hwnd: isize,
    hwnd_valid: bool,
    is_child_of_host: bool,
) -> EmbedCacheAction {
    match cached {
        None => EmbedCacheAction::EmbedFresh { evict_stale: false },
        Some(entry) => {
            if entry.pid == current_pid
                && entry.host_hwnd == current_host_hwnd
                && hwnd_valid
                && is_child_of_host
            {
                EmbedCacheAction::AlreadyEmbedded(entry.hwnd)
            } else {
                EmbedCacheAction::EmbedFresh { evict_stale: true }
            }
        }
    }
}

/// True when a cached embed entry can be used for resize without re-resolving by PID.
pub fn is_cached_embed_valid_for_resize(
    cached: Option<EmbeddedEntry>,
    active_pid: Option<u32>,
    current_host_hwnd: isize,
    hwnd_valid: bool,
) -> bool {
    let Some(entry) = cached else {
        return false;
    };
    let Some(pid) = active_pid else {
        return false;
    };
    entry.pid == pid && entry.host_hwnd == current_host_hwnd && hwnd_valid
}

/// True when an embed entry should be removed on cleanup (either unconditional or matching `expected_pid`).
pub fn should_remove_embed_for_pid(
    cached: Option<EmbeddedEntry>,
    expected_pid: Option<u32>,
) -> bool {
    match (cached, expected_pid) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(entry), Some(pid)) => entry.pid == pid,
    }
}

/// True when `Ctrl+V` keypress inside an embedded mirror should be intercepted to
/// push a clipboard image to the phone instead of passing through to scrcpy.
/// Text paste (`has_text`) always takes precedence and passes through to scrcpy.
pub fn should_intercept_ctrl_v_image(
    scrcpy_focused: bool,
    has_text: bool,
    has_image: bool,
) -> bool {
    scrcpy_focused && !has_text && has_image
}

/// Convert raw Windows `CF_DIB` clipboard bytes into an encoded PNG byte buffer.
pub fn dib_to_png(dib: &[u8]) -> Result<Vec<u8>, String> {
    if dib.len() < 40 {
        return Err("Clipboard DIB header too small".to_string());
    }
    let bi_size = u32::from_le_bytes(dib[0..4].try_into().unwrap());
    if (bi_size as usize) < 40 || (bi_size as usize) > dib.len() {
        return Err("Invalid clipboard DIB header size".to_string());
    }
    let bi_bit_count = u16::from_le_bytes(dib[14..16].try_into().unwrap());
    let bi_compression = u32::from_le_bytes(dib[16..20].try_into().unwrap());
    let bi_clr_used = u32::from_le_bytes(dib[32..36].try_into().unwrap());

    let color_table_bytes = if bi_clr_used > 0 {
        bi_clr_used.saturating_mul(4)
    } else if bi_bit_count <= 8 {
        (1u32 << bi_bit_count).saturating_mul(4)
    } else if bi_compression == 3 && bi_size == 40 {
        // BI_BITFIELDS appends 3 DWORD color masks right after BITMAPINFOHEADER.
        12
    } else {
        0
    };

    let bf_off_bits = 14u32
        .saturating_add(bi_size)
        .saturating_add(color_table_bytes);
    let total_len = 14u32.saturating_add(dib.len() as u32);

    let mut bmp = Vec::with_capacity(14 + dib.len());
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&total_len.to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&bf_off_bits.to_le_bytes());
    bmp.extend_from_slice(dib);

    let img = image::load_from_memory_with_format(&bmp, image::ImageFormat::Bmp)
        .map_err(|e| format!("Failed to decode clipboard DIB: {e}"))?;
    let mut png = Vec::new();
    let mut cursor = std::io::Cursor::new(&mut png);
    img.write_to(&mut cursor, image::ImageFormat::Png)
        .map_err(|e| format!("Failed to encode clipboard PNG: {e}"))?;
    Ok(png)
}

#[cfg(windows)]
mod win_embed {
    use super::{
        decide_click_focus, decide_embed_cache, dib_to_png, is_cached_embed_valid_for_resize,
        is_point_in_rect, should_focus_scrcpy_for_foreground, should_intercept_ctrl_v_image,
        should_remove_embed_for_pid, EmbedCacheAction, EmbedFocusAction, EmbeddedEntry,
    };
    use crate::state::AppState;
    use serde::Serialize;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::Duration;
    use tauri::{AppHandle, Emitter, Manager};
    use windows::core::{w, BOOL, PCWSTR};
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::System::DataExchange::{
        CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
        RegisterClipboardFormatW,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, GetFocus, SetFocus, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_V,
    };
    use windows::Win32::UI::Shell::{
        DefSubclassProc, GetWindowSubclass, RemoveWindowSubclass, SetWindowSubclass,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, EnumWindows, GetClientRect, GetCursorPos, GetForegroundWindow,
        GetGUIThreadInfo, GetMessageW, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId,
        IsChild, IsHungAppWindow, IsIconic, IsWindow, IsWindowVisible, PostMessageW,
        SendMessageTimeoutW, SetParent, SetWindowLongPtrW, SetWindowPos, SetWindowsHookExW,
        GUITHREADINFO, GWL_EXSTYLE, GWL_STYLE, HWND_TOP, KBDLLHOOKSTRUCT, LLKHF_INJECTED, MSG,
        SMTO_ABORTIFHUNG, SMTO_BLOCK, SWP_ASYNCWINDOWPOS, SWP_FRAMECHANGED, SWP_NOACTIVATE,
        SWP_NOSIZE, SWP_SHOWWINDOW, WA_INACTIVE, WH_KEYBOARD_LL, WM_ACTIVATE, WM_APP, WM_KEYDOWN,
        WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEACTIVATE, WM_NCDESTROY, WM_NULL, WM_PARENTNOTIFY,
        WM_RBUTTONDOWN, WM_SETFOCUS, WM_SYSKEYDOWN, WM_XBUTTONDOWN, WS_CAPTION, WS_CHILD,
        WS_CLIPCHILDREN, WS_EX_NOPARENTNOTIFY, WS_POPUP, WS_THICKFRAME, WS_VISIBLE,
    };

    const WM_APP_FOCUS_SCRCPY: u32 = WM_APP + 0x4B;
    const MIRROR_HOST_SUBCLASS_ID: usize = 0x414C_454D;
    const RESPONSIVE_TIMEOUT_MS: u32 = 50;

    static EMBEDDED_HWNDS: OnceLock<Arc<Mutex<HashMap<String, EmbeddedEntry>>>> = OnceLock::new();
    static HOST_STATES: OnceLock<Mutex<HashMap<isize, Arc<HostEmbedState>>>> = OnceLock::new();
    static HAS_EMBEDDED_MIRROR: AtomicBool = AtomicBool::new(false);
    static PASTE_TX: OnceLock<tokio::sync::mpsc::Sender<PasteRequest>> = OnceLock::new();
    static PNG_CLIPBOARD_FORMAT: OnceLock<u32> = OnceLock::new();
    static KBD_HOOK_STARTED: OnceLock<()> = OnceLock::new();

    struct PasteRequest {
        serial: String,
        window_label: String,
    }

    #[derive(Clone, Serialize)]
    struct MirrorClipboardImageResult {
        serial: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        dest: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    }

    struct HostEmbedState {
        scrcpy_hwnd: AtomicIsize,
        prefer_scrcpy_focus: AtomicBool,
        attached_ui_thread: AtomicU32,
        attached_scrcpy_thread: AtomicU32,
        serial: Mutex<String>,
        window_label: Mutex<String>,
    }

    fn get_embedded_map() -> &'static Arc<Mutex<HashMap<String, EmbeddedEntry>>> {
        EMBEDDED_HWNDS.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
    }

    fn get_host_states() -> &'static Mutex<HashMap<isize, Arc<HostEmbedState>>> {
        HOST_STATES.get_or_init(|| Mutex::new(HashMap::new()))
    }

    fn png_clipboard_format() -> u32 {
        *PNG_CLIPBOARD_FORMAT.get_or_init(|| unsafe { RegisterClipboardFormatW(w!("PNG")) })
    }

    unsafe fn detach_thread_input(state: &HostEmbedState) {
        let ui_tid = state.attached_ui_thread.swap(0, Ordering::AcqRel);
        let scrcpy_tid = state.attached_scrcpy_thread.swap(0, Ordering::AcqRel);
        if ui_tid != 0 && scrcpy_tid != 0 && ui_tid != scrcpy_tid {
            let _ = AttachThreadInput(ui_tid, scrcpy_tid, false);
        }
    }

    unsafe fn is_window_responsive(hwnd: HWND) -> bool {
        if hwnd.0.is_null() || !IsWindow(Some(hwnd)).as_bool() {
            return false;
        }
        if IsHungAppWindow(hwnd).as_bool() {
            return false;
        }
        let mut result: usize = 0;
        let sent = SendMessageTimeoutW(
            hwnd,
            WM_NULL,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            RESPONSIVE_TIMEOUT_MS,
            Some(&mut result),
        );
        sent.0 != 0
    }

    pub fn get_embedded_hwnd_for_serial(serial: &str) -> Option<isize> {
        get_embedded_map()
            .lock()
            .ok()
            .and_then(|map| map.get(serial).map(|e| e.hwnd))
    }

    pub fn remove_embedded_hwnd_for_serial_if_pid(serial: &str, expected_pid: Option<u32>) {
        let (removed, any_left) = get_embedded_map()
            .lock()
            .ok()
            .map(|mut map| {
                let cached = map.get(serial).copied();
                let removed = if should_remove_embed_for_pid(cached, expected_pid) {
                    map.remove(serial).map(|e| e.hwnd)
                } else {
                    None
                };
                (removed, !map.is_empty())
            })
            .unwrap_or((None, false));
        HAS_EMBEDDED_MIRROR.store(any_left, Ordering::Release);

        if expected_pid.is_some() && removed.is_none() {
            // Cached entry already belongs to a newer session PID; leave host state intact.
            return;
        }

        if let Ok(mut hosts) = get_host_states().lock() {
            hosts.retain(|&raw_hwnd, _| unsafe {
                IsWindow(Some(HWND(raw_hwnd as *mut _))).as_bool()
            });
            for st in hosts.values() {
                let matches_serial = st
                    .serial
                    .lock()
                    .ok()
                    .map(|s| s.as_str() == serial)
                    .unwrap_or(false);
                let matches_hwnd = removed
                    .map(|h| st.scrcpy_hwnd.load(Ordering::Acquire) == h)
                    .unwrap_or(false);
                if (expected_pid.is_none() && matches_serial) || matches_hwnd {
                    st.scrcpy_hwnd.store(0, Ordering::Release);
                    st.prefer_scrcpy_focus.store(false, Ordering::Release);
                    unsafe { detach_thread_input(st) };
                }
            }
        }
    }

    pub fn remove_embedded_hwnd_for_serial(serial: &str) {
        remove_embedded_hwnd_for_serial_if_pid(serial, None);
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

    unsafe fn point_click_action(
        tauri_hwnd: HWND,
        scrcpy_hwnd: HWND,
        state: &HostEmbedState,
        screen_pt: Option<POINT>,
    ) -> EmbedFocusAction {
        if scrcpy_hwnd.0.is_null() || !IsWindow(Some(scrcpy_hwnd)).as_bool() {
            detach_thread_input(state);
            state.scrcpy_hwnd.store(0, Ordering::Release);
            state.prefer_scrcpy_focus.store(false, Ordering::Release);
            return EmbedFocusAction::None;
        }
        if IsHungAppWindow(scrcpy_hwnd).as_bool() {
            detach_thread_input(state);
            return EmbedFocusAction::None;
        }
        let pt = match screen_pt {
            Some(p) => p,
            None => {
                let mut p = POINT::default();
                if GetCursorPos(&mut p).is_err() {
                    return EmbedFocusAction::None;
                }
                p
            }
        };
        let mut rect = RECT::default();
        if GetWindowRect(scrcpy_hwnd, &mut rect).is_err() {
            return EmbedFocusAction::None;
        }
        let in_rect = is_point_in_rect(pt.x, pt.y, rect.left, rect.top, rect.right, rect.bottom);
        decide_click_focus(
            in_rect,
            IsWindowVisible(scrcpy_hwnd).as_bool(),
            IsIconic(tauri_hwnd).as_bool(),
        )
    }

    unsafe fn focus_scrcpy_child(
        tauri_hwnd: HWND,
        scrcpy_hwnd: HWND,
        state: &HostEmbedState,
        from_direct_click: bool,
    ) {
        if scrcpy_hwnd.0.is_null()
            || !IsWindow(Some(scrcpy_hwnd)).as_bool()
            || !IsWindowVisible(scrcpy_hwnd).as_bool()
            || IsIconic(tauri_hwnd).as_bool()
            || !IsChild(tauri_hwnd, scrcpy_hwnd).as_bool()
        {
            detach_thread_input(state);
            return;
        }

        if !is_window_responsive(scrcpy_hwnd) {
            detach_thread_input(state);
            return;
        }

        let fg = GetForegroundWindow();
        let fg_is_child = !fg.0.is_null() && IsChild(tauri_hwnd, fg).as_bool();
        if !should_focus_scrcpy_for_foreground(
            fg.0 as isize,
            tauri_hwnd.0 as isize,
            fg_is_child,
            from_direct_click,
        ) {
            return;
        }

        if GetFocus() == scrcpy_hwnd {
            return;
        }

        let _ = SetFocus(Some(scrcpy_hwnd));
        if GetFocus() != scrcpy_hwnd {
            let current_thread = GetCurrentThreadId();
            let target_thread = GetWindowThreadProcessId(scrcpy_hwnd, None);
            if target_thread != 0 && target_thread != current_thread {
                let prev_target = state.attached_scrcpy_thread.load(Ordering::Acquire);
                if prev_target != 0 && prev_target != target_thread {
                    detach_thread_input(state);
                }
                if prev_target == target_thread
                    || AttachThreadInput(current_thread, target_thread, true).as_bool()
                {
                    state
                        .attached_ui_thread
                        .store(current_thread, Ordering::Release);
                    state
                        .attached_scrcpy_thread
                        .store(target_thread, Ordering::Release);
                    let _ = SetFocus(Some(scrcpy_hwnd));
                }
            }
        }
    }

    unsafe fn apply_click_focus_action(
        hwnd: HWND,
        state: &HostEmbedState,
        scrcpy_hwnd: HWND,
        action: EmbedFocusAction,
        immediate: bool,
    ) {
        match action {
            EmbedFocusAction::FocusScrcpy => {
                state.prefer_scrcpy_focus.store(true, Ordering::Release);
                if immediate {
                    focus_scrcpy_child(hwnd, scrcpy_hwnd, state, true);
                }
                let _ = PostMessageW(Some(hwnd), WM_APP_FOCUS_SCRCPY, WPARAM(1), LPARAM(0));
            }
            EmbedFocusAction::KeepWebview => {
                state.prefer_scrcpy_focus.store(false, Ordering::Release);
                detach_thread_input(state);
            }
            EmbedFocusAction::None => {}
        }
    }

    unsafe extern "system" fn mirror_host_subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        uid_subclass: usize,
        dw_ref_data: usize,
    ) -> LRESULT {
        if dw_ref_data == 0 {
            return DefSubclassProc(hwnd, msg, wparam, lparam);
        }
        let state = &*(dw_ref_data as *const HostEmbedState);
        let scrcpy_raw = state.scrcpy_hwnd.load(Ordering::Acquire);
        let scrcpy_hwnd = HWND(scrcpy_raw as *mut _);

        match msg {
            WM_PARENTNOTIFY => {
                let child_msg = (wparam.0 & 0xFFFF) as u32;
                if matches!(
                    child_msg,
                    WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN
                ) {
                    let mut pt = POINT {
                        x: (lparam.0 & 0xFFFF) as i16 as i32,
                        y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32,
                    };
                    let screen_pt = if ClientToScreen(hwnd, &mut pt).as_bool() {
                        Some(pt)
                    } else {
                        None
                    };
                    let action = point_click_action(hwnd, scrcpy_hwnd, state, screen_pt);
                    apply_click_focus_action(hwnd, state, scrcpy_hwnd, action, true);
                }
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            WM_MOUSEACTIVATE => {
                let res = DefSubclassProc(hwnd, msg, wparam, lparam);
                let action = point_click_action(hwnd, scrcpy_hwnd, state, None);
                apply_click_focus_action(hwnd, state, scrcpy_hwnd, action, false);
                res
            }
            WM_SETFOCUS => {
                let res = DefSubclassProc(hwnd, msg, wparam, lparam);
                if state.prefer_scrcpy_focus.load(Ordering::Acquire) && !scrcpy_hwnd.0.is_null() {
                    let _ = PostMessageW(Some(hwnd), WM_APP_FOCUS_SCRCPY, WPARAM(0), LPARAM(0));
                }
                res
            }
            WM_ACTIVATE => {
                let res = DefSubclassProc(hwnd, msg, wparam, lparam);
                let active_state = (wparam.0 & 0xFFFF) as u32;
                if active_state == WA_INACTIVE {
                    detach_thread_input(state);
                } else if state.prefer_scrcpy_focus.load(Ordering::Acquire)
                    && !scrcpy_hwnd.0.is_null()
                {
                    let _ = PostMessageW(Some(hwnd), WM_APP_FOCUS_SCRCPY, WPARAM(0), LPARAM(0));
                }
                res
            }
            WM_APP_FOCUS_SCRCPY => {
                if state.prefer_scrcpy_focus.load(Ordering::Acquire) && !scrcpy_hwnd.0.is_null() {
                    focus_scrcpy_child(hwnd, scrcpy_hwnd, state, wparam.0 != 0);
                }
                LRESULT(0)
            }
            WM_NCDESTROY => {
                detach_thread_input(state);
                state.scrcpy_hwnd.store(0, Ordering::Release);
                state.prefer_scrcpy_focus.store(false, Ordering::Release);
                if let Ok(mut map) = get_host_states().lock() {
                    map.remove(&(hwnd.0 as isize));
                }
                let _ = RemoveWindowSubclass(hwnd, Some(mirror_host_subclass_proc), uid_subclass);
                drop(Arc::from_raw(dw_ref_data as *const HostEmbedState));
                DefSubclassProc(hwnd, msg, wparam, lparam)
            }
            _ => DefSubclassProc(hwnd, msg, wparam, lparam),
        }
    }

    unsafe fn install_or_update_host_subclass(
        tauri_hwnd: HWND,
        scrcpy_hwnd: HWND,
        serial: &str,
        window_label: &str,
    ) {
        let host_key = tauri_hwnd.0 as isize;
        let scrcpy_raw = scrcpy_hwnd.0 as isize;

        let mut existing_ref: usize = 0;
        if GetWindowSubclass(
            tauri_hwnd,
            Some(mirror_host_subclass_proc),
            MIRROR_HOST_SUBCLASS_ID,
            Some(&mut existing_ref),
        )
        .as_bool()
            && existing_ref != 0
        {
            let existing = &*(existing_ref as *const HostEmbedState);
            let prev = existing.scrcpy_hwnd.swap(scrcpy_raw, Ordering::AcqRel);
            if prev != scrcpy_raw {
                detach_thread_input(existing);
            }
            if let Ok(mut s) = existing.serial.lock() {
                *s = serial.to_string();
            }
            if let Ok(mut l) = existing.window_label.lock() {
                *l = window_label.to_string();
            }
            existing.prefer_scrcpy_focus.store(true, Ordering::Release);
            return;
        }

        let state = Arc::new(HostEmbedState {
            scrcpy_hwnd: AtomicIsize::new(scrcpy_raw),
            prefer_scrcpy_focus: AtomicBool::new(true),
            attached_ui_thread: AtomicU32::new(0),
            attached_scrcpy_thread: AtomicU32::new(0),
            serial: Mutex::new(serial.to_string()),
            window_label: Mutex::new(window_label.to_string()),
        });
        let raw_ref = Arc::into_raw(state.clone()) as usize;
        if SetWindowSubclass(
            tauri_hwnd,
            Some(mirror_host_subclass_proc),
            MIRROR_HOST_SUBCLASS_ID,
            raw_ref,
        )
        .as_bool()
        {
            if let Ok(mut map) = get_host_states().lock() {
                map.insert(host_key, state);
            }
        } else {
            drop(Arc::from_raw(raw_ref as *const HostEmbedState));
        }
    }

    unsafe fn read_clipboard_bytes(format: u32) -> Option<Vec<u8>> {
        if IsClipboardFormatAvailable(format).is_err() {
            return None;
        }
        for _ in 0..5 {
            if OpenClipboard(None).is_ok() {
                let bytes = GetClipboardData(format).ok().and_then(|handle| {
                    let h_global = windows::Win32::Foundation::HGLOBAL(handle.0);
                    let size = GlobalSize(h_global);
                    if size == 0 || size > 25_000_000 {
                        return None;
                    }
                    let ptr = GlobalLock(h_global) as *const u8;
                    if ptr.is_null() {
                        return None;
                    }
                    let out = std::slice::from_raw_parts(ptr, size).to_vec();
                    let _ = GlobalUnlock(h_global);
                    Some(out)
                });
                let _ = CloseClipboard();
                return bytes;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        None
    }

    fn read_clipboard_image_as_png() -> Result<Vec<u8>, String> {
        unsafe {
            let png_fmt = png_clipboard_format();
            if png_fmt != 0 {
                if let Some(png_bytes) = read_clipboard_bytes(png_fmt) {
                    if !png_bytes.is_empty() {
                        return Ok(png_bytes);
                    }
                }
            }
            if let Some(dib) = read_clipboard_bytes(8) {
                return dib_to_png(&dib);
            }
        }
        Err("No image found on clipboard".to_string())
    }

    unsafe fn is_scrcpy_focused_in_foreground(fg: HWND) -> Option<(String, String)> {
        let fg_raw = fg.0 as isize;
        let hosts = get_host_states().try_lock().ok()?;
        let state = hosts.get(&fg_raw)?.clone();
        drop(hosts);

        let scrcpy_raw = state.scrcpy_hwnd.load(Ordering::Acquire);
        if scrcpy_raw == 0 {
            return None;
        }
        let scrcpy_hwnd = HWND(scrcpy_raw as *mut _);
        if !IsWindow(Some(scrcpy_hwnd)).as_bool() || !IsWindowVisible(scrcpy_hwnd).as_bool() {
            return None;
        }

        let fg_tid = GetWindowThreadProcessId(fg, None);
        let mut fg_gui = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        let scrcpy_focused = if fg_tid != 0 && GetGUIThreadInfo(fg_tid, &mut fg_gui).is_ok() {
            if !fg_gui.hwndFocus.0.is_null() {
                fg_gui.hwndFocus == scrcpy_hwnd || IsChild(scrcpy_hwnd, fg_gui.hwndFocus).as_bool()
            } else {
                state.prefer_scrcpy_focus.load(Ordering::Acquire)
            }
        } else {
            state.prefer_scrcpy_focus.load(Ordering::Acquire)
        };

        if !scrcpy_focused {
            return None;
        }

        let serial = state.serial.try_lock().ok()?.clone();
        let label = state.window_label.try_lock().ok()?.clone();
        if serial.is_empty() {
            return None;
        }
        Some((serial, label))
    }

    unsafe extern "system" fn low_level_kbd_proc(
        code: i32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if code >= 0 && HAS_EMBEDDED_MIRROR.load(Ordering::Relaxed) {
            let msg = wparam.0 as u32;
            if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                let kbd = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                if kbd.vkCode == VK_V.0 as u32 && (kbd.flags.0 & LLKHF_INJECTED.0) == 0 {
                    let ctrl_down = GetAsyncKeyState(VK_CONTROL.0 as i32) < 0;
                    let alt_down = GetAsyncKeyState(VK_MENU.0 as i32) < 0;
                    let win_down = GetAsyncKeyState(VK_LWIN.0 as i32) < 0
                        || GetAsyncKeyState(VK_RWIN.0 as i32) < 0;
                    if ctrl_down && !alt_down && !win_down {
                        let fg = GetForegroundWindow();
                        if !fg.0.is_null() {
                            if let Some((serial, window_label)) =
                                is_scrcpy_focused_in_foreground(fg)
                            {
                                let has_text = IsClipboardFormatAvailable(13).is_ok();
                                let png_fmt = png_clipboard_format();
                                let has_image = IsClipboardFormatAvailable(8).is_ok()
                                    || (png_fmt != 0
                                        && IsClipboardFormatAvailable(png_fmt).is_ok());
                                if should_intercept_ctrl_v_image(true, has_text, has_image) {
                                    if let Some(tx) = PASTE_TX.get() {
                                        if tx
                                            .try_send(PasteRequest {
                                                serial,
                                                window_label,
                                            })
                                            .is_ok()
                                        {
                                            return LRESULT(1);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    fn ensure_paste_hook_started(app: &AppHandle) {
        KBD_HOOK_STARTED.get_or_init(|| {
            let _ = png_clipboard_format();
            let (tx, mut rx) = tokio::sync::mpsc::channel::<PasteRequest>(8);
            let _ = PASTE_TX.set(tx);

            let app_handle = app.clone();
            tauri::async_runtime::spawn(async move {
                while let Some(req) = rx.recv().await {
                    let app_clone = app_handle.clone();
                    let serial = req.serial;
                    let label = req.window_label;
                    let img_res = tokio::task::spawn_blocking(read_clipboard_image_as_png).await;
                    let payload = match img_res {
                        Ok(Ok(png_bytes)) => match crate::commands::push_clipboard_image_bytes(
                            &app_clone, &serial, png_bytes, None,
                        )
                        .await
                        {
                            Ok(dest) => MirrorClipboardImageResult {
                                serial,
                                dest: Some(dest),
                                error: None,
                            },
                            Err(err) => MirrorClipboardImageResult {
                                serial,
                                dest: None,
                                error: Some(err),
                            },
                        },
                        Ok(Err(err)) => MirrorClipboardImageResult {
                            serial,
                            dest: None,
                            error: Some(err),
                        },
                        Err(err) => MirrorClipboardImageResult {
                            serial,
                            dest: None,
                            error: Some(err.to_string()),
                        },
                    };
                    let _ = app_clone.emit_to(&label, "mirror-clipboard-image-result", payload);
                }
            });

            std::thread::spawn(|| unsafe {
                let hmod = match GetModuleHandleW(PCWSTR::null()) {
                    Ok(m) => m,
                    Err(_) => return,
                };
                let hinstance: HINSTANCE = hmod.into();
                if SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_kbd_proc), Some(hinstance), 0)
                    .is_err()
                {
                    return;
                }
                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
            });
        });
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
            map.values()
                .find(|s| s.serial == serial && s.mode == "display")
                .or_else(|| map.values().find(|s| s.serial == serial))
                .map(|s| s.pid)
        }
        .ok_or_else(|| format!("No active session found for serial '{}'", serial))?;

        let cached = get_embedded_map()
            .lock()
            .ok()
            .and_then(|map| map.get(serial).copied());
        let (hwnd_valid, is_child_of_host) = match cached {
            Some(entry) => unsafe {
                let h = HWND(entry.hwnd as *mut _);
                let host = HWND(tauri_hwnd_raw as *mut _);
                let valid = !h.0.is_null() && IsWindow(Some(h)).as_bool();
                let child = valid && IsChild(host, h).as_bool();
                (valid, child)
            },
            None => (false, false),
        };

        let scrcpy_hwnd_raw = match decide_embed_cache(
            cached,
            target_pid,
            tauri_hwnd_raw,
            hwnd_valid,
            is_child_of_host,
        ) {
            EmbedCacheAction::AlreadyEmbedded(existing_hwnd) => existing_hwnd,
            EmbedCacheAction::EmbedFresh { evict_stale } => {
                if evict_stale {
                    remove_embedded_hwnd_for_serial_if_pid(serial, None);
                }
                // scrcpy may take a brief moment (especially on wireless) to create its window; retry for up to 12 seconds.
                let mut found_hwnd: Option<isize> = None;
                for _ in 0..120 {
                    if let Some(h) = find_hwnd_by_pid(target_pid, tauri_hwnd_raw) {
                        found_hwnd = Some(h);
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
                found_hwnd.ok_or_else(|| format!("scrcpy HWND not found for pid {}", target_pid))?
            }
        };

        if scrcpy_hwnd_raw == tauri_hwnd_raw {
            return Err("Cannot embed host Tauri window into itself".to_string());
        }

        ensure_paste_hook_started(app);

        let scale = win.scale_factor().unwrap_or(1.0);
        let serial_owned = serial.to_string();
        let label_owned = window_label.to_string();
        let (tx, rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
        let win_for_main = win.clone();
        win.run_on_main_thread(move || {
            let _ = win_for_main;
            let scrcpy_hwnd = HWND(scrcpy_hwnd_raw as *mut _);
            let tauri_hwnd = HWND(tauri_hwnd_raw as *mut _);
            unsafe {
                if !is_window_responsive(scrcpy_hwnd) {
                    let _ = tx.send(Err("scrcpy window is unresponsive".to_string()));
                    return;
                }

                // Reparent window on the host UI thread so child messages, WndProc
                // subclassing, and thread input attachment stay coherent.
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

                // Ensure clicks inside scrcpy deliver WM_PARENTNOTIFY to the host window.
                let ex_style = GetWindowLongPtrW(scrcpy_hwnd, GWL_EXSTYLE);
                if (ex_style & (WS_EX_NOPARENTNOTIFY.0 as isize)) != 0 {
                    SetWindowLongPtrW(
                        scrcpy_hwnd,
                        GWL_EXSTYLE,
                        ex_style & !(WS_EX_NOPARENTNOTIFY.0 as isize),
                    );
                }

                install_or_update_host_subclass(
                    tauri_hwnd,
                    scrcpy_hwnd,
                    &serial_owned,
                    &label_owned,
                );

                // Initial bounds: full client area minus right sidebar (60px * scale)
                let mut rect = RECT::default();
                let _ = GetClientRect(tauri_hwnd, &mut rect);
                let title_h = (36.0 * scale) as i32;
                let sidebar_w = (60.0 * scale) as i32;
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

                // Move focus into the embedded child right away if the host window is active.
                if let Ok(hosts) = get_host_states().lock() {
                    if let Some(st) = hosts.get(&tauri_hwnd_raw).cloned() {
                        drop(hosts);
                        focus_scrcpy_child(tauri_hwnd, scrcpy_hwnd, &st, false);
                    }
                }
                let _ = PostMessageW(Some(tauri_hwnd), WM_APP_FOCUS_SCRCPY, WPARAM(0), LPARAM(0));
            }
            let _ = tx.send(Ok(()));
        })
        .map_err(|e| e.to_string())?;

        rx.await.map_err(|e| e.to_string())??;

        let still_active = state
            .sessions
            .lock()
            .map(|map| {
                map.values()
                    .any(|s| s.serial == serial && s.pid == target_pid)
            })
            .unwrap_or(false);
        if !still_active {
            return Err(format!("Session for '{}' exited during embed", serial));
        }

        if let Ok(mut map) = get_embedded_map().lock() {
            map.insert(
                serial.to_string(),
                EmbeddedEntry {
                    hwnd: scrcpy_hwnd_raw,
                    pid: target_pid,
                    host_hwnd: tauri_hwnd_raw,
                },
            );
            HAS_EMBEDDED_MIRROR.store(!map.is_empty(), Ordering::Release);
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

        let state = app.state::<AppState>();
        let active_pid = {
            let map = state.sessions.lock().map_err(|e| e.to_string())?;
            map.values()
                .find(|s| s.serial == serial && s.mode == "display")
                .or_else(|| map.values().find(|s| s.serial == serial))
                .map(|s| s.pid)
        };

        let cached = get_embedded_map()
            .lock()
            .ok()
            .and_then(|map| map.get(serial).copied());
        let cached_valid = match cached {
            Some(entry) => unsafe {
                let h = HWND(entry.hwnd as *mut _);
                !h.0.is_null() && IsWindow(Some(h)).as_bool()
            },
            None => false,
        };

        let scrcpy_hwnd_raw =
            if is_cached_embed_valid_for_resize(cached, active_pid, tauri_hwnd_raw, cached_valid) {
                cached.unwrap().hwnd
            } else {
                if cached.is_some() {
                    remove_embedded_hwnd_for_serial_if_pid(serial, None);
                }
                let pid = active_pid
                    .ok_or_else(|| format!("No active session found for serial '{}'", serial))?;
                find_hwnd_by_pid(pid, tauri_hwnd_raw)
                    .ok_or_else(|| format!("scrcpy HWND not found for pid {}", pid))?
            };

        if scrcpy_hwnd_raw == tauri_hwnd_raw {
            return Err("Target window is host window".to_string());
        }

        let scrcpy_hwnd = HWND(scrcpy_hwnd_raw as *mut _);
        unsafe {
            if !IsWindow(Some(scrcpy_hwnd)).as_bool() || IsHungAppWindow(scrcpy_hwnd).as_bool() {
                return Ok(());
            }
        }

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
                SWP_ASYNCWINDOWPOS | SWP_NOACTIVATE | SWP_SHOWWINDOW,
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

pub fn remove_embedded_scrcpy_hwnd_for_pid(serial: &str, pid: u32) {
    #[cfg(windows)]
    {
        win_embed::remove_embedded_hwnd_for_serial_if_pid(serial, Some(pid));
    }
    #[cfg(not(windows))]
    {
        let _ = (serial, pid);
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

    #[test]
    fn click_focus_routing_respects_rect_visibility_and_minimize() {
        assert_eq!(
            decide_click_focus(true, true, false),
            EmbedFocusAction::FocusScrcpy
        );
        assert_eq!(
            decide_click_focus(false, true, false),
            EmbedFocusAction::KeepWebview
        );
        assert_eq!(
            decide_click_focus(true, false, false),
            EmbedFocusAction::None
        );
        assert_eq!(decide_click_focus(true, true, true), EmbedFocusAction::None);
    }

    #[test]
    fn rect_hit_test_uses_half_open_bounds() {
        assert!(is_point_in_rect(10, 20, 10, 20, 100, 200));
        assert!(is_point_in_rect(99, 199, 10, 20, 100, 200));
        assert!(!is_point_in_rect(100, 199, 10, 20, 100, 200));
        assert!(!is_point_in_rect(99, 200, 10, 20, 100, 200));
        assert!(!is_point_in_rect(9, 20, 10, 20, 100, 200));
    }

    #[test]
    fn foreground_guard_never_steals_from_other_apps() {
        // Other app in foreground: never focus scrcpy even on direct click.
        assert!(!should_focus_scrcpy_for_foreground(999, 100, false, true));
        assert!(!should_focus_scrcpy_for_foreground(999, 100, false, false));
        // Host or its child in foreground: allow.
        assert!(should_focus_scrcpy_for_foreground(100, 100, false, false));
        assert!(should_focus_scrcpy_for_foreground(200, 100, true, false));
        // No foreground window (e.g. desktop transition): allow only on direct click.
        assert!(!should_focus_scrcpy_for_foreground(0, 100, false, false));
        assert!(should_focus_scrcpy_for_foreground(0, 100, false, true));
        // Invalid host HWND: never.
        assert!(!should_focus_scrcpy_for_foreground(100, 0, false, true));
    }

    #[test]
    fn ctrl_v_image_intercept_prefers_text_and_requires_scrcpy_focus() {
        assert!(should_intercept_ctrl_v_image(true, false, true));
        assert!(!should_intercept_ctrl_v_image(true, true, true));
        assert!(!should_intercept_ctrl_v_image(false, false, true));
        assert!(!should_intercept_ctrl_v_image(true, false, false));
    }

    #[test]
    fn dib_to_png_converts_2x2_bgr_dib_and_rejects_invalid() {
        assert!(dib_to_png(&[0u8; 10]).is_err());

        // Minimal 2x2 24-bit BI_RGB DIB (each row padded to 4 bytes: 6 -> 8 bytes/row)
        let mut dib = Vec::new();
        dib.extend_from_slice(&40u32.to_le_bytes()); // biSize
        dib.extend_from_slice(&2i32.to_le_bytes()); // biWidth
        dib.extend_from_slice(&2i32.to_le_bytes()); // biHeight
        dib.extend_from_slice(&1u16.to_le_bytes()); // biPlanes
        dib.extend_from_slice(&24u16.to_le_bytes()); // biBitCount
        dib.extend_from_slice(&0u32.to_le_bytes()); // biCompression = BI_RGB
        dib.extend_from_slice(&16u32.to_le_bytes()); // biSizeImage
        dib.extend_from_slice(&[0u8; 16]); // pels/clrUsed/clrImportant
                                           // 2 rows x 8 bytes (BGRBGR + 2 pad bytes)
        dib.extend_from_slice(&[0xFF, 0x00, 0x00, 0x00, 0xFF, 0x00, 0x00, 0x00]);
        dib.extend_from_slice(&[0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00]);

        let png = dib_to_png(&dib).expect("valid 24-bit DIB should encode to PNG");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let decoded = image::load_from_memory_with_format(&png, image::ImageFormat::Png).unwrap();
        assert_eq!(decoded.width(), 2);
        assert_eq!(decoded.height(), 2);
    }

    #[test]
    fn embed_cache_evicts_stale_entries_and_reuses_live_same_pid_and_host() {
        let entry = EmbeddedEntry {
            hwnd: 111,
            pid: 42,
            host_hwnd: 900,
        };

        assert_eq!(
            decide_embed_cache(None, 42, 900, false, false),
            EmbedCacheAction::EmbedFresh { evict_stale: false }
        );
        assert_eq!(
            decide_embed_cache(Some(entry), 42, 900, true, true),
            EmbedCacheAction::AlreadyEmbedded(111)
        );
        // Old session PID -> stale.
        assert_eq!(
            decide_embed_cache(Some(entry), 43, 900, true, true),
            EmbedCacheAction::EmbedFresh { evict_stale: true }
        );
        // Reopened mirror window (different host HWND) -> stale.
        assert_eq!(
            decide_embed_cache(Some(entry), 42, 901, true, false),
            EmbedCacheAction::EmbedFresh { evict_stale: true }
        );
        // Dead HWND or not reparented -> stale.
        assert_eq!(
            decide_embed_cache(Some(entry), 42, 900, false, false),
            EmbedCacheAction::EmbedFresh { evict_stale: true }
        );
        assert_eq!(
            decide_embed_cache(Some(entry), 42, 900, true, false),
            EmbedCacheAction::EmbedFresh { evict_stale: true }
        );

        assert!(is_cached_embed_valid_for_resize(
            Some(entry),
            Some(42),
            900,
            true
        ));
        assert!(!is_cached_embed_valid_for_resize(
            Some(entry),
            Some(43),
            900,
            true
        ));
        assert!(!is_cached_embed_valid_for_resize(
            Some(entry),
            Some(42),
            901,
            true
        ));
        assert!(!is_cached_embed_valid_for_resize(
            Some(entry),
            Some(42),
            900,
            false
        ));
        assert!(!is_cached_embed_valid_for_resize(
            Some(entry),
            None,
            900,
            true
        ));

        assert!(should_remove_embed_for_pid(Some(entry), Some(42)));
        assert!(!should_remove_embed_for_pid(Some(entry), Some(43)));
        assert!(should_remove_embed_for_pid(Some(entry), None));
        assert!(!should_remove_embed_for_pid(None, Some(42)));
    }
}
