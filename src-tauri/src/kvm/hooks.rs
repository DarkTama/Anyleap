use super::debug_log::{self, DebugEvent, HookKind, KeyDecision, KeySnapshot};
use super::input_logic::{
    detect_controller_edge, map_leap_relative_y, map_return_position, union_rect, Edge,
    PressedInputTracker,
};
use super::monitors::{get_local_monitors, MonitorDisplay};
use super::protocol::{KvmPacket, ScreenRect, TopologyConfig};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::mpsc;

#[cfg(windows)]
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
#[cfg(windows)]
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONULL};
#[cfg(windows)]
use windows::Win32::Security::{
    GetSidSubAuthority, GetSidSubAuthorityCount, GetTokenInformation, TokenElevation,
    TokenIntegrityLevel, TOKEN_ELEVATION, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
#[cfg(windows)]
use windows::Win32::System::SystemInformation::GetTickCount;
#[cfg(windows)]
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, OpenProcess, OpenProcessToken,
    QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::*;

#[cfg(windows)]
const WM_KVM_RESTORE_CURSOR: u32 = WM_APP + 1;
#[cfg(windows)]
const WM_KVM_ENGAGE: u32 = WM_APP + 2;
#[cfg(windows)]
const WATCHDOG_INTERVAL_MS: u32 = 250;
#[cfg(any(windows, test))]
const WATCHDOG_BLIND_THRESHOLD_MS: u32 = 1000;
const BLOCKED_NOTICE_COOLDOWN_MS: u64 = 1000;
const SECURITY_MANDATORY_MEDIUM_RID: u32 = 0x2000;
const SECURITY_MANDATORY_HIGH_RID: u32 = 0x3000;

pub type LeapBlockedCallback = Arc<dyn Fn(&str) + Send + Sync>;

static PACKET_SENDER: OnceLock<Mutex<Option<mpsc::Sender<KvmPacket>>>> = OnceLock::new();
static MOUSE_DELTA_SENDER: OnceLock<Mutex<Option<mpsc::Sender<(i32, i32)>>>> = OnceLock::new();
static LEAP_BLOCKED_CALLBACK: OnceLock<Mutex<Option<LeapBlockedCallback>>> = OnceLock::new();
static LOCAL_MONITORS: OnceLock<Mutex<Vec<MonitorDisplay>>> = OnceLock::new();
static CONTROLLER_TRACKER: OnceLock<Mutex<PressedInputTracker>> = OnceLock::new();

static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);
static IS_TRAPPED: AtomicBool = AtomicBool::new(false);
static ENGAGE_PENDING: AtomicBool = AtomicBool::new(false);
static CURSOR_CENTERED: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);

static CLAMP_CENTER_X: AtomicI32 = AtomicI32::new(960);
static CLAMP_CENTER_Y: AtomicI32 = AtomicI32::new(540);
static TRAP_MON_X: AtomicI32 = AtomicI32::new(0);
static TRAP_MON_Y: AtomicI32 = AtomicI32::new(0);
static TRAP_MON_W: AtomicI32 = AtomicI32::new(1920);
static TRAP_MON_H: AtomicI32 = AtomicI32::new(1080);
static PENDING_ENGAGE_REL_Y: AtomicU32 = AtomicU32::new(0);

static LINK_FROM_EDGE: AtomicU8 = AtomicU8::new(Edge::Right as u8);
static LINK_TO_EDGE: AtomicU8 = AtomicU8::new(Edge::Left as u8);
static HAS_ACTIVE_LINK: AtomicBool = AtomicBool::new(false);
static DEADZONE_PX: AtomicI32 = AtomicI32::new(48);
static RESISTANCE_MS: AtomicU64 = AtomicU64::new(120);
static LOCAL_UNION_X: AtomicI32 = AtomicI32::new(0);
static LOCAL_UNION_Y: AtomicI32 = AtomicI32::new(0);
static LOCAL_UNION_W: AtomicI32 = AtomicI32::new(1920);
static LOCAL_UNION_H: AtomicI32 = AtomicI32::new(1080);
static PEER_UNION_W: AtomicI32 = AtomicI32::new(0);
static PEER_UNION_H: AtomicI32 = AtomicI32::new(0);
static LINK_OFFSET_X: AtomicI32 = AtomicI32::new(0);
static LINK_OFFSET_Y: AtomicI32 = AtomicI32::new(0);

static MOUSE_BUTTONS_DOWN: AtomicU8 = AtomicU8::new(0);
static KEY_CTRL_DOWN: AtomicBool = AtomicBool::new(false);
static KEY_ALT_DOWN: AtomicBool = AtomicBool::new(false);
static EDGE_DWELL_START: AtomicU64 = AtomicU64::new(0);
#[cfg(windows)]
static LAST_BLOCKED_NOTICE_MS: AtomicU64 = AtomicU64::new(0);
#[cfg(windows)]
static LAST_HOOK_TICK_MS: AtomicU32 = AtomicU32::new(0);
#[cfg(windows)]
static HAS_LAST_HOOK_TICK: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static LAST_MOUSE_HOOK_US: AtomicU64 = AtomicU64::new(0);
#[cfg(windows)]
static LAST_KBD_HOOK_US: AtomicU64 = AtomicU64::new(0);

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn packet_sender() -> &'static Mutex<Option<mpsc::Sender<KvmPacket>>> {
    PACKET_SENDER.get_or_init(|| Mutex::new(None))
}

fn mouse_delta_sender() -> &'static Mutex<Option<mpsc::Sender<(i32, i32)>>> {
    MOUSE_DELTA_SENDER.get_or_init(|| Mutex::new(None))
}

fn leap_blocked_callback_slot() -> &'static Mutex<Option<LeapBlockedCallback>> {
    LEAP_BLOCKED_CALLBACK.get_or_init(|| Mutex::new(None))
}

fn local_monitors() -> &'static Mutex<Vec<MonitorDisplay>> {
    LOCAL_MONITORS.get_or_init(|| Mutex::new(Vec::new()))
}

fn controller_tracker() -> &'static Mutex<PressedInputTracker> {
    CONTROLLER_TRACKER.get_or_init(|| Mutex::new(PressedInputTracker::new()))
}

fn try_send_packet(pkt: KvmPacket) {
    if let Ok(guard) = packet_sender().try_lock() {
        if let Some(ref tx) = *guard {
            let _ = tx.try_send(pkt);
        }
    }
}

/// Reset cursor clip and display counter so cursor never stays hidden.
pub fn restore_cursor_visibility() {
    #[cfg(windows)]
    unsafe {
        let _ = ClipCursor(None);
        for _ in 0..16 {
            if ShowCursor(true) >= 0 {
                break;
            }
        }
    }
}

#[cfg(windows)]
fn post_hook_thread_message(msg: u32) {
    unsafe {
        let tid = HOOK_THREAD_ID.load(Ordering::Acquire);
        if tid != 0 {
            let _ = PostThreadMessageW(tid, msg, WPARAM(0), LPARAM(0));
        }
    }
}

fn notify_hook_thread_restore() {
    restore_cursor_visibility();
    #[cfg(windows)]
    post_hook_thread_message(WM_KVM_RESTORE_CURSOR);
}

#[cfg(any(windows, test))]
fn compute_trap_center(mx: i32, my: i32, mw: i32, mh: i32) -> (i32, i32) {
    (mx + mw / 2, my + mh / 2)
}

#[cfg(any(windows, test))]
fn trapped_mouse_delta(
    cursor_centered: bool,
    pt: (i32, i32),
    center: (i32, i32),
) -> Option<(i32, i32)> {
    if !cursor_centered {
        return None;
    }
    let dx = pt.0 - center.0;
    let dy = pt.1 - center.1;
    if dx == 0 && dy == 0 {
        None
    } else {
        Some((dx, dy))
    }
}

