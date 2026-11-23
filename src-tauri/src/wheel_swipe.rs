//! Wheel scroll scaling and wheel-to-swipe (SPEC-30 & Paged mode).
//!
//! Why: scrcpy's default `sdk` mouse mode forwards the wheel as Android
//! ACTION_SCROLL (~64 dp per notch). On low-density Flex virtual displays
//! (e.g. 160 dpi), scrolling feels ~2.5x slower than on the phone's native
//! screen (~400+ dpi). Smooth style scales `WM_MOUSEWHEEL` / `WM_MOUSEHWHEEL`
//! deltas by the effective multiplier and re-posts them via `PostMessageW`
//! (which does not re-enter `WH_MOUSE_LL`), while Paged style converts wheel
//! notches into `adb shell input swipe` touch flings for short-video feeds.
//!
//! Architecture (Windows only):
//! - A dedicated hook thread runs WH_MOUSE_LL + a message loop. Never the
//!   Tauri main thread: stalls there would delay all system mouse input, and
//!   Windows silently removes hooks that repeatedly exceed the low-level hook
//!   timeout.
//! - The hook callback stays cheap and non-blocking: AtomicBool fast path,
//!   `try_lock` on the registry (fail open), `try_send` to the worker, and
//!   swallow only after `try_send` succeeds. No cross-process calls or adb
//!   on the hook thread.
//! - A refresher thread resolves mirror HWNDs (both standalone and embedded),
//!   caches device resolution and display density from `wm density` / `dumpsys
//!   display`, and computes effective multipliers outside the registry lock.
//! - A single worker thread owns per-axis remainder accumulators (`PostMessageW`)
//!   and serializes Paged touch flings.

use crate::scroll_feel::{ScrollStyle, WheelSource};

/// Wheel axis for a mouse-wheel hook event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WheelAxis {
    Vertical,
    Horizontal,
}

/// Decision returned by the pure hook classifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookDecision {
    Swallow,
    PassThrough,
}

/// Keyboard modifier flags sampled non-blockingly at hook time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ModifierState {
    pub ctrl: bool,
    pub alt: bool,
    pub win: bool,
    pub shift: bool,
}

impl ModifierState {
    /// Ctrl, Alt, or Win + wheel must always pass through untouched (e.g. Ctrl+wheel zoom).
    pub fn blocks_scaling(self) -> bool {
        self.ctrl || self.alt || self.win
    }
}

/// Pure hook decision: decide whether to swallow and enqueue a wheel event or pass through.
#[allow(clippy::too_many_arguments)]
pub fn decide(
    modifiers: ModifierState,
    axis: WheelAxis,
    source: WheelSource,
    over_mirror: bool,
    style: ScrollStyle,
    multiplier: f32,
    touchpad_speed: f32,
    smooth: bool,
    queue_ok: bool,
) -> HookDecision {
    if !over_mirror || modifiers.blocks_scaling() || !queue_ok {
        return HookDecision::PassThrough;
    }
    match style {
        ScrollStyle::Paged => match axis {
            WheelAxis::Vertical => HookDecision::Swallow,
            WheelAxis::Horizontal => HookDecision::PassThrough,
        },
        ScrollStyle::Smooth => match source {
            WheelSource::Touchpad => {
                // Touchpad input skips Auto multiplier and smoothing; pass through untouched at 1x.
                if !touchpad_speed.is_finite() || (touchpad_speed - 1.0).abs() <= 1e-4 {
                    HookDecision::PassThrough
                } else {
                    HookDecision::Swallow
                }
            }
            WheelSource::MouseWheel => {
                if !multiplier.is_finite() || (!smooth && (multiplier - 1.0).abs() <= 1e-4) {
                    HookDecision::PassThrough
                } else {
                    HookDecision::Swallow
                }
            }
        },
    }
}

