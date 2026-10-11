use super::protocol::{KvmPacket, TopologyConfig, TopologyLink};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::mpsc;

#[cfg(windows)]
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, RECT, WPARAM};
#[cfg(windows)]
use windows::Win32::System::Threading::GetCurrentThreadId;
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::*;

static GLOBAL_TOPOLOGY: OnceLock<Mutex<Option<TopologyConfig>>> = OnceLock::new();
static PACKET_SENDER: OnceLock<Mutex<Option<mpsc::Sender<KvmPacket>>>> = OnceLock::new();
static MOUSE_DELTA_SENDER: OnceLock<Mutex<Option<mpsc::Sender<(i32, i32)>>>> = OnceLock::new();
static CLAMP_CENTER: OnceLock<Mutex<POINT>> = OnceLock::new();
static LEAP_EDGE: OnceLock<Mutex<String>> = OnceLock::new();

static VIRTUAL_REMOTE_X: AtomicI32 = AtomicI32::new(0);
static VIRTUAL_REMOTE_Y: AtomicI32 = AtomicI32::new(0);
static RECENTERING: AtomicBool = AtomicBool::new(false);
static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);
static IS_TRAPPED: AtomicBool = AtomicBool::new(false);
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);

// Multi-monitor virtual desktop bounds across all local displays
static DESKTOP_MIN_X: AtomicI32 = AtomicI32::new(0);
static DESKTOP_MAX_X: AtomicI32 = AtomicI32::new(1920);
static DESKTOP_MIN_Y: AtomicI32 = AtomicI32::new(0);
static DESKTOP_MAX_Y: AtomicI32 = AtomicI32::new(1080);

static PEER_WIDTH: AtomicI32 = AtomicI32::new(1920);
static PEER_HEIGHT: AtomicI32 = AtomicI32::new(1080);

// Key combo states for custom emergency unhook
static KEY_CTRL_DOWN: AtomicBool = AtomicBool::new(false);
static KEY_ALT_DOWN: AtomicBool = AtomicBool::new(false);

static EDGE_DWELL_START: AtomicU64 = AtomicU64::new(0);

fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn global_topology() -> &'static Mutex<Option<TopologyConfig>> {
    GLOBAL_TOPOLOGY.get_or_init(|| Mutex::new(None))
}

fn packet_sender() -> &'static Mutex<Option<mpsc::Sender<KvmPacket>>> {
    PACKET_SENDER.get_or_init(|| Mutex::new(None))
}

fn mouse_delta_sender() -> &'static Mutex<Option<mpsc::Sender<(i32, i32)>>> {
    MOUSE_DELTA_SENDER.get_or_init(|| Mutex::new(None))
}

fn clamp_center() -> &'static Mutex<POINT> {
    CLAMP_CENTER.get_or_init(|| Mutex::new(POINT { x: 960, y: 540 }))
}

fn leap_edge() -> &'static Mutex<String> {
    LEAP_EDGE.get_or_init(|| Mutex::new("right".to_string()))
}