#[cfg(any(windows, test))]
fn should_perform_engage(hook_active: bool, is_trapped: bool, engage_pending: bool) -> bool {
    hook_active && is_trapped && engage_pending
}

pub struct HookController {
    _running: Arc<AtomicBool>,
}

impl HookController {
    pub fn new() -> Self {
        Self {
            _running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_active(&self) -> bool {
        HOOK_ACTIVE.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    pub(crate) fn set_active_for_test(&self, active: bool, edge: Edge) {
        HOOK_ACTIVE.store(active, Ordering::SeqCst);
        LINK_FROM_EDGE.store(edge as u8, Ordering::Relaxed);
        HAS_ACTIVE_LINK.store(active, Ordering::Release);
    }

    /// Edge that leaps to the peer, when capture is active with a link.
    pub fn active_link_edge(&self) -> Option<&'static str> {
        (HOOK_ACTIVE.load(Ordering::Relaxed) && HAS_ACTIVE_LINK.load(Ordering::Acquire))
            .then(|| Edge::from_u8(LINK_FROM_EDGE.load(Ordering::Relaxed)).as_str())
    }

    pub fn is_trapped(&self) -> bool {
        IS_TRAPPED.load(Ordering::Acquire)
    }

    pub fn set_trapped(&self, trapped: bool) {
        IS_TRAPPED.store(trapped, Ordering::SeqCst);
        if !trapped {
            CURSOR_CENTERED.store(false, Ordering::Release);
            ENGAGE_PENDING.store(false, Ordering::Release);
            notify_hook_thread_restore();
        }
    }

    pub fn handle_cursor_return(&self, relative_y: f32) {
        let was_trapped = IS_TRAPPED.swap(false, Ordering::SeqCst);
        if debug_log::is_enabled() {
            debug_log::record_event(DebugEvent::Return {
                ts_us: debug_log::now_us(),
                relative_y,
                was_trapped,
                hwnd: debug_log::foreground_hwnd_raw(),
            });
        }
        if !was_trapped {
            return;
        }
        CURSOR_CENTERED.store(false, Ordering::Release);
        ENGAGE_PENDING.store(false, Ordering::Release);
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        notify_hook_thread_restore();

        let fallback_mon = MonitorDisplay {
            id: String::new(),
            x: TRAP_MON_X.load(Ordering::Relaxed),
            y: TRAP_MON_Y.load(Ordering::Relaxed),
            width: TRAP_MON_W.load(Ordering::Relaxed),
            height: TRAP_MON_H.load(Ordering::Relaxed),
            is_primary: true,
        };
        let peer_union = ScreenRect {
            x: 0,
            y: 0,
            width: PEER_UNION_W.load(Ordering::Relaxed),
            height: PEER_UNION_H.load(Ordering::Relaxed),
        };
        let link_edge = Edge::from_u8(LINK_FROM_EDGE.load(Ordering::Relaxed));
        let offset_x = LINK_OFFSET_X.load(Ordering::Relaxed);
        let offset_y = LINK_OFFSET_Y.load(Ordering::Relaxed);
        let (rx, ry) = if let Ok(mons) = local_monitors().try_lock() {
            map_return_position(
                &mons,
                &fallback_mon,
                &peer_union,
                link_edge,
                offset_x,
                offset_y,
                relative_y,
            )
        } else {
            map_return_position(
                &[],
                &fallback_mon,
                &peer_union,
                link_edge,
                offset_x,
                offset_y,
                relative_y,
            )
        };
        #[cfg(windows)]
        unsafe {
            let _ = SetCursorPos(rx, ry);
        }
        #[cfg(not(windows))]
        let _ = (rx, ry);
    }

    pub fn emergency_release(&self) {
        if debug_log::is_enabled() {
            debug_log::record_event(DebugEvent::Release {
                ts_us: debug_log::now_us(),
                reason: "emergency_release",
            });
        }
        self.set_trapped(false);
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        MOUSE_BUTTONS_DOWN.store(0, Ordering::Relaxed);
        KEY_CTRL_DOWN.store(false, Ordering::Relaxed);
        KEY_ALT_DOWN.store(false, Ordering::Relaxed);
        if let Ok(mut tracker) = controller_tracker().try_lock() {
            let _ = tracker.release_all();
        }
        notify_hook_thread_restore();

        try_send_packet(KvmPacket::EmergencyRelease);
        try_send_packet(KvmPacket::ReleaseAll);
    }

    pub fn start_hooks(
        &self,
        topology: TopologyConfig,
        packet_tx: mpsc::Sender<KvmPacket>,
        delta_tx: mpsc::Sender<(i32, i32)>,
        on_leap_blocked: Option<LeapBlockedCallback>,
    ) {
        let monitors = get_local_monitors();
        let local_union = union_rect(&monitors);
        LOCAL_UNION_X.store(local_union.x, Ordering::Relaxed);
        LOCAL_UNION_Y.store(local_union.y, Ordering::Relaxed);
        LOCAL_UNION_W.store(local_union.width, Ordering::Relaxed);
        LOCAL_UNION_H.store(local_union.height, Ordering::Relaxed);
        if let Ok(mut mons) = local_monitors().lock() {
            *mons = monitors;
        }

        let local_id = topology
            .computers
            .iter()
            .find(|c| c.is_local)
            .map(|c| c.id.as_str());

        if let Some(link) = local_id.and_then(|id| topology.links.iter().find(|l| l.from_id == id)) {
            let from_edge = Edge::from_str(&link.from_edge).unwrap_or(Edge::Right);
            let to_edge = Edge::from_str(&link.to_edge).unwrap_or_else(|| from_edge.opposite());
            let peer_bounds = topology
                .computers
                .iter()
                .find(|c| c.id == link.to_id)
                .or_else(|| topology.computers.iter().find(|c| !c.is_local))
                .map(|c| {
                    if !c.monitors.is_empty() {
                        union_rect(&c.monitors)
                    } else {
                        c.bounds.clone()
                    }
                })
                .unwrap_or_else(|| ScreenRect {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                });
            LINK_FROM_EDGE.store(from_edge as u8, Ordering::Relaxed);
            LINK_TO_EDGE.store(to_edge as u8, Ordering::Relaxed);
            DEADZONE_PX.store(link.deadzone_px.unwrap_or(48), Ordering::Relaxed);
            RESISTANCE_MS.store(link.resistance_ms.unwrap_or(120) as u64, Ordering::Relaxed);
            LINK_OFFSET_X.store(link.offset_x.unwrap_or(0), Ordering::Relaxed);
            LINK_OFFSET_Y.store(link.offset_y.unwrap_or(0), Ordering::Relaxed);
            PEER_UNION_W.store(peer_bounds.width, Ordering::Relaxed);
            PEER_UNION_H.store(peer_bounds.height, Ordering::Relaxed);
            HAS_ACTIVE_LINK.store(true, Ordering::Release);
        } else {
            HAS_ACTIVE_LINK.store(false, Ordering::Release);
        }

        if let Ok(mut g) = packet_sender().lock() {
            *g = Some(packet_tx);
        }
        if let Ok(mut g) = mouse_delta_sender().lock() {
            *g = Some(delta_tx);
        }
        if let Ok(mut g) = leap_blocked_callback_slot().lock() {
            *g = on_leap_blocked;
        }

        if HOOK_ACTIVE.swap(true, Ordering::SeqCst) {
            return;
        }

        #[cfg(windows)]
        {
            LAST_BLOCKED_NOTICE_MS.store(0, Ordering::Relaxed);
            HAS_LAST_HOOK_TICK.store(false, Ordering::Release);
            LAST_HOOK_TICK_MS.store(0, Ordering::Release);
            LAST_MOUSE_HOOK_US.store(0, Ordering::Relaxed);
            LAST_KBD_HOOK_US.store(0, Ordering::Relaxed);
        }

        #[cfg(windows)]
        std::thread::spawn(move || unsafe {
            let mut msg = MSG::default();
            // Force message queue creation before publishing HOOK_THREAD_ID so
            // PostThreadMessageW from a hook callback never races queue init.
            let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);

            let tid = GetCurrentThreadId();
            HOOK_THREAD_ID.store(tid, Ordering::Release);

            let h_mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), None, 0);
            let h_kbd = SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), None, 0);
            let mut watchdog_timer_id: usize = 0;