#[cfg(windows)]
mod imp {
    use std::collections::HashMap;
    use std::os::windows::process::CommandExt;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use tauri::{AppHandle, Manager};

    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Media::{timeBeginPeriod, timeEndPeriod};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, FindWindowW, GetAncestor, GetMessageW, IsWindow, PostMessageW,
        SetWindowsHookExW, WindowFromPoint, GA_ROOT, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL,
        WM_MOUSEHWHEEL, WM_MOUSEWHEEL,
    };

    use super::{decide, HookDecision, ModifierState, WheelAxis};
    use crate::commands::{adb_path, parse_virtual_display};
    use crate::scroll_feel::smooth::SMOOTH_TICK;
    use crate::scroll_feel::speed::{
        parse_default_display_density, parse_virtual_display_density_for_id, parse_wm_density,
        DEFAULT_NATIVE_DENSITY,
    };
    use crate::scroll_feel::{
        classify_wheel_source, effective_multiplier, effective_touchpad_speed, AxisAccumulator,
        AxisSmoother, ScrollConfig, ScrollStyle, WheelSource,
    };
    use crate::state::{poison_error, AppState};

    const WHEEL_DELTA: i32 = 120;
    const SWIPE_COOLDOWN: Duration = Duration::from_millis(300);
    const REFRESH_TICK: Duration = Duration::from_millis(300);
    /// Re-read dumpsys for flex display size/density every N refresher ticks (~1.5 s).
    const VD_REFRESH_TICKS: u32 = 5;
    /// Re-read `wm density` for Mirror sessions every N refresher ticks (~5.1 s).
    const MIRROR_DENSITY_TICKS: u32 = 17;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const MK_SHIFT_FLAG: u16 = 0x0004;

    struct SwipeTarget {
        serial: Arc<str>,
        hwnd: isize,
        style: ScrollStyle,
        multiplier: f32,
        touchpad_speed: f32,
        smooth: bool,
        /// `Some` for flex sessions: inject with `input -d <id>`.
        display_id: Option<u32>,
        w: u32,
        h: u32,
    }

    #[derive(Clone, Copy, Default)]
    struct DensityCache {
        native: Option<u32>,
        mirror_override: Option<u32>,
        flex: Option<u32>,
    }

    #[derive(Default)]
    struct Registry {
        /// Per-serial scroll configuration (set on session start / edit).
        configs: HashMap<String, ScrollConfig>,
        /// Per-session More-popover override (`true` => Paged, `false` => Smooth).
        paged_override: HashMap<String, bool>,
        /// Cached display densities per serial.
        densities: HashMap<String, DensityCache>,
        /// Last computed multiplier per serial (for UI live display).
        multipliers: HashMap<String, f32>,
        /// Resolved mirror windows, keyed by scrcpy HWND.
        targets: HashMap<isize, SwipeTarget>,
    }

    enum WheelEvent {
        Paged {
            serial: Arc<str>,
            delta: i16,
            at: Instant,
        },
        Smooth {
            hwnd: isize,
            axis: WheelAxis,
            source: WheelSource,
            delta: i16,
            key_state: u16,
            screen_x: i32,
            screen_y: i32,
            multiplier: f32,
            smooth: bool,
            at: Instant,
        },
    }

    /// Fast path for the system-wide hook: false ⇒ the callback costs ~nothing.
    static ANY_ENABLED: AtomicBool = AtomicBool::new(false);
    static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
    static TX: OnceLock<mpsc::SyncSender<WheelEvent>> = OnceLock::new();

    fn registry() -> &'static Mutex<Registry> {
        REGISTRY.get_or_init(|| Mutex::new(Registry::default()))
    }

    fn lock_registry() -> Result<std::sync::MutexGuard<'static, Registry>, String> {
        registry().lock().map_err(poison_error)
    }

    fn effective_config_for(reg: &Registry, serial: &str) -> ScrollConfig {
        let mut cfg = reg.configs.get(serial).copied().unwrap_or_default();
        if let Some(&paged) = reg.paged_override.get(serial) {
            cfg.style = if paged {
                ScrollStyle::Paged
            } else {
                ScrollStyle::Smooth
            };
        }
        cfg
    }

    fn compute_multiplier_for(
        reg: &Registry,
        serial: &str,
        is_flex: bool,
        cfg: &ScrollConfig,
    ) -> f32 {
        let dens = reg.densities.get(serial).copied().unwrap_or_default();
        let native = dens.native.or(Some(DEFAULT_NATIVE_DENSITY));
        let session = if is_flex {
            dens.flex
        } else {
            dens.mirror_override.or(dens.native)
        };
        effective_multiplier(cfg, native, session)
    }

    fn sync_any_enabled(reg: &Registry) {
        let any = reg.targets.values().any(|t| {
            t.style == ScrollStyle::Paged
                || t.smooth
                || (t.multiplier - 1.0).abs() > 1e-4
                || (t.touchpad_speed - 1.0).abs() > 1e-4
        });
        ANY_ENABLED.store(any, Ordering::Relaxed);
    }

    /// Spawn the hook, worker, and refresher threads. Call once from setup.
    pub fn init(app: &AppHandle) {
        let (tx, rx) = mpsc::sync_channel::<WheelEvent>(64);
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

    /// Toggle Paged mode for a running session (More popover toggle).
    pub fn set_enabled(app: &AppHandle, serial: &str, enabled: bool) -> Result<(), String> {
        let session = {
            let state = app.state::<AppState>();
            let sessions = state.sessions.lock().map_err(poison_error)?;
            sessions
                .values()
                .find(|s| s.serial == serial && s.mode != "camera")
                .map(|s| {
                    (
                        s.display_id,
                        s.virtual_size,
                        s.args.iter().any(|a| a.starts_with("--new-display")),
                    )
                })
        };
        let Some((display_id, virtual_size, is_flex)) = session else {
            if !enabled {
                let mut reg = lock_registry()?;
                reg.paged_override.remove(serial);
                reg.targets.retain(|_, t| t.serial.as_ref() != serial);
                sync_any_enabled(&reg);
                return Ok(());
            }
            return Err(format!("No running mirror for {serial}"));
        };

        // Resolve everything before mutating state, so a failed enable leaves
        // the registry untouched (the UI shows the error and stays off).
        let resolved = if enabled {
            let hwnd =
                resolve_mirror_hwnd(serial).ok_or_else(|| "Mirror window not found".to_string())?;
            let size = virtual_size
                .or_else(|| adb_path().and_then(|adb| query_device_size(&adb, serial)))
                .ok_or_else(|| "Could not determine device screen size".to_string())?;
            Some((hwnd, size))
        } else {
            None
        };

        let mut reg = lock_registry()?;
        reg.paged_override.insert(serial.to_string(), enabled);
        let cfg = effective_config_for(&reg, serial);
        let mult = compute_multiplier_for(&reg, serial, is_flex, &cfg);
        let touchpad_speed = effective_touchpad_speed(&cfg);
        reg.multipliers.insert(serial.to_string(), mult);

        if let Some((hwnd, size)) = resolved {
            reg.targets.insert(
                hwnd,
                SwipeTarget {
                    serial: Arc::from(serial),
                    hwnd,
                    style: ScrollStyle::Paged,
                    multiplier: mult,
                    touchpad_speed,
                    smooth: cfg.smooth,
                    display_id,
                    w: size.0,
                    h: size.1,
                },
            );
        } else if let Some(t) = reg.targets.values_mut().find(|t| &*t.serial == serial) {
            t.style = cfg.style;
            t.multiplier = mult;
            t.touchpad_speed = touchpad_speed;
            t.smooth = cfg.smooth;
        }

        sync_any_enabled(&reg);
        Ok(())
    }

    /// Update scroll-feel settings for a running session (or pre-register on launch).
    pub fn set_scroll_feel(
        app: &AppHandle,
        serial: &str,
        config: ScrollConfig,
    ) -> Result<Option<f32>, String> {
        let is_flex = {
            let state = app.state::<AppState>();
            let sessions = state.sessions.lock().map_err(poison_error)?;
            sessions
                .values()
                .find(|s| s.serial == serial && s.mode != "camera")
                .map(|s| s.args.iter().any(|a| a.starts_with("--new-display")))
                .unwrap_or(false)
        };

        let mut reg = lock_registry()?;
        reg.configs.insert(serial.to_string(), config);
        reg.paged_override.remove(serial);
        let mult = compute_multiplier_for(&reg, serial, is_flex, &config);
        let touchpad_speed = effective_touchpad_speed(&config);
        reg.multipliers.insert(serial.to_string(), mult);

        if let Some(t) = reg.targets.values_mut().find(|t| &*t.serial == serial) {
            t.style = config.style;
            t.multiplier = mult;
            t.touchpad_speed = touchpad_speed;
            t.smooth = config.smooth;
        }
        sync_any_enabled(&reg);
        Ok(Some(mult))
    }

    pub fn get_scroll_multiplier(serial: &str) -> Option<f32> {
        lock_registry()
            .ok()
            .and_then(|reg| reg.multipliers.get(serial).copied())
    }

    pub fn get_scroll_style(serial: &str) -> Option<ScrollStyle> {
        lock_registry()
            .ok()
            .map(|reg| effective_config_for(&reg, serial).style)
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

    #[inline]
    unsafe fn is_vk_down(vk: VIRTUAL_KEY) -> bool {
        (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0
    }

    unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        let msg = wparam.0 as u32;
        let axis = match msg {
            WM_MOUSEWHEEL => Some(WheelAxis::Vertical),
            WM_MOUSEHWHEEL => Some(WheelAxis::Horizontal),
            _ => None,
        };

        // HC_ACTION only, wheel only, and the atomic gate before anything else.
        if code >= 0 && ANY_ENABLED.load(Ordering::Relaxed) {
            if let Some(axis) = axis {
                let modifiers = ModifierState {
                    ctrl: is_vk_down(VK_CONTROL),
                    alt: is_vk_down(VK_MENU),
                    win: is_vk_down(VK_LWIN) || is_vk_down(VK_RWIN),
                    shift: is_vk_down(VK_SHIFT),
                };
                if !modifiers.blocks_scaling() {
                    let info = &*(lparam.0 as *const MSLLHOOKSTRUCT);
                    let hwnd = WindowFromPoint(info.pt);
                    let root = GetAncestor(hwnd, GA_ROOT);

                    // try_lock: if the refresher holds the lock, pass the event
                    // through rather than stall the system-wide hook.
                    let matched = registry().try_lock().ok().and_then(|reg| {
                        reg.targets
                            .get(&(hwnd.0 as isize))
                            .or_else(|| reg.targets.get(&(root.0 as isize)))
                            .map(|t| {
                                (
                                    t.serial.clone(),
                                    t.hwnd,
                                    t.style,
                                    t.multiplier,
                                    t.touchpad_speed,
                                    t.smooth,
                                )
                            })
                    });

                    if let Some((serial, target_hwnd, style, multiplier, touchpad_speed, smooth)) =
                        matched
                    {
                        let delta = ((info.mouseData >> 16) & 0xFFFF) as u16 as i16;
                        let source = classify_wheel_source(info.flags, delta);
                        if decide(
                            modifiers,
                            axis,
                            source,
                            true,
                            style,
                            multiplier,
                            touchpad_speed,
                            smooth,
                            true,
                        ) == HookDecision::Swallow
                        {
                            let key_state = if modifiers.shift { MK_SHIFT_FLAG } else { 0 };
                            let ev = match style {
                                ScrollStyle::Paged => WheelEvent::Paged {
                                    serial,
                                    delta,
                                    at: Instant::now(),
                                },
                                ScrollStyle::Smooth => WheelEvent::Smooth {
                                    hwnd: target_hwnd,
                                    axis,
                                    source,
                                    delta,
                                    key_state,
                                    screen_x: info.pt.x,
                                    screen_y: info.pt.y,
                                    multiplier: match source {
                                        WheelSource::Touchpad => touchpad_speed,
                                        WheelSource::MouseWheel => multiplier,
                                    },
                                    smooth: match source {
                                        WheelSource::Touchpad => false,
                                        WheelSource::MouseWheel => smooth,
                                    },
                                    at: Instant::now(),
                                },
                            };
                            // Swallow ONLY after try_send succeeds; if full, pass through (1x).
                            if let Some(tx) = TX.get() {
                                if tx.try_send(ev).is_ok() {
                                    return LRESULT(1);
                                }
                            }
                        }
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

            // Snapshot display sessions (tiny lock scope — see state.rs).
            type SessionSnapshot = (String, Option<u32>, Option<(u32, u32)>, bool);
            let sessions: Vec<SessionSnapshot> = {
                let Some(state) = app.try_state::<AppState>() else {
                    continue;
                };
                let list = match state.sessions.lock() {
                    Ok(map) => map
                        .values()
                        .filter(|s| s.mode != "camera")
                        .map(|s| {
                            (
                                s.serial.clone(),
                                s.display_id,
                                s.virtual_size,
                                s.args.iter().any(|a| a.starts_with("--new-display")),
                            )
                        })
                        .collect(),
                    Err(e) => {
                        eprintln!("Sessions mutex poisoned in refresher: {e}");
                        continue;
                    }
                };
                list
            };

            if sessions.is_empty() {
                if let Ok(mut reg) = lock_registry() {
                    reg.targets.clear();
                    reg.configs.clear();
                    reg.paged_override.clear();
                    reg.densities.clear();
                    reg.multipliers.clear();
                    ANY_ENABLED.store(false, Ordering::Relaxed);
                }
                size_cache.clear();
                continue;
            }

            // Snapshot which serials need density or size queries BEFORE doing adb I/O
            // so the registry mutex is never held across cross-process adb calls.
            let known_densities: HashMap<String, DensityCache> = lock_registry()
                .map(|r| r.densities.clone())
                .unwrap_or_default();

            let mut updated_densities: HashMap<String, DensityCache> = HashMap::new();
            let mut updated_vd: HashMap<String, (u32, (u32, u32))> = HashMap::new();

            if let Some(adb) = &adb {
                for (serial, display_id, _virtual_size, is_flex) in &sessions {
                    let prev = known_densities.get(serial).copied();
                    let is_new = prev.is_none();
                    let mut cache = prev.unwrap_or_default();
                    let mut changed = false;

                    // Query `wm density` at session start, and periodically for Mirror sessions.
                    if is_new || (!is_flex && tick.is_multiple_of(MIRROR_DENSITY_TICKS)) {
                        if let Some(raw) = adb_shell_output(adb, serial, &["wm", "density"]) {
                            let wm = parse_wm_density(&raw);
                            if wm.physical.is_some() {
                                cache.native = wm.physical;
                            }
                            cache.mirror_override = wm.override_;
                            changed = true;
                        }
                    }

                    // Flex sessions: query `dumpsys display` at start and every VD_REFRESH_TICKS
                    // for live virtual display id, size, and density.
                    if *is_flex && (is_new || tick.is_multiple_of(VD_REFRESH_TICKS)) {
                        if let Some(dump) = adb_shell_output(adb, serial, &["dumpsys", "display"]) {
                            if let Some((did, size)) = parse_virtual_display(&dump) {
                                updated_vd.insert(serial.clone(), (did, size));
                                if let Some(dpi) =
                                    parse_virtual_display_density_for_id(&dump, Some(did))
                                {
                                    cache.flex = Some(dpi);
                                }
                                changed = true;
                            } else if let Some(dpi) =
                                parse_virtual_display_density_for_id(&dump, *display_id)
                            {
                                cache.flex = Some(dpi);
                                changed = true;
                            }
                            if cache.native.is_none() {
                                if let Some(def_dpi) = parse_default_display_density(&dump) {
                                    cache.native = Some(def_dpi);
                                    changed = true;
                                }
                            }
                        }
                    }

                    // Fallback native density from `dumpsys display` if `wm density` lacked it.
                    if !*is_flex && cache.native.is_none() {
                        if let Some(dump) = adb_shell_output(adb, serial, &["dumpsys", "display"]) {
                            if let Some(def_dpi) = parse_default_display_density(&dump) {
                                cache.native = Some(def_dpi);
                                changed = true;
                            }
                        }
                    }

                    if !size_cache.contains_key(serial) && !is_flex {
                        if let Some(sz) = query_device_size(adb, serial) {
                            size_cache.insert(serial.clone(), sz);
                        }
                    }

                    if changed || is_new {
                        updated_densities.insert(serial.clone(), cache);
                    }
                }
            }

            // Apply updated Flex virtual display metadata to AppState.
            if !updated_vd.is_empty() {
                if let Some(state) = app.try_state::<AppState>() {
                    match state.sessions.lock() {
                        Ok(mut map) => {
                            for (serial, (did, size)) in &updated_vd {
                                if let Some(s) = map.values_mut().find(|s| &s.serial == serial) {
                                    s.display_id = Some(*did);
                                    s.virtual_size = Some(*size);
                                }
                            }
                        }
                        Err(e) => {
                            eprintln!("Sessions mutex poisoned in flex display refresher: {e}");
                        }
                    }
                }
            }

            // Resolve HWNDs outside the registry lock as well.
            let resolved_hwnds: HashMap<String, isize> = sessions
                .iter()
                .filter_map(|(serial, ..)| resolve_mirror_hwnd(serial).map(|h| (serial.clone(), h)))
                .collect();

            let Ok(mut reg) = lock_registry() else {
                continue;
            };

            for (serial, cache) in updated_densities {
                reg.densities.insert(serial, cache);
            }

            let mut targets: HashMap<isize, SwipeTarget> = HashMap::new();
            for (serial, display_id, virtual_size, is_flex) in &sessions {
                let cfg = effective_config_for(&reg, serial);
                let mult = compute_multiplier_for(&reg, serial, *is_flex, &cfg);
                let touchpad_speed = effective_touchpad_speed(&cfg);
                reg.multipliers.insert(serial.clone(), mult);

                let Some(&hwnd) = resolved_hwnds.get(serial) else {
                    continue;
                };
                let (did, vsize) = updated_vd
                    .get(serial)
                    .map(|(d, sz)| (Some(*d), Some(*sz)))
                    .unwrap_or((*display_id, *virtual_size));
                let (w, h) = vsize
                    .or_else(|| size_cache.get(serial).copied())
                    .unwrap_or((1080, 1920));
                targets.insert(
                    hwnd,
                    SwipeTarget {
                        serial: Arc::from(serial.as_str()),
                        hwnd,
                        style: cfg.style,
                        multiplier: mult,
                        touchpad_speed,
                        smooth: cfg.smooth,
                        display_id: did,
                        w,
                        h,
                    },
                );
            }

            reg.targets = targets;
            reg.configs
                .retain(|serial, _| sessions.iter().any(|(s, ..)| s == serial));
            reg.paged_override
                .retain(|serial, _| sessions.iter().any(|(s, ..)| s == serial));
            reg.densities
                .retain(|serial, _| sessions.iter().any(|(s, ..)| s == serial));
            reg.multipliers
                .retain(|serial, _| sessions.iter().any(|(s, ..)| s == serial));
            size_cache.retain(|serial, _| sessions.iter().any(|(s, ..)| s == serial));
            sync_any_enabled(&reg);
        }
    }

    fn resolve_mirror_hwnd(serial: &str) -> Option<isize> {
        if let Some(h) = crate::embed::get_embedded_scrcpy_hwnd(serial) {
            if unsafe { IsWindow(Some(HWND(h as *mut _))).as_bool() } {
                return Some(h);
            }
        }
        let title = format!("AnyLeap — {}", serial);
        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let hwnd: HWND = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(wide.as_ptr())) }.ok()?;
        Some(hwnd.0 as isize)
    }

    fn adb_shell_output(
        adb: &std::path::Path,
        serial: &str,
        shell_args: &[&str],
    ) -> Option<String> {
        let out = std::process::Command::new(adb)
            .env("ADB_MDNS_OPENSCREEN", "0")
            .env("ADB_MDNS_AUTO_CONNECT", "0")
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
            let Some((label, value)) = line.split_once(':') else {
                continue;
            };
            let value = value.trim();
            let Some((w, h)) = value.split_once('x') else {
                continue;
            };
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

    // ---- worker side -------------------------------------------------------

    /// Scoped 1 ms Windows timer resolution active only while smooth scrolling animates.
    struct ScopedTimerPeriod {
        active: bool,
    }

    impl ScopedTimerPeriod {
        fn new() -> Self {
            Self { active: false }
        }

        fn set_active(&mut self, want: bool) {
            if want && !self.active {
                if unsafe { timeBeginPeriod(1) } == 0 {
                    self.active = true;
                }
            } else if !want && self.active {
                unsafe {
                    timeEndPeriod(1);
                }
                self.active = false;
            }
        }
    }

    impl Drop for ScopedTimerPeriod {
        fn drop(&mut self) {
            self.set_active(false);
        }
    }

    #[derive(Default)]
    struct SmoothChannel {
        acc: AxisAccumulator,
        touchpad_acc: AxisAccumulator,
        smoother: AxisSmoother,
        last_key_state: u16,
        last_screen_x: i32,
        last_screen_y: i32,
        last_tick_at: Option<Instant>,
        next_tick: Option<Instant>,
    }

    /// Post scaled wheel messages directly to the target scrcpy window.
    ///
    /// Uses `PostMessageW` (never `SendInput`) so AnyLeap's posted messages never
    /// re-enter `WH_MOUSE_LL` and cannot loop back.
    fn post_wheel_deltas(
        hwnd: isize,
        axis: WheelAxis,
        deltas: &[i16],
        key_state: u16,
        screen_x: i32,
        screen_y: i32,
    ) {
        if deltas.is_empty() {
            return;
        }
        let msg = match axis {
            WheelAxis::Vertical => WM_MOUSEWHEEL,
            WheelAxis::Horizontal => WM_MOUSEHWHEEL,
        };
        let lx = (screen_x as i16 as u16) as usize;
        let ly = (screen_y as i16 as u16) as usize;
        let lparam = LPARAM(((ly << 16) | lx) as isize);
        for &d in deltas {
            let wparam = WPARAM((((d as u16) as usize) << 16) | (key_state as usize));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), msg, wparam, lparam);
            }
        }
    }

    fn worker_thread(rx: mpsc::Receiver<WheelEvent>, adb: Option<PathBuf>) {
        // serial -> (last swipe start, wheel-delta accumulator) for Paged mode
        let mut per_serial: HashMap<Arc<str>, (Instant, i32)> = HashMap::new();
        // (hwnd, axis) -> direct accumulator and smoother state
        let mut channels: HashMap<(isize, WheelAxis), SmoothChannel> = HashMap::new();
        let mut timer_period = ScopedTimerPeriod::new();

        loop {
            let earliest_tick = channels
                .values()
                .filter(|ch| ch.smoother.has_pending())
                .filter_map(|ch| ch.next_tick)
                .min();

            let ev = match earliest_tick {
                None => {
                    timer_period.set_active(false);
                    if channels.len() > 16 {
                        channels.retain(|&(hwnd, _), _| unsafe {
                            IsWindow(Some(HWND(hwnd as *mut _))).as_bool()
                        });
                    }
                    match rx.recv() {
                        Ok(ev) => Some(ev),
                        Err(_) => break,
                    }
                }
                Some(deadline) => {
                    timer_period.set_active(true);
                    let wait = deadline.saturating_duration_since(Instant::now());
                    match rx.recv_timeout(wait) {
                        Ok(ev) => Some(ev),
                        Err(mpsc::RecvTimeoutError::Timeout) => None,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
            };

            if let Some(ev) = ev {
                match ev {
                    WheelEvent::Smooth {
                        hwnd,
                        axis,
                        source,
                        delta,
                        key_state,
                        screen_x,
                        screen_y,
                        multiplier,
                        smooth,
                        at,
                    } => {
                        let ch = channels.entry((hwnd, axis)).or_default();
                        ch.last_key_state = key_state;
                        ch.last_screen_x = screen_x;
                        ch.last_screen_y = screen_y;

                        match source {
                            WheelSource::Touchpad => {
                                // Touchpad input always skips smoothing and scales 1:1 with touchpad_speed.
                                let deltas = ch.touchpad_acc.push(delta, multiplier, at);
                                post_wheel_deltas(
                                    hwnd, axis, &deltas, key_state, screen_x, screen_y,
                                );
                            }
                            WheelSource::MouseWheel => {
                                if smooth {
                                    let immediate = ch.smoother.push(delta, multiplier, at);
                                    post_wheel_deltas(
                                        hwnd, axis, &immediate, key_state, screen_x, screen_y,
                                    );
                                    if ch.smoother.has_pending() {
                                        if ch.last_tick_at.is_none() {
                                            ch.last_tick_at = Some(at);
                                            ch.next_tick = Some(at + SMOOTH_TICK);
                                        }
                                    } else {
                                        ch.last_tick_at = None;
                                        ch.next_tick = None;
                                    }
                                } else {
                                    if ch.smoother.has_pending() {
                                        let flushed = ch.smoother.flush();
                                        post_wheel_deltas(
                                            hwnd, axis, &flushed, key_state, screen_x, screen_y,
                                        );
                                        ch.last_tick_at = None;
                                        ch.next_tick = None;
                                    }
                                    let deltas = ch.acc.push(delta, multiplier, at);
                                    post_wheel_deltas(
                                        hwnd, axis, &deltas, key_state, screen_x, screen_y,
                                    );
                                }
                            }
                        }
                    }
                    WheelEvent::Paged { serial, delta, at } => {
                        let Some(adb) = adb.as_deref() else { continue };
                        let entry = per_serial
                            .entry(serial.clone())
                            .or_insert_with(|| (Instant::now() - SWIPE_COOLDOWN, 0));

                        // Events generated while a swipe was in flight are stale — the
                        // user was reacting to the previous state; don't replay them.
                        if at < entry.0 {
                            continue;
                        }
                        entry.1 += delta as i32;
                        if entry.1.abs() < WHEEL_DELTA {
                            continue; // free-spin wheels emit sub-notch deltas; accumulate
                        }
                        let wheel_down = entry.1 < 0;
                        entry.1 = 0;
                        if entry.0.elapsed() < SWIPE_COOLDOWN {
                            continue; // at most ~1 fling per cooldown, no backlog buildup
                        }

                        let (target, size) = {
                            let Ok(reg) = lock_registry() else { continue };
                            match reg.targets.values().find(|t| t.serial == serial) {
                                Some(t) => (t.display_id, (t.w, t.h)),
                                None => continue,
                            }
                        };
                        run_swipe(adb, &serial, target, size, wheel_down);
                        entry.0 = Instant::now();
                    }
                }
            }

            let now = Instant::now();
            for (&(hwnd, axis), ch) in channels.iter_mut() {
                if !ch.smoother.has_pending() {
                    continue;
                }
                let Some(due) = ch.next_tick else { continue };
                if now < due {
                    continue;
                }
                let dt = now.saturating_duration_since(ch.last_tick_at.unwrap_or(now));
                let deltas = ch.smoother.tick(dt, now);
                post_wheel_deltas(
                    hwnd,
                    axis,
                    &deltas,
                    ch.last_key_state,
                    ch.last_screen_x,
                    ch.last_screen_y,
                );
                if ch.smoother.has_pending() {
                    ch.last_tick_at = Some(now);
                    ch.next_tick = Some(now + SMOOTH_TICK);
                } else {
                    ch.last_tick_at = None;
                    ch.next_tick = None;
                }
            }
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
        let x = (w / 2).to_string();
        let (y_from, y_to) = if wheel_down {
            (h * 60 / 100, h * 40 / 100)
        } else {
            (h * 40 / 100, h * 60 / 100)
        };

        let mut cmd = std::process::Command::new(adb);
        cmd.env("ADB_MDNS_OPENSCREEN", "0")
            .env("ADB_MDNS_AUTO_CONNECT", "0");
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
            assert_eq!(
                parse_wm_size("Physical size: 1080x2436\n"),
                Some((1080, 2436))
            );
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

#[cfg(test)]
mod decision_tests {
    use super::*;

    #[test]
    fn passes_through_when_not_over_mirror_or_queue_full() {
        assert_eq!(
            decide(
                ModifierState::default(),
                WheelAxis::Vertical,
                WheelSource::MouseWheel,
                false,
                ScrollStyle::Smooth,
                2.5,
                1.0,
                true,
                true,
            ),
            HookDecision::PassThrough
        );
        assert_eq!(
            decide(
                ModifierState::default(),
                WheelAxis::Vertical,
                WheelSource::MouseWheel,
                true,
                ScrollStyle::Smooth,
                2.5,
                1.0,
                true,
                false,
            ),
            HookDecision::PassThrough
        );
    }

    #[test]
    fn ctrl_alt_win_pass_through_shift_scales() {
        for mods in [
            ModifierState {
                ctrl: true,
                ..Default::default()
            },
            ModifierState {
                alt: true,
                ..Default::default()
            },
            ModifierState {
                win: true,
                ..Default::default()
            },
        ] {
            assert_eq!(
                decide(
                    mods,
                    WheelAxis::Vertical,
                    WheelSource::MouseWheel,
                    true,
                    ScrollStyle::Smooth,
                    2.5,
                    1.0,
                    true,
                    true
                ),
                HookDecision::PassThrough
            );
            assert_eq!(
                decide(
                    mods,
                    WheelAxis::Vertical,
                    WheelSource::Touchpad,
                    true,
                    ScrollStyle::Smooth,
                    2.5,
                    2.0,
                    true,
                    true
                ),
                HookDecision::PassThrough
            );
            assert_eq!(
                decide(
                    mods,
                    WheelAxis::Vertical,
                    WheelSource::MouseWheel,
                    true,
                    ScrollStyle::Paged,
                    1.0,
                    1.0,
                    false,
                    true
                ),
                HookDecision::PassThrough
            );
        }

        assert_eq!(
            decide(
                ModifierState {
                    shift: true,
                    ..Default::default()
                },
                WheelAxis::Vertical,
                WheelSource::MouseWheel,
                true,
                ScrollStyle::Smooth,
                2.5,
                1.0,
                false,
                true,
            ),
            HookDecision::Swallow
        );
    }

    #[test]
    fn smooth_1x_without_smoothing_is_noop_and_smoothing_or_higher_swallows_both_axes() {
        assert_eq!(
            decide(
                ModifierState::default(),
                WheelAxis::Vertical,
                WheelSource::MouseWheel,
                true,
                ScrollStyle::Smooth,
                1.0,
                1.0,
                false,
                true,
            ),
            HookDecision::PassThrough
        );
        assert_eq!(
            decide(
                ModifierState::default(),
                WheelAxis::Horizontal,
                WheelSource::MouseWheel,
                true,
                ScrollStyle::Smooth,
                1.0,
                1.0,
                false,
                true,
            ),
            HookDecision::PassThrough
        );
        assert_eq!(
            decide(
                ModifierState::default(),
                WheelAxis::Vertical,
                WheelSource::MouseWheel,
                true,
                ScrollStyle::Smooth,
                1.0,
                1.0,
                true,
                true,
            ),
            HookDecision::Swallow
        );
        assert_eq!(
            decide(
                ModifierState::default(),
                WheelAxis::Horizontal,
                WheelSource::MouseWheel,
                true,
                ScrollStyle::Smooth,
                1.0,
                1.0,
                true,
                true,
            ),
            HookDecision::Swallow
        );
        assert_eq!(
            decide(
                ModifierState::default(),
                WheelAxis::Vertical,
                WheelSource::MouseWheel,
                true,
                ScrollStyle::Smooth,
                2.5,
                1.0,
                false,
                true,
            ),
            HookDecision::Swallow
        );
        assert_eq!(
            decide(
                ModifierState::default(),
                WheelAxis::Horizontal,
                WheelSource::MouseWheel,
                true,
                ScrollStyle::Smooth,
                2.5,
                1.0,
                false,
                true,
            ),
            HookDecision::Swallow
        );
    }

    #[test]
    fn touchpad_at_1x_passes_through_even_with_auto_and_smoothing_on() {
        for axis in [WheelAxis::Vertical, WheelAxis::Horizontal] {
            assert_eq!(
                decide(
                    ModifierState::default(),
                    axis,
                    WheelSource::Touchpad,
                    true,
                    ScrollStyle::Smooth,
                    2.75,
                    1.0,
                    true,
                    true,
                ),
                HookDecision::PassThrough
            );
            assert_eq!(
                decide(
                    ModifierState::default(),
                    axis,
                    WheelSource::Touchpad,
                    true,
                    ScrollStyle::Smooth,
                    1.0,
                    1.5,
                    false,
                    true,
                ),
                HookDecision::Swallow
            );
            assert_eq!(
                decide(
                    ModifierState::default(),
                    axis,
                    WheelSource::Touchpad,
                    true,
                    ScrollStyle::Smooth,
                    2.75,
                    0.5,
                    true,
                    true,
                ),
                HookDecision::Swallow
            );
        }
    }

    #[test]
    fn paged_swallows_vertical_and_passes_horizontal() {
        assert_eq!(
            decide(
                ModifierState::default(),
                WheelAxis::Vertical,
                WheelSource::MouseWheel,
                true,
                ScrollStyle::Paged,
                1.0,
                1.0,
                true,
                true,
            ),
            HookDecision::Swallow
        );
        assert_eq!(
            decide(
                ModifierState::default(),
                WheelAxis::Horizontal,
                WheelSource::MouseWheel,
                true,
                ScrollStyle::Paged,
                2.5,
                1.5,
                true,
                true,
            ),
            HookDecision::PassThrough
        );
    }
}

use tauri::AppHandle;

use crate::scroll_feel::ScrollConfig;

/// Enable/disable wheel-to-swipe (Paged scrolling) for a device's mirror window.
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

/// Set scroll-feel configuration for a device's mirror session.
#[tauri::command]
pub async fn set_scroll_feel(
    app: AppHandle,
    serial: String,
    config: ScrollConfig,
) -> Result<Option<f32>, String> {
    #[cfg(windows)]
    {
        imp::set_scroll_feel(&app, &serial, config)
    }
    #[cfg(not(windows))]
    {
        let _ = (app, serial, config);
        Ok(None)
    }
}

/// Read the latest computed scroll multiplier for a running mirror session.
#[tauri::command]
pub async fn get_scroll_multiplier(serial: String) -> Result<Option<f32>, String> {
    #[cfg(windows)]
    {
        Ok(imp::get_scroll_multiplier(&serial))
    }
    #[cfg(not(windows))]
    {
        let _ = serial;
        Ok(None)
    }
}

/// Read the active scroll style for a running mirror session.
#[tauri::command]
pub async fn get_scroll_style(serial: String) -> Result<Option<ScrollStyle>, String> {
    #[cfg(windows)]
    {
        Ok(imp::get_scroll_style(&serial))
    }
    #[cfg(not(windows))]
    {
        let _ = serial;
        Ok(None)
    }
}

/// Start the hook/refresher/worker threads. No-op off Windows.
pub fn init(app: &AppHandle) {
    #[cfg(windows)]
    imp::init(app);
    #[cfg(not(windows))]
    let _ = app;
}