/// Reset cursor display counter so cursor never stays hidden
pub fn restore_cursor_visibility() {
    #[cfg(windows)]
    unsafe {
        let _ = ClipCursor(None);
        while ShowCursor(true) < 0 {}
    }
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

    pub fn is_trapped(&self) -> bool {
        IS_TRAPPED.load(Ordering::Relaxed)
    }

    pub fn set_trapped(&self, trapped: bool) {
        IS_TRAPPED.store(trapped, Ordering::SeqCst);
        if !trapped {
            restore_cursor_visibility();
        }
    }

    pub fn emergency_release(&self) {
        self.set_trapped(false);
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        KEY_CTRL_DOWN.store(false, Ordering::Relaxed);
        KEY_ALT_DOWN.store(false, Ordering::Relaxed);
        restore_cursor_visibility();

        let guard = packet_sender().lock().unwrap();
        if let Some(ref tx) = *guard {
            let _ = tx.try_send(KvmPacket::EmergencyRelease);
        }
    }

    pub fn start_hooks(
        &self,
        topology: TopologyConfig,
        packet_tx: mpsc::Sender<KvmPacket>,
        delta_tx: mpsc::Sender<(i32, i32)>,
    ) {
        // Calculate true combined bounding box of all local physical monitors
        #[cfg(windows)]
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::*;
            let min_x = GetSystemMetrics(SM_XVIRTUALSCREEN);
            let min_y = GetSystemMetrics(SM_YVIRTUALSCREEN);
            let total_w = GetSystemMetrics(SM_CXVIRTUALSCREEN);
            let total_h = GetSystemMetrics(SM_CYVIRTUALSCREEN);
            DESKTOP_MIN_X.store(min_x, Ordering::Relaxed);
            DESKTOP_MAX_X.store(min_x + total_w, Ordering::Relaxed);
            DESKTOP_MIN_Y.store(min_y, Ordering::Relaxed);
            DESKTOP_MAX_Y.store(min_y + total_h, Ordering::Relaxed);
        }

        if let Some(peer) = topology.computers.iter().find(|c| !c.is_local) {
            PEER_WIDTH.store(peer.bounds.width, Ordering::Relaxed);
            PEER_HEIGHT.store(peer.bounds.height, Ordering::Relaxed);
        }

        *global_topology().lock().unwrap() = Some(topology);
        *packet_sender().lock().unwrap() = Some(packet_tx);
        *mouse_delta_sender().lock().unwrap() = Some(delta_tx);

        if HOOK_ACTIVE.swap(true, Ordering::SeqCst) {
            return;
        }

        #[cfg(windows)]
        std::thread::spawn(move || unsafe {
            let tid = GetCurrentThreadId();
            HOOK_THREAD_ID.store(tid, Ordering::SeqCst);

            let h_mouse = SetWindowsHookExW(
                WH_MOUSE_LL,
                Some(low_level_mouse_proc),
                None,
                0,
            );
            let h_kbd = SetWindowsHookExW(
                WH_KEYBOARD_LL,
                Some(low_level_keyboard_proc),
                None,
                0,
            );

            let mut msg = MSG::default();
            while HOOK_ACTIVE.load(Ordering::Relaxed) {
                let res = GetMessageW(&mut msg, None, 0, 0);
                if res.0 <= 0 {
                    break;
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
            restore_cursor_visibility();
        });
    }

    pub fn stop_hooks(&self) {
        HOOK_ACTIVE.store(false, Ordering::SeqCst);
        self.emergency_release();

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
unsafe extern "system" fn low_level_mouse_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 && HOOK_ACTIVE.load(Ordering::Relaxed) {
        let hook_struct = *(l_param.0 as *const MSLLHOOKSTRUCT);
        let pt = hook_struct.pt;
        let trapped = IS_TRAPPED.load(Ordering::Relaxed);

        if trapped {
            if RECENTERING.load(Ordering::Relaxed) {
                return LRESULT(1);
            }

            let center = *clamp_center().lock().unwrap();
            let dx = pt.x - center.x;
            let dy = pt.y - center.y;

            if dx != 0 || dy != 0 {
                RECENTERING.store(true, Ordering::SeqCst);
                let _ = SetCursorPos(center.x, center.y);
                RECENTERING.store(false, Ordering::SeqCst);

                let desk_max_x = DESKTOP_MAX_X.load(Ordering::Relaxed);
                let desk_min_x = DESKTOP_MIN_X.load(Ordering::Relaxed);
                let remote_w = PEER_WIDTH.load(Ordering::Relaxed);
                let remote_h = PEER_HEIGHT.load(Ordering::Relaxed);
                let edge = leap_edge().lock().unwrap().clone();

                let raw_vx = VIRTUAL_REMOTE_X.load(Ordering::Relaxed) + dx;
                let raw_vy = (VIRTUAL_REMOTE_Y.load(Ordering::Relaxed) + dy).clamp(0, remote_h);
                VIRTUAL_REMOTE_Y.store(raw_vy, Ordering::Relaxed);

                // Auto-return check against virtual bounds
                if edge == "right" {
                    if raw_vx < 0 {
                        untrap_and_return(desk_max_x - 30, pt.y, "right");
                        return LRESULT(1);
                    }
                    let clamped_vx = raw_vx.min(remote_w + 20);
                    VIRTUAL_REMOTE_X.store(clamped_vx, Ordering::Relaxed);
                } else if edge == "left" {
                    if raw_vx > remote_w {
                        untrap_and_return(desk_min_x + 30, pt.y, "left");
                        return LRESULT(1);
                    }
                    let clamped_vx = raw_vx.max(-20);
                    VIRTUAL_REMOTE_X.store(clamped_vx, Ordering::Relaxed);
                }

                let guard = mouse_delta_sender().lock().unwrap();
                if let Some(ref tx) = *guard {
                    let _ = tx.try_send((dx, dy));
                }

                let cur_vx = VIRTUAL_REMOTE_X.load(Ordering::Relaxed);
                let cur_vy = VIRTUAL_REMOTE_Y.load(Ordering::Relaxed);
                let pkt_guard = packet_sender().lock().unwrap();
                if let Some(ref tx) = *pkt_guard {
                    let _ = tx.try_send(KvmPacket::CursorPosition {
                        norm_x: (cur_vx as f32 / remote_w as f32).clamp(0.0, 1.0),
                        norm_y: (cur_vy as f32 / remote_h as f32).clamp(0.0, 1.0),
                    });
                }
            }

            match w_param.0 as u32 {
                WM_LBUTTONDOWN => send_button(1, true),
                WM_LBUTTONUP => send_button(1, false),
                WM_RBUTTONDOWN => send_button(2, true),
                WM_RBUTTONUP => send_button(2, false),
                WM_MBUTTONDOWN => send_button(3, true),
                WM_MBUTTONUP => send_button(3, false),
                WM_MOUSEWHEEL => {
                    let delta = (hook_struct.mouseData >> 16) as i16 as i32;
                    let guard = packet_sender().lock().unwrap();
                    if let Some(ref tx) = *guard {
                        let _ = tx.try_send(KvmPacket::MouseWheel { delta });
                    }
                }
                _ => {}
            }

            return LRESULT(1);
        } else {
            // Check boundary hit against true multi-monitor virtual desktop bounds
            let topo_guard = global_topology().lock().unwrap();
            if let Some(ref topo) = *topo_guard {
                if let Some(primary) = topo.computers.iter().find(|c| c.is_local) {
                    let desk_max_x = DESKTOP_MAX_X.load(Ordering::Relaxed);
                    let desk_min_x = DESKTOP_MIN_X.load(Ordering::Relaxed);
                    let max_boundary_x = desk_max_x - 3;
                    let min_boundary_x = desk_min_x + 3;

                    let mut at_boundary = false;

                    for link in &topo.links {
                        if link.from_id == primary.id {
                            let deadzone = link.deadzone_px.unwrap_or(48);
                            let resistance_ms = link.resistance_ms.unwrap_or(120) as u64;
                            let offset_y = link.offset_y.unwrap_or(0);

                            let peer_node = topo.computers.iter().find(|c| c.id == link.to_id);
                            let peer_h = peer_node.map(|p| p.bounds.height).unwrap_or(1080);

                            let primary_top = DESKTOP_MIN_Y.load(Ordering::Relaxed);
                            let primary_bottom = DESKTOP_MAX_Y.load(Ordering::Relaxed);
                            let peer_top = primary_top + offset_y;
                            let peer_bottom = peer_top + peer_h;

                            let overlap_top = primary_top.max(peer_top) + deadzone;
                            let overlap_bottom = primary_bottom.min(peer_bottom) - deadzone;

                            let inside_overlap = pt.y >= overlap_top && pt.y <= overlap_bottom;

                            // Right edge of virtual desktop span (far right of 2nd monitor)
                            if link.from_edge == "right" && pt.x >= max_boundary_x {
                                at_boundary = true;
                                if inside_overlap {
                                    if check_dwell_resistance(resistance_ms) {
                                        engage_leap(link, pt, "right", desk_max_x, desk_min_x);
                                        return LRESULT(1);
                                    }
                                }
                            } else if link.from_edge == "left" && pt.x <= min_boundary_x {
                                at_boundary = true;
                                if inside_overlap {
                                    if check_dwell_resistance(resistance_ms) {
                                        engage_leap(link, pt, "left", desk_max_x, desk_min_x);
                                        return LRESULT(1);
                                    }
                                }
                            }
                        }
                    }

                    if !at_boundary {
                        EDGE_DWELL_START.store(0, Ordering::Relaxed);
                    }
                }
            }
        }
    }

    CallNextHookEx(None, n_code, w_param, l_param)
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

#[cfg(windows)]
unsafe fn engage_leap(
    link: &TopologyLink,
    pt: POINT,
    edge: &str,
    desk_max_x: i32,
    desk_min_x: i32,
) {
    IS_TRAPPED.store(true, Ordering::SeqCst);
    *leap_edge().lock().unwrap() = edge.to_string();

    let remote_w = PEER_WIDTH.load(Ordering::Relaxed);
    if edge == "right" {
        VIRTUAL_REMOTE_X.store(0, Ordering::SeqCst);
    } else {
        VIRTUAL_REMOTE_X.store(remote_w, Ordering::SeqCst);
    }
    VIRTUAL_REMOTE_Y.store(pt.y, Ordering::SeqCst);

    let center_x = if edge == "right" { desk_max_x - 60 } else { desk_min_x + 60 };
    let center_y = pt.y.clamp(60, 1020);
    let center = POINT { x: center_x, y: center_y };
    *clamp_center().lock().unwrap() = center;

    let clip_rect = RECT {
        left: center_x - 30,
        top: center_y - 30,
        right: center_x + 30,
        bottom: center_y + 30,
    };
    let _ = ClipCursor(Some(&clip_rect));
    RECENTERING.store(true, Ordering::SeqCst);
    let _ = SetCursorPos(center_x, center_y);
    RECENTERING.store(false, Ordering::SeqCst);

    if ShowCursor(false) >= 0 {
        let _ = ShowCursor(false);
    }

    let guard = packet_sender().lock().unwrap();
    if let Some(ref tx) = *guard {
        let total_h = (DESKTOP_MAX_Y.load(Ordering::Relaxed) - DESKTOP_MIN_Y.load(Ordering::Relaxed)).max(1);
        let _ = tx.try_send(KvmPacket::CursorEnter {
            edge: link.to_edge.clone(),
            relative_y: (pt.y as f32) / (total_h as f32),
        });
    }
}

#[cfg(windows)]
unsafe fn untrap_and_return(return_x: i32, return_y: i32, edge: &str) {
    IS_TRAPPED.store(false, Ordering::SeqCst);
    EDGE_DWELL_START.store(0, Ordering::Relaxed);
    restore_cursor_visibility();
    let _ = SetCursorPos(return_x, return_y);

    let guard = packet_sender().lock().unwrap();
    if let Some(ref tx) = *guard {
        let total_h = (DESKTOP_MAX_Y.load(Ordering::Relaxed) - DESKTOP_MIN_Y.load(Ordering::Relaxed)).max(1);
        let _ = tx.try_send(KvmPacket::CursorReturn {
            edge: edge.to_string(),
            relative_y: (return_y as f32) / (total_h as f32),
        });
    }
}

#[cfg(windows)]
fn send_button(button: u8, is_down: bool) {
    let guard = packet_sender().lock().unwrap();
    if let Some(ref tx) = *guard {
        let _ = tx.try_send(KvmPacket::MouseButton { button, is_down });
    }
}

#[cfg(windows)]
unsafe extern "system" fn low_level_keyboard_proc(
    n_code: i32,
    w_param: WPARAM,
    l_param: LPARAM,
) -> LRESULT {
    if n_code >= 0 && HOOK_ACTIVE.load(Ordering::Relaxed) {
        let kbd = *(l_param.0 as *const KBDLLHOOKSTRUCT);
        let vk_code = kbd.vkCode as u16;
        let scan_code = kbd.scanCode as u16;
        let is_down = w_param.0 as u32 == WM_KEYDOWN || w_param.0 as u32 == WM_SYSKEYDOWN;
        let extended = (kbd.flags.0 & 1) != 0;

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
            IS_TRAPPED.store(false, Ordering::SeqCst);
            EDGE_DWELL_START.store(0, Ordering::Relaxed);
            restore_cursor_visibility();

            let guard = packet_sender().lock().unwrap();
            if let Some(ref tx) = *guard {
                let _ = tx.try_send(KvmPacket::EmergencyRelease);
            }
            return LRESULT(0);
        }

        if IS_TRAPPED.load(Ordering::Relaxed) {
            let guard = packet_sender().lock().unwrap();
            if let Some(ref tx) = *guard {
                let _ = tx.try_send(KvmPacket::KeyEvent {
                    key_code: vk_code,
                    scan_code,
                    is_down,
                    extended,
                });
            }
            return LRESULT(1);
        }
    }

    CallNextHookEx(None, n_code, w_param, l_param)
}