            while HOOK_ACTIVE.load(Ordering::Relaxed) {
                let res = GetMessageW(&mut msg, None, 0, 0);
                if res.0 <= 0 {
                    break;
                }
                if msg.message == WM_KVM_RESTORE_CURSOR {
                    restore_cursor_visibility();
                    if !IS_TRAPPED.load(Ordering::Acquire) && watchdog_timer_id != 0 {
                        let _ = KillTimer(None, watchdog_timer_id);
                        watchdog_timer_id = 0;
                    }
                    continue;
                }
                if msg.message == WM_KVM_ENGAGE {
                    if perform_engage_leap() && watchdog_timer_id == 0 {
                        watchdog_timer_id = SetTimer(None, 0, WATCHDOG_INTERVAL_MS, None);
                    }
                    continue;
                }
                if msg.message == WM_TIMER
                    && watchdog_timer_id != 0
                    && msg.wParam.0 == watchdog_timer_id
                {
                    if !IS_TRAPPED.load(Ordering::Acquire) {
                        let _ = KillTimer(None, watchdog_timer_id);
                        watchdog_timer_id = 0;
                    } else if let Some(last_input_tick) = query_last_input_tick() {
                        let hook_tick = HAS_LAST_HOOK_TICK
                            .load(Ordering::Acquire)
                            .then(|| LAST_HOOK_TICK_MS.load(Ordering::Acquire));
                        if let Some(gap_ms) = evaluate_capture_watchdog(
                            true,
                            hook_tick,
                            last_input_tick,
                            WATCHDOG_BLIND_THRESHOLD_MS,
                        ) {
                            let _ = KillTimer(None, watchdog_timer_id);
                            watchdog_timer_id = 0;
                            perform_watchdog_release(gap_ms);
                        }
                    }
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            if watchdog_timer_id != 0 {
                let _ = KillTimer(None, watchdog_timer_id);
            }
            if let Ok(hook) = h_mouse {
                let _ = UnhookWindowsHookEx(hook);
            }
            if let Ok(hook) = h_kbd {
                let _ = UnhookWindowsHookEx(hook);
            }
            HOOK_THREAD_ID.store(0, Ordering::SeqCst);
            restore_cursor_visibility();
        });
    }

    pub fn stop_hooks(&self) {
        let was_active = HOOK_ACTIVE.swap(false, Ordering::SeqCst);
        if was_active || IS_TRAPPED.load(Ordering::Acquire) {
            self.emergency_release();
        }
        HAS_ACTIVE_LINK.store(false, Ordering::Release);
        if let Ok(mut g) = leap_blocked_callback_slot().try_lock() {
            *g = None;
        }

        #[cfg(windows)]
        unsafe {
            let tid = HOOK_THREAD_ID.load(Ordering::SeqCst);
            if tid != 0 {
                let _ = PostThreadMessageW(tid, WM_QUIT, WPARAM(0), LPARAM(0));
            }
        }
    }
}

#[cfg(windows)]
fn disarm_trapped_in_callback() {
    IS_TRAPPED.store(false, Ordering::SeqCst);
    CURSOR_CENTERED.store(false, Ordering::Release);
    ENGAGE_PENDING.store(false, Ordering::Release);
    EDGE_DWELL_START.store(0, Ordering::Relaxed);
    MOUSE_BUTTONS_DOWN.store(0, Ordering::Relaxed);
}

#[cfg(windows)]
const SLOW_HOOK_THRESHOLD_US: u64 = 20_000;
#[cfg(windows)]
const HOOK_GAP_THRESHOLD_US: u64 = 250_000;

#[cfg(windows)]
#[inline]
fn record_hook_tick() {
    let tick = unsafe { GetTickCount() };
    LAST_HOOK_TICK_MS.store(tick, Ordering::Release);
    HAS_LAST_HOOK_TICK.store(true, Ordering::Release);
}

#[cfg(windows)]
fn record_hook_entry(kind: HookKind) -> Option<std::time::Instant> {
    // Unconditional so the capture watchdog tracks live hook cadence even when
    // debug logging is turned off.
    record_hook_tick();
    if !debug_log::is_enabled() {
        return None;
    }
    let now = debug_log::now_us();
    let slot = match kind {
        HookKind::Keyboard => &LAST_KBD_HOOK_US,
        HookKind::Mouse => &LAST_MOUSE_HOOK_US,
    };
    let prev = slot.swap(now, Ordering::Relaxed);
    if prev != 0 && IS_TRAPPED.load(Ordering::Acquire) {
        let gap_us = now.saturating_sub(prev);
        if gap_us > HOOK_GAP_THRESHOLD_US {
            debug_log::record_event(DebugEvent::HookGap {
                ts_us: now,
                kind,
                gap_us,
            });
        }
    }
    Some(std::time::Instant::now())
}

#[cfg(windows)]
fn record_if_slow_hook(kind: HookKind, started: Option<std::time::Instant>) {
    if let Some(t0) = started {
        let duration_us = t0.elapsed().as_micros() as u64;
        if duration_us > SLOW_HOOK_THRESHOLD_US {
            debug_log::record_event(DebugEvent::SlowHook {
                ts_us: debug_log::now_us(),
                kind,
                duration_us,
            });
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn low_level_mouse_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    let started = record_hook_entry(HookKind::Mouse);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_mouse_event(n_code, w_param, l_param)
    }));
    record_if_slow_hook(HookKind::Mouse, started);

    match outcome {
        Ok(Some(res)) => res,
        Ok(None) => CallNextHookEx(None, n_code, w_param, l_param),
        Err(_) => {
            disarm_trapped_in_callback();
            debug_log::record_event(DebugEvent::Release {
                ts_us: debug_log::now_us(),
                reason: "mouse_hook_panic",
            });
            post_hook_thread_message(WM_KVM_RESTORE_CURSOR);
            CallNextHookEx(None, n_code, w_param, l_param)
        }
    }
}

