//! Wheel-to-swipe: converts mouse-wheel events over a mirror window into
//! Android touch flings (`adb shell input swipe`).
//!
//! Why: scrcpy's default `sdk` mouse mode forwards the wheel as Android
//! ACTION_SCROLL, which feeds/lists handle but Reels/Shorts-style vertical
//! pagers ignore — they only respond to touch flings. In `sdk` mode SDL reads
//! the wheel via WM_MOUSEWHEEL (not raw input), so swallowing the message in a
//! low-level hook reliably keeps it from scrcpy. If a `--mouse` mode setting
//! is ever exposed, disable this feature for non-sdk sessions.
//!
//! Architecture (Windows only):
//! - A dedicated hook thread runs WH_MOUSE_LL + a message loop. Never the
//!   Tauri main thread: stalls there would delay all system mouse input, and
//!   Windows silently removes hooks that repeatedly exceed the low-level hook
//!   timeout.
//! - The hook callback must stay cheap and non-blocking: AtomicBool fast
//!   path, `try_lock` on the registry (fail open), `try_send` to the worker.
//!   No cross-process calls (e.g. GetWindowText) — deadlock/timeout risk.
//! - A refresher thread resolves mirror HWNDs by window title and caches the
//!   device resolution; for flex (virtual display) sessions it re-reads
//!   `dumpsys display` periodically since the display resizes with the window.
//! - A single worker thread serializes swipes and coalesces wheel bursts.

