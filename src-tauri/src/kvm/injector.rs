#[cfg(windows)]
use windows::Win32::Foundation::{HANDLE, POINT};
#[cfg(windows)]
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, GetUserObjectInformationW, OpenInputDesktop, DESKTOP_ACCESS_FLAGS,
    DESKTOP_CONTROL_FLAGS, UOI_NAME,
};
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::*;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorInfo, GetCursorPos, SetCursorPos, CURSORINFO, CURSOR_SHOWING, CURSOR_SUPPRESSED,
};

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

pub const CURSOR_FLAG_SHOWING: u32 = 0x0000_0001;
pub const CURSOR_FLAG_SUPPRESSED: u32 = 0x0000_0002;
const DESKTOP_READOBJECTS_ACCESS: u32 = 0x0001;

static POST_UNLOCK_NUDGE_PENDING: AtomicBool = AtomicBool::new(false);
static LAST_LOGGED_CURSOR: Mutex<Option<(Option<u32>, Option<usize>)>> = Mutex::new(None);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CursorVisibilityInfo {
    pub flags: u32,
    pub showing: bool,
    pub suppressed: bool,
    pub hcursor_raw: usize,
}

impl CursorVisibilityInfo {
    pub fn from_flags(flags: u32) -> Self {
        Self::from_flags_and_handle(flags, 0)
    }

    pub fn from_flags_and_handle(flags: u32, hcursor_raw: usize) -> Self {
        Self {
            flags,
            showing: (flags & CURSOR_FLAG_SHOWING) != 0,
            suppressed: (flags & CURSOR_FLAG_SUPPRESSED) != 0,
            hcursor_raw,
        }
    }

    pub fn hcursor_null(&self) -> bool {
        self.hcursor_raw == 0
    }
}

/// Pure decision retained for unit clarity: checks if `GetCursorInfo` itself
/// reports hidden/suppressed/unknown. Post-unlock nudges are sent
/// unconditionally because Win32 can report `CURSOR_SHOWING` with a stale
/// cursor shape after leaving the secure desktop.
pub fn cursor_visibility_needs_nudge(info: Option<CursorVisibilityInfo>) -> bool {
    match info {
        Some(i) => !i.showing || i.suppressed || i.hcursor_null(),
        None => true,
    }
}

/// Pure change-detection helper for `CURSOR_INFO` logging: returns `true` only
/// on the first observation or when `(flags, hcursor_raw)` changes.
pub fn should_log_cursor_info(
    last_seen: &mut Option<(Option<u32>, Option<usize>)>,
    current: Option<CursorVisibilityInfo>,
) -> bool {
    let key = match current {
        Some(i) => (Some(i.flags), Some(i.hcursor_raw)),
        None => (None, None),
    };
    if last_seen.as_ref() == Some(&key) {
        false
    } else {
        *last_seen = Some(key);
        true
    }
}

pub fn mark_post_unlock_nudge_pending() {
    POST_UNLOCK_NUDGE_PENDING.store(true, Ordering::Release);
}

pub fn take_post_unlock_nudge_pending() -> bool {
    POST_UNLOCK_NUDGE_PENDING.swap(false, Ordering::AcqRel)
}

/// One-shot nudge on the first receiver mouse delta after WTS unlock.
/// Fast path is a single atomic load; only acquires `SendInput` once per unlock.
pub fn maybe_nudge_first_delta_after_unlock() {
    if POST_UNLOCK_NUDGE_PENDING.load(Ordering::Acquire) && take_post_unlock_nudge_pending() {
        super::hooks::restore_cursor_visibility();
        inject_cursor_nudge_1px();
    }
}

/// Sends a real 1-px relative move followed by the inverse -1-px relative move
/// (net zero displacement) so Windows dispatches `WM_SETCURSOR` and refreshes
/// the hardware cursor sprite without visible drift. Never call inside a hook callback.
pub fn inject_cursor_nudge_1px() {
    inject_mouse_delta(1, 0);
    inject_mouse_delta(-1, 0);
}