#[cfg(windows)]
unsafe fn handle_mouse_event(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> Option<LRESULT> {
    if n_code < 0 || !HOOK_ACTIVE.load(Ordering::Relaxed) {
        return None;
    }

    let hook_struct = *(l_param.0 as *const MSLLHOOKSTRUCT);
    let pt = hook_struct.pt;
    let msg = w_param.0 as u32;

    match msg {
        WM_LBUTTONDOWN => {
            MOUSE_BUTTONS_DOWN.fetch_or(1, Ordering::Relaxed);
        }
        WM_LBUTTONUP => {
            MOUSE_BUTTONS_DOWN.fetch_and(!1, Ordering::Relaxed);
        }
        WM_RBUTTONDOWN => {
            MOUSE_BUTTONS_DOWN.fetch_or(2, Ordering::Relaxed);
        }
        WM_RBUTTONUP => {
            MOUSE_BUTTONS_DOWN.fetch_and(!2, Ordering::Relaxed);
        }
        WM_MBUTTONDOWN => {
            MOUSE_BUTTONS_DOWN.fetch_or(4, Ordering::Relaxed);
        }
        WM_MBUTTONUP => {
            MOUSE_BUTTONS_DOWN.fetch_and(!4, Ordering::Relaxed);
        }
        WM_XBUTTONDOWN => {
            let xbtn = ((hook_struct.mouseData >> 16) & 0xFFFF) as u16;
            let bit = if xbtn == 2 { 16 } else { 8 };
            MOUSE_BUTTONS_DOWN.fetch_or(bit, Ordering::Relaxed);
        }
        WM_XBUTTONUP => {
            let xbtn = ((hook_struct.mouseData >> 16) & 0xFFFF) as u16;
            let bit = if xbtn == 2 { 16 } else { 8 };
            MOUSE_BUTTONS_DOWN.fetch_and(!bit, Ordering::Relaxed);
        }
        _ => {}
    }

    if IS_TRAPPED.load(Ordering::Acquire) {
        if msg == WM_MOUSEMOVE {
            let center = (
                CLAMP_CENTER_X.load(Ordering::Relaxed),
                CLAMP_CENTER_Y.load(Ordering::Relaxed),
            );
            if let Some(delta) =
                trapped_mouse_delta(CURSOR_CENTERED.load(Ordering::Acquire), (pt.x, pt.y), center)
            {
                if let Ok(guard) = mouse_delta_sender().try_lock() {
                    if let Some(ref tx) = *guard {
                        let _ = tx.try_send(delta);
                    }
                }
            }
        } else {
            match msg {
                WM_LBUTTONDOWN => try_send_packet(KvmPacket::MouseButton { button: 1, is_down: true }),
                WM_LBUTTONUP => try_send_packet(KvmPacket::MouseButton { button: 1, is_down: false }),
                WM_RBUTTONDOWN => try_send_packet(KvmPacket::MouseButton { button: 2, is_down: true }),
                WM_RBUTTONUP => try_send_packet(KvmPacket::MouseButton { button: 2, is_down: false }),
                WM_MBUTTONDOWN => try_send_packet(KvmPacket::MouseButton { button: 3, is_down: true }),
                WM_MBUTTONUP => try_send_packet(KvmPacket::MouseButton { button: 3, is_down: false }),
                WM_MOUSEWHEEL => {
                    let delta = (hook_struct.mouseData >> 16) as i16 as i32;
                    try_send_packet(KvmPacket::MouseWheel { delta });
                }
                _ => {}
            }
        }
        return Some(LRESULT(1));
    }

    if msg != WM_MOUSEMOVE || !HAS_ACTIVE_LINK.load(Ordering::Acquire) {
        return None;
    }

    if MOUSE_BUTTONS_DOWN.load(Ordering::Relaxed) != 0 {
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        return None;
    }

    let link_edge = Edge::from_u8(LINK_FROM_EDGE.load(Ordering::Relaxed));
    let deadzone = DEADZONE_PX.load(Ordering::Relaxed);
    let resistance_ms = RESISTANCE_MS.load(Ordering::Relaxed);

    let hit = {
        let Ok(mons) = local_monitors().try_lock() else {
            return None;
        };
        detect_controller_edge((pt.x, pt.y), &mons, link_edge, deadzone).map(|(idx, _)| {
            let m = &mons[idx];
            let clamped = (
                pt.x.clamp(m.x, m.x + m.width - 1),
                pt.y.clamp(m.y, m.y + m.height - 1),
            );
            let local_union = ScreenRect {
                x: LOCAL_UNION_X.load(Ordering::Relaxed),
                y: LOCAL_UNION_Y.load(Ordering::Relaxed),
                width: LOCAL_UNION_W.load(Ordering::Relaxed),
                height: LOCAL_UNION_H.load(Ordering::Relaxed),
            };
            let peer_union = ScreenRect {
                x: 0,
                y: 0,
                width: PEER_UNION_W.load(Ordering::Relaxed),
                height: PEER_UNION_H.load(Ordering::Relaxed),
            };
            let rel = map_leap_relative_y(
                clamped,
                &local_union,
                &peer_union,
                link_edge,
                LINK_OFFSET_X.load(Ordering::Relaxed),
                LINK_OFFSET_Y.load(Ordering::Relaxed),
            );
            (m.x, m.y, m.width, m.height, rel)
        })
    };

    let Some((mx, my, mw, mh, rel)) = hit else {
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        return None;
    };

    // Double-check live OS monitor topology on the adjacent pixel beyond the link edge
    let neighbor = match link_edge {
        // pt may lie past the screen edge; clamp onto the edge monitor first.
        Edge::Right => POINT { x: mx + mw, y: pt.y.clamp(my, my + mh - 1) },
        Edge::Left => POINT { x: mx - 1, y: pt.y.clamp(my, my + mh - 1) },
        Edge::Top => POINT { x: pt.x.clamp(mx, mx + mw - 1), y: my - 1 },
        Edge::Bottom => POINT { x: pt.x.clamp(mx, mx + mw - 1), y: my + mh },
    };
    if !MonitorFromPoint(neighbor, MONITOR_DEFAULTTONULL).is_invalid() {
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        return None;
    }

    if check_dwell_resistance(resistance_ms) {
        request_engage_leap(mx, my, mw, mh, rel);
        return Some(LRESULT(1));
    }

    None
}

fn check_dwell_resistance(resistance_ms: u64) -> bool {
    if resistance_ms == 0 {
        return true;
    }
    let now = now_millis();
    let start = EDGE_DWELL_START.load(Ordering::Relaxed);
    if start == 0 {
        EDGE_DWELL_START.store(now, Ordering::Relaxed);
        false
    } else if now.saturating_sub(start) >= resistance_ms {
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        true
    } else {
        false
    }
}

/// Callback-safe leap transition: updates atomics and posts `WM_KVM_ENGAGE` to
/// the hook thread so Win32 cursor/SendInput work runs outside hook callbacks.
#[cfg(windows)]
fn request_engage_leap(mx: i32, my: i32, mw: i32, mh: i32, relative_y: f32) {
    IS_TRAPPED.store(true, Ordering::SeqCst);
    CURSOR_CENTERED.store(false, Ordering::Release);

    TRAP_MON_X.store(mx, Ordering::Relaxed);
    TRAP_MON_Y.store(my, Ordering::Relaxed);
    TRAP_MON_W.store(mw, Ordering::Relaxed);
    TRAP_MON_H.store(mh, Ordering::Relaxed);

    let (center_x, center_y) = compute_trap_center(mx, my, mw, mh);
    CLAMP_CENTER_X.store(center_x, Ordering::Relaxed);
    CLAMP_CENTER_Y.store(center_y, Ordering::Relaxed);
    PENDING_ENGAGE_REL_Y.store(relative_y.to_bits(), Ordering::Relaxed);
    ENGAGE_PENDING.store(true, Ordering::Release);

    EDGE_DWELL_START.store(0, Ordering::Relaxed);
    KEY_CTRL_DOWN.store(false, Ordering::Relaxed);
    KEY_ALT_DOWN.store(false, Ordering::Relaxed);

    if debug_log::is_enabled() {
        debug_log::record_event(DebugEvent::Engage {
            ts_us: debug_log::now_us(),
            phase: "request",
            edge: Edge::from_u8(LINK_FROM_EDGE.load(Ordering::Relaxed)),
            relative_y,
            hwnd: debug_log::foreground_hwnd_raw(),
        });
    }

    post_hook_thread_message(WM_KVM_ENGAGE);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandoffMethod {
    Shell,
    Next,
    Minimize,
}

impl HandoffMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::Shell => "shell",
            Self::Next => "next",
            Self::Minimize => "minimize",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessElevation {
    Standard { integrity_rid: u32 },
    Elevated,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowCandidateInfo {
    hwnd: isize,
    pid: u32,
    visible: bool,
    cloaked: bool,
    minimized: bool,
    tool_window: bool,
    elevation: ProcessElevation,
}

fn is_safe_handoff_elevation(elevation: ProcessElevation) -> bool {
    matches!(
        elevation,
        ProcessElevation::Standard { integrity_rid }
            if integrity_rid <= SECURITY_MANDATORY_MEDIUM_RID
    )
}

fn is_eligible_handoff_target(info: WindowCandidateInfo, our_pid: u32) -> bool {
    info.hwnd != 0
        && info.pid != 0
        && info.pid != our_pid
        && info.visible
        && !info.cloaked
        && !info.minimized
        && !info.tool_window
        && is_safe_handoff_elevation(info.elevation)
}

#[cfg(test)]
fn select_next_handoff_target(
    windows_in_z_order: &[WindowCandidateInfo],
    our_pid: u32,
) -> Option<isize> {
    windows_in_z_order
        .iter()
        .copied()
        .find(|w| is_eligible_handoff_target(*w, our_pid))
        .map(|w| w.hwnd)
}

#[cfg(test)]
fn select_preferred_handoff_targets(
    shell: Option<WindowCandidateInfo>,
    windows_in_z_order: &[WindowCandidateInfo],
    our_pid: u32,
    fg_hwnd: isize,
) -> Vec<(HandoffMethod, isize)> {
    let mut targets = Vec::with_capacity(2);
    if let Some(sh) = shell {
        if sh.hwnd != 0
            && sh.hwnd != fg_hwnd
            && sh.pid != 0
            && sh.pid != our_pid
            && is_safe_handoff_elevation(sh.elevation)
        {
            targets.push((HandoffMethod::Shell, sh.hwnd));
        }
    }
    if let Some(next_hwnd) = select_next_handoff_target(windows_in_z_order, our_pid) {
        if next_hwnd != fg_hwnd && !targets.iter().any(|&(_, h)| h == next_hwnd) {
            targets.push((HandoffMethod::Next, next_hwnd));
        }
    }
    targets
}

fn should_block_leap_for_foreground(
    our_elevation: ProcessElevation,
    fg_elevation: ProcessElevation,
) -> bool {
    let (our_elevated, our_rid) = match our_elevation {
        ProcessElevation::Elevated => (true, None),
        ProcessElevation::Standard { integrity_rid } => (false, Some(integrity_rid)),
        ProcessElevation::Unknown => (false, None),
    };
    if our_elevated {
        return false;
    }
    match fg_elevation {
        ProcessElevation::Standard { integrity_rid } => match our_rid {
            Some(ours) => integrity_rid > ours,
            None => integrity_rid > SECURITY_MANDATORY_MEDIUM_RID,
        },
        ProcessElevation::Elevated | ProcessElevation::Unknown => true,
    }
}

#[cfg(any(windows, test))]
fn evaluate_capture_watchdog(
    is_trapped: bool,
    last_hook_tick: Option<u32>,
    last_input_tick: u32,
    threshold_ms: u32,
) -> Option<u32> {
    if !is_trapped {
        return None;
    }
    let hook_tick = last_hook_tick?;
    let delta = last_input_tick.wrapping_sub(hook_tick) as i32;
    if delta > threshold_ms as i32 {
        Some(delta as u32)
    } else {
        None
    }
}

#[cfg(windows)]
unsafe fn query_token_elevation(token: HANDLE) -> ProcessElevation {
    let mut te = TOKEN_ELEVATION::default();
    let mut ret_len = 0u32;
    if GetTokenInformation(
        token,
        TokenElevation,
        Some(&mut te as *mut _ as *mut std::ffi::c_void),
        std::mem::size_of::<TOKEN_ELEVATION>() as u32,
        &mut ret_len,
    )
    .is_err()
    {
        return ProcessElevation::Unknown;
    }
    if te.TokenIsElevated != 0 {
        return ProcessElevation::Elevated;
    }

    let mut needed = 0u32;
    let _ = GetTokenInformation(token, TokenIntegrityLevel, None, 0, &mut needed);
    if needed < std::mem::size_of::<TOKEN_MANDATORY_LABEL>() as u32 {
        return ProcessElevation::Unknown;
    }
    let mut buf = vec![0u8; needed as usize];
    if GetTokenInformation(
        token,
        TokenIntegrityLevel,
        Some(buf.as_mut_ptr() as *mut std::ffi::c_void),
        needed,
        &mut needed,
    )
    .is_err()
    {
        return ProcessElevation::Unknown;
    }

    let tml = &*(buf.as_ptr() as *const TOKEN_MANDATORY_LABEL);
    let sid = tml.Label.Sid;
    if sid.is_invalid() {
        return ProcessElevation::Unknown;
    }
    let count_ptr = GetSidSubAuthorityCount(sid);
    if count_ptr.is_null() {
        return ProcessElevation::Unknown;
    }
    let count = *count_ptr as u32;
    if count == 0 {
        return ProcessElevation::Unknown;
    }
    let rid_ptr = GetSidSubAuthority(sid, count - 1);
    if rid_ptr.is_null() {
        return ProcessElevation::Unknown;
    }
    let rid = *rid_ptr;
    if rid >= SECURITY_MANDATORY_HIGH_RID {
        ProcessElevation::Elevated
    } else {
        ProcessElevation::Standard { integrity_rid: rid }
    }
}

#[cfg(windows)]
unsafe fn query_own_elevation() -> ProcessElevation {
    let mut token = HANDLE::default();
    if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
        return ProcessElevation::Unknown;
    }
    let elev = query_token_elevation(token);
    let _ = CloseHandle(token);
    elev
}

#[cfg(windows)]
unsafe fn query_process_info(pid: u32) -> (ProcessElevation, String) {
    if pid == 0 {
        return (ProcessElevation::Unknown, "unknown".to_string());
    }
    let Ok(proc_handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return (ProcessElevation::Unknown, format!("pid:{pid}"));
    };

    let mut path_buf = [0u16; 512];
    let mut path_len = path_buf.len() as u32;
    let proc_name = if QueryFullProcessImageNameW(
        proc_handle,
        PROCESS_NAME_FORMAT(0),
        windows::core::PWSTR(path_buf.as_mut_ptr()),
        &mut path_len,
    )
    .is_ok()
        && path_len > 0
    {
        let full = String::from_utf16_lossy(&path_buf[..path_len as usize]);
        std::path::Path::new(&full)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(&full)
            .to_string()
    } else {
        format!("pid:{pid}")
    };

    let mut token = HANDLE::default();
    if OpenProcessToken(proc_handle, TOKEN_QUERY, &mut token).is_err() {
        let _ = CloseHandle(proc_handle);
        return (ProcessElevation::Unknown, proc_name);
    }

    let elev = query_token_elevation(token);
    let _ = CloseHandle(token);
    let _ = CloseHandle(proc_handle);
    (elev, proc_name)
}

#[cfg(windows)]
unsafe fn is_window_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    let res = DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut std::ffi::c_void,
        std::mem::size_of::<u32>() as u32,
    );
    res.is_ok() && cloaked != 0
}

