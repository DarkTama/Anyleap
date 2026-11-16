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

/// Minimum spacing between blocked-input reports.
const INPUT_BLOCKED_REPORT_INTERVAL_MS: u64 = 2000;

/// Pure rate limiter: returns true (and records `now_ms`) when a report is due.
fn rate_limit_allows(last_ms: &mut Option<u64>, now_ms: u64, interval_ms: u64) -> bool {
    match *last_ms {
        Some(prev) if now_ms.saturating_sub(prev) < interval_ms => false,
        _ => {
            *last_ms = Some(now_ms);
            true
        }
    }
}

type InputBlockedSink = Box<dyn Fn(String) + Send + Sync>;

static INPUT_BLOCKED_SINK: Mutex<Option<InputBlockedSink>> = Mutex::new(None);
static INPUT_BLOCKED_LAST_MS: Mutex<Option<u64>> = Mutex::new(None);

/// Registers where blocked-input reports go (the receiver's link to the Main PC).
pub fn set_input_blocked_sink(sink: impl Fn(String) + Send + Sync + 'static) {
    if let Ok(mut g) = INPUT_BLOCKED_SINK.lock() {
        *g = Some(Box::new(sink));
    }
}

#[cfg(windows)]
fn foreground_process_name() -> String {
    super::hooks::foreground_process_name()
}

#[cfg(not(windows))]
fn foreground_process_name() -> String {
    "unknown".to_string()
}

/// Called when a receiver `SendInput` returned 0 (typically UIPI: elevated foreground
/// window). Rate-limited to one report per 2 s: logs and notifies the Main PC.
pub fn note_input_blocked(kind: &str) {
    let now_ms = super::debug_log::now_us() / 1000;
    let due = INPUT_BLOCKED_LAST_MS
        .lock()
        .map(|mut last| rate_limit_allows(&mut last, now_ms, INPUT_BLOCKED_REPORT_INTERVAL_MS))
        .unwrap_or(false);
    if !due {
        return;
    }
    let proc_name = foreground_process_name();
    super::debug_log::note(|| format!("INPUT_BLOCKED_UIPI kind={kind} fg_process={proc_name}"));
    if let Ok(g) = INPUT_BLOCKED_SINK.lock() {
        if let Some(sink) = g.as_ref() {
            sink(proc_name);
        }
    }
}

/// Normalizes a virtual-desktop pixel to the 0..=65535 absolute range expected by
/// `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK`, rounded so the round-trip lands
/// on the exact pixel. Out-of-range input is clamped to the virtual screen.
pub fn normalize_virtual_desk(x: i32, y: i32, vx: i32, vy: i32, vw: i32, vh: i32) -> (i32, i32) {
    fn axis(v: i32, origin: i32, size: i32) -> i32 {
        if size <= 1 {
            return 0;
        }
        let span = (size - 1) as i64;
        let rel = (v as i64 - origin as i64).clamp(0, span);
        ((rel * 65535 + span / 2) / span) as i32
    }
    (axis(x, vx, vw), axis(y, vy, vh))
}

/// Moves the cursor via a real absolute `SendInput` move so shell surfaces see hover.
/// Returns false when the input was rejected (blocked by UIPI or no input desktop).
pub fn inject_mouse_move_absolute(x: i32, y: i32) -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
            SM_YVIRTUALSCREEN,
        };
        let (nx, ny) = normalize_virtual_desk(
            x,
            y,
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        );
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    dx: nx,
                    dy: ny,
                    mouseData: 0,
                    dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        if SendInput(&[input], std::mem::size_of::<INPUT>() as i32) == 0 {
            note_input_blocked("move");
            return false;
        }
        true
    }
    #[cfg(not(windows))]
    {
        let _ = (x, y);
        false
    }
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
        if SendInput(&[input], std::mem::size_of::<INPUT>() as i32) == 0 {
            note_input_blocked("move_rel");
        }
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
        if sent == 0 {
            note_input_blocked("button");
        }
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
        if SendInput(&[input], std::mem::size_of::<INPUT>() as i32) == 0 {
            note_input_blocked("wheel");
        }
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
        if sent == 0 {
            note_input_blocked("key");
        }
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
    fn normalize_primary_monitor_edges_and_round_trip() {
        let (vx, vy, vw, vh) = (0, 0, 1920, 1080);
        assert_eq!(normalize_virtual_desk(0, 0, vx, vy, vw, vh), (0, 0));
        assert_eq!(
            normalize_virtual_desk(1919, 1079, vx, vy, vw, vh),
            (65535, 65535)
        );
        for x in [0, 1, 500, 959, 1918, 1919] {
            let (nx, _) = normalize_virtual_desk(x, 0, vx, vy, vw, vh);
            let back = (nx as i64 * (vw as i64 - 1) + 32767) / 65535;
            assert_eq!(back, x as i64);
        }
    }

    #[test]
    fn normalize_negative_origin_virtual_desktop() {
        let (vx, vy, vw, vh) = (-1920, -200, 3840, 1280);
        assert_eq!(normalize_virtual_desk(-1920, -200, vx, vy, vw, vh), (0, 0));
        assert_eq!(
            normalize_virtual_desk(1919, 1079, vx, vy, vw, vh),
            (65535, 65535)
        );
        let (nx, ny) = normalize_virtual_desk(0, 0, vx, vy, vw, vh);
        assert_eq!(nx, ((1920i64 * 65535 + 1919) / 3839) as i32);
        assert_eq!(ny, ((200i64 * 65535 + 639) / 1279) as i32);
        for x in [-1920, -1919, -1, 0, 1, 1919] {
            let (nx, _) = normalize_virtual_desk(x, 0, vx, vy, vw, vh);
            let back = (nx as i64 * (vw as i64 - 1) + 32767) / 65535 + vx as i64;
            assert_eq!(back, x as i64);
        }
    }

    #[test]
    fn normalize_clamps_and_handles_degenerate_size() {
        assert_eq!(
            normalize_virtual_desk(-5000, 9999, 0, 0, 100, 100),
            (0, 65535)
        );
        assert_eq!(normalize_virtual_desk(5, 5, 0, 0, 1, 1), (0, 0));
    }

    #[test]
    fn rate_limiter_allows_one_report_per_interval() {
        let mut last = None;
        assert!(rate_limit_allows(&mut last, 10_000, 2000));
        assert!(!rate_limit_allows(&mut last, 10_001, 2000));
        assert!(!rate_limit_allows(&mut last, 11_999, 2000));
        assert!(rate_limit_allows(&mut last, 12_000, 2000));
        assert!(!rate_limit_allows(&mut last, 12_500, 2000));
    }

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