pub fn current_input_desktop_name() -> String {
    #[cfg(windows)]
    unsafe {
        if let Ok(hdesk) = OpenInputDesktop(
            DESKTOP_CONTROL_FLAGS(0),
            false,
            DESKTOP_ACCESS_FLAGS(DESKTOP_READOBJECTS_ACCESS),
        ) {
            let mut buf = [0u16; 128];
            let mut needed = 0u32;
            let res = GetUserObjectInformationW(
                HANDLE(hdesk.0),
                UOI_NAME,
                Some(buf.as_mut_ptr() as *mut _),
                (buf.len() * std::mem::size_of::<u16>()) as u32,
                Some(&mut needed),
            );
            let _ = CloseDesktop(hdesk);
            if res.is_ok() {
                let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                return String::from_utf16_lossy(&buf[..len]);
            }
            return "unknown".to_string();
        }
        "open_err".to_string()
    }
    #[cfg(not(windows))]
    {
        "non_windows".to_string()
    }
}

pub fn query_cursor_visibility() -> Option<CursorVisibilityInfo> {
    #[cfg(windows)]
    unsafe {
        let mut ci = CURSORINFO {
            cbSize: std::mem::size_of::<CURSORINFO>() as u32,
            ..Default::default()
        };
        if GetCursorInfo(&mut ci).is_ok() {
            let flags = ci.flags.0;
            return Some(CursorVisibilityInfo {
                flags,
                showing: (flags & CURSOR_SHOWING.0) != 0,
                suppressed: (flags & CURSOR_SUPPRESSED.0) != 0,
                hcursor_raw: ci.hCursor.0 as usize,
            });
        }
    }
    None
}

/// Logs `GetCursorInfo` state (only when `flags` or `hCursor` changes) and
/// unconditionally restores the local cursor visibility counter plus a 1-px
/// net-zero `SendInput` move (`+1, -1`). Call only on `CursorEnter` or WTS unlock
/// transitions — never per mouse delta and never from inside a hook callback.
pub fn ensure_cursor_visible_with_nudge(context: &str) {
    let before = query_cursor_visibility();
    let changed = LAST_LOGGED_CURSOR
        .lock()
        .map(|mut guard| should_log_cursor_info(&mut guard, before))
        .unwrap_or(false);
    if changed {
        let desktop = current_input_desktop_name();
        super::debug_log::note(|| {
            match before {
            Some(i) => format!(
                "CURSOR_INFO ctx={context} flags=0x{:X} showing={} suppressed={} hCursor=0x{:X} hcursor_null={} desktop=\"{desktop}\"",
                i.flags,
                i.showing,
                i.suppressed,
                i.hcursor_raw,
                i.hcursor_null(),
            ),
            None => format!("CURSOR_INFO ctx={context} query_err=1 desktop=\"{desktop}\""),
        }
        });
    }
    super::hooks::restore_cursor_visibility();
    inject_cursor_nudge_1px();
}

pub fn get_cursor_pos() -> Option<(i32, i32)> {
    #[cfg(windows)]
    unsafe {
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_ok() {
            return Some((pt.x, pt.y));
        }
    }
    None
}

