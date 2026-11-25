use super::debug_log::{self, DebugEvent, HookKind, KeyDecision, KeySnapshot};
use super::input_logic::{
    compute_seams, detect_controller_edge, detect_controller_edge_seams, map_controller_return_pos,
    map_leap_relative_y, resolve_remote_origin, union_rect, Edge, PressedInputTracker, Seam,
};
use super::monitors::{get_local_monitors, MonitorDisplay};
use super::protocol::{KvmPacket, Point, ScreenRect, TopologyConfig};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use tokio::sync::mpsc;

#[cfg(windows)]
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, HANDLE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM,
};
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
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, GetLastInputInfo, LASTINPUTINFO, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
    VK_XBUTTON1, VK_XBUTTON2,
};
#[cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::*;

#[cfg(windows)]
const WM_KVM_RESTORE_CURSOR: u32 = WM_APP + 1;
#[cfg(windows)]
const WM_KVM_ENGAGE: u32 = WM_APP + 2;
#[cfg(windows)]
const WM_KVM_REFRESH_OVERLAY: u32 = WM_APP + 3;
#[cfg(windows)]
const WM_KVM_RETURN_CURSOR: u32 = WM_APP + 4;
#[cfg(windows)]
const WATCHDOG_INTERVAL_MS: u32 = 250;
#[cfg(windows)]
const RETURN_VERIFY_DELAY_MS: u32 = 50;
#[cfg(any(windows, test))]
const WATCHDOG_BLIND_THRESHOLD_MS: u32 = 1000;
const BLOCKED_NOTICE_COOLDOWN_MS: u64 = 1000;
#[cfg(any(windows, test))]
const TIMER_BLOCKED_NOTICE_COOLDOWN_MS: u64 = 5000;
#[cfg(any(windows, test))]
const ENGAGE_HANDOFF_DEBOUNCE_MS: u64 = 300;
/// Short post-return hysteresis: require moving >= 16 px into Main PC or waiting
/// 150 ms before a new edge dwell/leap can arm on the same seam edge.
#[cfg(any(windows, test))]
const RETURN_REARM_COOLDOWN_MS: u64 = 150;
#[cfg(any(windows, test))]
const RETURN_REARM_ESCAPE_PX: i32 = 16;
#[cfg(windows)]
const SUPPRESSED_NOTE_COOLDOWN_MS: u64 = 1000;
const SECURITY_MANDATORY_MEDIUM_RID: u32 = 0x2000;
const SECURITY_MANDATORY_HIGH_RID: u32 = 0x3000;

pub type LeapBlockedCallback = Arc<dyn Fn(&str) + Send + Sync>;
type MouseDeltaSender = mpsc::Sender<(i32, i32)>;

static PACKET_SENDER: OnceLock<Mutex<Option<mpsc::Sender<KvmPacket>>>> = OnceLock::new();
static MOUSE_DELTA_SENDER: OnceLock<Mutex<Option<MouseDeltaSender>>> = OnceLock::new();
static LEAP_BLOCKED_CALLBACK: OnceLock<Mutex<Option<LeapBlockedCallback>>> = OnceLock::new();
static LOCAL_MONITORS: OnceLock<Mutex<Vec<MonitorDisplay>>> = OnceLock::new();
static CONTROLLER_LAYOUT: OnceLock<Mutex<ControllerLayout>> = OnceLock::new();
static CONTROLLER_TRACKER: OnceLock<Mutex<PressedInputTracker>> = OnceLock::new();

#[derive(Default)]
struct ControllerLayout {
    main_monitors: Vec<MonitorDisplay>,
    linked_monitors: Vec<MonitorDisplay>,
    remote_origin: Option<Point>,
    seams: Vec<Seam>,
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrapPhase {
    Idle = 0,
    EngagePending = 1,
    Trapped = 2,
    Returning = 3,
}

impl TrapPhase {
    fn from_u8(raw: u8) -> Self {
        match raw {
            1 => Self::EngagePending,
            2 => Self::Trapped,
            3 => Self::Returning,
            _ => Self::Idle,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::EngagePending => "engage_pending",
            Self::Trapped => "trapped",
            Self::Returning => "returning",
        }
    }

