use super::input_logic::{
    controller_return_pos, detect_controller_edge, Edge, PressedInputTracker,
};
use super::monitors::{get_local_monitors, MonitorDisplay};
use super::protocol::{KvmPacket, TopologyConfig};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::mpsc;

#[cfg(windows)]
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, RECT, WPARAM};
#[cfg(windows)]
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONULL};
#[cfg(windows)]
use windows::Win32::System::Threading::GetCurrentThreadId;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::*;

#[cfg(windows)]
const WM_KVM_RESTORE_CURSOR: u32 = WM_APP + 1;
#[cfg(windows)]
const WM_KVM_ENGAGE: u32 = WM_APP + 2;

static PACKET_SENDER: OnceLock<Mutex<Option<mpsc::Sender<KvmPacket>>>> = OnceLock::new();
static MOUSE_DELTA_SENDER: OnceLock<Mutex<Option<mpsc::Sender<(i32, i32)>>>> = OnceLock::new();
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

static MOUSE_BUTTONS_DOWN: AtomicU8 = AtomicU8::new(0);
static KEY_CTRL_DOWN: AtomicBool = AtomicBool::new(false);
static KEY_ALT_DOWN: AtomicBool = AtomicBool::new(false);
static EDGE_DWELL_START: AtomicU64 = AtomicU64::new(0);

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
        if !IS_TRAPPED.swap(false, Ordering::SeqCst) {
            return;
        }
        CURSOR_CENTERED.store(false, Ordering::Release);
        ENGAGE_PENDING.store(false, Ordering::Release);
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        notify_hook_thread_restore();

        let mon = MonitorDisplay {
            id: String::new(),
            x: TRAP_MON_X.load(Ordering::Relaxed),
            y: TRAP_MON_Y.load(Ordering::Relaxed),
            width: TRAP_MON_W.load(Ordering::Relaxed),
            height: TRAP_MON_H.load(Ordering::Relaxed),
            is_primary: true,
        };
        let link_edge = Edge::from_u8(LINK_FROM_EDGE.load(Ordering::Relaxed));
        let (rx, ry) = controller_return_pos(&mon, link_edge, relative_y);
        #[cfg(windows)]
        unsafe {
            let _ = SetCursorPos(rx, ry);
        }
        #[cfg(not(windows))]
        let _ = (rx, ry);
    }

    pub fn emergency_release(&self) {
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
    ) {
        let monitors = get_local_monitors();
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
            LINK_FROM_EDGE.store(from_edge as u8, Ordering::Relaxed);
            LINK_TO_EDGE.store(to_edge as u8, Ordering::Relaxed);
            DEADZONE_PX.store(link.deadzone_px.unwrap_or(48), Ordering::Relaxed);
            RESISTANCE_MS.store(link.resistance_ms.unwrap_or(120) as u64, Ordering::Relaxed);
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

        if HOOK_ACTIVE.swap(true, Ordering::SeqCst) {
            return;
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

            while HOOK_ACTIVE.load(Ordering::Relaxed) {
                let res = GetMessageW(&mut msg, None, 0, 0);
                if res.0 <= 0 {
                    break;
                }
                if msg.message == WM_KVM_RESTORE_CURSOR {
                    restore_cursor_visibility();
                    continue;
                }
                if msg.message == WM_KVM_ENGAGE {
                    perform_engage_leap();
                    continue;
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
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
unsafe extern "system" fn low_level_mouse_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_mouse_event(n_code, w_param, l_param)
    }));

    match outcome {
        Ok(Some(res)) => res,
        Ok(None) => CallNextHookEx(None, n_code, w_param, l_param),
        Err(_) => {
            disarm_trapped_in_callback();
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
        detect_controller_edge((pt.x, pt.y), &mons, link_edge, deadzone).map(|(idx, rel)| {
            let m = &mons[idx];
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

    post_hook_thread_message(WM_KVM_ENGAGE);
}

#[cfg(windows)]
unsafe fn perform_engage_leap() {
    if !should_perform_engage(
        HOOK_ACTIVE.load(Ordering::Relaxed),
        IS_TRAPPED.load(Ordering::Acquire),
        ENGAGE_PENDING.swap(false, Ordering::AcqRel),
    ) {
        return;
    }

    let mx = TRAP_MON_X.load(Ordering::Relaxed);
    let my = TRAP_MON_Y.load(Ordering::Relaxed);
    let mw = TRAP_MON_W.load(Ordering::Relaxed);
    let mh = TRAP_MON_H.load(Ordering::Relaxed);
    let center_x = CLAMP_CENTER_X.load(Ordering::Relaxed);
    let center_y = CLAMP_CENTER_Y.load(Ordering::Relaxed);
    let relative_y = f32::from_bits(PENDING_ENGAGE_REL_Y.load(Ordering::Relaxed));

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

    if let Ok(mut tracker) = controller_tracker().try_lock() {
        for k in tracker.release_keys() {
            super::injector::inject_key(k.key_code, k.scan_code, false, k.extended);
        }
    }

    let to_edge = Edge::from_u8(LINK_TO_EDGE.load(Ordering::Relaxed));
    try_send_packet(KvmPacket::CursorEnter {
        edge: to_edge.as_str().to_string(),
        relative_y,
    });
}

#[cfg(windows)]
unsafe extern "system" fn low_level_keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        handle_keyboard_event(n_code, w_param, l_param)
    }));

    match outcome {
        Ok(Some(res)) => res,
        Ok(None) => CallNextHookEx(None, n_code, w_param, l_param),
        Err(_) => {
            disarm_trapped_in_callback();
            post_hook_thread_message(WM_KVM_RESTORE_CURSOR);
            CallNextHookEx(None, n_code, w_param, l_param)
        }
    }
}

#[cfg(windows)]
unsafe fn handle_keyboard_event(n_code: i32, w_param: WPARAM, l_param: LPARAM) -> Option<LRESULT> {
    if n_code < 0 || !HOOK_ACTIVE.load(Ordering::Relaxed) {
        return None;
    }

    let kbd = *(l_param.0 as *const KBDLLHOOKSTRUCT);
    if (kbd.flags.0 & LLKHF_INJECTED.0) != 0 {
        return None;
    }

    let vk_code = kbd.vkCode as u16;
    let scan_code = kbd.scanCode as u16;
    let is_down = w_param.0 as u32 == WM_KEYDOWN || w_param.0 as u32 == WM_SYSKEYDOWN;
    let extended = (kbd.flags.0 & LLKHF_EXTENDED.0) != 0;

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
        post_hook_thread_message(WM_KVM_RESTORE_CURSOR);

        try_send_packet(KvmPacket::EmergencyRelease);
        try_send_packet(KvmPacket::ReleaseAll);
        return Some(LRESULT(0));
    }

    if IS_TRAPPED.load(Ordering::Acquire) {
        try_send_packet(KvmPacket::KeyEvent {
            key_code: vk_code,
            scan_code,
            is_down,
            extended,
        });
        return Some(LRESULT(1));
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
}