pub fn inject_mouse_delta(dx: i32, dy: i32) {
    #[cfg(windows)]
    unsafe {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx,
                    dy,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn set_cursor_position(x: i32, y: i32) {
    #[cfg(windows)]
    unsafe {
        let _ = SetCursorPos(x, y);
    }
}

pub fn inject_mouse_button(button: u8, is_down: bool) -> (u32, u32) {
    #[cfg(windows)]
    unsafe {
        let flag = match (button, is_down) {
            (1, true) => MOUSEEVENTF_LEFTDOWN,
            (1, false) => MOUSEEVENTF_LEFTUP,
            (2, true) => MOUSEEVENTF_RIGHTDOWN,
            (2, false) => MOUSEEVENTF_RIGHTUP,
            (3, true) => MOUSEEVENTF_MIDDLEDOWN,
            (3, false) => MOUSEEVENTF_MIDDLEUP,
            _ => return (0, 0),
        };

        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: 0,
                    dwFlags: flag,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let sent = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        let err = if sent == 0 {
            windows::Win32::Foundation::GetLastError().0
        } else {
            0
        };
        (sent, err)
    }
    #[cfg(not(windows))]
    {
        let _ = (button, is_down);
        (0, 0)
    }
}

pub fn inject_mouse_wheel(delta: i32) {
    #[cfg(windows)]
    unsafe {
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: 0,
                    dy: 0,
                    mouseData: delta as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn inject_key(key_code: u16, scan_code: u16, is_down: bool, extended: bool) -> (u32, u32) {
    #[cfg(windows)]
    unsafe {
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if !is_down {
            flags |= KEYEVENTF_KEYUP;
        }
        if extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if scan_code > 0 {
            flags |= KEYEVENTF_SCANCODE;
        }

        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(key_code),
                    wScan: scan_code,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        let sent = SendInput(&[input], std::mem::size_of::<INPUT>() as i32);
        let err = if sent == 0 {
            windows::Win32::Foundation::GetLastError().0
        } else {
            0
        };
        (sent, err)
    }
    #[cfg(not(windows))]
    {
        let _ = (key_code, scan_code, is_down, extended);
        (0, 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_visibility_nudge_triggers_only_when_hidden_suppressed_or_unknown() {
        let visible = CursorVisibilityInfo::from_flags_and_handle(CURSOR_FLAG_SHOWING, 0x1);
        assert!(visible.showing);
        assert!(!visible.suppressed);
        assert!(!cursor_visibility_needs_nudge(Some(visible)));

        let hidden = CursorVisibilityInfo::from_flags(0);
        assert!(!hidden.showing);
        assert!(cursor_visibility_needs_nudge(Some(hidden)));

        let visible_null_hcursor =
            CursorVisibilityInfo::from_flags_and_handle(CURSOR_FLAG_SHOWING, 0);
        assert!(visible_null_hcursor.hcursor_null());
        assert!(cursor_visibility_needs_nudge(Some(visible_null_hcursor)));

        let visible_valid =
            CursorVisibilityInfo::from_flags_and_handle(CURSOR_FLAG_SHOWING, 0x1001);
        assert!(!visible_valid.hcursor_null());
        assert!(!cursor_visibility_needs_nudge(Some(visible_valid)));

        let suppressed =
            CursorVisibilityInfo::from_flags(CURSOR_FLAG_SHOWING | CURSOR_FLAG_SUPPRESSED);
        assert!(suppressed.suppressed);
        assert!(cursor_visibility_needs_nudge(Some(suppressed)));

        assert!(cursor_visibility_needs_nudge(None));
    }

    #[test]
    fn cursor_info_log_deduplicates_identical_flags_and_hcursor() {
        let mut last = None;
        let a = Some(CursorVisibilityInfo::from_flags_and_handle(
            CURSOR_FLAG_SHOWING,
            0x1001,
        ));
        let b = Some(CursorVisibilityInfo::from_flags_and_handle(
            CURSOR_FLAG_SHOWING,
            0x2002,
        ));
        let c = Some(CursorVisibilityInfo::from_flags_and_handle(0, 0x2002));

        assert!(should_log_cursor_info(&mut last, a));
        assert!(!should_log_cursor_info(&mut last, a));
        assert!(should_log_cursor_info(&mut last, b));
        assert!(!should_log_cursor_info(&mut last, b));
        assert!(should_log_cursor_info(&mut last, c));
        assert!(should_log_cursor_info(&mut last, None));
        assert!(!should_log_cursor_info(&mut last, None));
    }

    #[test]
    fn post_unlock_nudge_flag_is_one_shot() {
        let _ = take_post_unlock_nudge_pending();
        mark_post_unlock_nudge_pending();
        assert!(take_post_unlock_nudge_pending());
        assert!(!take_post_unlock_nudge_pending());
    }
}