    /// True while input is actively captured or about to be captured.
    pub(crate) fn is_trapped_like(self) -> bool {
        matches!(self, Self::EngagePending | Self::Trapped)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TrapState {
    pub phase: TrapPhase,
    pub cursor_centered: bool,
    pub generation: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrapEvent {
    RequestEngage,
    BeginEngage { expected_gen: u32 },
    CommitEngage { expected_gen: u32 },
    AbortEngage { expected_gen: u32 },
    MarkCentered { expected_gen: u32 },
    ReturnRequested,
    CompleteReturn { expected_gen: u32 },
    ForceRelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TrapEffect {
    Accepted { prev_phase: TrapPhase, new_gen: u32 },
    Rejected,
}

impl TrapState {
    pub(crate) const fn new() -> Self {
        Self {
            phase: TrapPhase::Idle,
            cursor_centered: false,
            generation: 0,
        }
    }

    pub(crate) const fn encode(self) -> u64 {
        ((self.generation as u64) << 16)
            | ((self.cursor_centered as u64) << 8)
            | (self.phase as u64)
    }

    pub(crate) fn decode(raw: u64) -> Self {
        Self {
            phase: TrapPhase::from_u8((raw & 0xFF) as u8),
            cursor_centered: ((raw >> 8) & 1) != 0,
            generation: ((raw >> 16) & 0xFFFF_FFFF) as u32,
        }
    }

    pub(crate) fn can_emit_trapped_delta(self) -> bool {
        self.phase == TrapPhase::Trapped && self.cursor_centered
    }

    #[cfg(test)]
    pub(crate) fn can_recenter_cursor(self) -> bool {
        self.phase == TrapPhase::Trapped
    }

    pub(crate) fn step(self, event: TrapEvent) -> (Self, TrapEffect) {
        match event {
            TrapEvent::RequestEngage => {
                if self.phase != TrapPhase::Idle {
                    return (self, TrapEffect::Rejected);
                }
                let next_gen = self.generation.wrapping_add(1);
                (
                    Self {
                        phase: TrapPhase::EngagePending,
                        cursor_centered: false,
                        generation: next_gen,
                    },
                    TrapEffect::Accepted {
                        prev_phase: self.phase,
                        new_gen: next_gen,
                    },
                )
            }
            TrapEvent::BeginEngage { expected_gen } => {
                if self.phase == TrapPhase::EngagePending && self.generation == expected_gen {
                    (
                        self,
                        TrapEffect::Accepted {
                            prev_phase: self.phase,
                            new_gen: self.generation,
                        },
                    )
                } else {
                    (self, TrapEffect::Rejected)
                }
            }
            TrapEvent::CommitEngage { expected_gen } => {
                if self.phase == TrapPhase::EngagePending && self.generation == expected_gen {
                    (
                        Self {
                            phase: TrapPhase::Trapped,
                            cursor_centered: false,
                            generation: self.generation,
                        },
                        TrapEffect::Accepted {
                            prev_phase: self.phase,
                            new_gen: self.generation,
                        },
                    )
                } else {
                    (self, TrapEffect::Rejected)
                }
            }
            TrapEvent::AbortEngage { expected_gen } => {
                if self.phase == TrapPhase::EngagePending && self.generation == expected_gen {
                    let next_gen = self.generation.wrapping_add(1);
                    (
                        Self {
                            phase: TrapPhase::Idle,
                            cursor_centered: false,
                            generation: next_gen,
                        },
                        TrapEffect::Accepted {
                            prev_phase: self.phase,
                            new_gen: next_gen,
                        },
                    )
                } else {
                    (self, TrapEffect::Rejected)
                }
            }
            TrapEvent::MarkCentered { expected_gen } => {
                if self.phase == TrapPhase::Trapped && self.generation == expected_gen {
                    (
                        Self {
                            phase: TrapPhase::Trapped,
                            cursor_centered: true,
                            generation: self.generation,
                        },
                        TrapEffect::Accepted {
                            prev_phase: self.phase,
                            new_gen: self.generation,
                        },
                    )
                } else {
                    (self, TrapEffect::Rejected)
                }
            }
            TrapEvent::ReturnRequested => {
                if self.phase == TrapPhase::Idle {
                    return (self, TrapEffect::Rejected);
                }
                let next_gen = self.generation.wrapping_add(1);
                (
                    Self {
                        phase: TrapPhase::Returning,
                        cursor_centered: false,
                        generation: next_gen,
                    },
                    TrapEffect::Accepted {
                        prev_phase: self.phase,
                        new_gen: next_gen,
                    },
                )
            }
            TrapEvent::CompleteReturn { expected_gen } => {
                if self.phase == TrapPhase::Returning && self.generation == expected_gen {
                    let next_gen = self.generation.wrapping_add(1);
                    (
                        Self {
                            phase: TrapPhase::Idle,
                            cursor_centered: false,
                            generation: next_gen,
                        },
                        TrapEffect::Accepted {
                            prev_phase: self.phase,
                            new_gen: next_gen,
                        },
                    )
                } else {
                    (self, TrapEffect::Rejected)
                }
            }
            TrapEvent::ForceRelease => {
                let next_gen = if self.phase == TrapPhase::Idle {
                    self.generation
                } else {
                    self.generation.wrapping_add(1)
                };
                (
                    Self {
                        phase: TrapPhase::Idle,
                        cursor_centered: false,
                        generation: next_gen,
                    },
                    TrapEffect::Accepted {
                        prev_phase: self.phase,
                        new_gen: next_gen,
                    },
                )
            }
        }
    }
}

static HOOK_ACTIVE: AtomicBool = AtomicBool::new(false);
static TRAP_STATE_WORD: AtomicU64 = AtomicU64::new(TrapState::new().encode());
static PENDING_RETURN_POS: AtomicU64 = AtomicU64::new(0);
static RETURN_MAIN_EDGE: AtomicU8 = AtomicU8::new(Edge::Left as u8);
static POST_RETURN_HYSTERESIS_ACTIVE: AtomicBool = AtomicBool::new(false);
static LAST_RETURN_MS: AtomicU64 = AtomicU64::new(0);
static LAST_RETURN_POS: AtomicU64 = AtomicU64::new(0);
static LAST_RETURN_LEAP_EDGE: AtomicU8 = AtomicU8::new(Edge::Left as u8);
static HOOK_THREAD_ID: AtomicU32 = AtomicU32::new(0);

fn pack_pos(x: i32, y: i32) -> u64 {
    ((x as u32 as u64) << 32) | (y as u32 as u64)
}

fn unpack_pos(raw: u64) -> (i32, i32) {
    ((raw >> 32) as u32 as i32, (raw & 0xFFFF_FFFF) as u32 as i32)
}

fn load_trap_state() -> TrapState {
    TrapState::decode(TRAP_STATE_WORD.load(Ordering::Acquire))
}

fn apply_trap_event(ev: TrapEvent) -> (TrapState, TrapEffect) {
    // `fetch_update` is deprecated (renamed `try_update`) on newer stable; keep it until the MSRV has `try_update`.
    #[allow(deprecated)]
    match TRAP_STATE_WORD.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |raw| {
        let cur = TrapState::decode(raw);
        let (next, eff) = cur.step(ev);
        if matches!(eff, TrapEffect::Rejected) {
            None
        } else {
            Some(next.encode())
        }
    }) {
        Ok(prev_raw) => TrapState::decode(prev_raw).step(ev),
        Err(cur_raw) => (TrapState::decode(cur_raw), TrapEffect::Rejected),
    }
}

fn try_apply_trap_event(ev: TrapEvent) -> bool {
    matches!(apply_trap_event(ev).1, TrapEffect::Accepted { .. })
}

fn arm_post_return_hysteresis(rx: i32, ry: i32, leap_edge: Edge, now_ms: u64) {
    LAST_RETURN_POS.store(pack_pos(rx, ry), Ordering::Relaxed);
    LAST_RETURN_LEAP_EDGE.store(leap_edge as u8, Ordering::Relaxed);
    LAST_RETURN_MS.store(now_ms, Ordering::Relaxed);
    POST_RETURN_HYSTERESIS_ACTIVE.store(true, Ordering::Release);
}

fn clear_post_return_hysteresis() {
    POST_RETURN_HYSTERESIS_ACTIVE.store(false, Ordering::Release);
}

static CLAMP_CENTER_X: AtomicI32 = AtomicI32::new(960);
static CLAMP_CENTER_Y: AtomicI32 = AtomicI32::new(540);
static TRAP_MON_X: AtomicI32 = AtomicI32::new(0);
static TRAP_MON_Y: AtomicI32 = AtomicI32::new(0);
static TRAP_MON_W: AtomicI32 = AtomicI32::new(1920);
static TRAP_MON_H: AtomicI32 = AtomicI32::new(1080);
static PENDING_ENGAGE_REL_Y: AtomicU32 = AtomicU32::new(0);
static PENDING_ENGAGE_ALONG_PX: AtomicI32 = AtomicI32::new(0);
static PENDING_ENGAGE_ENTRY_EDGE: AtomicU8 = AtomicU8::new(Edge::Left as u8);
static PENDING_ENGAGE_LEAP_SIDE: AtomicU8 = AtomicU8::new(Edge::Right as u8);
static PENDING_ENGAGE_HAS_SEAM: AtomicBool = AtomicBool::new(false);
static PENDING_ENGAGE_TARGET_MON: OnceLock<Mutex<String>> = OnceLock::new();

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
// --- SPEC-25a ---
static DRAG_MODE_ENGAGE: AtomicBool = AtomicBool::new(false);
static DRAG_MODE_BUTTON: AtomicU8 = AtomicU8::new(1);
pub type DragModeEngagedCallback = Arc<dyn Fn(u8) + Send + Sync>;
static DRAG_MODE_ENGAGED_CB: OnceLock<Mutex<Option<DragModeEngagedCallback>>> = OnceLock::new();

fn drag_mode_engaged_slot() -> &'static Mutex<Option<DragModeEngagedCallback>> {
    DRAG_MODE_ENGAGED_CB.get_or_init(|| Mutex::new(None))
}
// --- end SPEC-25a ---
static KEY_CTRL_DOWN: AtomicBool = AtomicBool::new(false);
static KEY_ALT_DOWN: AtomicBool = AtomicBool::new(false);
static EDGE_DWELL_START: AtomicU64 = AtomicU64::new(0);
static EDGE_DWELL_SEAM_IDX: AtomicI32 = AtomicI32::new(-1);
static LAST_HANDOFF_MS: AtomicU64 = AtomicU64::new(0);
#[cfg(windows)]
static CACHED_OVERLAY_SUPPRESSED: AtomicBool = AtomicBool::new(false);
#[cfg(windows)]
static LAST_SUPPRESSED_NOTE_MS: AtomicU64 = AtomicU64::new(0);
#[cfg(windows)]
static LAST_BLOCKED_NOTICE_MS: AtomicU64 = AtomicU64::new(0);
#[cfg(windows)]
static LAST_TIMER_BLOCKED_NOTICE_MS: AtomicU64 = AtomicU64::new(0);
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

fn controller_layout() -> &'static Mutex<ControllerLayout> {
    CONTROLLER_LAYOUT.get_or_init(|| Mutex::new(ControllerLayout::default()))
}

fn pending_engage_target_mon() -> &'static Mutex<String> {
    PENDING_ENGAGE_TARGET_MON.get_or_init(|| Mutex::new(String::new()))
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
fn post_hook_thread_message_with_param(msg: u32, wparam: usize) -> bool {
    unsafe {
        let tid = HOOK_THREAD_ID.load(Ordering::Acquire);
        if tid != 0 {
            PostThreadMessageW(tid, msg, WPARAM(wparam), LPARAM(0)).is_ok()
        } else {
            false
        }
    }
}

#[cfg(windows)]
fn post_hook_thread_message(msg: u32) {
    let _ = post_hook_thread_message_with_param(msg, 0);
}

fn notify_hook_thread_restore() {
    restore_cursor_visibility();
    #[cfg(windows)]
    post_hook_thread_message(WM_KVM_RESTORE_CURSOR);
}

// --- SPEC-25a ---
fn drag_leap_edge_tolerance(strip_width_px: i32) -> i32 {
    (strip_width_px + 2).max(4)
}

fn should_ignore_injected_mouse_hook_event(
    flags: u32,
    local_drag_source_active: bool,
    is_trapped: bool,
) -> bool {
    const LLMHF_INJECTED: u32 = 1;
    (flags & LLMHF_INJECTED) != 0 && local_drag_source_active && !is_trapped
}

fn is_physical_mouse_hook_event(flags: u32) -> bool {
    const LLMHF_INJECTED: u32 = 0x0000_0001;
    const LLMHF_LOWER_IL_INJECTED: u32 = 0x0000_0002;
    (flags & (LLMHF_INJECTED | LLMHF_LOWER_IL_INJECTED)) == 0
}

/// Test-only `dwExtraInfo` magic stamped on `SendInput` moves by
/// `scripts/e2e/kvm-soak.ps1`. Keep in sync with `$script:AnyLeapE2EInputTag`
/// in `scripts/e2e/common.ps1`. AnyLeap's own injections use `dwExtraInfo: 0`.
#[cfg(any(windows, test))]
const ANYLEAP_E2E_INPUT_TAG: usize = 0x414E_5945_3245_4531;

/// Opt-in flag file (under `%LOCALAPPDATA%id.co.dak.anyleap`) that makes the
/// hook accept tagged injected mouse events as physical. Test-only hook.
#[cfg(windows)]
const E2E_ACCEPT_INJECTED_FLAG_FILE: &str = "e2e-accept-injected";

/// Cached result of the flag-file check; refreshed only on the hook thread
/// (at start and on the watchdog timer), never inside the hook callback.
#[cfg(windows)]
static E2E_ACCEPT_INJECTED: AtomicBool = AtomicBool::new(false);

/// True if the event is genuinely physical, or (E2E flag on) a tagged
/// injected event. A lower-IL injected event is accepted only when
/// `LLMHF_INJECTED` is also set, which Windows does for all injected input.
#[cfg(any(windows, test))]
fn counts_as_physical(flags: u32, extra_info: usize, e2e_enabled: bool) -> bool {
    const LLMHF_INJECTED: u32 = 0x0000_0001;
    if is_physical_mouse_hook_event(flags) {
        return true;
    }
    e2e_enabled && (flags & LLMHF_INJECTED) != 0 && extra_info == ANYLEAP_E2E_INPUT_TAG
}

#[cfg(windows)]
fn refresh_e2e_accept_injected() {
    let present = std::env::var_os("LOCALAPPDATA")
        .map(|base| {
            std::path::Path::new(&base)
                .join("id.co.dak.anyleap")
                .join(E2E_ACCEPT_INJECTED_FLAG_FILE)
                .exists()
        })
        .unwrap_or(false);
    let prev = E2E_ACCEPT_INJECTED.swap(present, Ordering::Relaxed);
    if prev != present {
        debug_log::note(|| format!("E2E_INJECTED_INPUT enabled={}", u8::from(present)));
    }
}

pub fn is_hook_active() -> bool {
    HOOK_ACTIVE.load(Ordering::Acquire)
}

pub fn is_physical_button_down(button: u8) -> bool {
    let bit = match button {
        1 => 1u8,
        2 => 2u8,
        3 => 4u8,
        _ => return false,
    };
    (MOUSE_BUTTONS_DOWN.load(Ordering::Acquire) & bit) != 0
}

fn button_from_mouse_up_msg(msg: u32) -> Option<u8> {
    match msg {
        0x0202 => Some(1), // WM_LBUTTONUP
        0x0205 => Some(2), // WM_RBUTTONUP
        0x0208 => Some(3), // WM_MBUTTONUP
        _ => None,
    }
}
// --- end SPEC-25a ---

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PostReturnHysteresis {
    Inactive,
    Suppress,
    ClearedByTimeout,
    ClearedByEscape,
    ClearedByEdgeChange,
}

#[cfg(any(windows, test))]
#[allow(clippy::too_many_arguments)]
fn evaluate_post_return_hysteresis(
    active: bool,
    now_ms: u64,
    return_ms: u64,
    return_pos: (i32, i32),
    return_edge: Edge,
    cursor_pos: (i32, i32),
    candidate_edge: Edge,
    cooldown_ms: u64,
    escape_px: i32,
) -> PostReturnHysteresis {
    if !active {
        return PostReturnHysteresis::Inactive;
    }
    if candidate_edge != return_edge {
        return PostReturnHysteresis::ClearedByEdgeChange;
    }
    if now_ms.saturating_sub(return_ms) >= cooldown_ms {
        return PostReturnHysteresis::ClearedByTimeout;
    }
    let away_px = match return_edge {
        Edge::Left => cursor_pos.0.saturating_sub(return_pos.0),
        Edge::Right => return_pos.0.saturating_sub(cursor_pos.0),
        Edge::Top => cursor_pos.1.saturating_sub(return_pos.1),
        Edge::Bottom => return_pos.1.saturating_sub(cursor_pos.1),
    };
    if away_px >= escape_px {
        PostReturnHysteresis::ClearedByEscape
    } else {
        PostReturnHysteresis::Suppress
    }
}

#[cfg(windows)]
fn maybe_clear_post_return_hysteresis_on_move(cursor_pos: (i32, i32), now_ms: u64) {
    if !POST_RETURN_HYSTERESIS_ACTIVE.load(Ordering::Acquire) {
        return;
    }
    let return_edge = Edge::from_u8(LAST_RETURN_LEAP_EDGE.load(Ordering::Relaxed));
    let decision = evaluate_post_return_hysteresis(
        true,
        now_ms,
        LAST_RETURN_MS.load(Ordering::Relaxed),
        unpack_pos(LAST_RETURN_POS.load(Ordering::Relaxed)),
        return_edge,
        cursor_pos,
        return_edge,
        RETURN_REARM_COOLDOWN_MS,
        RETURN_REARM_ESCAPE_PX,
    );
    if matches!(
        decision,
        PostReturnHysteresis::ClearedByTimeout | PostReturnHysteresis::ClearedByEscape
    ) {
        clear_post_return_hysteresis();
    }
}

#[cfg(windows)]
fn check_post_return_hysteresis(cursor_pos: (i32, i32), candidate_edge: Edge, now_ms: u64) -> bool {
    let active = POST_RETURN_HYSTERESIS_ACTIVE.load(Ordering::Acquire);
    if !active {
        return false;
    }
    let decision = evaluate_post_return_hysteresis(
        true,
        now_ms,
        LAST_RETURN_MS.load(Ordering::Relaxed),
        unpack_pos(LAST_RETURN_POS.load(Ordering::Relaxed)),
        Edge::from_u8(LAST_RETURN_LEAP_EDGE.load(Ordering::Relaxed)),
        cursor_pos,
        candidate_edge,
        RETURN_REARM_COOLDOWN_MS,
        RETURN_REARM_ESCAPE_PX,
    );
    match decision {
        PostReturnHysteresis::Suppress => true,
        PostReturnHysteresis::Inactive => false,
        PostReturnHysteresis::ClearedByTimeout
        | PostReturnHysteresis::ClearedByEscape
        | PostReturnHysteresis::ClearedByEdgeChange => {
            clear_post_return_hysteresis();
            false
        }
    }
}

fn compute_return_position(
    edge: &str,
    relative_y: f32,
    target_monitor: Option<&str>,
    along_px: Option<i32>,
) -> (Edge, (i32, i32)) {
    let fallback_mon = MonitorDisplay {
        id: String::new(),
        device_name: String::new(),
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
    let return_edge = Edge::from_str(edge).unwrap_or_else(|| link_edge.opposite());
    let main_leap_edge = if target_monitor.is_some() && along_px.is_some() {
        return_edge.opposite()
    } else {
        link_edge
    };
    let offset_x = LINK_OFFSET_X.load(Ordering::Relaxed);
    let offset_y = LINK_OFFSET_Y.load(Ordering::Relaxed);
    let pos = if let Ok(mons) = local_monitors().try_lock() {
        map_controller_return_pos(
            &mons,
            target_monitor,
            return_edge,
            along_px,
            &fallback_mon,
            &peer_union,
            link_edge,
            offset_x,
            offset_y,
            relative_y,
        )
    } else {
        map_controller_return_pos(
            &[],
            target_monitor,
            return_edge,
            along_px,
            &fallback_mon,
            &peer_union,
            link_edge,
            offset_x,
            offset_y,
            relative_y,
        )
    };
    (main_leap_edge, pos)
}

pub struct HookController {
    _running: Arc<AtomicBool>,
}

impl Default for HookController {
    fn default() -> Self {
        Self::new()
    }
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
        load_trap_state().phase.is_trapped_like()
    }

    pub fn set_trapped(&self, trapped: bool) {
        if !trapped {
            apply_trap_event(TrapEvent::ForceRelease);
            reset_dwell_and_drag_state();
            notify_hook_thread_restore();
        } else {
            let _ = apply_trap_event(TrapEvent::RequestEngage);
        }
    }

    pub fn handle_cursor_return(
        &self,
        edge: &str,
        relative_y: f32,
        target_monitor: Option<&str>,
        along_px: Option<i32>,
    ) {
        let (main_leap_edge, (rx, ry)) =
            compute_return_position(edge, relative_y, target_monitor, along_px);
        PENDING_RETURN_POS.store(pack_pos(rx, ry), Ordering::Release);
        RETURN_MAIN_EDGE.store(main_leap_edge as u8, Ordering::Release);

        let (_, effect) = apply_trap_event(TrapEvent::ReturnRequested);
        let (should_place_cursor, was_trapped, prev_phase, new_gen) = match effect {
            TrapEffect::Accepted {
                prev_phase,
                new_gen,
            } => (true, prev_phase.is_trapped_like(), prev_phase, new_gen),
            TrapEffect::Rejected => (false, false, TrapPhase::Idle, 0),
        };

        if debug_log::is_enabled() {
            debug_log::record_event(DebugEvent::Return {
                ts_us: debug_log::now_us(),
                relative_y,
                was_trapped,
                prev_phase: prev_phase.as_str(),
                target: should_place_cursor.then_some((rx, ry)),
                hwnd: debug_log::foreground_hwnd_raw(),
            });
        }
        // Always clear pending dwell/drag/modifier flags and restore cursor clip
        // & visibility so return never leaves stale trap state.
        reset_dwell_and_drag_state();
        KEY_CTRL_DOWN.store(false, Ordering::Relaxed);
        KEY_ALT_DOWN.store(false, Ordering::Relaxed);
        if !should_place_cursor {
            notify_hook_thread_restore();
            return;
        }

        arm_post_return_hysteresis(rx, ry, main_leap_edge, now_millis());
        #[cfg(windows)]
        {
            // Release ClipCursor immediately on desktop so cursor isn't clamped to
            // a single monitor while waiting for the hook thread message.
            unsafe {
                let _ = ClipCursor(None);
            }
            if !post_hook_thread_message_with_param(WM_KVM_RETURN_CURSOR, new_gen as usize) {
                unsafe {
                    place_return_cursor_and_complete(rx, ry, new_gen);
                }
            }
        }
        #[cfg(not(windows))]
        {
            let _ = apply_trap_event(TrapEvent::CompleteReturn {
                expected_gen: new_gen,
            });
        }
    }

    pub fn emergency_release(&self) {
        if debug_log::is_enabled() {
            debug_log::record_event(DebugEvent::Release {
                ts_us: debug_log::now_us(),
                reason: "emergency_release",
            });
        }
        apply_trap_event(TrapEvent::ForceRelease);
        reset_dwell_and_drag_state();
        clear_post_return_hysteresis();
        MOUSE_BUTTONS_DOWN.store(0, Ordering::Relaxed);
        KEY_CTRL_DOWN.store(false, Ordering::Relaxed);
        KEY_ALT_DOWN.store(false, Ordering::Relaxed);
        if let Ok(mut tracker) = controller_tracker().try_lock() {
            let _ = tracker.release_all();
        }
        notify_hook_thread_restore();
        super::dragdrop::cancel_active_target_drag_with_reason("emergency_release");

        try_send_packet(KvmPacket::EmergencyRelease);
        try_send_packet(KvmPacket::ReleaseAll);
    }

    // --- SPEC-25a ---
    pub fn set_drag_mode_engaged_callback(&self, cb: Option<DragModeEngagedCallback>) {
        if let Ok(mut g) = drag_mode_engaged_slot().lock() {
            *g = cb;
        }
    }

    /// Pure guard for whether a drag-mode leap may engage.
    pub fn can_engage_drag_leap(
        drag_drop_enabled: bool,
        hook_active: bool,
        is_trapped: bool,
        has_active_link: bool,
        mouse_buttons_down: u8,
    ) -> bool {
        drag_drop_enabled
            && hook_active
            && !is_trapped
            && has_active_link
            && mouse_buttons_down != 0
    }

    /// Engage a "drag mode" leap after the edge-catcher helper thread has
    /// cancelled the local OLE drag with Escape. Never call from a hook callback.
    #[cfg(windows)]
    pub fn engage_drag_leap(&self, pt: (i32, i32), button: u8) -> bool {
        if !Self::can_engage_drag_leap(
            super::dragdrop::drag_drop_enabled(),
            HOOK_ACTIVE.load(Ordering::Acquire),
            load_trap_state().phase != TrapPhase::Idle,
            HAS_ACTIVE_LINK.load(Ordering::Acquire),
            MOUSE_BUTTONS_DOWN.load(Ordering::Acquire),
        ) {
            return false;
        }

        let link_edge = Edge::from_u8(LINK_FROM_EDGE.load(Ordering::Relaxed));
        let tol = drag_leap_edge_tolerance(2);
        let hit = {
            let Ok(layout) = controller_layout().lock() else {
                return false;
            };
            if let Some(origin) = layout.remote_origin {
                detect_controller_edge_seams(
                    pt,
                    &layout.main_monitors,
                    &layout.linked_monitors,
                    origin,
                    &layout.seams,
                    tol,
                )
                .map(|h| {
                    let m = &layout.main_monitors[h.main_monitor_index];
                    (
                        m.x,
                        m.y,
                        m.width,
                        m.height,
                        h.seam.side,
                        h.seam.side.opposite(),
                        h.relative_y,
                        Some((h.target_monitor, h.along_px)),
                    )
                })
            } else {
                detect_controller_edge(pt, &layout.main_monitors, link_edge, tol).map(|(idx, _)| {
                    let m = &layout.main_monitors[idx];
                    let clamped = (
                        pt.0.clamp(m.x, m.x + m.width - 1),
                        pt.1.clamp(m.y, m.y + m.height - 1),
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
                    (
                        m.x,
                        m.y,
                        m.width,
                        m.height,
                        link_edge,
                        Edge::from_u8(LINK_TO_EDGE.load(Ordering::Relaxed)),
                        rel,
                        None,
                    )
                })
            }
        };

        debug_log::note(|| {
            format!(
                "DRAG_LEAP_ENGAGE pt=({},{}) button={} hit={}",
                pt.0,
                pt.1,
                button,
                hit.is_some(),
            )
        });
        let Some((mx, my, mw, mh, leap_side, entry_edge, relative_y, seam_target)) = hit else {
            return false;
        };

        DRAG_MODE_BUTTON.store(button, Ordering::Release);
        DRAG_MODE_ENGAGE.store(true, Ordering::Release);
        if !request_engage_leap(
            mx,
            my,
            mw,
            mh,
            leap_side,
            entry_edge,
            relative_y,
            seam_target,
        ) {
            DRAG_MODE_ENGAGE.store(false, Ordering::Release);
            return false;
        }
        true
    }

    #[cfg(not(windows))]
    pub fn engage_drag_leap(&self, _pt: (i32, i32), _button: u8) -> bool {
        false
    }
    // --- end SPEC-25a ---

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
            *mons = monitors.clone();
        }

        let local_id = topology
            .computers
            .iter()
            .find(|c| c.is_local)
            .map(|c| c.id.as_str());

        let link = local_id
            .and_then(|id| topology.links.iter().find(|l| l.from_id == id))
            .or_else(|| topology.links.first());

        let peer_node = link
            .and_then(|l| topology.computers.iter().find(|c| c.id == l.to_id))
            .or_else(|| topology.computers.iter().find(|c| !c.is_local));

        let linked_monitors: Vec<MonitorDisplay> = peer_node
            .map(|c| {
                if !c.monitors.is_empty() {
                    c.monitors.clone()
                } else if c.bounds.width > 0 && c.bounds.height > 0 {
                    vec![MonitorDisplay {
                        id: "peer-1".into(),
                        device_name: "peer-1".into(),
                        x: c.bounds.x,
                        y: c.bounds.y,
                        width: c.bounds.width,
                        height: c.bounds.height,
                        is_primary: true,
                    }]
                } else {
                    Vec::new()
                }
            })
            .unwrap_or_default();

        let peer_bounds = if !linked_monitors.is_empty() {
            union_rect(&linked_monitors)
        } else {
            peer_node
                .map(|c| c.bounds.clone())
                .unwrap_or_else(|| ScreenRect {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                })
        };

        let remote_origin = if !linked_monitors.is_empty() {
            resolve_remote_origin(&topology, &monitors, &linked_monitors, local_id)
        } else {
            topology.remote_origin
        };
        let seams = match remote_origin {
            Some(origin) if !linked_monitors.is_empty() => {
                compute_seams(&monitors, &linked_monitors, origin)
            }
            _ => Vec::new(),
        };

        if let Ok(mut layout) = controller_layout().lock() {
            layout.main_monitors = monitors;
            layout.linked_monitors = linked_monitors;
            layout.remote_origin = remote_origin;
            layout.seams = seams.clone();
        }

        if link.is_some() || !seams.is_empty() {
            let from_edge = link
                .and_then(|l| Edge::from_str(&l.from_edge))
                .or_else(|| seams.first().map(|s| s.side))
                .unwrap_or(Edge::Right);
            let to_edge = link
                .and_then(|l| Edge::from_str(&l.to_edge))
                .unwrap_or_else(|| from_edge.opposite());
            LINK_FROM_EDGE.store(from_edge as u8, Ordering::Relaxed);
            LINK_TO_EDGE.store(to_edge as u8, Ordering::Relaxed);
            DEADZONE_PX.store(
                link.and_then(|l| l.deadzone_px).unwrap_or(48),
                Ordering::Relaxed,
            );
            RESISTANCE_MS.store(
                link.and_then(|l| l.resistance_ms).unwrap_or(120) as u64,
                Ordering::Relaxed,
            );
            LINK_OFFSET_X.store(
                link.and_then(|l| l.offset_x).unwrap_or(0),
                Ordering::Relaxed,
            );
            LINK_OFFSET_Y.store(
                link.and_then(|l| l.offset_y).unwrap_or(0),
                Ordering::Relaxed,
            );
            PEER_UNION_W.store(peer_bounds.width, Ordering::Relaxed);
            PEER_UNION_H.store(peer_bounds.height, Ordering::Relaxed);
            let active = if remote_origin.is_some() {
                !seams.is_empty()
            } else {
                link.is_some()
            };
            HAS_ACTIVE_LINK.store(active, Ordering::Release);
        } else {
            HAS_ACTIVE_LINK.store(false, Ordering::Release);
        }
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        EDGE_DWELL_SEAM_IDX.store(-1, Ordering::Relaxed);

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

        TRAP_STATE_WORD.store(TrapState::new().encode(), Ordering::SeqCst);
        clear_post_return_hysteresis();
        LAST_HANDOFF_MS.store(0, Ordering::Relaxed);
        #[cfg(windows)]
        {
            CACHED_OVERLAY_SUPPRESSED.store(false, Ordering::Release);
            LAST_SUPPRESSED_NOTE_MS.store(0, Ordering::Relaxed);
            LAST_BLOCKED_NOTICE_MS.store(0, Ordering::Relaxed);
            LAST_TIMER_BLOCKED_NOTICE_MS.store(0, Ordering::Relaxed);
            HAS_LAST_HOOK_TICK.store(false, Ordering::Release);
            LAST_HOOK_TICK_MS.store(0, Ordering::Release);
            LAST_MOUSE_HOOK_US.store(0, Ordering::Relaxed);
            LAST_KBD_HOOK_US.store(0, Ordering::Relaxed);
        }

        #[cfg(windows)]
        std::thread::spawn(move || unsafe {
            struct ReturnVerifyPending {
                timer_id: usize,
                target_x: i32,
                target_y: i32,
                placed_at: std::time::Instant,
                generation: u32,
            }

            let mut msg = MSG::default();
            // Force message queue creation before publishing HOOK_THREAD_ID so
            // PostThreadMessageW from a hook callback never races queue init.
            let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);

            let tid = GetCurrentThreadId();
            HOOK_THREAD_ID.store(tid, Ordering::Release);
            let our_pid = GetCurrentProcessId();

            let h_mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(low_level_mouse_proc), None, 0);
            let h_kbd = SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_proc), None, 0);
            let mut watchdog_timer_id: usize = SetTimer(None, 0, WATCHDOG_INTERVAL_MS, None);
            refresh_e2e_accept_injected();
            let mut return_verify: Option<ReturnVerifyPending> = None;