#[cfg(windows)]
mod imp {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use tauri::{AppHandle, Manager};

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, FindWindowW, GetAncestor, GetMessageW, SetWindowsHookExW,
        WindowFromPoint, GA_ROOT, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL, WM_MOUSEWHEEL,
    };

    use crate::commands::{adb_path, parse_virtual_display};
    use crate::state::AppState;

    const WHEEL_DELTA: i32 = 120;
    const SWIPE_COOLDOWN: Duration = Duration::from_millis(300);
    const REFRESH_TICK: Duration = Duration::from_millis(300);
    /// Re-read dumpsys for flex display size every N refresher ticks (~1.5 s).
    const VD_REFRESH_TICKS: u32 = 5;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    struct SwipeTarget {
        serial: Arc<str>,
        enabled: bool,
        /// `Some` for flex sessions: inject with `input -d <id>`.
        display_id: Option<u32>,
        w: u32,
        h: u32,
    }

    #[derive(Default)]
    struct Registry {
        /// Per-serial user toggle; survives HWND churn / window recreation.
        enabled: HashMap<String, bool>,
        /// Resolved mirror windows, rebuilt by the refresher.
        targets: HashMap<isize, SwipeTarget>,
    }

    struct SwipeEvent {
        serial: Arc<str>,
        delta: i16,
        at: Instant,
    }

    /// Fast path for the system-wide hook: false ⇒ the callback costs ~nothing.
    static ANY_ENABLED: AtomicBool = AtomicBool::new(false);
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    static TX: OnceLock<mpsc::SyncSender<SwipeEvent>> = OnceLock::new();

    fn registry() -> &'static Mutex<Registry> {
        REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
    }

    fn lock_registry() -> std::sync::MutexGuard<'static, Registry> {
        // Writers can't leave the maps logically broken, so recover from poison.
        registry().lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Spawn the hook, worker, and refresher threads. Call once from setup.
    pub fn init(app: &AppHandle) {
        let (tx, rx) = mpsc::sync_channel::<SwipeEvent>(64);
        let _ = TX.set(tx);

        let adb = adb_path();
        std::thread::Builder::new()
            .name("wheel-swipe-worker".into())
            .spawn(move || worker_thread(rx, adb))
            .expect("spawn wheel-swipe worker");

        std::thread::Builder::new()
            .name("wheel-swipe-hook".into())
            .spawn(hook_thread)
            .expect("spawn wheel-swipe hook");

        let app = app.clone();
        std::thread::Builder::new()
            .name("wheel-swipe-refresher".into())
            .spawn(move || refresher_thread(app))
            .expect("spawn wheel-swipe refresher");
    }

    /// Toggle swipe mode for a serial. Resolves the mirror window inline so the
    /// switch is effective immediately (not on the next refresher tick).
    pub fn set_enabled(app: &AppHandle, serial: &str, enabled: bool) -> Result<(), String> {
        let session = {
            let state = app.state::<AppState>();
            let sessions = state.sessions.lock().unwrap();
            sessions
                .values()
                .find(|s| s.serial == serial)
                .map(|s| (s.display_id, s.virtual_size))
        };
        let Some((display_id, virtual_size)) = session else {
            return Err(format!("No running mirror for {serial}"));
        };

        // Resolve everything before mutating state, so a failed enable leaves
        // the registry untouched (the UI shows the error and stays off).
        let resolved = if enabled {
            let hwnd = resolve_mirror_hwnd(serial)
                .ok_or_else(|| "Mirror window not found".to_string())?;
            let size = virtual_size
                .or_else(|| adb_path().and_then(|adb| query_device_size(&adb, serial)))
                .ok_or_else(|| "Could not determine device screen size".to_string())?;
            Some((hwnd, size))
        } else {
            None
        };

        let mut reg = lock_registry();
        reg.enabled.insert(serial.to_string(), enabled);
        if let Some((hwnd, size)) = resolved {
            reg.targets.insert(
                hwnd,
                SwipeTarget {
                    serial: Arc::from(serial),
                    enabled: true,
                    display_id,
                    w: size.0,
                    h: size.1,
                },
            );
        } else if let Some(t) = reg.targets.values_mut().find(|t| &*t.serial == serial) {
            t.enabled = false;
        }

        let any = reg.enabled.values().any(|&e| e);
        ANY_ENABLED.store(any, Ordering::Relaxed);
        Ok(())
    }

    // ---- hook side -------------------------------------------------------

    fn hook_thread() {
        unsafe {
            let hmod = GetModuleHandleW(PCWSTR::null()).expect("GetModuleHandleW");
            let hinstance: HINSTANCE = hmod.into();
            let _hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(hinstance), 0)
                .expect("SetWindowsHookExW(WH_MOUSE_LL)");
            // LL hooks fire while this thread pumps; it does nothing else.
            // No unhook on exit: the hook dies with the process.
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
        }
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // HC_ACTION only, wheel only, and the atomic gate before anything else.
        if code >= 0
            && wparam.0 as u32 == WM_MOUSEWHEEL
            && ANY_ENABLED.load(Ordering::Relaxed)
        {
            let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let hwnd = WindowFromPoint(info.pt);
            let root = GetAncestor(hwnd, GA_ROOT);
            // try_lock: if the refresher holds the lock, pass the event
            // through rather than stall the system-wide hook.
            if let Ok(reg) = registry().try_lock() {
                if let Some(t) = reg.targets.get(&(root.0 as isize)) {
                    if t.enabled {
                        let delta = ((info.mouseData >> 16) & 0xFFFF) as u16 as i16;
                        if let Some(tx) = TX.get() {
                            let _ = tx.try_send(SwipeEvent {
                                serial: t.serial.clone(),
                                delta,
                                at: Instant::now(),
                            });
                        }
                        return LRESULT(1); // swallow: scrcpy must not scroll too
                    }
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    // ---- refresher side --------------------------------------------------

    fn refresher_thread(app: AppHandle) {
        let adb = adb_path();
        let mut size_cache: HashMap<String, (u32, u32)> = HashMap::new();
        let mut tick: u32 = 0;

        loop {
            std::thread::sleep(REFRESH_TICK);
            tick = tick.wrapping_add(1);

            // Snapshot sessions (tiny lock scope — see state.rs).
            let sessions: Vec<(String, Option<u32>, Option<(u32, u32)>, bool)> = {
                let Some(state) = app.try_state::<AppState>() else { continue };
                let map = state.sessions.lock().unwrap();
                map.values()
                    .map(|s| {
                        (
                            s.serial.clone(),
                            s.display_id,
                            s.virtual_size,
                            s.args.iter().any(|a| a.starts_with("--new-display")),
                        )
                    })
                    .collect()
            };

            // Flex sessions: refresh virtual display id/size occasionally
            // (flex resizes the display when the window resizes).
            if tick % VD_REFRESH_TICKS == 0 {
                if let Some(adb) = &adb {
                    for (serial, ..) in sessions.iter().filter(|s| s.3) {
                        if let Some((did, size)) = query_virtual_display(adb, serial) {
                            if let Some(state) = app.try_state::<AppState>() {
                                let mut map = state.sessions.lock().unwrap();
                                if let Some(s) = map.values_mut().find(|s| &s.serial == serial) {
                                    s.display_id = Some(did);
                                    s.virtual_size = Some(size);
                                }
                            }
                        }
                    }
                }
            }

            let mut reg = lock_registry();
            if reg.enabled.values().all(|&e| !e) {
                // Nothing enabled: drop stale targets and stand down.
                reg.targets.clear();
                reg.enabled.retain(|serial, _| {
                    sessions.iter().any(|(s, ..)| s == serial)
                });
                ANY_ENABLED.store(false, Ordering::Relaxed);
                continue;
            }

            let mut targets: HashMap<isize, SwipeTarget> = HashMap::new();
            for (serial, display_id, virtual_size, _is_flex) in &sessions {
                let enabled = *reg.enabled.get(serial).unwrap_or(&false);
                if !enabled {
                    continue;
                }
                let Some(hwnd) = resolve_mirror_hwnd(serial) else { continue };
                let size = virtual_size.or_else(|| {
                    if let Some(s) = size_cache.get(serial) {
                        return Some(*s);
                    }
                    let s = adb.as_deref().and_then(|a| query_device_size(a, serial));
                    if let Some(s) = s {
                        size_cache.insert(serial.clone(), s);
                    }
                    s
                });
                let Some((w, h)) = size else { continue };
                targets.insert(
                    hwnd,
                    SwipeTarget {
                        serial: Arc::from(serial.as_str()),
                        enabled: true,
                        display_id: *display_id,
                        w,
                        h,
                    },
                );
            }
            reg.targets = targets;
            reg.enabled.retain(|serial, _| sessions.iter().any(|(s, ..)| s == serial));
            let any = reg.enabled.values().any(|&e| e);
            ANY_ENABLED.store(any, Ordering::Relaxed);
        }
    }

    fn resolve_mirror_hwnd(serial: &str) -> Option<isize> {
        let title = format!("AnyLeap — {}", serial);
        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let hwnd: HWND =
            unsafe { FindWindowW(PCWSTR::null(), PCWSTR(wide.as_ptr())) }.ok()?;
        Some(hwnd.0 as isize)
    }

    fn adb_shell_output(adb: &std::path::Path, serial: &str, shell_args: &[&str]) -> Option<String> {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new(adb)
            .args(["-s", serial, "shell"])
            .args(shell_args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .ok()?;
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Parse `adb shell wm size`; "Override size" wins over "Physical size".
    pub(crate) fn parse_wm_size(text: &str) -> Option<(u32, u32)> {
        let mut physical = None;
        let mut override_ = None;
        for line in text.lines() {
            let Some((label, value)) = line.split_once(':') else { continue };
            let value = value.trim();
            let Some((w, h)) = value.split_once('x') else { continue };
            let parsed = match (w.trim().parse::<u32>(), h.trim().parse::<u32>()) {
                (Ok(w), Ok(h)) => Some((w, h)),
                _ => None,
            };
            if label.contains("Override") {
                override_ = parsed;
            } else if label.contains("Physical") {
                physical = parsed;
            }
        }
        override_.or(physical)
    }

    fn query_device_size(adb: &std::path::Path, serial: &str) -> Option<(u32, u32)> {
        parse_wm_size(&adb_shell_output(adb, serial, &["wm", "size"])?)
    }

    fn query_virtual_display(adb: &std::path::Path, serial: &str) -> Option<(u32, (u32, u32))> {
        parse_virtual_display(&adb_shell_output(adb, serial, &["dumpsys", "display"])?)
    }

    // ---- worker side -------------------------------------------------------

    fn worker_thread(rx: mpsc::Receiver<SwipeEvent>, adb: Option<PathBuf>) {
        // serial -> (last swipe start, wheel-delta accumulator)
        let mut per_serial: HashMap<Arc<str>, (Instant, i32)> = HashMap::new();

        while let Ok(ev) = rx.recv() {
            let Some(adb) = adb.as_deref() else { continue };
            let entry = per_serial
                .entry(ev.serial.clone())
                .or_insert_with(|| (Instant::now() - SWIPE_COOLDOWN, 0));

            // Events generated while a swipe was in flight are stale — the
            // user was reacting to the previous state; don't replay them.
            if ev.at < entry.0 {
                continue;
            }
            entry.1 += ev.delta as i32;
            if entry.1.abs() < WHEEL_DELTA {
                continue; // free-spin wheels emit sub-notch deltas; accumulate
            }
            let wheel_down = entry.1 < 0;
            entry.1 = 0;
            if entry.0.elapsed() < SWIPE_COOLDOWN {
                continue; // at most ~1 fling per cooldown, no backlog buildup
            }
            entry.0 = Instant::now();

            let (target, size) = {
                let reg = lock_registry();
                match reg.targets.values().find(|t| t.serial == ev.serial) {
                    Some(t) => (t.display_id, (t.w, t.h)),
                    None => continue,
                }
            };
            run_swipe(adb, &ev.serial, target, size, wheel_down);
        }
    }

    /// Wheel toward the user (down) ⇒ finger flings up ⇒ next video.
    fn run_swipe(
        adb: &std::path::Path,
        serial: &str,
        display_id: Option<u32>,
        (w, h): (u32, u32),
        wheel_down: bool,
    ) {
        use std::os::windows::process::CommandExt;
        let x = (w / 2).to_string();
        let (y_from, y_to) = if wheel_down {
            (h * 60 / 100, h * 40 / 100)
        } else {
            (h * 40 / 100, h * 60 / 100)
        };

        let mut cmd = std::process::Command::new(adb);
        cmd.args(["-s", serial, "shell", "input"]);
        if let Some(did) = display_id {
            cmd.args(["-d", &did.to_string()]);
        }
        cmd.args([
            "swipe",
            &x,
            &y_from.to_string(),
            &x,
            &y_to.to_string(),
            "100",
        ]);
        let _ = cmd.creation_flags(CREATE_NO_WINDOW).output();
    }

    #[cfg(test)]
    mod tests {
        use super::parse_wm_size;

        #[test]
        fn wm_size_physical_only() {
            assert_eq!(parse_wm_size("Physical size: 1080x2436\n"), Some((1080, 2436)));
        }

        #[test]
        fn wm_size_override_wins() {
            let text = "Physical size: 1080x2436\nOverride size: 720x1624\n";
            assert_eq!(parse_wm_size(text), Some((720, 1624)));
        }

        #[test]
        fn wm_size_garbage() {
            assert_eq!(parse_wm_size("no sizes here"), None);
            assert_eq!(parse_wm_size(""), None);
        }
    }
}

use tauri::AppHandle;

/// Enable/disable wheel-to-swipe for a device's mirror window.
#[tauri::command]
pub async fn set_wheel_swipe(app: AppHandle, serial: String, enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        imp::set_enabled(&app, &serial, enabled)
    }
    #[cfg(not(windows))]
    {
        let _ = (app, serial, enabled);
        Err("Wheel-to-swipe is only supported on Windows".to_string())
    }
}

/// Start the hook/refresher/worker threads. No-op off Windows.
pub fn init(app: &AppHandle) {
    #[cfg(windows)]
    imp::init(app);
    #[cfg(not(windows))]
    let _ = app;
}