#[cfg(windows)]
unsafe fn is_window_safe_foreground(hwnd: HWND, our_pid: u32) -> bool {
    if hwnd.0.is_null() {
        return true;
    }
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 || pid == our_pid {
        return false;
    }
    let (elev, _) = query_process_info(pid);
    is_safe_handoff_elevation(elev)
}

#[cfg(windows)]
unsafe fn find_next_handoff_windows(fg: HWND, our_pid: u32, limit: usize) -> Vec<HWND> {
    let mut results = Vec::new();
    let mut cur = fg;
    for _ in 0..1024 {
        let Ok(next) = GetWindow(cur, GW_HWNDNEXT) else {
            break;
        };
        if next.0.is_null() {
            break;
        }
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(next, Some(&mut pid));
        let visible = IsWindowVisible(next).as_bool();
        let minimized = IsIconic(next).as_bool();
        let ex_style = GetWindowLongW(next, GWL_EXSTYLE) as u32;
        let tool_window = (ex_style & WS_EX_TOOLWINDOW.0) != 0;

        // Check cheap window attributes before DWM / OpenProcess token queries.
        if next.0 as isize != 0
            && pid != 0
            && pid != our_pid
            && visible
            && !minimized
            && !tool_window
        {
            let cloaked = is_window_cloaked(next);
            let elevation = if cloaked {
                ProcessElevation::Unknown
            } else {
                query_process_info(pid).0
            };
            let info = WindowCandidateInfo {
                hwnd: next.0 as isize,
                pid,
                visible,
                cloaked,
                minimized,
                tool_window,
                elevation,
            };
            if is_eligible_handoff_target(info, our_pid) {
                results.push(next);
                if results.len() >= limit {
                    break;
                }
            }
        }
        cur = next;
    }
    results
}