            while HOOK_ACTIVE.load(Ordering::Relaxed) {
                let res = GetMessageW(&mut msg, None, 0, 0);
                if res.0 <= 0 {
                    break;
                }
                if msg.message == WM_KVM_RESTORE_CURSOR {
                    restore_cursor_visibility();
                    continue;
                }
                if msg.message == WM_KVM_RETURN_CURSOR {
                    if let Some((rx, ry, gen)) = perform_return_cursor() {
                        if let Some(prev) = return_verify.take() {
                            if prev.timer_id != 0 {
                                let _ = KillTimer(None, prev.timer_id);
                            }
                        }
                        if debug_log::is_enabled() {
                            let timer_id = SetTimer(None, 0, RETURN_VERIFY_DELAY_MS, None);
                            if timer_id != 0 {
                                return_verify = Some(ReturnVerifyPending {
                                    timer_id,
                                    target_x: rx,
                                    target_y: ry,
                                    placed_at: std::time::Instant::now(),
                                    generation: gen,
                                });
                            }
                        }
                    }
                    continue;
                }
                if msg.message == WM_KVM_REFRESH_OVERLAY {
                    refresh_cached_overlay_suppression(our_pid);
                    continue;
                }
                if msg.message == WM_KVM_ENGAGE {
                    let expected_gen = msg.wParam.0 as u32;
                    if perform_engage_leap(expected_gen) && watchdog_timer_id == 0 {
                        watchdog_timer_id = SetTimer(None, 0, WATCHDOG_INTERVAL_MS, None);
                    }
                    continue;
                }
                if msg.message == WM_TIMER {
                    if let Some(pending) = return_verify.as_ref() {
                        if pending.timer_id != 0 && msg.wParam.0 == pending.timer_id {
                            let pending = return_verify.take().unwrap();
                            let _ = KillTimer(None, pending.timer_id);
                            if debug_log::is_enabled() {
                                let mut actual = POINT::default();
                                let actual_ok = GetCursorPos(&mut actual).is_ok();
                                let (ax, ay) = if actual_ok {
                                    (actual.x, actual.y)
                                } else {
                                    (i32::MIN, i32::MIN)
                                };
                                let snap = load_trap_state();
                                debug_log::record_event(DebugEvent::Note {
                                    ts_us: debug_log::now_us(),
                                    hwnd: debug_log::foreground_hwnd_raw(),
                                    text: format!(
                                        "RETURN_VERIFY_50MS target=({},{}) actual=({ax},{ay}) actual_ok={} elapsed_ms={} phase={} state_gen={} return_gen={}",
                                        pending.target_x,
                                        pending.target_y,
                                        u8::from(actual_ok),
                                        pending.placed_at.elapsed().as_millis(),
                                        snap.phase.as_str(),
                                        snap.generation,
                                        pending.generation,
                                    ),
                                });
                            }
                            continue;
                        }
                    }
                    if watchdog_timer_id != 0 && msg.wParam.0 == watchdog_timer_id {
                        refresh_e2e_accept_injected();
                        if load_trap_state().phase == TrapPhase::Trapped {
                            if let Some(last_input_tick) = query_last_input_tick() {
                                let hook_tick = HAS_LAST_HOOK_TICK
                                    .load(Ordering::Acquire)
                                    .then(|| LAST_HOOK_TICK_MS.load(Ordering::Acquire));
                                if let Some(gap_ms) = evaluate_capture_watchdog(
                                    true,
                                    hook_tick,
                                    last_input_tick,
                                    WATCHDOG_BLIND_THRESHOLD_MS,
                                ) {
                                    perform_watchdog_release(gap_ms);
                                }
                            }
                        } else if HAS_ACTIVE_LINK.load(Ordering::Acquire) {
                            refresh_cached_overlay_suppression(our_pid);
                            if MOUSE_BUTTONS_DOWN.load(Ordering::Acquire) != 0
                                && !any_async_mouse_button_down()
                            {
                                MOUSE_BUTTONS_DOWN.store(0, Ordering::Relaxed);
                                debug_log::note(|| {
                                    "MOUSE_BUTTONS_RECONCILED cleared=1".to_string()
                                });
                            }
                            check_timer_elevated_leap_block(our_pid);
                        }
                        continue;
                    }
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }

            if let Some(pending) = return_verify.take() {
                if pending.timer_id != 0 {
                    let _ = KillTimer(None, pending.timer_id);
                }
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
        if was_active || load_trap_state().phase != TrapPhase::Idle {
            self.emergency_release();
        }
        HAS_ACTIVE_LINK.store(false, Ordering::Release);
        reset_trapped_runtime_state();
        clear_post_return_hysteresis();
        LAST_HANDOFF_MS.store(0, Ordering::Relaxed);
        #[cfg(windows)]
        CACHED_OVERLAY_SUPPRESSED.store(false, Ordering::Release);
        if let Ok(mut g) = drag_mode_engaged_slot().try_lock() {
            *g = None;
        }
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

fn reset_dwell_and_drag_state() {
    EDGE_DWELL_START.store(0, Ordering::Relaxed);
    EDGE_DWELL_SEAM_IDX.store(-1, Ordering::Relaxed);
    // --- SPEC-25a ---
    DRAG_MODE_ENGAGE.store(false, Ordering::Release);
    PENDING_ENGAGE_HAS_SEAM.store(false, Ordering::Release);
    super::dragdrop::set_drag_leap_in_progress(false);
    super::dragdrop::hide_edge_catcher();
    // --- end SPEC-25a ---
}

fn reset_trapped_runtime_state() {
    apply_trap_event(TrapEvent::ForceRelease);
    reset_dwell_and_drag_state();
}

#[cfg(windows)]
fn disarm_trapped_in_callback() {
    reset_trapped_runtime_state();
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
    if prev != 0 && load_trap_state().phase == TrapPhase::Trapped {
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
    let trap_state = load_trap_state();
    let is_trapped_now = trap_state.phase.is_trapped_like();
    let is_physical = counts_as_physical(
        hook_struct.flags,
        hook_struct.dwExtraInfo,
        E2E_ACCEPT_INJECTED.load(Ordering::Relaxed),
    );
    if should_ignore_injected_mouse_hook_event(
        hook_struct.flags,
        super::dragdrop::is_local_drag_source_active(),
        is_trapped_now,
    ) || (!is_trapped_now && !is_physical)
    {
        return None;
    }

    if is_physical {
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
    }

    let trap_state = load_trap_state();
    if trap_state.phase == TrapPhase::Returning {
        if msg == WM_MOUSEMOVE {
            // Suppress local cursor movement until WM_KVM_RETURN_CURSOR finishes
            // placing the cursor at the seam-entry edge.
            return Some(LRESULT(1));
        }
        if is_physical {
            if let Some(btn) = button_from_mouse_up_msg(msg) {
                super::dragdrop::on_target_physical_mouse_up(btn);
            }
            if MOUSE_BUTTONS_DOWN.load(Ordering::Relaxed) == 0
                && (super::dragdrop::is_edge_catcher_visible()
                    || button_from_mouse_up_msg(msg).is_some())
            {
                super::dragdrop::on_hook_mouse_up();
            }
        }
        return None;
    }

    if trap_state.phase.is_trapped_like() {
        if is_physical
            && button_from_mouse_up_msg(msg).is_some()
            && super::dragdrop::is_local_drag_source_active()
        {
            if let Some(btn) = button_from_mouse_up_msg(msg) {
                super::dragdrop::on_target_physical_mouse_up(btn);
            }
            return None;
        }
        if msg == WM_MOUSEMOVE {
            let center = (
                CLAMP_CENTER_X.load(Ordering::Relaxed),
                CLAMP_CENTER_Y.load(Ordering::Relaxed),
            );
            if let Some(delta) =
                trapped_mouse_delta(trap_state.can_emit_trapped_delta(), (pt.x, pt.y), center)
            {
                if let Ok(guard) = mouse_delta_sender().try_lock() {
                    if let Some(ref tx) = *guard {
                        let _ = tx.try_send(delta);
                    }
                }
            }
        } else {
            match msg {
                WM_LBUTTONDOWN => try_send_packet(KvmPacket::MouseButton {
                    button: 1,
                    is_down: true,
                }),
                WM_LBUTTONUP => try_send_packet(KvmPacket::MouseButton {
                    button: 1,
                    is_down: false,
                }),
                WM_RBUTTONDOWN => try_send_packet(KvmPacket::MouseButton {
                    button: 2,
                    is_down: true,
                }),
                WM_RBUTTONUP => try_send_packet(KvmPacket::MouseButton {
                    button: 2,
                    is_down: false,
                }),
                WM_MBUTTONDOWN => try_send_packet(KvmPacket::MouseButton {
                    button: 3,
                    is_down: true,
                }),
                WM_MBUTTONUP => try_send_packet(KvmPacket::MouseButton {
                    button: 3,
                    is_down: false,
                }),
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
        // --- SPEC-25a ---
        if is_physical {
            if let Some(btn) = button_from_mouse_up_msg(msg) {
                super::dragdrop::on_target_physical_mouse_up(btn);
            }
            if MOUSE_BUTTONS_DOWN.load(Ordering::Relaxed) == 0
                && (super::dragdrop::is_edge_catcher_visible()
                    || button_from_mouse_up_msg(msg).is_some())
            {
                super::dragdrop::on_hook_mouse_up();
            }
        }
        // --- end SPEC-25a ---
        return None;
    }

    let btns_down = MOUSE_BUTTONS_DOWN.load(Ordering::Relaxed);
    if btns_down != 0 {
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        EDGE_DWELL_SEAM_IDX.store(-1, Ordering::Relaxed);
        // --- SPEC-25a ---
        if super::dragdrop::drag_drop_enabled()
            && !CACHED_OVERLAY_SUPPRESSED.load(Ordering::Acquire)
        {
            super::dragdrop::on_mouse_drag_probe((pt.x, pt.y), btns_down, false);
        } else if super::dragdrop::is_edge_catcher_visible() {
            super::dragdrop::hide_edge_catcher();
        }
        // --- end SPEC-25a ---
        return None;
    }
    if super::dragdrop::is_edge_catcher_visible() {
        super::dragdrop::hide_edge_catcher();
    }

    let now_ms = now_millis();
    maybe_clear_post_return_hysteresis_on_move((pt.x, pt.y), now_ms);

    let link_edge = Edge::from_u8(LINK_FROM_EDGE.load(Ordering::Relaxed));
    let deadzone = DEADZONE_PX.load(Ordering::Relaxed);
    let resistance_ms = RESISTANCE_MS.load(Ordering::Relaxed);

    struct EdgeLeapCandidate {
        mx: i32,
        my: i32,
        mw: i32,
        mh: i32,
        leap_side: Edge,
        entry_edge: Edge,
        relative_y: f32,
        seam_info: Option<(i32, String, i32)>,
    }

    let hit: Option<EdgeLeapCandidate> = {
        let Ok(layout) = controller_layout().try_lock() else {
            return None;
        };
        if let Some(origin) = layout.remote_origin {
            detect_controller_edge_seams(
                (pt.x, pt.y),
                &layout.main_monitors,
                &layout.linked_monitors,
                origin,
                &layout.seams,
                deadzone,
            )
            .map(|h| {
                let m = &layout.main_monitors[h.main_monitor_index];
                EdgeLeapCandidate {
                    mx: m.x,
                    my: m.y,
                    mw: m.width,
                    mh: m.height,
                    leap_side: h.seam.side,
                    entry_edge: h.seam.side.opposite(),
                    relative_y: h.relative_y,
                    seam_info: Some((h.seam_index as i32, h.target_monitor, h.along_px)),
                }
            })
        } else {
            detect_controller_edge((pt.x, pt.y), &layout.main_monitors, link_edge, deadzone).map(
                |(idx, _)| {
                    let m = &layout.main_monitors[idx];
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
                    EdgeLeapCandidate {
                        mx: m.x,
                        my: m.y,
                        mw: m.width,
                        mh: m.height,
                        leap_side: link_edge,
                        entry_edge: Edge::from_u8(LINK_TO_EDGE.load(Ordering::Relaxed)),
                        relative_y: rel,
                        seam_info: None,
                    }
                },
            )
        }
    };

    let Some(cand) = hit else {
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        EDGE_DWELL_SEAM_IDX.store(-1, Ordering::Relaxed);
        return None;
    };

    // Double-check live OS monitor topology on the adjacent pixel beyond the leap edge
    let neighbor = match cand.leap_side {
        // pt may lie past the screen edge; clamp onto the edge monitor first.
        Edge::Right => POINT {
            x: cand.mx + cand.mw,
            y: pt.y.clamp(cand.my, cand.my + cand.mh - 1),
        },
        Edge::Left => POINT {
            x: cand.mx - 1,
            y: pt.y.clamp(cand.my, cand.my + cand.mh - 1),
        },
        Edge::Top => POINT {
            x: pt.x.clamp(cand.mx, cand.mx + cand.mw - 1),
            y: cand.my - 1,
        },
        Edge::Bottom => POINT {
            x: pt.x.clamp(cand.mx, cand.mx + cand.mw - 1),
            y: cand.my + cand.mh,
        },
    };
    if !MonitorFromPoint(neighbor, MONITOR_DEFAULTTONULL).is_invalid() {
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        EDGE_DWELL_SEAM_IDX.store(-1, Ordering::Relaxed);
        return None;
    }

    if CACHED_OVERLAY_SUPPRESSED.load(Ordering::Acquire) {
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        EDGE_DWELL_SEAM_IDX.store(-1, Ordering::Relaxed);
        emit_leap_suppressed_note("topmost_fullscreen_overlay", 0, None);
        return None;
    }

    if should_debounce_engage_after_handoff(
        now_ms,
        LAST_HANDOFF_MS.load(Ordering::Relaxed),
        ENGAGE_HANDOFF_DEBOUNCE_MS,
    ) {
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        EDGE_DWELL_SEAM_IDX.store(-1, Ordering::Relaxed);
        emit_leap_suppressed_note("handoff_debounce", 0, None);
        return None;
    }

    if check_post_return_hysteresis((pt.x, pt.y), cand.leap_side, now_ms) {
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
        EDGE_DWELL_SEAM_IDX.store(-1, Ordering::Relaxed);
        emit_leap_suppressed_note("return_hysteresis", 0, None);
        return None;
    }

    let seam_idx = cand.seam_info.as_ref().map(|(i, _, _)| *i).unwrap_or(-1);
    let prev_seam = EDGE_DWELL_SEAM_IDX.swap(seam_idx, Ordering::Relaxed);
    if prev_seam != seam_idx {
        EDGE_DWELL_START.store(0, Ordering::Relaxed);
    }

    if check_dwell_resistance(resistance_ms)
        && request_engage_leap(
            cand.mx,
            cand.my,
            cand.mw,
            cand.mh,
            cand.leap_side,
            cand.entry_edge,
            cand.relative_y,
            cand.seam_info.map(|(_, t, a)| (t, a)),
        )
    {
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
        #[cfg(windows)]
        post_hook_thread_message(WM_KVM_REFRESH_OVERLAY);
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
#[allow(clippy::too_many_arguments)]
fn request_engage_leap(
    mx: i32,
    my: i32,
    mw: i32,
    mh: i32,
    leap_side: Edge,
    entry_edge: Edge,
    relative_y: f32,
    seam_target: Option<(String, i32)>,
) -> bool {
    let (_, effect) = apply_trap_event(TrapEvent::RequestEngage);
    let TrapEffect::Accepted { new_gen, .. } = effect else {
        return false;
    };

    TRAP_MON_X.store(mx, Ordering::Relaxed);
    TRAP_MON_Y.store(my, Ordering::Relaxed);
    TRAP_MON_W.store(mw, Ordering::Relaxed);
    TRAP_MON_H.store(mh, Ordering::Relaxed);

    let (center_x, center_y) = compute_trap_center(mx, my, mw, mh);
    CLAMP_CENTER_X.store(center_x, Ordering::Relaxed);
    CLAMP_CENTER_Y.store(center_y, Ordering::Relaxed);
    PENDING_ENGAGE_REL_Y.store(relative_y.to_bits(), Ordering::Relaxed);
    PENDING_ENGAGE_LEAP_SIDE.store(leap_side as u8, Ordering::Relaxed);
    PENDING_ENGAGE_ENTRY_EDGE.store(entry_edge as u8, Ordering::Relaxed);
    if let Some((target_mon, along_px)) = seam_target {
        if let Ok(mut guard) = pending_engage_target_mon().try_lock() {
            *guard = target_mon;
            PENDING_ENGAGE_ALONG_PX.store(along_px, Ordering::Relaxed);
            PENDING_ENGAGE_HAS_SEAM.store(true, Ordering::Release);
        } else {
            PENDING_ENGAGE_HAS_SEAM.store(false, Ordering::Release);
        }
    } else {
        PENDING_ENGAGE_HAS_SEAM.store(false, Ordering::Release);
    }

    EDGE_DWELL_START.store(0, Ordering::Relaxed);
    EDGE_DWELL_SEAM_IDX.store(-1, Ordering::Relaxed);
    KEY_CTRL_DOWN.store(false, Ordering::Relaxed);
    KEY_ALT_DOWN.store(false, Ordering::Relaxed);

    if debug_log::is_enabled() {
        debug_log::record_event(DebugEvent::Engage {
            ts_us: debug_log::now_us(),
            phase: "request",
            edge: leap_side,
            relative_y,
            hwnd: debug_log::foreground_hwnd_raw(),
        });
    }

    if !post_hook_thread_message_with_param(WM_KVM_ENGAGE, new_gen as usize) {
        let _ = apply_trap_event(TrapEvent::AbortEngage {
            expected_gen: new_gen,
        });
        return false;
    }
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HandoffMethod {
    Next,
    None,
}

impl HandoffMethod {
    fn as_str(self) -> &'static str {
        match self {
            Self::Next => "next",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RectBounds {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl RectBounds {
    fn from_monitor(m: &MonitorDisplay) -> Self {
        Self {
            left: m.x,
            top: m.y,
            right: m.x.saturating_add(m.width),
            bottom: m.y.saturating_add(m.height),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ForegroundOverlayCandidate {
    pub hwnd: isize,
    pub pid: u32,
    pub visible: bool,
    pub minimized: bool,
    pub cloaked: bool,
    pub topmost: bool,
    pub rect: RectBounds,
}

pub(crate) fn rect_covers_monitor(window: RectBounds, monitor: RectBounds) -> bool {
    monitor.right > monitor.left
        && monitor.bottom > monitor.top
        && window.left <= monitor.left
        && window.top <= monitor.top
        && window.right >= monitor.right
        && window.bottom >= monitor.bottom
}

pub(crate) fn should_suppress_leap_for_topmost_overlay(
    fg: ForegroundOverlayCandidate,
    edge_monitors: &[RectBounds],
    our_pid: u32,
) -> bool {
    if fg.hwnd == 0 || fg.pid == 0 || fg.pid == our_pid {
        return false;
    }
    if !fg.visible || fg.minimized || fg.cloaked || !fg.topmost {
        return false;
    }
    edge_monitors
        .iter()
        .any(|&mon| rect_covers_monitor(fg.rect, mon))
}

#[cfg(any(windows, test))]
fn collect_edge_monitor_bounds(
    main_monitors: &[MonitorDisplay],
    seams: &[Seam],
    has_remote_origin: bool,
    link_edge: Edge,
) -> Vec<RectBounds> {
    if has_remote_origin {
        let mut seen = Vec::new();
        for s in seams {
            if let Some(m) = main_monitors
                .iter()
                .find(|m| m.effective_id() == s.main_monitor_id)
            {
                let bounds = RectBounds::from_monitor(m);
                if !seen.contains(&bounds) {
                    seen.push(bounds);
                }
            }
        }
        return seen;
    }

    main_monitors
        .iter()
        .enumerate()
        .filter(|(idx, m)| {
            !main_monitors.iter().enumerate().any(|(other_idx, other)| {
                if other_idx == *idx {
                    return false;
                }
                match link_edge {
                    Edge::Left => {
                        other.x + other.width == m.x
                            && other.y < m.y + m.height
                            && other.y + other.height > m.y
                    }
                    Edge::Right => {
                        other.x == m.x + m.width
                            && other.y < m.y + m.height
                            && other.y + other.height > m.y
                    }
                    Edge::Top => {
                        other.y + other.height == m.y
                            && other.x < m.x + m.width
                            && other.x + other.width > m.x
                    }
                    Edge::Bottom => {
                        other.y == m.y + m.height
                            && other.x < m.x + m.width
                            && other.x + other.width > m.x
                    }
                }
            })
        })
        .map(|(_, m)| RectBounds::from_monitor(m))
        .collect()
}

pub(crate) fn should_debounce_engage_after_handoff(
    now_ms: u64,
    last_handoff_ms: u64,
    debounce_ms: u64,
) -> bool {
    last_handoff_ms != 0 && now_ms.saturating_sub(last_handoff_ms) < debounce_ms
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
    is_shell_desktop: bool,
    elevation: ProcessElevation,
}

fn is_shell_or_desktop_class(class_name: &str) -> bool {
    class_name.eq_ignore_ascii_case("Progman")
        || class_name.eq_ignore_ascii_case("WorkerW")
        || class_name.eq_ignore_ascii_case("Shell_TrayWnd")
        || class_name.eq_ignore_ascii_case("Shell_SecondaryTrayWnd")
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
        && !info.is_shell_desktop
        && is_safe_handoff_elevation(info.elevation)
}

#[cfg(test)]
fn select_handoff_target(
    windows_in_z_order: &[WindowCandidateInfo],
    our_pid: u32,
    fg_hwnd: isize,
) -> Option<(HandoffMethod, isize)> {
    windows_in_z_order
        .iter()
        .copied()
        .find(|w| w.hwnd != fg_hwnd && is_eligible_handoff_target(*w, our_pid))
        .map(|w| (HandoffMethod::Next, w.hwnd))
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

/// Executable name of the current foreground window's process (`unknown` if unavailable).
#[cfg(windows)]
pub fn foreground_process_name() -> String {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() {
            return "unknown".to_string();
        }
        let mut fg_pid = 0u32;
        let _ = GetWindowThreadProcessId(fg, Some(&mut fg_pid));
        query_process_info(fg_pid).1
    }
}

/// True when AnyLeap's own process is elevated (unknown counts as elevated: no false reports).
#[cfg(windows)]
pub fn own_process_elevated() -> bool {
    !matches!(
        unsafe { query_own_elevation() },
        ProcessElevation::Standard { .. }
    )
}

/// Elevation of the process owning `hwnd`; `None` when unknown.
#[cfg(windows)]
pub fn hwnd_process_elevated(hwnd: isize) -> Option<bool> {
    unsafe {
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(HWND(hwnd as *mut _), Some(&mut pid));
        match query_process_info(pid).0 {
            ProcessElevation::Elevated => Some(true),
            ProcessElevation::Standard { .. } => Some(false),
            ProcessElevation::Unknown => None,
        }
    }
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
            .map(str::to_string)
            .unwrap_or_else(|| format!("pid:{pid}"))
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
unsafe fn query_window_class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 128];
    let len = GetClassNameW(hwnd, &mut buf);
    if len > 0 {
        String::from_utf16_lossy(&buf[..len as usize])
    } else {
        String::new()
    }
}

#[cfg(windows)]
unsafe fn is_window_safe_foreground(hwnd: HWND, our_pid: u32) -> bool {
    if hwnd.0.is_null() {
        return false;
    }
    let mut pid = 0u32;
    let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 || pid == our_pid {
        return false;
    }
    let class_name = query_window_class_name(hwnd);
    if is_shell_or_desktop_class(&class_name) {
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

        // Check cheap window attributes before class/DWM/OpenProcess queries.
        if next.0 as isize != 0
            && next != fg
            && pid != 0
            && pid != our_pid
            && visible
            && !minimized
            && !tool_window
        {
            let class_name = query_window_class_name(next);
            let is_shell_desktop = is_shell_or_desktop_class(&class_name);
            let cloaked = if is_shell_desktop {
                true
            } else {
                is_window_cloaked(next)
            };
            let elevation = if cloaked || is_shell_desktop {
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
                is_shell_desktop,
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
unsafe fn any_async_mouse_button_down() -> bool {
    GetAsyncKeyState(i32::from(VK_LBUTTON.0)) < 0
        || GetAsyncKeyState(i32::from(VK_RBUTTON.0)) < 0
        || GetAsyncKeyState(i32::from(VK_MBUTTON.0)) < 0
        || GetAsyncKeyState(i32::from(VK_XBUTTON1.0)) < 0
        || GetAsyncKeyState(i32::from(VK_XBUTTON2.0)) < 0
}

#[cfg(windows)]
fn emit_leap_suppressed_note(reason: &str, hwnd: isize, proc_name: Option<&str>) {
    if !debug_log::is_enabled() {
        return;
    }
    let now = now_millis();
    let prev = LAST_SUPPRESSED_NOTE_MS.load(Ordering::Relaxed);
    if now.saturating_sub(prev) < SUPPRESSED_NOTE_COOLDOWN_MS {
        return;
    }
    if LAST_SUPPRESSED_NOTE_MS
        .compare_exchange(prev, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return;
    }
    let resolved_hwnd = if hwnd != 0 {
        hwnd
    } else {
        debug_log::foreground_hwnd_raw()
    };
    let text = match proc_name {
        Some(proc_name) => format!("LEAP_SUPPRESSED reason={reason} proc={proc_name}"),
        None => format!("LEAP_SUPPRESSED reason={reason}"),
    };
    debug_log::record_event(DebugEvent::Note {
        ts_us: debug_log::now_us(),
        hwnd: resolved_hwnd,
        text,
    });
}

#[cfg(windows)]
unsafe fn query_foreground_overlay_candidate(
    our_pid: u32,
) -> Option<(ForegroundOverlayCandidate, u32)> {
    let fg = GetForegroundWindow();
    if fg.0.is_null() {
        return None;
    }
    let mut fg_pid = 0u32;
    let _ = GetWindowThreadProcessId(fg, Some(&mut fg_pid));
    if fg_pid == 0 || fg_pid == our_pid {
        return None;
    }
    let visible = IsWindowVisible(fg).as_bool();
    let minimized = IsIconic(fg).as_bool();
    let ex_style = GetWindowLongW(fg, GWL_EXSTYLE) as u32;
    let topmost = (ex_style & WS_EX_TOPMOST.0) != 0;
    if !visible || minimized || !topmost {
        return None;
    }
    let mut rect = RECT::default();
    if GetWindowRect(fg, &mut rect).is_err() {
        return None;
    }
    let cloaked = is_window_cloaked(fg);
    let candidate = ForegroundOverlayCandidate {
        hwnd: fg.0 as isize,
        pid: fg_pid,
        visible,
        minimized,
        cloaked,
        topmost,
        rect: RectBounds {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        },
    };
    Some((candidate, fg_pid))
}

#[cfg(windows)]
unsafe fn refresh_cached_overlay_suppression(our_pid: u32) {
    let Some((candidate, _)) = query_foreground_overlay_candidate(our_pid) else {
        CACHED_OVERLAY_SUPPRESSED.store(false, Ordering::Release);
        return;
    };
    let edge_monitors = if let Ok(layout) = controller_layout().try_lock() {
        collect_edge_monitor_bounds(
            &layout.main_monitors,
            &layout.seams,
            layout.remote_origin.is_some(),
            Edge::from_u8(LINK_FROM_EDGE.load(Ordering::Relaxed)),
        )
    } else {
        Vec::new()
    };
    let suppressed = should_suppress_leap_for_topmost_overlay(candidate, &edge_monitors, our_pid);
    CACHED_OVERLAY_SUPPRESSED.store(suppressed, Ordering::Release);
}

#[cfg(windows)]
unsafe fn check_foreground_overlay_suppresses_leap(
    our_pid: u32,
    edge_mon: RectBounds,
) -> Option<(HWND, String)> {
    let (candidate, fg_pid) = query_foreground_overlay_candidate(our_pid)?;
    if should_suppress_leap_for_topmost_overlay(candidate, &[edge_mon], our_pid) {
        let (_, proc_name) = query_process_info(fg_pid);
        Some((HWND(candidate.hwnd as *mut std::ffi::c_void), proc_name))
    } else {
        None
    }
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
unsafe fn abort_engage_perform(expected_gen: u32) -> bool {
    let accepted = try_apply_trap_event(TrapEvent::AbortEngage { expected_gen });
    if accepted {
        reset_dwell_and_drag_state();
        MOUSE_BUTTONS_DOWN.store(0, Ordering::Relaxed);
        KEY_CTRL_DOWN.store(false, Ordering::Relaxed);
        KEY_ALT_DOWN.store(false, Ordering::Relaxed);
    }
    accepted
}

#[cfg(windows)]
unsafe fn block_leap_for_elevated_foreground(
    fg: HWND,
    proc_name: &str,
    expected_gen: u32,
    log_enabled: bool,
) {
    let _ = abort_engage_perform(expected_gen);
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

#[cfg(any(windows, test))]
fn should_emit_timer_blocked_notice(
    has_active_link: bool,
    is_trapped: bool,
    elevated_foreground: bool,
    at_leap_edge: bool,
    now_ms: u64,
    last_emitted_ms: u64,
    cooldown_ms: u64,
) -> bool {
    has_active_link
        && !is_trapped
        && elevated_foreground
        && at_leap_edge
        && (last_emitted_ms == 0 || now_ms.saturating_sub(last_emitted_ms) >= cooldown_ms)
}

#[cfg(windows)]
fn is_point_at_active_leap_edge(pt: (i32, i32)) -> bool {
    let link_edge = Edge::from_u8(LINK_FROM_EDGE.load(Ordering::Relaxed));
    let deadzone = DEADZONE_PX.load(Ordering::Relaxed);
    let Ok(layout) = controller_layout().try_lock() else {
        return false;
    };
    if let Some(origin) = layout.remote_origin {
        detect_controller_edge_seams(
            pt,
            &layout.main_monitors,
            &layout.linked_monitors,
            origin,
            &layout.seams,
            deadzone,
        )
        .is_some()
    } else {
        detect_controller_edge(pt, &layout.main_monitors, link_edge, deadzone).is_some()
    }
}

#[cfg(windows)]
unsafe fn check_timer_elevated_leap_block(our_pid: u32) {
    let has_active_link = HAS_ACTIVE_LINK.load(Ordering::Acquire);
    let is_trapped = load_trap_state().phase != TrapPhase::Idle;
    if !has_active_link || is_trapped {
        return;
    }
    let Some((fg, proc_name)) = check_foreground_blocks_leap(our_pid) else {
        return;
    };
    let mut pt = POINT::default();
    if GetCursorPos(&mut pt).is_err() {
        return;
    }
    let at_edge = is_point_at_active_leap_edge((pt.x, pt.y));
    let now = now_millis();
    let prev = LAST_TIMER_BLOCKED_NOTICE_MS.load(Ordering::Relaxed);
    if !should_emit_timer_blocked_notice(
        has_active_link,
        is_trapped,
        true,
        at_edge,
        now,
        prev,
        TIMER_BLOCKED_NOTICE_COOLDOWN_MS,
    ) {
        return;
    }
    LAST_TIMER_BLOCKED_NOTICE_MS.store(now, Ordering::Relaxed);
    if debug_log::is_enabled() {
        debug_log::record_event(DebugEvent::Note {
            ts_us: debug_log::now_us(),
            hwnd: fg.0 as isize,
            text: format!("LEAP_BLOCKED reason=elevated_foreground source=timer proc={proc_name}"),
        });
    }
    if let Ok(guard) = leap_blocked_callback_slot().try_lock() {
        if let Some(ref cb) = *guard {
            cb(&proc_name);
        }
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
    super::dragdrop::cancel_active_target_drag_with_reason("emergency_release");
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

    LAST_HANDOFF_MS.store(now_millis(), Ordering::Relaxed);

    // Select at most one visible, non-cloaked, non-minimized, non-shell,
    // non-elevated top-level window in Z-order and make at most one
    // SetForegroundWindow attempt (never target Progman/desktop or minimize).
    let next_target = find_next_handoff_windows(fg, our_pid, 1).into_iter().next();

    let (final_method, final_to, api_name, api_ok, after, attempts, ok) = match next_target {
        Some(next) => {
            let set_fg_ok = SetForegroundWindow(next).as_bool();
            let after = GetForegroundWindow();
            let safe_ok = set_fg_ok
                && !after.0.is_null()
                && after != fg
                && is_window_safe_foreground(after, our_pid);
            (
                HandoffMethod::Next,
                if safe_ok { after } else { next },
                "SetForegroundWindow",
                set_fg_ok,
                after,
                1u32,
                safe_ok,
            )
        }
        None => {
            let after = GetForegroundWindow();
            (
                HandoffMethod::None,
                HWND::default(),
                "none",
                false,
                after,
                0u32,
                false,
            )
        }
    };

    if log_enabled {
        debug_log::record_event(DebugEvent::Note {
            ts_us: debug_log::now_us(),
            hwnd: final_to.0 as isize,
            text: format!(
                "FOCUS_HANDOFF from=0x{:X} to=0x{:X} method={} api={} api_ok={} after=0x{:X} attempts={} ok={ok}",
                fg.0 as isize,
                final_to.0 as isize,
                final_method.as_str(),
                api_name,
                api_ok,
                after.0 as isize,
                attempts,
            ),
        });
    }
}

#[cfg(windows)]
unsafe fn set_cursor_to_trap_center(
    center_x: i32,
    center_y: i32,
    reason: &'static str,
    generation: u32,
    log_enabled: bool,
) {
    let set_res = SetCursorPos(center_x, center_y);
    let set_ok = set_res.is_ok();
    let last_err = if set_ok { 0 } else { GetLastError().0 };
    if log_enabled {
        let mut actual = POINT::default();
        let actual_ok = GetCursorPos(&mut actual).is_ok();
        let (ax, ay) = if actual_ok {
            (actual.x, actual.y)
        } else {
            (i32::MIN, i32::MIN)
        };
        debug_log::record_event(DebugEvent::Note {
            ts_us: debug_log::now_us(),
            hwnd: debug_log::foreground_hwnd_raw(),
            text: format!(
                "TRAP_CENTER_SET reason={reason} center=({center_x},{center_y}) set_ok={} err={last_err} actual=({ax},{ay}) actual_ok={} gen={generation}",
                u8::from(set_ok),
                u8::from(actual_ok),
            ),
        });
    }
}

#[cfg(windows)]
unsafe fn place_return_cursor_and_complete(rx: i32, ry: i32, expected_gen: u32) -> bool {
    restore_cursor_visibility();
    let set_res = SetCursorPos(rx, ry);
    let set_ok = set_res.is_ok();
    let last_err = if set_ok { 0 } else { GetLastError().0 };
    let mut actual = POINT::default();
    let actual_ok = GetCursorPos(&mut actual).is_ok();

    arm_post_return_hysteresis(
        rx,
        ry,
        Edge::from_u8(RETURN_MAIN_EDGE.load(Ordering::Relaxed)),
        now_millis(),
    );

    if debug_log::is_enabled() {
        let (ax, ay) = if actual_ok {
            (actual.x, actual.y)
        } else {
            (i32::MIN, i32::MIN)
        };
        debug_log::record_event(DebugEvent::Note {
            ts_us: debug_log::now_us(),
            hwnd: debug_log::foreground_hwnd_raw(),
            text: format!(
                "RETURN_PLACED target=({rx},{ry}) set_ok={} err={last_err} actual=({ax},{ay}) actual_ok={} gen={expected_gen}",
                u8::from(set_ok),
                u8::from(actual_ok),
            ),
        });
    }

    try_apply_trap_event(TrapEvent::CompleteReturn { expected_gen })
}

#[cfg(windows)]
unsafe fn perform_return_cursor() -> Option<(i32, i32, u32)> {
    loop {
        let snap = load_trap_state();
        if snap.phase != TrapPhase::Returning {
            return None;
        }
        let (rx, ry) = unpack_pos(PENDING_RETURN_POS.load(Ordering::Acquire));
        if place_return_cursor_and_complete(rx, ry, snap.generation) {
            return Some((rx, ry, snap.generation));
        }
    }
}

#[cfg(windows)]
unsafe fn perform_engage_leap(expected_gen: u32) -> bool {
    let log_enabled = debug_log::is_enabled();
    if !HOOK_ACTIVE.load(Ordering::Relaxed) {
        let _ = abort_engage_perform(expected_gen);
        return false;
    }
    if !try_apply_trap_event(TrapEvent::BeginEngage { expected_gen }) {
        if log_enabled {
            let cur = load_trap_state();
            debug_log::record_event(DebugEvent::Note {
                ts_us: debug_log::now_us(),
                hwnd: debug_log::foreground_hwnd_raw(),
                text: format!(
                    "ENGAGE_ABORTED reason=stale_before_begin expected_gen={expected_gen} current_phase={} current_gen={}",
                    cur.phase.as_str(),
                    cur.generation,
                ),
            });
        }
        return false;
    }

    let our_pid = GetCurrentProcessId();
    let is_drag_leap = DRAG_MODE_ENGAGE.load(Ordering::Acquire);

    if should_debounce_engage_after_handoff(
        now_millis(),
        LAST_HANDOFF_MS.load(Ordering::Relaxed),
        ENGAGE_HANDOFF_DEBOUNCE_MS,
    ) {
        let _ = abort_engage_perform(expected_gen);
        if log_enabled {
            debug_log::record_event(DebugEvent::Note {
                ts_us: debug_log::now_us(),
                hwnd: debug_log::foreground_hwnd_raw(),
                text: "LEAP_SUPPRESSED reason=handoff_debounce source=engage".to_string(),
            });
        }
        return false;
    }

    if !is_drag_leap
        && (MOUSE_BUTTONS_DOWN.load(Ordering::Acquire) != 0 || any_async_mouse_button_down())
    {
        let _ = abort_engage_perform(expected_gen);
        if log_enabled {
            debug_log::record_event(DebugEvent::Note {
                ts_us: debug_log::now_us(),
                hwnd: debug_log::foreground_hwnd_raw(),
                text: "LEAP_SUPPRESSED reason=mouse_button_held source=engage".to_string(),
            });
        }
        return false;
    }

    if let Some((fg, proc_name)) = check_foreground_blocks_leap(our_pid) {
        block_leap_for_elevated_foreground(fg, &proc_name, expected_gen, log_enabled);
        return false;
    }

    let mx = TRAP_MON_X.load(Ordering::Relaxed);
    let my = TRAP_MON_Y.load(Ordering::Relaxed);
    let mw = TRAP_MON_W.load(Ordering::Relaxed);
    let mh = TRAP_MON_H.load(Ordering::Relaxed);
    let edge_mon = RectBounds {
        left: mx,
        top: my,
        right: mx.saturating_add(mw),
        bottom: my.saturating_add(mh),
    };

    if let Some((fg, proc_name)) = check_foreground_overlay_suppresses_leap(our_pid, edge_mon) {
        CACHED_OVERLAY_SUPPRESSED.store(true, Ordering::Release);
        let _ = abort_engage_perform(expected_gen);
        if log_enabled {
            LAST_SUPPRESSED_NOTE_MS.store(now_millis(), Ordering::Relaxed);
            debug_log::record_event(DebugEvent::Note {
                ts_us: debug_log::now_us(),
                hwnd: fg.0 as isize,
                text: format!(
                    "LEAP_SUPPRESSED reason=topmost_fullscreen_overlay source=engage proc={proc_name}"
                ),
            });
        }
        return false;
    }

    handoff_own_foreground_focus(log_enabled);

    // Guard again in case handoff changed focus to an elevated window or topmost overlay.
    if let Some((fg, proc_name)) = check_foreground_blocks_leap(our_pid) {
        block_leap_for_elevated_foreground(fg, &proc_name, expected_gen, log_enabled);
        return false;
    }
    if let Some((fg, proc_name)) = check_foreground_overlay_suppresses_leap(our_pid, edge_mon) {
        CACHED_OVERLAY_SUPPRESSED.store(true, Ordering::Release);
        let _ = abort_engage_perform(expected_gen);
        if log_enabled {
            LAST_SUPPRESSED_NOTE_MS.store(now_millis(), Ordering::Relaxed);
            debug_log::record_event(DebugEvent::Note {
                ts_us: debug_log::now_us(),
                hwnd: fg.0 as isize,
                text: format!(
                    "LEAP_SUPPRESSED reason=topmost_fullscreen_overlay source=post_handoff proc={proc_name}"
                ),
            });
        }
        return false;
    }
    CACHED_OVERLAY_SUPPRESSED.store(false, Ordering::Release);

    // Atomic commit gate: if a RETURN or release raced while we ran Win32 checks
    // or handoff, abort BEFORE touching ClipCursor/SetCursorPos/ShowCursor/CursorEnter.
    if !HOOK_ACTIVE.load(Ordering::Relaxed)
        || !try_apply_trap_event(TrapEvent::CommitEngage { expected_gen })
    {
        restore_cursor_visibility();
        if log_enabled {
            let cur = load_trap_state();
            debug_log::record_event(DebugEvent::Note {
                ts_us: debug_log::now_us(),
                hwnd: debug_log::foreground_hwnd_raw(),
                text: format!(
                    "ENGAGE_ABORTED reason=return_or_release_raced expected_gen={expected_gen} current_phase={} current_gen={}",
                    cur.phase.as_str(),
                    cur.generation,
                ),
            });
        }
        return false;
    }

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
    set_cursor_to_trap_center(
        center_x,
        center_y,
        "engage_perform",
        expected_gen,
        log_enabled,
    );
    let _ = apply_trap_event(TrapEvent::MarkCentered { expected_gen });

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
            (KeySnapshot::lock_failed(), KeySnapshot::lock_failed(), 0)
        };

    record_hook_tick();

    if log_enabled {
        debug_log::record_event(DebugEvent::EngagePerform {
            ts_us: debug_log::now_us(),
            edge: Edge::from_u8(PENDING_ENGAGE_LEAP_SIDE.load(Ordering::Relaxed)),
            relative_y,
            hwnd: fg_hwnd,
            tracker_before,
            released,
            send_input_us,
        });
    }

    let to_edge = Edge::from_u8(PENDING_ENGAGE_ENTRY_EDGE.load(Ordering::Relaxed));
    let dwell_ms = RESISTANCE_MS.load(Ordering::Relaxed) as u32;
    let deadzone_px = DEADZONE_PX.load(Ordering::Relaxed);
    let (target_monitor, along_px) = if PENDING_ENGAGE_HAS_SEAM.swap(false, Ordering::AcqRel) {
        let mon = pending_engage_target_mon()
            .try_lock()
            .ok()
            .map(|g| g.clone())
            .filter(|s| !s.is_empty());
        let along = mon
            .as_ref()
            .map(|_| PENDING_ENGAGE_ALONG_PX.load(Ordering::Relaxed));
        (mon, along)
    } else {
        (None, None)
    };
    try_send_packet(KvmPacket::CursorEnter {
        edge: to_edge.as_str().to_string(),
        relative_y,
        dwell_ms,
        deadzone_px,
        target_monitor,
        along_px,
    });
    // --- SPEC-25a ---
    if DRAG_MODE_ENGAGE.swap(false, Ordering::AcqRel) {
        let btn = DRAG_MODE_BUTTON.load(Ordering::Acquire);
        super::dragdrop::set_drag_leap_in_progress(false);
        let mut dispatched = false;
        if super::dragdrop::drag_drop_enabled() {
            if let Ok(g) = drag_mode_engaged_slot().try_lock() {
                if let Some(ref cb) = *g {
                    dispatched = true;
                    cb(btn);
                }
            }
        }
        debug_log::note(|| format!("DRAG_LEAP_DISPATCH_CB button={btn} dispatched={dispatched}"));
    }
    // --- end SPEC-25a ---
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
    let trapped = load_trap_state().phase.is_trapped_like();
    let log_enabled = debug_log::is_enabled();
    let ts_us = if log_enabled { debug_log::now_us() } else { 0 };
    let hwnd = if log_enabled {
        debug_log::foreground_hwnd_raw()
    } else {
        0
    };

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
        super::dragdrop::cancel_active_target_drag_with_reason("emergency_release");
        super::dragdrop::hide_edge_catcher();

        try_send_packet(KvmPacket::EmergencyRelease);
        try_send_packet(KvmPacket::ReleaseAll);
        return Some(LRESULT(0));
    }

    // --- SPEC-25b ---
    if super::media_keys::is_media_key(vk_code) {
        let target = super::media_keys::current_media_key_target_for_vk(vk_code, trapped);
        let (decision, res) = match target {
            super::media_keys::MediaKeyTarget::Linked => {
                try_send_packet(KvmPacket::MediaKey {
                    vk: vk_code,
                    is_down,
                });
                (KeyDecision::Forward, Some(LRESULT(1)))
            }
            super::media_keys::MediaKeyTarget::Local => (KeyDecision::Pass, None),
        };
        if log_enabled {
            debug_log::record_event(DebugEvent::Key {
                ts_us,
                vk: vk_code,
                scan_code,
                is_down,
                extended,
                injected: false,
                is_trapped: trapped,
                decision,
                hwnd,
            });
        }
        return res;
    }
    // --- /SPEC-25b ---

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
    fn trap_state_machine_return_wins_over_pending_engage_and_blocks_late_center() {
        let s0 = TrapState::new();
        assert_eq!(s0.phase, TrapPhase::Idle);
        assert!(!s0.can_emit_trapped_delta());
        assert!(!s0.can_recenter_cursor());

        // RequestEngage: Idle -> EngagePending(gen=1)
        let (s1, eff1) = s0.step(TrapEvent::RequestEngage);
        assert_eq!(
            eff1,
            TrapEffect::Accepted {
                prev_phase: TrapPhase::Idle,
                new_gen: 1,
            }
        );
        assert_eq!(s1.phase, TrapPhase::EngagePending);
        assert!(!s1.can_emit_trapped_delta());
        assert!(!s1.can_recenter_cursor());

        // Case A: Return arrives while WM_KVM_ENGAGE is still queued (before BeginEngage).
        let (s2, eff2) = s1.step(TrapEvent::ReturnRequested);
        assert_eq!(
            eff2,
            TrapEffect::Accepted {
                prev_phase: TrapPhase::EngagePending,
                new_gen: 2,
            }
        );
        assert_eq!(s2.phase, TrapPhase::Returning);
        assert!(!s2.can_recenter_cursor());

        // Stale WM_KVM_ENGAGE(gen=1) is rejected; cannot commit trap or center cursor.
        let (s2b, eff_begin) = s2.step(TrapEvent::BeginEngage { expected_gen: 1 });
        assert_eq!(eff_begin, TrapEffect::Rejected);
        assert_eq!(s2b, s2);
        let (s2c, eff_commit) = s2.step(TrapEvent::CommitEngage { expected_gen: 1 });
        assert_eq!(eff_commit, TrapEffect::Rejected);
        assert_eq!(s2c, s2);

        // CompleteReturn(gen=2) finishes placing cursor at seam edge -> Idle(gen=3).
        let (s3, eff3) = s2.step(TrapEvent::CompleteReturn { expected_gen: 2 });
        assert_eq!(
            eff3,
            TrapEffect::Accepted {
                prev_phase: TrapPhase::Returning,
                new_gen: 3,
            }
        );
        assert_eq!(s3.phase, TrapPhase::Idle);
        assert!(!s3.can_recenter_cursor());
    }

    #[test]
    fn trap_state_machine_return_during_inflight_engage_prevents_center_and_abort_clobber() {
        let (s1, _) = TrapState::new().step(TrapEvent::RequestEngage);
        // Hook thread enters perform_engage_leap(gen=1) and starts slow focus handoff.
        let (s2, eff_begin) = s1.step(TrapEvent::BeginEngage { expected_gen: 1 });
        assert!(matches!(eff_begin, TrapEffect::Accepted { .. }));

        // Concurrent CursorReturn arrives on Tokio thread during focus handoff.
        let (s3, eff_ret) = s2.step(TrapEvent::ReturnRequested);
        assert_eq!(s3.phase, TrapPhase::Returning);
        assert_eq!(
            eff_ret,
            TrapEffect::Accepted {
                prev_phase: TrapPhase::EngagePending,
                new_gen: 2,
            }
        );

        // Hook thread reaches CommitEngage(gen=1) OR AbortEngage(gen=1): BOTH must be rejected!
        let (s3a, eff_commit) = s3.step(TrapEvent::CommitEngage { expected_gen: 1 });
        assert_eq!(eff_commit, TrapEffect::Rejected);
        assert_eq!(s3a.phase, TrapPhase::Returning);
        let (s3b, eff_abort) = s3.step(TrapEvent::AbortEngage { expected_gen: 1 });
        assert_eq!(eff_abort, TrapEffect::Rejected);
        assert_eq!(s3b.phase, TrapPhase::Returning);

        // Late MarkCentered(gen=1) is also rejected when not Trapped(gen=1).
        let (s3c, eff_center) = s3.step(TrapEvent::MarkCentered { expected_gen: 1 });
        assert_eq!(eff_center, TrapEffect::Rejected);
        assert!(!s3c.cursor_centered);
    }

    #[test]
    fn trap_state_machine_full_engage_then_return_ordering() {
        let (s1, _) = TrapState::new().step(TrapEvent::RequestEngage);
        let (s2, _) = s1.step(TrapEvent::BeginEngage { expected_gen: 1 });
        let (s3, eff_commit) = s2.step(TrapEvent::CommitEngage { expected_gen: 1 });
        assert!(matches!(eff_commit, TrapEffect::Accepted { .. }));
        assert_eq!(s3.phase, TrapPhase::Trapped);
        assert!(!s3.can_emit_trapped_delta());
        assert!(s3.can_recenter_cursor());

        let (s4, eff_center) = s3.step(TrapEvent::MarkCentered { expected_gen: 1 });
        assert!(matches!(eff_center, TrapEffect::Accepted { .. }));
        assert!(s4.can_emit_trapped_delta());

        // Second RequestEngage while Trapped is rejected.
        let (_, eff_dup) = s4.step(TrapEvent::RequestEngage);
        assert_eq!(eff_dup, TrapEffect::Rejected);

        // ReturnRequested transitions Trapped -> Returning (clears cursor_centered).
        let (s5, eff_ret) = s4.step(TrapEvent::ReturnRequested);
        assert_eq!(
            eff_ret,
            TrapEffect::Accepted {
                prev_phase: TrapPhase::Trapped,
                new_gen: 2,
            }
        );
        assert_eq!(s5.phase, TrapPhase::Returning);
        assert!(!s5.cursor_centered);
        assert!(!s5.can_emit_trapped_delta());
        assert!(!s5.can_recenter_cursor());

        // New RequestEngage while Returning is rejected until CompleteReturn runs.
        let (_, eff_reengage) = s5.step(TrapEvent::RequestEngage);
        assert_eq!(eff_reengage, TrapEffect::Rejected);

        // CompleteReturn(gen=2) -> Idle(gen=3).
        let (s6, eff_done) = s5.step(TrapEvent::CompleteReturn { expected_gen: 2 });
        assert!(matches!(eff_done, TrapEffect::Accepted { .. }));
        assert_eq!(s6.phase, TrapPhase::Idle);

        // Idle + ReturnRequested (late/duplicate after watchdog) is rejected (!was_trapped).
        let (s7, eff_idle_ret) = s6.step(TrapEvent::ReturnRequested);
        assert_eq!(eff_idle_ret, TrapEffect::Rejected);
        assert_eq!(s7.phase, TrapPhase::Idle);
    }

    #[test]
    fn post_return_hysteresis_blocks_bounce_and_clears_on_escape_or_timeout() {
        let ret_pos = (2, 400);
        // Immediate bounce at same edge (e.g. 2 px away, 80 ms later) is suppressed.
        assert_eq!(
            evaluate_post_return_hysteresis(
                true,
                1080,
                1000,
                ret_pos,
                Edge::Left,
                (4, 410),
                Edge::Left,
                RETURN_REARM_COOLDOWN_MS,
                RETURN_REARM_ESCAPE_PX,
            ),
            PostReturnHysteresis::Suppress
        );
        // Sliding vertically along the edge without moving into Main PC stays suppressed.
        assert_eq!(
            evaluate_post_return_hysteresis(
                true,
                1100,
                1000,
                ret_pos,
                Edge::Left,
                (2, 500),
                Edge::Left,
                RETURN_REARM_COOLDOWN_MS,
                RETURN_REARM_ESCAPE_PX,
            ),
            PostReturnHysteresis::Suppress
        );
        // Moving >= 16 px into Main PC clears hysteresis early.
        assert_eq!(
            evaluate_post_return_hysteresis(
                true,
                1040,
                1000,
                ret_pos,
                Edge::Left,
                (18, 400),
                Edge::Left,
                RETURN_REARM_COOLDOWN_MS,
                RETURN_REARM_ESCAPE_PX,
            ),
            PostReturnHysteresis::ClearedByEscape
        );
        // Waiting >= 150 ms clears hysteresis by timeout.
        assert_eq!(
            evaluate_post_return_hysteresis(
                true,
                1150,
                1000,
                ret_pos,
                Edge::Left,
                (2, 400),
                Edge::Left,
                RETURN_REARM_COOLDOWN_MS,
                RETURN_REARM_ESCAPE_PX,
            ),
            PostReturnHysteresis::ClearedByTimeout
        );
        // Different edge clears hysteresis immediately.
        assert_eq!(
            evaluate_post_return_hysteresis(
                true,
                1010,
                1000,
                ret_pos,
                Edge::Left,
                (1918, 400),
                Edge::Right,
                RETURN_REARM_COOLDOWN_MS,
                RETURN_REARM_ESCAPE_PX,
            ),
            PostReturnHysteresis::ClearedByEdgeChange
        );
    }

    fn candidate(hwnd: isize, pid: u32, elevation: ProcessElevation) -> WindowCandidateInfo {
        WindowCandidateInfo {
            hwnd,
            pid,
            visible: true,
            cloaked: false,
            minimized: false,
            tool_window: false,
            is_shell_desktop: false,
            elevation,
        }
    }

    #[test]
    fn select_handoff_target_skips_shell_elevated_unknown_cloaked_and_ineligible_windows() {
        let our_pid = 1000;
        let fg_hwnd = 0x05;
        let standard = ProcessElevation::Standard {
            integrity_rid: SECURITY_MANDATORY_MEDIUM_RID,
        };
        let list = [
            candidate(fg_hwnd, 2000, standard),
            candidate(0x10, our_pid, standard),
            WindowCandidateInfo {
                is_shell_desktop: true,
                ..candidate(0x15, 1999, standard)
            },
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
        assert_eq!(
            select_handoff_target(&list, our_pid, fg_hwnd),
            Some((HandoffMethod::Next, 0x50))
        );
    }

    #[test]
    fn select_handoff_target_returns_none_when_only_shell_elevated_or_ineligible_exist() {
        let our_pid = 1000;
        let fg_hwnd = 0x05;
        let standard = ProcessElevation::Standard {
            integrity_rid: SECURITY_MANDATORY_MEDIUM_RID,
        };
        let list = [
            candidate(0, 2000, standard),
            candidate(fg_hwnd, 2000, standard),
            candidate(0x10, our_pid, standard),
            WindowCandidateInfo {
                is_shell_desktop: true,
                ..candidate(0x15, 1500, standard)
            },
            candidate(0x20, 3000, ProcessElevation::Elevated),
            candidate(0x30, 4000, ProcessElevation::Unknown),
        ];
        assert_eq!(select_handoff_target(&list, our_pid, fg_hwnd), None);
    }

    #[test]
    fn shell_and_desktop_window_classes_are_never_targeted() {
        assert!(is_shell_or_desktop_class("Progman"));
        assert!(is_shell_or_desktop_class("progman"));
        assert!(is_shell_or_desktop_class("WorkerW"));
        assert!(is_shell_or_desktop_class("Shell_TrayWnd"));
        assert!(is_shell_or_desktop_class("Shell_SecondaryTrayWnd"));
        assert!(!is_shell_or_desktop_class("Chrome_WidgetWin_1"));
        assert!(!is_shell_or_desktop_class(""));
    }

    #[test]
    fn topmost_fullscreen_overlay_rule_suppresses_only_non_anyleap_topmost_covering_edge_monitor() {
        let our_pid = 1000;
        let edge_mon = RectBounds {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let base = ForegroundOverlayCandidate {
            hwnd: 0x1234,
            pid: 2000,
            visible: true,
            minimized: false,
            cloaked: false,
            topmost: true,
            rect: edge_mon,
        };

        // Topmost overlay covering the exact edge monitor suppresses leap.
        assert!(should_suppress_leap_for_topmost_overlay(
            base,
            &[edge_mon],
            our_pid
        ));

        // Multi-monitor overlay spanning across multiple displays also covers the edge monitor.
        let multi_mon = ForegroundOverlayCandidate {
            rect: RectBounds {
                left: -1920,
                top: 0,
                right: 1920,
                bottom: 1080,
            },
            ..base
        };
        assert!(should_suppress_leap_for_topmost_overlay(
            multi_mon,
            &[edge_mon],
            our_pid
        ));

        // Own process (AnyLeap) is never suppressed by this rule.
        assert!(!should_suppress_leap_for_topmost_overlay(
            ForegroundOverlayCandidate {
                pid: our_pid,
                ..base
            },
            &[edge_mon],
            our_pid
        ));

        // Non-topmost fullscreen windows (e.g. normal maximized browser) still allow leaps.
        assert!(!should_suppress_leap_for_topmost_overlay(
            ForegroundOverlayCandidate {
                topmost: false,
                ..base
            },
            &[edge_mon],
            our_pid
        ));

        // Partial topmost windows (e.g. taskbar or small floating widget) still allow leaps.
        assert!(!should_suppress_leap_for_topmost_overlay(
            ForegroundOverlayCandidate {
                rect: RectBounds {
                    left: 0,
                    top: 1040,
                    right: 1920,
                    bottom: 1080,
                },
                ..base
            },
            &[edge_mon],
            our_pid
        ));

        // Hidden, minimized, or cloaked topmost windows do not suppress leaps.
        assert!(!should_suppress_leap_for_topmost_overlay(
            ForegroundOverlayCandidate {
                visible: false,
                ..base
            },
            &[edge_mon],
            our_pid
        ));
        assert!(!should_suppress_leap_for_topmost_overlay(
            ForegroundOverlayCandidate {
                minimized: true,
                ..base
            },
            &[edge_mon],
            our_pid
        ));
        assert!(!should_suppress_leap_for_topmost_overlay(
            ForegroundOverlayCandidate {
                cloaked: true,
                ..base
            },
            &[edge_mon],
            our_pid
        ));
    }

    #[test]
    fn collect_edge_monitor_bounds_picks_outer_edge_monitors() {
        let left_mon = MonitorDisplay {
            id: "mon-1".into(),
            device_name: "DISPLAY1".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            is_primary: true,
        };
        let right_mon = MonitorDisplay {
            id: "mon-2".into(),
            device_name: "DISPLAY2".into(),
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
            is_primary: false,
        };
        let bounds = collect_edge_monitor_bounds(
            &[left_mon.clone(), right_mon.clone()],
            &[],
            false,
            Edge::Left,
        );
        assert_eq!(bounds, vec![RectBounds::from_monitor(&left_mon)]);

        let seam = Seam {
            side: Edge::Left,
            main_monitor_id: "DISPLAY1".into(),
            linked_monitor_id: "LINKED1".into(),
            start: 0,
            end: 1080,
        };
        let seam_bounds =
            collect_edge_monitor_bounds(&[left_mon.clone(), right_mon], &[seam], true, Edge::Right);
        assert_eq!(seam_bounds, vec![RectBounds::from_monitor(&left_mon)]);
    }

    #[test]
    fn engage_debounce_suppresses_only_within_window_after_handoff() {
        let cd = ENGAGE_HANDOFF_DEBOUNCE_MS;
        assert_eq!(cd, 300);

        // No prior handoff -> never debounced.
        assert!(!should_debounce_engage_after_handoff(150, 0, cd));

        // Within 300 ms after handoff (e.g. 180 ms later) -> debounced.
        assert!(should_debounce_engage_after_handoff(1000, 1000, cd));
        assert!(should_debounce_engage_after_handoff(1180, 1000, cd));
        assert!(should_debounce_engage_after_handoff(1299, 1000, cd));

        // At or beyond 300 ms -> allowed again.
        assert!(!should_debounce_engage_after_handoff(1300, 1000, cd));
        assert!(!should_debounce_engage_after_handoff(2000, 1000, cd));
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
        assert_eq!(
            evaluate_capture_watchdog(true, Some(10_000), 10_000, th),
            None
        );
        // Hook tick recorded right after GetLastInputInfo() updated.
        assert_eq!(
            evaluate_capture_watchdog(true, Some(10_005), 10_000, th),
            None
        );
        // Boundary: exactly 1000 ms is not > 1000 ms.
        assert_eq!(
            evaluate_capture_watchdog(true, Some(10_000), 11_000, th),
            None
        );

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

    #[test]
    fn timer_blocked_notice_checks_conditions_and_5s_throttle() {
        let cd = TIMER_BLOCKED_NOTICE_COOLDOWN_MS;
        assert_eq!(cd, 5000);

        // First trigger emits even when now_ms < cooldown (last_emitted_ms == 0).
        assert!(should_emit_timer_blocked_notice(
            true, false, true, true, 250, 0, cd
        ));

        // Throttled for 5 seconds after last emission.
        assert!(!should_emit_timer_blocked_notice(
            true, false, true, true, 500, 250, cd
        ));
        assert!(!should_emit_timer_blocked_notice(
            true, false, true, true, 5249, 250, cd
        ));

        // Emits again once 5000 ms have elapsed.
        assert!(should_emit_timer_blocked_notice(
            true, false, true, true, 5250, 250, cd
        ));

        // Suppressed if link inactive, already trapped, foreground not elevated, or cursor not at edge.
        assert!(!should_emit_timer_blocked_notice(
            false, false, true, true, 10_000, 0, cd
        ));
        assert!(!should_emit_timer_blocked_notice(
            true, true, true, true, 10_000, 0, cd
        ));
        assert!(!should_emit_timer_blocked_notice(
            true, false, false, true, 10_000, 0, cd
        ));
        assert!(!should_emit_timer_blocked_notice(
            true, false, true, false, 10_000, 0, cd
        ));
    }

    #[test]
    fn counts_as_physical_gates_tagged_injected_input() {
        const INJ: u32 = 1;
        const LOWER: u32 = 2;
        let tag = ANYLEAP_E2E_INPUT_TAG;
        assert!(counts_as_physical(0, 0, false));
        assert!(counts_as_physical(0, 0, true));
        // untagged injected: rejected regardless of flag
        assert!(!counts_as_physical(INJ, 0, false));
        assert!(!counts_as_physical(INJ, 0, true));
        assert!(!counts_as_physical(INJ, tag + 1, true));
        // tagged, flag off: rejected
        assert!(!counts_as_physical(INJ, tag, false));
        // tagged, flag on: accepted
        assert!(counts_as_physical(INJ, tag, true));
        // lower-IL: accepted only when LLMHF_INJECTED is also set
        assert!(counts_as_physical(INJ | LOWER, tag, true));
        assert!(!counts_as_physical(LOWER, tag, true));
        assert!(!counts_as_physical(INJ | LOWER, tag, false));
    }

    // --- SPEC-25a ---
    #[test]
    fn drag_leap_edge_helpers_and_injected_guard() {
        assert_eq!(drag_leap_edge_tolerance(2), 4);
        assert_eq!(drag_leap_edge_tolerance(0), 4);

        const LLMHF_INJECTED: u32 = 1;
        assert!(should_ignore_injected_mouse_hook_event(
            LLMHF_INJECTED,
            true,
            false
        ));
        assert!(!should_ignore_injected_mouse_hook_event(0, true, false));
        assert!(!should_ignore_injected_mouse_hook_event(
            LLMHF_INJECTED,
            false,
            false
        ));
        assert!(!should_ignore_injected_mouse_hook_event(
            LLMHF_INJECTED,
            true,
            true
        ));

        assert!(is_physical_mouse_hook_event(0));
        assert!(!is_physical_mouse_hook_event(LLMHF_INJECTED));
        assert!(!is_physical_mouse_hook_event(0x0000_0002));
        assert!(!is_physical_mouse_hook_event(0x0000_0003));

        assert_eq!(button_from_mouse_up_msg(0x0202), Some(1)); // WM_LBUTTONUP
        assert_eq!(button_from_mouse_up_msg(0x0205), Some(2)); // WM_RBUTTONUP
        assert_eq!(button_from_mouse_up_msg(0x0208), Some(3)); // WM_MBUTTONUP
        assert_eq!(button_from_mouse_up_msg(0x0200), None);

        assert!(!HookController::can_engage_drag_leap(
            false, true, false, true, 1
        ));
        assert!(HookController::can_engage_drag_leap(
            true, true, false, true, 1
        ));
        assert!(!HookController::can_engage_drag_leap(
            true, true, true, true, 1
        ));
        assert!(!HookController::can_engage_drag_leap(
            true, true, false, true, 0
        ));
    }
    // --- end SPEC-25a ---
}