#[cfg(windows)]
unsafe fn query_last_input_tick() -> Option<u32> {
    let mut info = LASTINPUTINFO {
        cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
        dwTime: 0,
    };
    if GetLastInputInfo(&mut info).as_bool() {
        Some(info.dwTime)
    } else {
        None
    }
}

#[cfg(windows)]
fn emit_leap_blocked_notice(proc_name: &str) {
    let now = now_millis();
    let prev = LAST_BLOCKED_NOTICE_MS.load(Ordering::Relaxed);
    if now.saturating_sub(prev) < BLOCKED_NOTICE_COOLDOWN_MS {
        return;
    }
    LAST_BLOCKED_NOTICE_MS.store(now, Ordering::Relaxed);
    if let Ok(guard) = leap_blocked_callback_slot().try_lock() {
        if let Some(ref cb) = *guard {
            cb(proc_name);
        }
    }
}

#[cfg(windows)]
unsafe fn block_leap_for_elevated_foreground(fg: HWND, proc_name: &str, log_enabled: bool) {
    disarm_trapped_in_callback();
    KEY_CTRL_DOWN.store(false, Ordering::Relaxed);
    KEY_ALT_DOWN.store(false, Ordering::Relaxed);
    if log_enabled {
        debug_log::record_event(DebugEvent::Note {
            ts_us: debug_log::now_us(),
            hwnd: fg.0 as isize,
            text: format!("LEAP_BLOCKED reason=elevated_foreground proc={proc_name}"),
        });
    }
    emit_leap_blocked_notice(proc_name);
}

#[cfg(windows)]
unsafe fn check_foreground_blocks_leap(our_pid: u32) -> Option<(HWND, String)> {
    let fg = GetForegroundWindow();
    if fg.0.is_null() {
        return None;
    }
    let mut fg_pid = 0u32;
    let _ = GetWindowThreadProcessId(fg, Some(&mut fg_pid));
    if fg_pid == our_pid {
        return None;
    }
    let our_elev = query_own_elevation();
    let (fg_elev, proc_name) = query_process_info(fg_pid);
    if should_block_leap_for_foreground(our_elev, fg_elev) {
        Some((fg, proc_name))
    } else {
        None
    }
}

#[cfg(windows)]
unsafe fn perform_watchdog_release(gap_ms: u32) {
    if debug_log::is_enabled() {
        debug_log::record_event(DebugEvent::Note {
            ts_us: debug_log::now_us(),
            hwnd: debug_log::foreground_hwnd_raw(),
            text: format!("WATCHDOG_RELEASE gap_ms={gap_ms}"),
        });
        debug_log::record_event(DebugEvent::Release {
            ts_us: debug_log::now_us(),
            reason: "watchdog_release",
        });
    }
    disarm_trapped_in_callback();
    KEY_CTRL_DOWN.store(false, Ordering::Relaxed);
    KEY_ALT_DOWN.store(false, Ordering::Relaxed);
    if let Ok(mut tracker) = controller_tracker().try_lock() {
        let _ = tracker.release_all();
    }
    restore_cursor_visibility();
    try_send_packet(KvmPacket::EmergencyRelease);
    try_send_packet(KvmPacket::ReleaseAll);
}

#[cfg(windows)]
unsafe fn handoff_own_foreground_focus(log_enabled: bool) {
    let fg = GetForegroundWindow();
    if fg.0.is_null() {
        return;
    }
    let our_pid = GetCurrentProcessId();
    let mut fg_pid = 0u32;
    let _ = GetWindowThreadProcessId(fg, Some(&mut fg_pid));
    if fg_pid != our_pid {
        return;
    }

    let mut attempts: u32 = 0;
    let mut final_method = HandoffMethod::Minimize;
    let mut final_to = fg;
    let mut ok = false;

    // Prefer GetShellWindow() (desktop) first without AttachThreadInput so we
    // never risk selecting an elevated Z-order neighbour like Taskmgr.exe.
    let shell = GetShellWindow();
    if !shell.0.is_null() && shell != fg {
        let mut shell_pid = 0u32;
        let _ = GetWindowThreadProcessId(shell, Some(&mut shell_pid));
        if shell_pid != 0
            && shell_pid != our_pid
            && is_safe_handoff_elevation(query_process_info(shell_pid).0)
        {
            attempts += 1;
            let _ = SetForegroundWindow(shell);
            let after = GetForegroundWindow();
            if after != fg && is_window_safe_foreground(after, our_pid) {
                final_method = HandoffMethod::Shell;
                final_to = after;
                ok = true;
            }
        }
    }

    // Only if GetShellWindow() failed, walk Z-order for a visible, non-cloaked,
    // non-minimised, non-elevated top-level window that is not ours.
    if !ok {
        for next in find_next_handoff_windows(fg, our_pid, 8) {
            attempts += 1;
            let _ = SetForegroundWindow(next);
            let _ = BringWindowToTop(next);
            let after = GetForegroundWindow();
            if after != fg && is_window_safe_foreground(after, our_pid) {
                final_method = HandoffMethod::Next;
                final_to = after;
                ok = true;
                break;
            }
        }
    }

    if !ok {
        attempts += 1;
        let _ = ShowWindow(fg, SW_MINIMIZE);
        let after = GetForegroundWindow();
        final_method = HandoffMethod::Minimize;
        final_to = after;
        ok = after != fg;
    }

    if log_enabled {
        debug_log::record_event(DebugEvent::Note {
            ts_us: debug_log::now_us(),
            hwnd: final_to.0 as isize,
            text: format!(
                "FOCUS_HANDOFF from=0x{:X} to=0x{:X} method={} attempts={} ok={ok}",
                fg.0 as isize,
                final_to.0 as isize,
                final_method.as_str(),
                attempts,
            ),
        });
    }
}

#[cfg(windows)]
unsafe fn perform_engage_leap() -> bool {
    if !should_perform_engage(
        HOOK_ACTIVE.load(Ordering::Relaxed),
        IS_TRAPPED.load(Ordering::Acquire),
        ENGAGE_PENDING.swap(false, Ordering::AcqRel),
    ) {
        return false;
    }

    let log_enabled = debug_log::is_enabled();
    let our_pid = GetCurrentProcessId();

    if let Some((fg, proc_name)) = check_foreground_blocks_leap(our_pid) {
        block_leap_for_elevated_foreground(fg, &proc_name, log_enabled);
        return false;
    }

    handoff_own_foreground_focus(log_enabled);

    // Guard again in case minimizing our own window promoted an elevated window.
    if let Some((fg, proc_name)) = check_foreground_blocks_leap(our_pid) {
        block_leap_for_elevated_foreground(fg, &proc_name, log_enabled);
        return false;
    }

    let mx = TRAP_MON_X.load(Ordering::Relaxed);
    let my = TRAP_MON_Y.load(Ordering::Relaxed);
    let mw = TRAP_MON_W.load(Ordering::Relaxed);
    let mh = TRAP_MON_H.load(Ordering::Relaxed);
    let center_x = CLAMP_CENTER_X.load(Ordering::Relaxed);
    let center_y = CLAMP_CENTER_Y.load(Ordering::Relaxed);
    let relative_y = f32::from_bits(PENDING_ENGAGE_REL_Y.load(Ordering::Relaxed));
    let fg_hwnd = if log_enabled {
        debug_log::foreground_hwnd_raw()
    } else {
        0
    };

    let clip_rect = RECT {
        left: mx,
        top: my,
        right: mx + mw,
        bottom: my + mh,
    };
    let _ = ClipCursor(Some(&clip_rect));
    let _ = SetCursorPos(center_x, center_y);
    CURSOR_CENTERED.store(true, Ordering::Release);

    for _ in 0..16 {
        if ShowCursor(false) < 0 {
            break;
        }
    }

    let (tracker_before, released, send_input_us) =
        if let Ok(mut tracker) = controller_tracker().try_lock() {
            let before = if log_enabled {
                KeySnapshot::from_slice(tracker.held_keys())
            } else {
                KeySnapshot::lock_failed()
            };
            let keys = tracker.release_keys();
            let rel_snap = if log_enabled {
                KeySnapshot::from_slice(&keys)
            } else {
                KeySnapshot::lock_failed()
            };
            let t0 = log_enabled.then(std::time::Instant::now);
            for k in &keys {
                let _ = super::injector::inject_key(k.key_code, k.scan_code, false, k.extended);
            }
            let elapsed_us = t0.map(|t| t.elapsed().as_micros() as u64).unwrap_or(0);
            (before, rel_snap, elapsed_us)
        } else {
            (
                KeySnapshot::lock_failed(),
                KeySnapshot::lock_failed(),
                0,
            )
        };

    record_hook_tick();

    if log_enabled {
        debug_log::record_event(DebugEvent::EngagePerform {
            ts_us: debug_log::now_us(),
            edge: Edge::from_u8(LINK_FROM_EDGE.load(Ordering::Relaxed)),
            relative_y,
            hwnd: fg_hwnd,
            tracker_before,
            released,
            send_input_us,
        });
    }

    let to_edge = Edge::from_u8(LINK_TO_EDGE.load(Ordering::Relaxed));
    let dwell_ms = RESISTANCE_MS.load(Ordering::Relaxed) as u32;
    let deadzone_px = DEADZONE_PX.load(Ordering::Relaxed);
    try_send_packet(KvmPacket::CursorEnter {
        edge: to_edge.as_str().to_string(),
        relative_y,
        dwell_ms,
        deadzone_px,
    });
    true
}

#[cfg(windows)]
unsafe extern "system" fn low_level_keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    let started = record_hook_entry(HookKind::Keyboard);
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_keyboard_event(n_code, w_param, l_param)
    }));
    record_if_slow_hook(HookKind::Keyboard, started);

    match outcome {
        Ok(Some(res)) => res,
        Ok(None) => CallNextHookEx(None, n_code, w_param, l_param),
        Err(_) => {
            disarm_trapped_in_callback();
            debug_log::record_event(DebugEvent::Release {
                ts_us: debug_log::now_us(),
                reason: "kbd_hook_panic",
            });
            post_hook_thread_message(WM_KVM_RESTORE_CURSOR);
            CallNextHookEx(None, n_code, w_param, l_param)
        }
    }
}

#[cfg(windows)]
unsafe fn handle_keyboard_event(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> Option<LRESULT> {
    if n_code < 0 {
        return None;
    }

    let kbd = *(l_param.0 as *const KBDLLHOOKSTRUCT);
    let vk_code = kbd.vkCode as u16;
    let scan_code = kbd.scanCode as u16;
    let is_down = w_param.0 as u32 == WM_KEYDOWN || w_param.0 as u32 == WM_SYSKEYDOWN;
    let extended = (kbd.flags.0 & LLKHF_EXTENDED.0) != 0;
    let injected = (kbd.flags.0 & (LLKHF_INJECTED.0 | 0x02)) != 0;
    let hook_active = HOOK_ACTIVE.load(Ordering::Relaxed);
    let trapped = IS_TRAPPED.load(Ordering::Acquire);
    let log_enabled = debug_log::is_enabled();
    let ts_us = if log_enabled { debug_log::now_us() } else { 0 };
    let hwnd = if log_enabled { debug_log::foreground_hwnd_raw() } else { 0 };

    if !hook_active {
        if log_enabled {
            debug_log::record_event(DebugEvent::Key {
                ts_us,
                vk: vk_code,
                scan_code,
                is_down,
                extended,
                injected,
                is_trapped: trapped,
                decision: KeyDecision::Pass,
                hwnd,
            });
        }
        return None;
    }

    if injected {
        if log_enabled {
            debug_log::record_event(DebugEvent::Key {
                ts_us,
                vk: vk_code,
                scan_code,
                is_down,
                extended,
                injected: true,
                is_trapped: trapped,
                decision: KeyDecision::Pass,
                hwnd,
            });
        }
        return None;
    }

    if vk_code == 0x11 || vk_code == 0xA2 || vk_code == 0xA3 {
        KEY_CTRL_DOWN.store(is_down, Ordering::Relaxed);
    } else if vk_code == 0x12 || vk_code == 0xA4 || vk_code == 0xA5 {
        KEY_ALT_DOWN.store(is_down, Ordering::Relaxed);
    }

    let is_scroll_lock = vk_code == 0x91;
    let is_pause = vk_code == 0x13;
    let is_ctrl_alt_esc = vk_code == 0x1B
        && KEY_CTRL_DOWN.load(Ordering::Relaxed)
        && KEY_ALT_DOWN.load(Ordering::Relaxed);

    if is_scroll_lock || is_pause || is_ctrl_alt_esc {
        disarm_trapped_in_callback();
        KEY_CTRL_DOWN.store(false, Ordering::Relaxed);
        KEY_ALT_DOWN.store(false, Ordering::Relaxed);
        if let Ok(mut tracker) = controller_tracker().try_lock() {
            let _ = tracker.release_all();
        }
        if log_enabled {
            debug_log::record_event(DebugEvent::Key {
                ts_us,
                vk: vk_code,
                scan_code,
                is_down,
                extended,
                injected: false,
                is_trapped: trapped,
                decision: KeyDecision::Block,
                hwnd,
            });
            debug_log::record_event(DebugEvent::Release {
                ts_us,
                reason: "emergency_hotkey",
            });
        }
        post_hook_thread_message(WM_KVM_RESTORE_CURSOR);

        try_send_packet(KvmPacket::EmergencyRelease);
        try_send_packet(KvmPacket::ReleaseAll);
        return Some(LRESULT(0));
    }

    if trapped {
        if log_enabled {
            debug_log::record_event(DebugEvent::Key {
                ts_us,
                vk: vk_code,
                scan_code,
                is_down,
                extended,
                injected: false,
                is_trapped: true,
                decision: KeyDecision::Forward,
                hwnd,
            });
        }
        try_send_packet(KvmPacket::KeyEvent {
            key_code: vk_code,
            scan_code,
            is_down,
            extended,
        });
        return Some(LRESULT(1));
    }

    if log_enabled {
        debug_log::record_event(DebugEvent::Key {
            ts_us,
            vk: vk_code,
            scan_code,
            is_down,
            extended,
            injected: false,
            is_trapped: false,
            decision: KeyDecision::Pass,
            hwnd,
        });
    }

    if let Ok(mut tracker) = controller_tracker().try_lock() {
        tracker.record_key(vk_code, scan_code, is_down, extended, false);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_trap_center_handles_origin_and_offset_monitors() {
        assert_eq!(compute_trap_center(0, 0, 1920, 1080), (960, 540));
        assert_eq!(compute_trap_center(-1920, 100, 1920, 1080), (-960, 640));
    }

    #[test]
    fn trapped_mouse_delta_suppresses_until_centered_and_skips_zero() {
        assert_eq!(trapped_mouse_delta(false, (1920, 540), (960, 540)), None);
        assert_eq!(trapped_mouse_delta(true, (960, 540), (960, 540)), None);
        assert_eq!(
            trapped_mouse_delta(true, (975, 532), (960, 540)),
            Some((15, -8))
        );
    }

    #[test]
    fn should_perform_engage_requires_active_trapped_and_pending() {
        assert!(should_perform_engage(true, true, true));
        assert!(!should_perform_engage(false, true, true));
        assert!(!should_perform_engage(true, false, true));
        assert!(!should_perform_engage(true, true, false));
    }

    fn candidate(hwnd: isize, pid: u32, elevation: ProcessElevation) -> WindowCandidateInfo {
        WindowCandidateInfo {
            hwnd,
            pid,
            visible: true,
            cloaked: false,
            minimized: false,
            tool_window: false,
            elevation,
        }
    }

    #[test]
    fn select_next_handoff_target_skips_elevated_unknown_cloaked_and_ineligible_windows() {
        let our_pid = 1000;
        let standard = ProcessElevation::Standard {
            integrity_rid: SECURITY_MANDATORY_MEDIUM_RID,
        };
        let list = [
            candidate(0x10, our_pid, standard),
            WindowCandidateInfo {
                visible: false,
                ..candidate(0x20, 2000, standard)
            },
            WindowCandidateInfo {
                cloaked: true,
                ..candidate(0x25, 2001, standard)
            },
            WindowCandidateInfo {
                minimized: true,
                ..candidate(0x30, 2002, standard)
            },
            WindowCandidateInfo {
                tool_window: true,
                ..candidate(0x40, 2003, standard)
            },
            candidate(0x45, 2004, ProcessElevation::Elevated),
            candidate(0x46, 2005, ProcessElevation::Unknown),
            candidate(
                0x47,
                2006,
                ProcessElevation::Standard {
                    integrity_rid: 0x2100, // Medium Plus (UIAccess)
                },
            ),
            candidate(0x50, 3000, standard),
            candidate(0x60, 4000, standard),
        ];
        assert_eq!(select_next_handoff_target(&list, our_pid), Some(0x50));
    }

    #[test]
    fn select_next_handoff_target_returns_none_when_only_elevated_or_ineligible_exist() {
        let our_pid = 1000;
        let standard = ProcessElevation::Standard {
            integrity_rid: SECURITY_MANDATORY_MEDIUM_RID,
        };
        let list = [
            candidate(0, 2000, standard),
            candidate(0x10, our_pid, standard),
            candidate(0x20, 3000, ProcessElevation::Elevated),
            candidate(0x30, 4000, ProcessElevation::Unknown),
        ];
        assert_eq!(select_next_handoff_target(&list, our_pid), None);
    }

    #[test]
    fn select_preferred_handoff_targets_prefers_shell_first_and_skips_unsafe_shell() {
        let our_pid = 1000;
        let fg_hwnd = 0x05;
        let standard = ProcessElevation::Standard {
            integrity_rid: SECURITY_MANDATORY_MEDIUM_RID,
        };
        let shell = candidate(0x20, 1500, standard);
        let z_order = [
            candidate(0x10, 2000, ProcessElevation::Elevated),
            candidate(0x30, 3000, standard),
        ];
        assert_eq!(
            select_preferred_handoff_targets(Some(shell), &z_order, our_pid, fg_hwnd),
            vec![(HandoffMethod::Shell, 0x20), (HandoffMethod::Next, 0x30)]
        );

        let elevated_shell = candidate(0x20, 1500, ProcessElevation::Elevated);
        assert_eq!(
            select_preferred_handoff_targets(Some(elevated_shell), &z_order, our_pid, fg_hwnd),
            vec![(HandoffMethod::Next, 0x30)]
        );
    }

    #[test]
    fn pre_leap_guard_blocks_elevated_and_unknown_when_not_elevated() {
        let med = ProcessElevation::Standard {
            integrity_rid: SECURITY_MANDATORY_MEDIUM_RID,
        };
        let low = ProcessElevation::Standard {
            integrity_rid: 0x1000,
        };
        let med_plus = ProcessElevation::Standard {
            integrity_rid: 0x2100,
        };

        assert!(!should_block_leap_for_foreground(med, med));
        assert!(!should_block_leap_for_foreground(med, low));
        assert!(should_block_leap_for_foreground(med, med_plus));
        assert!(should_block_leap_for_foreground(
            med,
            ProcessElevation::Elevated
        ));
        assert!(should_block_leap_for_foreground(
            med,
            ProcessElevation::Unknown
        ));

        // If AnyLeap itself is elevated, admin foreground windows do not blind hooks.
        assert!(!should_block_leap_for_foreground(
            ProcessElevation::Elevated,
            ProcessElevation::Elevated
        ));
        assert!(!should_block_leap_for_foreground(
            ProcessElevation::Elevated,
            ProcessElevation::Unknown
        ));
    }

    #[test]
    fn capture_watchdog_ignores_idle_and_detects_blinded_input() {
        let th = WATCHDOG_BLIND_THRESHOLD_MS;
        assert_eq!(evaluate_capture_watchdog(false, Some(1000), 5000, th), None);
        assert_eq!(evaluate_capture_watchdog(true, None, 5000, th), None);

        // Normal idle: GetLastInputInfo().dwTime remains at last hook timestamp.
        assert_eq!(evaluate_capture_watchdog(true, Some(10_000), 10_000, th), None);
        // Hook tick recorded right after GetLastInputInfo() updated.
        assert_eq!(evaluate_capture_watchdog(true, Some(10_005), 10_000, th), None);
        // Boundary: exactly 1000 ms is not > 1000 ms.
        assert_eq!(evaluate_capture_watchdog(true, Some(10_000), 11_000, th), None);

        // System saw input > 1000 ms after last hook callback -> trigger release.
        assert_eq!(
            evaluate_capture_watchdog(true, Some(10_000), 11_001, th),
            Some(1001)
        );
        assert_eq!(
            evaluate_capture_watchdog(true, Some(10_000), 12_350, th),
            Some(2350)
        );

        // 32-bit GetTickCount wraparound safety.
        assert_eq!(
            evaluate_capture_watchdog(true, Some(u32::MAX - 400), 700, th),
            Some(1101)
        );
        assert_eq!(
            evaluate_capture_watchdog(true, Some(10), u32::MAX - 5, th),
            None
        );
    }
}
