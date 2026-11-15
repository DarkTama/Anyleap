//! Cross-PC OLE drag-and-drop edge catcher, drag-mode settings, and target-side DoDragDrop (SPEC-25a).

use super::input_logic::{Edge, Seam};
use super::monitors::MonitorDisplay;
use super::protocol::{DragOutcome, LinkedSeam};
use crate::state::AppState;
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, State};

pub const DEFAULT_DRAG_DROP_ENABLED: bool = false;

static DRAG_DROP_ENABLED: AtomicBool = AtomicBool::new(DEFAULT_DRAG_DROP_ENABLED);
static FILE_CLIPBOARD_ENABLED: AtomicBool = AtomicBool::new(true);
static POINTER_SCHEME_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn drag_drop_enabled() -> bool {
    DRAG_DROP_ENABLED.load(Ordering::Relaxed)
}

pub fn file_clipboard_enabled() -> bool {
    FILE_CLIPBOARD_ENABLED.load(Ordering::Relaxed)
}

pub fn pointer_scheme_enabled() -> bool {
    POINTER_SCHEME_ENABLED.load(Ordering::Relaxed)
}

pub fn set_files_settings(
    drag_drop: bool,
    file_clipboard: bool,
    staging_limit_mb: u32,
    pointer_scheme: bool,
    clip_prefetch_limit_mb: u32,
) {
    DRAG_DROP_ENABLED.store(drag_drop, Ordering::SeqCst);
    FILE_CLIPBOARD_ENABLED.store(file_clipboard, Ordering::SeqCst);
    super::staging::set_staging_limit_mb(staging_limit_mb);
    super::staging::set_clip_prefetch_limit_mb(clip_prefetch_limit_mb);
    POINTER_SCHEME_ENABLED.store(pointer_scheme, Ordering::SeqCst);
    if !drag_drop {
        DRAG_LEAP_IN_PROGRESS.store(false, Ordering::Release);
        hide_edge_catcher();
        cancel_active_target_drag_with_reason("disabled");
    } else if edge_config().lock().is_ok_and(|g| !g.strips.is_empty()) {
        ensure_edge_catcher_thread();
    }
    post_edge_catcher_msg(WM_EC_RELOAD_STRIPS);
}

#[tauri::command]
pub async fn kvm_set_files_settings(
    drag_drop: bool,
    file_clipboard: bool,
    staging_limit_mb: u32,
    pointer_scheme: bool,
    clip_prefetch_limit_mb: Option<u32>,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let prev_pointer = pointer_scheme_enabled();
    set_files_settings(
        drag_drop,
        file_clipboard,
        staging_limit_mb,
        pointer_scheme,
        clip_prefetch_limit_mb.unwrap_or(super::staging::DEFAULT_CLIP_PREFETCH_LIMIT_MB),
    );

    if prev_pointer != pointer_scheme {
        super::commands::on_pointer_scheme_setting_changed(&state, &app_handle, pointer_scheme);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Pure seam-strip & trigger decision helpers (unit-tested)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeamStrip {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub side: Edge,
}

/// Compute a `thickness`-px strip inside `mon` along `side` spanning `[start, end)`.
pub fn compute_seam_strip(
    mon: &MonitorDisplay,
    side: Edge,
    start: i32,
    end: i32,
    thickness: i32,
) -> Option<SeamStrip> {
    if mon.width <= 0 || mon.height <= 0 || end <= start {
        return None;
    }
    let t = thickness.max(1);
    match side {
        Edge::Right => {
            let y0 = start.max(mon.y);
            let y1 = end.min(mon.y + mon.height);
            let h = y1 - y0;
            (h > 0).then(|| SeamStrip {
                x: mon.x + mon.width - t.min(mon.width),
                y: y0,
                width: t.min(mon.width),
                height: h,
                side,
            })
        }
        Edge::Left => {
            let y0 = start.max(mon.y);
            let y1 = end.min(mon.y + mon.height);
            let h = y1 - y0;
            (h > 0).then(|| SeamStrip {
                x: mon.x,
                y: y0,
                width: t.min(mon.width),
                height: h,
                side,
            })
        }
        Edge::Top => {
            let x0 = start.max(mon.x);
            let x1 = end.min(mon.x + mon.width);
            let w = x1 - x0;
            (w > 0).then(|| SeamStrip {
                x: x0,
                y: mon.y,
                width: w,
                height: t.min(mon.height),
                side,
            })
        }
        Edge::Bottom => {
            let x0 = start.max(mon.x);
            let x1 = end.min(mon.x + mon.width);
            let w = x1 - x0;
            (w > 0).then(|| SeamStrip {
                x: x0,
                y: mon.y + mon.height - t.min(mon.height),
                width: w,
                height: t.min(mon.height),
                side,
            })
        }
    }
}

pub fn compute_main_seam_strips(
    main_monitors: &[MonitorDisplay],
    seams: &[Seam],
) -> Vec<SeamStrip> {
    seams
        .iter()
        .filter_map(|s| {
            let mon = main_monitors
                .iter()
                .find(|m| m.effective_id() == s.main_monitor_id)?;
            compute_seam_strip(mon, s.side, s.start, s.end, 2)
        })
        .collect()
}

pub fn compute_linked_seam_strips(
    linked_monitors: &[MonitorDisplay],
    seams: &[LinkedSeam],
) -> Vec<SeamStrip> {
    seams
        .iter()
        .filter_map(|s| {
            let side = Edge::from_str(&s.side)?;
            let mon = linked_monitors
                .iter()
                .find(|m| m.effective_id() == s.linked_monitor_id)?;
            compute_seam_strip(mon, side, s.start, s.end, 2)
        })
        .collect()
}

pub fn is_cursor_near_seam_strip(pt: (i32, i32), strip: &SeamStrip, margin_px: i32) -> bool {
    let m = margin_px.max(0);
    pt.0 >= strip.x - m
        && pt.0 < strip.x + strip.width + m
        && pt.1 >= strip.y - m
        && pt.1 < strip.y + strip.height + m
}

/// Pure decision: should the invisible 2-px edge-catcher windows be shown?
pub fn should_show_edge_catcher(
    drag_drop_enabled: bool,
    has_active_strips: bool,
    is_trapped: bool,
    mouse_buttons_down: u8,
    drag_leap_in_progress: bool,
    local_drag_source_active: bool,
) -> bool {
    drag_drop_enabled
        && has_active_strips
        && !is_trapped
        && mouse_buttons_down != 0
        && !drag_leap_in_progress
        && !local_drag_source_active
}

pub fn button_from_grf_key_state(grf_key_state: u32) -> u8 {
    if (grf_key_state & 0x0002) != 0 {
        2 // MK_RBUTTON
    } else if (grf_key_state & 0x0010) != 0 {
        3 // MK_MBUTTON
    } else {
        1 // MK_LBUTTON
    }
}

pub fn vk_for_mouse_button(button: u8) -> u16 {
    match button {
        2 => 0x02, // VK_RBUTTON
        3 => 0x04, // VK_MBUTTON
        _ => 0x01, // VK_LBUTTON
    }
}

pub fn mk_flag_for_mouse_button(button: u8) -> u32 {
    match button {
        2 => 0x0002, // MK_RBUTTON
        3 => 0x0010, // MK_MBUTTON
        _ => 0x0001, // MK_LBUTTON
    }
}

/// Pure decision: whether the target PC must inject its own synthetic mouse-button
/// press for an incoming cross-PC drag once staging finishes (`DoDragDrop`).
/// On Linked PC (receiver) it was idle; on Main PC (controller) the physical
/// press was swallowed by `low_level_mouse_proc` while trapped, so both require
/// target-owned synthetic injection when `DoDragDrop` begins.
pub fn should_use_forwarded_drag_button_inject(was_trapped_or_receiver: bool) -> bool {
    was_trapped_or_receiver
}

pub const TARGET_DRAG_NEITHER_BTN_CANCEL_MS: u64 = 300;
pub const TARGET_DRAG_MAX_DURATION_MS: u64 = 60_000;
pub const TARGET_DRAG_QUERY_STALL_MS: u64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetDragDecision {
    Continue,
    Drop,
    Cancel(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchdogEvalInput {
    pub elapsed_ms: u64,
    pub in_drag_loop: bool,
    pub since_last_query_ms: u64,
    pub cancel_reason: Option<&'static str>,
    pub track_remote_button: bool,
    pub remote_button_down: bool,
    pub remote_up_ms: Option<u64>,
    pub physical_button_down: bool,
    pub neither_down_ms: Option<u64>,
    pub max_duration_ms: u64,
    pub neither_down_timeout_ms: u64,
    pub query_stall_timeout_ms: u64,
}

/// Pure decision for the independent watchdog thread monitoring target-side `DoDragDrop`.
/// Guarantees termination even when OLE stops calling `QueryContinueDrag`.
pub fn decide_watchdog_cancel(input: WatchdogEvalInput) -> Option<&'static str> {
    if let Some(reason) = input.cancel_reason {
        return Some(reason);
    }
    if input.max_duration_ms > 0 && input.elapsed_ms >= input.max_duration_ms {
        return Some("timeout");
    }
    if input.track_remote_button
        && !input.remote_button_down
        && input
            .remote_up_ms
            .is_some_and(|ms| ms > input.neither_down_timeout_ms)
    {
        return Some("remote_button_up_timeout");
    }
    if !input.physical_button_down
        && !input.remote_button_down
        && input
            .neither_down_ms
            .is_some_and(|ms| ms > input.neither_down_timeout_ms)
    {
        return Some("no_buttons_down_timeout");
    }
    if input.in_drag_loop && input.since_last_query_ms > input.query_stall_timeout_ms {
        return Some("query_stall_timeout");
    }
    None
}

/// Pure guard: Escape keystroke injection may only be attempted when AnyLeap's
/// drag window/thread currently owns foreground or mouse capture.
pub fn should_inject_escape_on_drag_timeout(drag_is_foreground_or_has_capture: bool) -> bool {
    drag_is_foreground_or_has_capture
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TargetDragQueryInput {
    pub escape_pressed: bool,
    pub cancel_reason: Option<&'static str>,
    pub track_remote_button: bool,
    pub remote_button_down: bool,
    pub physical_button_down: bool,
    pub seen_button_down: bool,
    pub elapsed_ms: u64,
    pub neither_down_ms: Option<u64>,
    pub max_duration_ms: u64,
    pub neither_down_timeout_ms: u64,
}

/// Pure decision for `IDropSource::QueryContinueDrag`: checks whether the drag
/// button is still considered held.
pub fn is_drag_button_held_in_query(
    track_remote_button: bool,
    remote_button_down: bool,
    grf_key_state: u32,
    expected_mk: u32,
) -> bool {
    if track_remote_button {
        remote_button_down
    } else {
        (grf_key_state & expected_mk) != 0
    }
}

/// Pure termination decision for target-side `DoDragDrop`.
/// Cancels on Escape, explicit cancel/disconnect signals, overall timeout,
/// or when neither the physical nor tracked remote button is down for > 300 ms.
pub fn decide_target_drag_continue(input: TargetDragQueryInput) -> TargetDragDecision {
    if input.escape_pressed {
        return TargetDragDecision::Cancel("escape");
    }
    if let Some(reason) = input.cancel_reason {
        return TargetDragDecision::Cancel(reason);
    }
    if input.max_duration_ms > 0 && input.elapsed_ms >= input.max_duration_ms {
        return TargetDragDecision::Cancel("timeout");
    }
    if !input.physical_button_down && !input.remote_button_down {
        if input
            .neither_down_ms
            .is_some_and(|ms| ms > input.neither_down_timeout_ms)
        {
            return TargetDragDecision::Cancel("no_buttons_down_timeout");
        }
        if !input.track_remote_button && input.seen_button_down {
            return TargetDragDecision::Drop;
        }
        return TargetDragDecision::Continue;
    }
    if input.track_remote_button && !input.remote_button_down {
        return TargetDragDecision::Drop;
    }
    TargetDragDecision::Continue
}

/// Pure decision: whether `run_target_drag_drop` must emit a matching synthetic
/// button-up when exiting `DoDragDrop` (whenever we injected a synthetic down).
pub fn should_release_target_drag_on_exit(
    forwarded_button_inject: bool,
    injected_down: bool,
) -> bool {
    forwarded_button_inject && injected_down
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchdogCancelAction {
    pub msg: u32,
    pub wparam: usize,
}

/// Messages posted by the forced-termination watchdog / cancel path.
/// Never includes synthetic mouse button-up messages (`WM_LBUTTONUP`, etc.),
/// so forced termination can only cancel `DoDragDrop` and never trigger a drop.
pub fn watchdog_cancel_messages() -> [WatchdogCancelAction; 4] {
    [
        WatchdogCancelAction {
            msg: 0x001F,
            wparam: 0,
        }, // WM_CANCELMODE
        WatchdogCancelAction {
            msg: 0x0100,
            wparam: 0x1B,
        }, // WM_KEYDOWN (VK_ESCAPE)
        WatchdogCancelAction {
            msg: 0x0101,
            wparam: 0x1B,
        }, // WM_KEYUP (VK_ESCAPE)
        WatchdogCancelAction {
            msg: 0x0200,
            wparam: 0,
        }, // WM_MOUSEMOVE
    ]
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetDragResult {
    pub outcome: DragOutcome,
    pub effect: u32,
    pub reason: &'static str,
    pub notice: Option<String>,
}

/// Pure classification of target-side `DoDragDrop` exit into terminal outcome
/// and optional user-facing continuity notice.
pub fn classify_target_drag_outcome(
    hr_u32: u32,
    effect_u32: u32,
    reason: &str,
) -> (DragOutcome, Option<String>) {
    const DRAGDROP_S_DROP_U32: u32 = 0x0004_0100;
    if hr_u32 == DRAGDROP_S_DROP_U32 {
        if effect_u32 != 0 {
            return (DragOutcome::Done, None);
        }
        return (
            DragOutcome::Failed,
            Some("Drop was not accepted by the target app".to_string()),
        );
    }
    if matches!(
        reason,
        "escape"
            | "drag_cancel"
            | "transfer_cancel"
            | "local_cancel"
            | "released_before_start"
            | "release_all"
            | "emergency_release"
            | "disconnect"
            | "superseded"
            | "disabled"
    ) {
        return (DragOutcome::Cancelled, None);
    }
    match reason {
        "hittest_blocked" => (
            DragOutcome::Failed,
            Some("Drag could not start because another window covered the cursor".to_string()),
        ),
        "remote_button_up_timeout" | "no_buttons_down_timeout" => (
            DragOutcome::Failed,
            Some("Drag failed: mouse button state was lost before drop".to_string()),
        ),
        "timeout" | "query_stall_timeout" => (
            DragOutcome::Failed,
            Some("Drag timed out before drop".to_string()),
        ),
        _ if (hr_u32 & 0x8000_0000) != 0 || matches!(reason, "ole_error" | "panic") => (
            DragOutcome::Failed,
            Some(format!("Drag failed (OLE error 0x{hr_u32:08X})")),
        ),
        _ => (DragOutcome::Cancelled, None),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragStage {
    Staging,
    WaitingForDrop,
    Finished(DragOutcome),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DragProgressPayload {
    pub id: String,
    pub name: String,
    pub pct: u8,
    pub state: &'static str,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelDragAction {
    NotTracked,
    Cancelled(DragProgressPayload),
    ClearedFinished,
}

#[derive(Debug, Clone)]
struct DragProgressEntry {
    name: String,
    total_bytes: u64,
    stage: DragStage,
    terminal_emitted: bool,
}

#[derive(Default)]
struct DragRegistryInner {
    entries: HashMap<String, DragProgressEntry>,
    order: VecDeque<String>,
}

const MAX_TRACKED_DRAGS: usize = 128;

pub struct DragTransferRegistry {
    inner: Mutex<DragRegistryInner>,
}

impl Default for DragTransferRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DragTransferRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(DragRegistryInner::default()),
        }
    }

    pub fn register_staging(&self, id: &str, name: &str, total_bytes: u64) -> DragProgressPayload {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if !guard.entries.contains_key(id) {
            guard.order.push_back(id.to_string());
            while guard.entries.len() >= MAX_TRACKED_DRAGS {
                let evict_idx = guard.order.iter().position(|k| {
                    guard
                        .entries
                        .get(k)
                        .is_some_and(|e| matches!(e.stage, DragStage::Finished(_)))
                });
                let Some(idx) = evict_idx else {
                    break;
                };
                if let Some(old_id) = guard.order.remove(idx) {
                    guard.entries.remove(&old_id);
                }
            }
        }
        guard.entries.insert(
            id.to_string(),
            DragProgressEntry {
                name: name.to_string(),
                total_bytes,
                stage: DragStage::Staging,
                terminal_emitted: false,
            },
        );
        DragProgressPayload {
            id: id.to_string(),
            name: name.to_string(),
            pct: 0,
            state: "active",
            size_bytes: total_bytes,
        }
    }

    pub fn mark_waiting_for_drop(&self, id: &str) -> Option<DragProgressPayload> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = guard.entries.get_mut(id)?;
        if matches!(entry.stage, DragStage::Finished(_)) || entry.terminal_emitted {
            return None;
        }
        entry.stage = DragStage::WaitingForDrop;
        Some(DragProgressPayload {
            id: id.to_string(),
            name: entry.name.clone(),
            pct: 99,
            state: "active",
            size_bytes: entry.total_bytes,
        })
    }

    pub fn complete_terminal(&self, id: &str, outcome: DragOutcome) -> Option<DragProgressPayload> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let entry = guard.entries.get_mut(id)?;
        if entry.terminal_emitted {
            return None;
        }
        entry.stage = DragStage::Finished(outcome);
        entry.terminal_emitted = true;
        Some(DragProgressPayload {
            id: id.to_string(),
            name: entry.name.clone(),
            pct: if outcome == DragOutcome::Done { 100 } else { 0 },
            state: outcome.as_str(),
            size_bytes: entry.total_bytes,
        })
    }

    pub fn cancel_at_any_stage(&self, id: &str) -> CancelDragAction {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = guard.entries.get_mut(id) else {
            return CancelDragAction::NotTracked;
        };
        if matches!(entry.stage, DragStage::Finished(_)) && entry.terminal_emitted {
            guard.entries.remove(id);
            guard.order.retain(|k| k != id);
            return CancelDragAction::ClearedFinished;
        }
        entry.stage = DragStage::Finished(DragOutcome::Cancelled);
        entry.terminal_emitted = true;
        CancelDragAction::Cancelled(DragProgressPayload {
            id: id.to_string(),
            name: entry.name.clone(),
            pct: 0,
            state: DragOutcome::Cancelled.as_str(),
            size_bytes: entry.total_bytes,
        })
    }

    pub fn drain_active(&self) -> Vec<DragProgressPayload> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::new();
        for (id, entry) in guard.entries.iter_mut() {
            if !matches!(entry.stage, DragStage::Finished(_)) && !entry.terminal_emitted {
                entry.stage = DragStage::Finished(DragOutcome::Cancelled);
                entry.terminal_emitted = true;
                out.push(DragProgressPayload {
                    id: id.clone(),
                    name: entry.name.clone(),
                    pct: 0,
                    state: DragOutcome::Cancelled.as_str(),
                    size_bytes: entry.total_bytes,
                });
            }
        }
        out
    }

    pub fn is_finished(&self, id: &str) -> bool {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .entries
            .get(id)
            .is_some_and(|e| matches!(e.stage, DragStage::Finished(_)))
    }

    pub fn get_stage(&self, id: &str) -> Option<DragStage> {
        let guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        guard.entries.get(id).map(|e| e.stage)
    }
}

pub fn global_drag_registry() -> &'static DragTransferRegistry {
    static REGISTRY: OnceLock<DragTransferRegistry> = OnceLock::new();
    REGISTRY.get_or_init(DragTransferRegistry::new)
}

#[cfg(test)]
pub(crate) fn test_files_settings_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------------------
// Shared edge-catcher & target drag runtime state
// ---------------------------------------------------------------------------

pub type DragCaughtCallback = Arc<dyn Fn(Vec<PathBuf>, u8, (i32, i32)) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DndStateSnapshot {
    pub target_tid: u32,
    pub local_src_active: bool,
    pub leap_in_progress: bool,
    pub catcher_visible: bool,
    pub remote_btn_down: bool,
}

static EDGE_CATCHER_VISIBLE: AtomicBool = AtomicBool::new(false);
static DRAG_LEAP_IN_PROGRESS: AtomicBool = AtomicBool::new(false);
static LOCAL_DRAG_SOURCE_ACTIVE: AtomicBool = AtomicBool::new(false);
static REMOTE_DRAG_BUTTON_DOWN: AtomicBool = AtomicBool::new(false);
static REMOTE_DRAG_BUTTON: AtomicU8 = AtomicU8::new(1);
static TARGET_DRAG_CANCELLED: AtomicBool = AtomicBool::new(false);
static PENDING_TARGET_DRAG_EARLY_RELEASE: AtomicBool = AtomicBool::new(false);
static EDGE_POLL_ENABLED: AtomicBool = AtomicBool::new(false);
static EDGE_POLL_RECEIVER_ENTERED: AtomicBool = AtomicBool::new(false);
static EDGE_CATCHER_TID: AtomicU32 = AtomicU32::new(0);
static TARGET_DRAG_TID: AtomicU32 = AtomicU32::new(0);
static TARGET_DRAG_HWND: AtomicIsize = AtomicIsize::new(0);
static NEXT_TARGET_DRAG_SEQ: AtomicU64 = AtomicU64::new(1);
static GLOBAL_CANCEL_UP_TO_SEQ: AtomicU64 = AtomicU64::new(0);
static GLOBAL_CANCEL_REASON_CODE: AtomicU8 = AtomicU8::new(0);

const WM_APP_WAKE_DRAG: u32 = 0x8000 + 1;

fn cancel_reason_to_code(reason: &str) -> u8 {
    match reason {
        "drag_cancel" => 1,
        "transfer_cancel" => 2,
        "release_all" => 3,
        "emergency_release" => 4,
        "disconnect" => 5,
        "superseded" => 6,
        "disabled" => 8,
        _ => 7,
    }
}

fn cancel_reason_from_code(code: u8) -> &'static str {
    match code {
        1 => "drag_cancel",
        2 => "transfer_cancel",
        3 => "release_all",
        4 => "emergency_release",
        5 => "disconnect",
        6 => "superseded",
        8 => "disabled",
        _ => "local_cancel",
    }
}

struct TargetDragControl {
    seq: u64,
    drag_id: String,
    button: u8,
    track_remote_button: bool,
    started_at: Instant,
    tid: AtomicU32,
    hwnd: AtomicIsize,
    remote_button_down: AtomicBool,
    cancelled: AtomicBool,
    seen_button_down: AtomicBool,
    in_drag_loop: AtomicBool,
    watchdog_stopped: AtomicBool,
    last_query_ms: AtomicU64,
    remote_up_since_ms: AtomicU64,
    neither_down_since_ms: AtomicU64,
    cancel_nudges: AtomicU32,
    cancel_reason: Mutex<Option<&'static str>>,
    decision_reason: Mutex<Option<&'static str>>,
    done_lock: Mutex<bool>,
    done_cv: Condvar,
}

impl TargetDragControl {
    fn new(seq: u64, drag_id: String, button: u8, track_remote_button: bool) -> Self {
        Self {
            seq,
            drag_id,
            button,
            track_remote_button,
            started_at: Instant::now(),
            tid: AtomicU32::new(0),
            hwnd: AtomicIsize::new(0),
            remote_button_down: AtomicBool::new(track_remote_button),
            cancelled: AtomicBool::new(false),
            seen_button_down: AtomicBool::new(false),
            in_drag_loop: AtomicBool::new(false),
            watchdog_stopped: AtomicBool::new(false),
            last_query_ms: AtomicU64::new(0),
            remote_up_since_ms: AtomicU64::new(u64::MAX),
            neither_down_since_ms: AtomicU64::new(u64::MAX),
            cancel_nudges: AtomicU32::new(0),
            cancel_reason: Mutex::new(None),
            decision_reason: Mutex::new(None),
            done_lock: Mutex::new(false),
            done_cv: Condvar::new(),
        }
    }

    fn mark_drag_loop_entered(&self) {
        let now_ms = self.started_at.elapsed().as_millis() as u64;
        self.last_query_ms.store(now_ms, Ordering::Release);
        self.in_drag_loop.store(true, Ordering::Release);
    }

    fn note_query_continue(&self) -> u64 {
        let now_ms = self.started_at.elapsed().as_millis() as u64;
        self.last_query_ms.store(now_ms, Ordering::Release);
        now_ms
    }

    fn update_remote_up(&self, remote_down: bool, now_ms: u64) -> Option<u64> {
        if !self.track_remote_button || remote_down {
            self.remote_up_since_ms.store(u64::MAX, Ordering::Release);
            None
        } else {
            let prev = self.remote_up_since_ms.compare_exchange(
                u64::MAX,
                now_ms,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            let since = match prev {
                Ok(_) => now_ms,
                Err(existing) => existing,
            };
            Some(now_ms.saturating_sub(since))
        }
    }

    fn request_cancel(&self, reason: &'static str) {
        self.cancelled.store(true, Ordering::Release);
        self.remote_button_down.store(false, Ordering::Release);
        if let Ok(mut g) = self.cancel_reason.try_lock() {
            if g.is_none() {
                *g = Some(reason);
            }
        }
    }

    fn effective_cancel_reason(&self) -> Option<&'static str> {
        if self.cancelled.load(Ordering::Acquire) {
            if let Ok(g) = self.cancel_reason.try_lock() {
                if let Some(r) = *g {
                    return Some(r);
                }
            }
            return Some("local_cancel");
        }
        if GLOBAL_CANCEL_UP_TO_SEQ.load(Ordering::Acquire) >= self.seq {
            let code = GLOBAL_CANCEL_REASON_CODE.load(Ordering::Acquire);
            return Some(cancel_reason_from_code(code));
        }
        None
    }

    fn set_decision_reason(&self, reason: &'static str) {
        if let Ok(mut g) = self.decision_reason.try_lock() {
            if g.is_none() {
                *g = Some(reason);
            }
        }
    }

    fn take_decision_reason(&self) -> Option<&'static str> {
        self.decision_reason.lock().ok().and_then(|mut g| g.take())
    }

    fn update_neither_down(
        &self,
        physical_down: bool,
        remote_down: bool,
        now_ms: u64,
    ) -> Option<u64> {
        if physical_down || remote_down {
            self.seen_button_down.store(true, Ordering::Release);
            self.neither_down_since_ms
                .store(u64::MAX, Ordering::Release);
            None
        } else {
            let prev = self.neither_down_since_ms.compare_exchange(
                u64::MAX,
                now_ms,
                Ordering::AcqRel,
                Ordering::Acquire,
            );
            let since = match prev {
                Ok(_) => now_ms,
                Err(existing) => existing,
            };
            Some(now_ms.saturating_sub(since))
        }
    }

    fn stop_watchdog(&self) {
        if let Ok(done) = self.done_lock.lock() {
            self.watchdog_stopped.store(true, Ordering::Release);
            self.done_cv.notify_all();
            drop(done);
        } else {
            self.watchdog_stopped.store(true, Ordering::Release);
        }
    }

    fn wait_watchdog_tick(&self, timeout: Duration) -> bool {
        let Ok(mut done) = self.done_lock.lock() else {
            return true;
        };
        if *done || self.watchdog_stopped.load(Ordering::Acquire) {
            return true;
        }
        let (guard, _) = match self.done_cv.wait_timeout(done, timeout) {
            Ok(v) => v,
            Err(e) => e.into_inner(),
        };
        done = guard;
        *done || self.watchdog_stopped.load(Ordering::Acquire)
    }

    fn mark_finished(&self) {
        self.watchdog_stopped.store(true, Ordering::Release);
        if let Ok(mut done) = self.done_lock.lock() {
            *done = true;
            self.done_cv.notify_all();
        }
    }

    fn wait_finished(&self, timeout: Duration) -> bool {
        let Ok(mut done) = self.done_lock.lock() else {
            return true;
        };
        let deadline = Instant::now() + timeout;
        while !*done {
            let Some(rem) = deadline.checked_duration_since(Instant::now()) else {
                return false;
            };
            let (guard, res) = match self.done_cv.wait_timeout(done, rem) {
                Ok(v) => v,
                Err(e) => e.into_inner(),
            };
            done = guard;
            if res.timed_out() && !*done {
                return false;
            }
        }
        true
    }
}

static ACTIVE_TARGET_DRAG: OnceLock<Mutex<Option<Arc<TargetDragControl>>>> = OnceLock::new();

fn active_target_drag_slot() -> &'static Mutex<Option<Arc<TargetDragControl>>> {
    ACTIVE_TARGET_DRAG.get_or_init(|| Mutex::new(None))
}

#[cfg_attr(not(windows), allow(unused_variables))]
fn wake_target_drag(tid: u32, hwnd_raw: isize) {
    #[cfg(windows)]
    unsafe {
        if tid != 0 {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                tid,
                windows::Win32::UI::WindowsAndMessaging::WM_NULL,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            );
        }
        if hwnd_raw != 0 {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                Some(windows::Win32::Foundation::HWND(
                    hwnd_raw as *mut core::ffi::c_void,
                )),
                WM_APP_WAKE_DRAG,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            );
        }
    }
}

pub fn dnd_state_snapshot() -> DndStateSnapshot {
    DndStateSnapshot {
        target_tid: TARGET_DRAG_TID.load(Ordering::Acquire),
        local_src_active: LOCAL_DRAG_SOURCE_ACTIVE.load(Ordering::Acquire),
        leap_in_progress: DRAG_LEAP_IN_PROGRESS.load(Ordering::Acquire),
        catcher_visible: EDGE_CATCHER_VISIBLE.load(Ordering::Acquire),
        remote_btn_down: REMOTE_DRAG_BUTTON_DOWN.load(Ordering::Acquire),
    }
}

pub fn is_edge_catcher_visible() -> bool {
    EDGE_CATCHER_VISIBLE.load(Ordering::Acquire)
}

#[derive(Default)]
struct EdgeCatcherConfig {
    strips: Vec<SeamStrip>,
    on_caught: Option<DragCaughtCallback>,
}

static EDGE_CATCHER_CONFIG: OnceLock<Mutex<EdgeCatcherConfig>> = OnceLock::new();

fn edge_config() -> &'static Mutex<EdgeCatcherConfig> {
    EDGE_CATCHER_CONFIG.get_or_init(|| Mutex::new(EdgeCatcherConfig::default()))
}

pub fn is_local_drag_source_active() -> bool {
    LOCAL_DRAG_SOURCE_ACTIVE.load(Ordering::Acquire)
}

pub fn set_drag_leap_in_progress(in_progress: bool) {
    DRAG_LEAP_IN_PROGRESS.store(in_progress, Ordering::Release);
}

pub fn set_pending_target_drag_early_release(released: bool) {
    PENDING_TARGET_DRAG_EARLY_RELEASE.store(released, Ordering::Release);
}

pub fn take_pending_target_drag_early_release() -> bool {
    PENDING_TARGET_DRAG_EARLY_RELEASE.swap(false, Ordering::AcqRel)
}

/// Lock-free / atomic-only hook-safe notification when the controller releases a
/// physical mouse button while untrapped during a pending or active target drag.
pub fn on_target_physical_mouse_up(button: u8) {
    PENDING_TARGET_DRAG_EARLY_RELEASE.store(true, Ordering::Release);
    if REMOTE_DRAG_BUTTON.load(Ordering::Acquire) == button {
        REMOTE_DRAG_BUTTON_DOWN.store(false, Ordering::Release);
        let mut woken = false;
        if let Ok(g) = active_target_drag_slot().try_lock() {
            if let Some(ref ctrl) = *g {
                if ctrl.button == button {
                    ctrl.remote_button_down.store(false, Ordering::Release);
                    wake_target_drag(
                        ctrl.tid.load(Ordering::Acquire),
                        ctrl.hwnd.load(Ordering::Acquire),
                    );
                    woken = true;
                }
            }
        }
        if !woken {
            wake_target_drag(
                TARGET_DRAG_TID.load(Ordering::Acquire),
                TARGET_DRAG_HWND.load(Ordering::Acquire),
            );
        }
    }
}

pub fn set_remote_drag_button_state(button: u8, is_down: bool) {
    if is_down {
        REMOTE_DRAG_BUTTON.store(button, Ordering::Relaxed);
        REMOTE_DRAG_BUTTON_DOWN.store(true, Ordering::Release);
    } else {
        REMOTE_DRAG_BUTTON_DOWN.store(false, Ordering::Release);
    }
    let mut woken = false;
    if let Ok(g) = active_target_drag_slot().try_lock() {
        if let Some(ref ctrl) = *g {
            if is_down || ctrl.button == button {
                ctrl.remote_button_down.store(is_down, Ordering::Release);
                wake_target_drag(
                    ctrl.tid.load(Ordering::Acquire),
                    ctrl.hwnd.load(Ordering::Acquire),
                );
                woken = true;
            }
        }
    }
    if !woken {
        wake_target_drag(
            TARGET_DRAG_TID.load(Ordering::Acquire),
            TARGET_DRAG_HWND.load(Ordering::Acquire),
        );
    }
}

pub fn cancel_active_target_drag_with_reason(reason: &'static str) {
    let tid = TARGET_DRAG_TID.load(Ordering::Acquire);
    let hwnd = TARGET_DRAG_HWND.load(Ordering::Acquire);
    let max_seq = NEXT_TARGET_DRAG_SEQ
        .load(Ordering::Acquire)
        .saturating_sub(1);
    if max_seq > 0 {
        GLOBAL_CANCEL_REASON_CODE.store(cancel_reason_to_code(reason), Ordering::Release);
        GLOBAL_CANCEL_UP_TO_SEQ.fetch_max(max_seq, Ordering::AcqRel);
    }
    TARGET_DRAG_CANCELLED.store(true, Ordering::Release);
    REMOTE_DRAG_BUTTON_DOWN.store(false, Ordering::Release);
    let mut active_id: Option<String> = None;
    if let Ok(g) = active_target_drag_slot().try_lock() {
        if let Some(ref ctrl) = *g {
            ctrl.request_cancel(reason);
            active_id = Some(ctrl.drag_id.clone());
            wake_target_drag(
                ctrl.tid.load(Ordering::Acquire),
                ctrl.hwnd.load(Ordering::Acquire),
            );
        }
    }
    if tid != 0 || active_id.is_some() {
        crate::kvm::debug_log::note(|| {
            format!(
                "TARGET_DND_CANCEL_LOCAL tid={tid} reason={reason} id={}",
                active_id.as_deref().unwrap_or("none")
            )
        });
    }
    wake_target_drag(tid, hwnd);
}

pub fn cancel_target_drag_for_id(drag_id: &str, reason: &'static str) {
    if let Ok(g) = active_target_drag_slot().lock() {
        if let Some(ref ctrl) = *g {
            if ctrl.drag_id == drag_id {
                ctrl.request_cancel(reason);
                REMOTE_DRAG_BUTTON_DOWN.store(false, Ordering::Release);
                TARGET_DRAG_CANCELLED.store(true, Ordering::Release);
                let tid = ctrl.tid.load(Ordering::Acquire);
                let hwnd = ctrl.hwnd.load(Ordering::Acquire);
                crate::kvm::debug_log::note(|| {
                    format!("TARGET_DND_CANCEL_LOCAL tid={tid} reason={reason} id={drag_id}")
                });
                wake_target_drag(tid, hwnd);
            }
        }
    }
}

pub fn cancel_active_target_drag() {
    cancel_active_target_drag_with_reason("local_cancel");
}

pub fn configure_edge_catcher(strips: Vec<SeamStrip>, on_caught: Option<DragCaughtCallback>) {
    let has_strips = !strips.is_empty();
    if let Ok(mut g) = edge_config().lock() {
        g.strips = strips;
        g.on_caught = on_caught;
    }
    if drag_drop_enabled() && has_strips {
        ensure_edge_catcher_thread();
        post_edge_catcher_msg(WM_EC_RELOAD_STRIPS);
    } else if EDGE_CATCHER_TID.load(Ordering::Acquire) != 0 {
        post_edge_catcher_msg(WM_EC_RELOAD_STRIPS);
    }
}

pub fn clear_edge_catcher() {
    EDGE_POLL_ENABLED.store(false, Ordering::Release);
    EDGE_POLL_RECEIVER_ENTERED.store(false, Ordering::Release);
    if let Ok(mut g) = edge_config().lock() {
        g.strips.clear();
        g.on_caught = None;
    }
    EDGE_CATCHER_VISIBLE.store(false, Ordering::Release);
    post_edge_catcher_msg(WM_EC_HIDE);
}

pub fn set_edge_catcher_poll_state(poll: bool, receiver_entered: bool) {
    EDGE_POLL_ENABLED.store(poll, Ordering::Release);
    EDGE_POLL_RECEIVER_ENTERED.store(receiver_entered, Ordering::Release);
    if EDGE_CATCHER_TID.load(Ordering::Acquire) != 0 {
        post_edge_catcher_msg(WM_EC_RELOAD_STRIPS);
    }
}

pub fn hide_edge_catcher() {
    if EDGE_CATCHER_VISIBLE.swap(false, Ordering::AcqRel) {
        post_edge_catcher_msg(WM_EC_HIDE);
    }
}

/// Lock-free / non-blocking notification from mouse hook callback or delta worker.
pub fn on_mouse_drag_probe(pt: (i32, i32), buttons_down: u8, is_trapped: bool) {
    if !drag_drop_enabled() {
        if EDGE_CATCHER_VISIBLE.swap(false, Ordering::AcqRel) {
            post_edge_catcher_msg(WM_EC_HIDE);
        }
        return;
    }
    let near_strip = if buttons_down != 0 && !is_trapped {
        if let Ok(g) = edge_config().try_lock() {
            !g.strips.is_empty()
                && g.strips
                    .iter()
                    .any(|s| is_cursor_near_seam_strip(pt, s, 32))
        } else {
            return;
        }
    } else {
        false
    };

    let want_show = should_show_edge_catcher(
        drag_drop_enabled(),
        near_strip,
        is_trapped,
        buttons_down,
        DRAG_LEAP_IN_PROGRESS.load(Ordering::Acquire),
        LOCAL_DRAG_SOURCE_ACTIVE.load(Ordering::Acquire),
    );

    let prev = EDGE_CATCHER_VISIBLE.swap(want_show, Ordering::AcqRel);
    if want_show != prev {
        post_edge_catcher_msg(if want_show { WM_EC_SHOW } else { WM_EC_HIDE });
    }
}

pub fn on_hook_mouse_up() {
    DRAG_LEAP_IN_PROGRESS.store(false, Ordering::Release);
    hide_edge_catcher();
}

const WM_EC_SHOW: u32 = 0x8000 + 11;
const WM_EC_HIDE: u32 = 0x8000 + 12;
const WM_EC_RELOAD_STRIPS: u32 = 0x8000 + 13;

fn post_edge_catcher_msg(msg: u32) {
    #[cfg(windows)]
    unsafe {
        let tid = EDGE_CATCHER_TID.load(Ordering::Acquire);
        if tid != 0 {
            let _ = windows::Win32::UI::WindowsAndMessaging::PostThreadMessageW(
                tid,
                msg,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            );
        }
    }
    #[cfg(not(windows))]
    let _ = msg;
}

static EDGE_THREAD_STARTED: OnceLock<()> = OnceLock::new();

fn ensure_edge_catcher_thread() {
    #[cfg(windows)]
    EDGE_THREAD_STARTED.get_or_init(|| {
        std::thread::spawn(edge_catcher_thread_main);
    });
}

// ---------------------------------------------------------------------------
// Windows OLE IDropTarget (Edge Catcher) & DoDragDrop (Target Drop Source)
// ---------------------------------------------------------------------------

#[cfg(windows)]
mod win_ole {
    use super::*;
    use std::mem::ManuallyDrop;
    use windows::core::{implement, w, PCWSTR};
    use windows::Win32::Foundation::{
        DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, DV_E_FORMATETC,
        E_NOTIMPL, HGLOBAL, HWND, LPARAM, LRESULT, OLE_E_ADVISENOTSUPPORTED, POINTL, S_OK, WPARAM,
    };
    use windows::Win32::System::Com::{
        IAdviseSink, IDataObject, IDataObject_Impl, IEnumFORMATETC, IEnumSTATDATA, DATADIR_GET,
        DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL,
    };
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    use windows::Win32::System::Memory::{
        GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE, GMEM_ZEROINIT,
    };
    use windows::Win32::System::Ole::{
        DoDragDrop, IDropSource, IDropSource_Impl, IDropTarget, IDropTarget_Impl, OleInitialize,
        OleUninitialize, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop, DROPEFFECT,
        DROPEFFECT_COPY, DROPEFFECT_NONE,
    };
    use windows::Win32::System::SystemServices::MODIFIERKEYS_FLAGS;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Shell::{SHCreateStdEnumFmtEtc, HDROP};
    use windows::Win32::UI::WindowsAndMessaging::*;

    const MAX_EDGE_WINDOWS: usize = 8;

    #[implement(IDropTarget)]
    struct EdgeDropTarget;

    impl IDropTarget_Impl for EdgeDropTarget_Impl {
        fn DragEnter(
            &self,
            pdataobj: windows::core::Ref<'_, IDataObject>,
            grfkeystate: MODIFIERKEYS_FLAGS,
            pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            unsafe {
                if !pdweffect.is_null() {
                    *pdweffect = DROPEFFECT_NONE;
                }
                let Some(data_obj) = pdataobj.as_ref() else {
                    return Ok(());
                };
                if !drag_drop_enabled() || LOCAL_DRAG_SOURCE_ACTIVE.load(Ordering::Acquire) {
                    return Ok(());
                }
                let fmt = FORMATETC {
                    cfFormat: 15, // CF_HDROP
                    ptd: std::ptr::null_mut(),
                    dwAspect: DVASPECT_CONTENT.0,
                    lindex: -1,
                    tymed: TYMED_HGLOBAL.0 as u32,
                };
                let Ok(mut medium) = data_obj.GetData(&fmt) else {
                    return Ok(());
                };
                let mut paths = Vec::new();
                if medium.tymed == TYMED_HGLOBAL.0 as u32 && !medium.u.hGlobal.is_invalid() {
                    paths = super::super::clipboard::read_hdrop_paths(HDROP(medium.u.hGlobal.0));
                }
                ReleaseStgMedium(&mut medium);

                let raw_count = paths.len();
                paths.retain(|p| p.exists());
                let exists_count = paths.len();
                let button = button_from_grf_key_state(grfkeystate.0);
                let cursor_pt = (pt.x, pt.y);
                crate::kvm::debug_log::note(|| {
                    format!(
                        "DRAG_CATCHER_DRAG_ENTER raw_files={raw_count} existing_files={exists_count} button={button} pt=({},{}) enabled={} in_prog={}",
                        cursor_pt.0,
                        cursor_pt.1,
                        drag_drop_enabled(),
                        DRAG_LEAP_IN_PROGRESS.load(Ordering::Acquire),
                    )
                });
                if paths.is_empty() {
                    return Ok(());
                }
                if DRAG_LEAP_IN_PROGRESS.swap(true, Ordering::AcqRel) {
                    return Ok(());
                }
                if !pdweffect.is_null() {
                    *pdweffect = DROPEFFECT_COPY;
                }

                hide_edge_catcher();
                let cb_opt = edge_config().lock().ok().and_then(|g| g.on_caught.clone());

                if let Some(cb) = cb_opt {
                    std::thread::spawn(move || {
                        // Cancel the local OLE DoDragDrop cleanly from a helper thread (never a hook callback).
                        let (d_sent, d_err) =
                            super::super::injector::inject_key(0x1B, 0x01, true, false);
                        let (u_sent, u_err) =
                            super::super::injector::inject_key(0x1B, 0x01, false, false);
                        crate::kvm::debug_log::note(|| {
                            format!(
                                "DRAG_ESCAPE_INJECT down_sent={d_sent}(err={d_err}) up_sent={u_sent}(err={u_err})"
                            )
                        });
                        std::thread::sleep(std::time::Duration::from_millis(18));
                        cb(paths, button, cursor_pt);
                    });
                } else {
                    DRAG_LEAP_IN_PROGRESS.store(false, Ordering::Release);
                }
            }
            Ok(())
        }

        fn DragOver(
            &self,
            _grfkeystate: MODIFIERKEYS_FLAGS,
            _pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            unsafe {
                if !pdweffect.is_null() {
                    *pdweffect = DROPEFFECT_NONE;
                }
            }
            Ok(())
        }

        fn DragLeave(&self) -> windows::core::Result<()> {
            Ok(())
        }

        fn Drop(
            &self,
            _pdataobj: windows::core::Ref<'_, IDataObject>,
            _grfkeystate: MODIFIERKEYS_FLAGS,
            _pt: &POINTL,
            pdweffect: *mut DROPEFFECT,
        ) -> windows::core::Result<()> {
            unsafe {
                if !pdweffect.is_null() {
                    *pdweffect = DROPEFFECT_NONE;
                }
            }
            Ok(())
        }
    }

    unsafe extern "system" fn edge_wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_NCHITTEST => LRESULT(1),     // HTCLIENT
            WM_MOUSEACTIVATE => LRESULT(3), // MA_NOACTIVATE
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    fn register_catcher_class(class_name: PCWSTR) {
        unsafe {
            let hinst =
                windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(edge_wnd_proc),
                hInstance: hinst.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassExW(&wc);
        }
    }

    fn create_hidden_ole_window(class_name: PCWSTR) -> Option<HWND> {
        unsafe {
            let hinst =
                windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
            let hwnd = CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
                class_name,
                w!(""),
                WS_POPUP,
                0,
                0,
                2,
                2,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .ok()?;
            let _ = SetLayeredWindowAttributes(
                hwnd,
                windows::Win32::Foundation::COLORREF(0),
                1,
                LWA_ALPHA,
            );
            Some(hwnd)
        }
    }

    pub(super) fn edge_catcher_thread_main() {
        unsafe {
            let _ = OleInitialize(None);
            let mut msg = MSG::default();
            let _ = PeekMessageW(&mut msg, None, WM_USER, WM_USER, PM_NOREMOVE);
            EDGE_CATCHER_TID.store(GetCurrentThreadId(), Ordering::Release);

            let class_name = w!("AnyLeapEdgeCatcher");
            register_catcher_class(class_name);

            let mut windows_pool: Vec<HWND> = Vec::with_capacity(MAX_EDGE_WINDOWS);
            for _ in 0..MAX_EDGE_WINDOWS {
                if let Some(hwnd) = create_hidden_ole_window(class_name) {
                    windows_pool.push(hwnd);
                }
            }
            let drop_target: IDropTarget = EdgeDropTarget.into();
            let mut registered = vec![false; windows_pool.len()];
            let mut visible = vec![false; windows_pool.len()];

            let mut active_strips: Vec<SeamStrip> = Vec::new();
            let mut shown = false;
            let mut poll_timer_armed = false;

            let apply_visibility = |strips: &[SeamStrip],
                                    show: bool,
                                    pool: &[HWND],
                                    registered: &mut [bool],
                                    visible: &mut [bool]| {
                for (i, hwnd) in pool.iter().enumerate() {
                    if show {
                        if let Some(s) = strips.get(i) {
                            if !registered[i] {
                                match RegisterDragDrop(*hwnd, &drop_target) {
                                    Ok(()) => registered[i] = true,
                                    Err(e) => {
                                        crate::kvm::debug_log::note(|| {
                                            format!("DRAG_CATCHER_ANOMALY op=register_drag_drop idx={i} err={e}")
                                        });
                                    }
                                }
                            }
                            let _ = SetWindowPos(
                                *hwnd,
                                Some(HWND_TOPMOST),
                                s.x,
                                s.y,
                                s.width,
                                s.height,
                                SWP_NOACTIVATE | SWP_SHOWWINDOW,
                            );
                            visible[i] = true;
                            if !IsWindowVisible(*hwnd).as_bool() {
                                crate::kvm::debug_log::note(|| {
                                    format!("DRAG_CATCHER_ANOMALY op=show_not_visible idx={i}")
                                });
                            }
                            continue;
                        }
                    }

                    if registered[i] {
                        if let Err(e) = RevokeDragDrop(*hwnd) {
                            crate::kvm::debug_log::note(|| {
                                format!("DRAG_CATCHER_ANOMALY op=revoke_drag_drop idx={i} err={e}")
                            });
                        }
                        registered[i] = false;
                    }
                    let os_visible = IsWindowVisible(*hwnd).as_bool();
                    let os_topmost =
                        (GetWindowLongW(*hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0) != 0;
                    if visible[i] || os_visible || os_topmost {
                        if !visible[i] && (os_visible || os_topmost) {
                            crate::kvm::debug_log::note(|| {
                                format!(
                                    "DRAG_CATCHER_ANOMALY op=unexpected_state_while_hidden idx={i} visible={os_visible} topmost={os_topmost}"
                                )
                            });
                        }
                        let _ = SetWindowPos(
                            *hwnd,
                            Some(HWND_NOTOPMOST),
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_HIDEWINDOW,
                        );
                        let _ = ShowWindow(*hwnd, SW_HIDE);
                        visible[i] = false;
                        let still_visible = IsWindowVisible(*hwnd).as_bool();
                        let still_topmost =
                            (GetWindowLongW(*hwnd, GWL_EXSTYLE) as u32 & WS_EX_TOPMOST.0) != 0;
                        if still_visible || still_topmost {
                            crate::kvm::debug_log::note(|| {
                                format!(
                                    "DRAG_CATCHER_ANOMALY op=hide_failed idx={i} visible={still_visible} topmost={still_topmost}"
                                )
                            });
                        }
                    }
                }
            };

            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                match msg.message {
                    WM_EC_RELOAD_STRIPS => {
                        if let Ok(g) = edge_config().lock() {
                            active_strips = g.strips.clone();
                        }
                        let enabled = drag_drop_enabled();
                        let want_timer = enabled
                            && !active_strips.is_empty()
                            && EDGE_POLL_ENABLED.load(Ordering::Acquire);
                        if want_timer && !poll_timer_armed {
                            let _ = SetTimer(None, 1, 24, None);
                            poll_timer_armed = true;
                        } else if !want_timer && poll_timer_armed {
                            let _ = KillTimer(None, 1);
                            poll_timer_armed = false;
                        }
                        if (!enabled || active_strips.is_empty()) && shown {
                            shown = false;
                            EDGE_CATCHER_VISIBLE.store(false, Ordering::Release);
                        }
                        apply_visibility(
                            &active_strips,
                            shown,
                            &windows_pool,
                            &mut registered,
                            &mut visible,
                        );
                    }
                    WM_TIMER => {
                        if EDGE_POLL_ENABLED.load(Ordering::Acquire)
                            && !EDGE_POLL_RECEIVER_ENTERED.load(Ordering::Acquire)
                            && drag_drop_enabled()
                            && !active_strips.is_empty()
                        {
                            use windows::Win32::UI::Input::KeyboardAndMouse::{
                                GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON,
                            };
                            let mut mask = 0u8;
                            if (GetAsyncKeyState(VK_LBUTTON.0 as i32) as u16 & 0x8000) != 0 {
                                mask |= 1;
                            }
                            if (GetAsyncKeyState(VK_RBUTTON.0 as i32) as u16 & 0x8000) != 0 {
                                mask |= 2;
                            }
                            if (GetAsyncKeyState(VK_MBUTTON.0 as i32) as u16 & 0x8000) != 0 {
                                mask |= 4;
                            }
                            if let Some(pt) = super::super::injector::get_cursor_pos() {
                                on_mouse_drag_probe(pt, mask, false);
                            } else if mask == 0 {
                                DRAG_LEAP_IN_PROGRESS.store(false, Ordering::Release);
                                hide_edge_catcher();
                            }
                            if mask == 0 {
                                DRAG_LEAP_IN_PROGRESS.store(false, Ordering::Release);
                            }
                        } else if shown && (!drag_drop_enabled() || active_strips.is_empty()) {
                            shown = false;
                            EDGE_CATCHER_VISIBLE.store(false, Ordering::Release);
                            apply_visibility(
                                &active_strips,
                                false,
                                &windows_pool,
                                &mut registered,
                                &mut visible,
                            );
                        }
                    }
                    WM_EC_SHOW => {
                        if active_strips.is_empty() || !drag_drop_enabled() {
                            crate::kvm::debug_log::note(|| {
                                format!(
                                    "DRAG_CATCHER_ANOMALY op=show_rejected strips={} enabled={}",
                                    active_strips.len(),
                                    drag_drop_enabled()
                                )
                            });
                            shown = false;
                            EDGE_CATCHER_VISIBLE.store(false, Ordering::Release);
                            apply_visibility(
                                &active_strips,
                                false,
                                &windows_pool,
                                &mut registered,
                                &mut visible,
                            );
                        } else {
                            shown = true;
                            crate::kvm::debug_log::note(|| {
                                format!(
                                    "DRAG_CATCHER_SHOW strips={} first={:?}",
                                    active_strips.len(),
                                    active_strips.first()
                                )
                            });
                            apply_visibility(
                                &active_strips,
                                true,
                                &windows_pool,
                                &mut registered,
                                &mut visible,
                            );
                        }
                    }
                    WM_EC_HIDE => {
                        if shown {
                            crate::kvm::debug_log::note(|| "DRAG_CATCHER_HIDE".to_string());
                        }
                        shown = false;
                        apply_visibility(
                            &active_strips,
                            false,
                            &windows_pool,
                            &mut registered,
                            &mut visible,
                        );
                    }
                    _ => {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            }

            for (i, hwnd) in windows_pool.into_iter().enumerate() {
                if registered[i] {
                    let _ = RevokeDragDrop(hwnd);
                }
                let _ = DestroyWindow(hwnd);
            }
            EDGE_CATCHER_TID.store(0, Ordering::SeqCst);
            OleUninitialize();
        }
    }

    #[implement(IDataObject)]
    struct DropDataObject {
        hdrop_bytes: Vec<u8>,
        cf_preferred_effect: u16,
    }

    impl DropDataObject {
        fn new(paths: &[PathBuf]) -> Self {
            let cf_preferred_effect =
                unsafe { RegisterClipboardFormatW(w!("Preferred DropEffect")) as u16 };
            Self {
                hdrop_bytes: super::super::clipboard::build_dropfiles_bytes(paths),
                cf_preferred_effect,
            }
        }

        fn matches(&self, fmt: &FORMATETC) -> Option<&'static str> {
            if (fmt.tymed & TYMED_HGLOBAL.0 as u32) == 0 {
                return None;
            }
            if fmt.cfFormat == 15 {
                Some("hdrop")
            } else if self.cf_preferred_effect != 0 && fmt.cfFormat == self.cf_preferred_effect {
                Some("effect")
            } else {
                None
            }
        }

        fn alloc_hglobal(bytes: &[u8]) -> windows::core::Result<STGMEDIUM> {
            unsafe {
                let h = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes.len()).map_err(|_| {
                    windows::core::Error::from(windows::Win32::Foundation::E_OUTOFMEMORY)
                })?;
                let ptr = GlobalLock(h);
                if ptr.is_null() {
                    return Err(windows::Win32::Foundation::E_OUTOFMEMORY.into());
                }
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len());
                let _ = GlobalUnlock(h);
                Ok(STGMEDIUM {
                    tymed: TYMED_HGLOBAL.0 as u32,
                    u: STGMEDIUM_0 {
                        hGlobal: HGLOBAL(h.0),
                    },
                    pUnkForRelease: ManuallyDrop::new(None),
                })
            }
        }
    }

    impl IDataObject_Impl for DropDataObject_Impl {
        fn GetData(&self, pformatetcin: *const FORMATETC) -> windows::core::Result<STGMEDIUM> {
            if pformatetcin.is_null() {
                return Err(DV_E_FORMATETC.into());
            }
            let fmt = unsafe { &*pformatetcin };
            match self.matches(fmt) {
                Some("hdrop") => DropDataObject::alloc_hglobal(&self.hdrop_bytes),
                Some("effect") => {
                    let eff = DROPEFFECT_COPY.0.to_le_bytes();
                    DropDataObject::alloc_hglobal(&eff)
                }
                _ => Err(DV_E_FORMATETC.into()),
            }
        }

        fn GetDataHere(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *mut STGMEDIUM,
        ) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }

        fn QueryGetData(&self, pformatetc: *const FORMATETC) -> windows::core::HRESULT {
            if pformatetc.is_null() {
                return DV_E_FORMATETC;
            }
            let fmt = unsafe { &*pformatetc };
            if self.matches(fmt).is_some() {
                S_OK
            } else {
                DV_E_FORMATETC
            }
        }

        fn GetCanonicalFormatEtc(
            &self,
            _pformatectin: *const FORMATETC,
            pformatetcout: *mut FORMATETC,
        ) -> windows::core::HRESULT {
            if !pformatetcout.is_null() {
                unsafe { (*pformatetcout).ptd = std::ptr::null_mut() };
            }
            E_NOTIMPL
        }

        fn SetData(
            &self,
            _pformatetc: *const FORMATETC,
            _pmedium: *const STGMEDIUM,
            _frelease: windows::core::BOOL,
        ) -> windows::core::Result<()> {
            Err(E_NOTIMPL.into())
        }

        fn EnumFormatEtc(&self, dwdirection: u32) -> windows::core::Result<IEnumFORMATETC> {
            if dwdirection != DATADIR_GET.0 as u32 {
                return Err(E_NOTIMPL.into());
            }
            let fmts = [
                FORMATETC {
                    cfFormat: 15,
                    ptd: std::ptr::null_mut(),
                    dwAspect: DVASPECT_CONTENT.0,
                    lindex: -1,
                    tymed: TYMED_HGLOBAL.0 as u32,
                },
                FORMATETC {
                    cfFormat: self.cf_preferred_effect,
                    ptd: std::ptr::null_mut(),
                    dwAspect: DVASPECT_CONTENT.0,
                    lindex: -1,
                    tymed: TYMED_HGLOBAL.0 as u32,
                },
            ];
            unsafe { SHCreateStdEnumFmtEtc(&fmts) }
        }

        fn DAdvise(
            &self,
            _pformatetc: *const FORMATETC,
            _advf: u32,
            _padvsink: windows::core::Ref<'_, IAdviseSink>,
        ) -> windows::core::Result<u32> {
            Err(OLE_E_ADVISENOTSUPPORTED.into())
        }

        fn DUnadvise(&self, _dwconnection: u32) -> windows::core::Result<()> {
            Err(OLE_E_ADVISENOTSUPPORTED.into())
        }

        fn EnumDAdvise(&self) -> windows::core::Result<IEnumSTATDATA> {
            Err(OLE_E_ADVISENOTSUPPORTED.into())
        }
    }

    const DRAG_WATCHDOG_TIMER_ID: usize = 7;
    const WM_NCHITTEST_MSG: u32 = 0x0084;
    const HTCLIENT_CODE: isize = 1;

    thread_local! {
        static CURRENT_DRAG_CTRL: std::cell::RefCell<Option<Arc<TargetDragControl>>> =
            const { std::cell::RefCell::new(None) };
    }

    unsafe fn is_physical_mouse_button_down(button: u8) -> bool {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        let vk = vk_for_mouse_button(button) as i32;
        (GetAsyncKeyState(vk) as u16 & 0x8000) != 0
    }

    unsafe fn inject_escape_key() {
        use windows::Win32::UI::Input::KeyboardAndMouse::{
            SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY,
        };
        let inputs = [
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0x1B),
                        wScan: 0,
                        dwFlags: Default::default(),
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
            INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 {
                    ki: KEYBDINPUT {
                        wVk: VIRTUAL_KEY(0x1B),
                        wScan: 0,
                        dwFlags: KEYEVENTF_KEYUP,
                        time: 0,
                        dwExtraInfo: 0,
                    },
                },
            },
        ];
        let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }

    #[implement(IDropSource)]
    struct DropSource {
        expected_mk: u32,
        ctrl: Arc<TargetDragControl>,
    }

    unsafe extern "system" fn drag_source_wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if msg == WM_NCHITTEST_MSG {
            return LRESULT(HTCLIENT_CODE);
        }
        if msg == WM_TIMER || msg == WM_APP_WAKE_DRAG {
            use windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture;
            use windows::Win32::UI::WindowsAndMessaging::{GetGUIThreadInfo, GUITHREADINFO};
            CURRENT_DRAG_CTRL.with(|cell| {
                if let Some(ref ctrl) = *cell.borrow() {
                    if !ctrl.in_drag_loop.load(Ordering::Acquire) {
                        return;
                    }
                    let remote_down = ctrl.remote_button_down.load(Ordering::Acquire);
                    // When tracking a remote/swallowed button across edge return, never rely on
                    // GetAsyncKeyState alone (Windows reports UP when LL hook swallowed the press).
                    let physical_down = if ctrl.track_remote_button {
                        if crate::kvm::hooks::is_hook_active() {
                            crate::kvm::hooks::is_physical_button_down(ctrl.button)
                        } else {
                            false
                        }
                    } else {
                        is_physical_mouse_button_down(ctrl.button)
                    };
                    let elapsed_ms = ctrl.started_at.elapsed().as_millis() as u64;
                    let neither_ms =
                        ctrl.update_neither_down(physical_down, remote_down, elapsed_ms);
                    let decision = decide_target_drag_continue(TargetDragQueryInput {
                        escape_pressed: false,
                        cancel_reason: ctrl.effective_cancel_reason(),
                        track_remote_button: ctrl.track_remote_button,
                        remote_button_down: remote_down,
                        physical_button_down: physical_down,
                        seen_button_down: ctrl.seen_button_down.load(Ordering::Acquire),
                        elapsed_ms,
                        neither_down_ms: neither_ms,
                        max_duration_ms: TARGET_DRAG_MAX_DURATION_MS,
                        neither_down_timeout_ms: TARGET_DRAG_NEITHER_BTN_CANCEL_MS,
                    });
                    let mut tid_info = GUITHREADINFO {
                        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                        ..Default::default()
                    };
                    let capture = if GetGUIThreadInfo(GetCurrentThreadId(), &mut tid_info).is_ok() {
                        tid_info.hwndCapture
                    } else {
                        HWND::default()
                    };
                    if decision == TargetDragDecision::Continue {
                        // Keep OLE QueryContinueDrag alive when user pauses pointer while still holding.
                        if !capture.0.is_null() {
                            let mk = mk_flag_for_mouse_button(ctrl.button) as usize;
                            let _ =
                                PostMessageW(Some(capture), WM_MOUSEMOVE, WPARAM(mk), LPARAM(0));
                        }
                    } else {
                        if let TargetDragDecision::Cancel(r) = decision {
                            ctrl.set_decision_reason(r);
                            ctrl.request_cancel(r);
                        }
                        if !capture.0.is_null() {
                            let _ = PostMessageW(Some(capture), WM_MOUSEMOVE, WPARAM(0), LPARAM(0));
                        } else {
                            let _ = PostThreadMessageW(
                                GetCurrentThreadId(),
                                WM_MOUSEMOVE,
                                WPARAM(0),
                                LPARAM(0),
                            );
                        }
                        if matches!(decision, TargetDragDecision::Cancel(_))
                            && ctrl.cancel_nudges.fetch_add(1, Ordering::Relaxed) >= 4
                        {
                            let _ = ReleaseCapture();
                            for action in watchdog_cancel_messages() {
                                let _ = PostMessageW(
                                    Some(hwnd),
                                    action.msg,
                                    WPARAM(action.wparam),
                                    LPARAM(0),
                                );
                                if !capture.0.is_null() {
                                    let _ = PostMessageW(
                                        Some(capture),
                                        action.msg,
                                        WPARAM(action.wparam),
                                        LPARAM(0),
                                    );
                                }
                                let _ = PostThreadMessageW(
                                    GetCurrentThreadId(),
                                    action.msg,
                                    WPARAM(action.wparam),
                                    LPARAM(0),
                                );
                            }
                        }
                    }
                }
            });
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    fn register_drag_source_class(class_name: PCWSTR) {
        unsafe {
            let hinst =
                windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(drag_source_wndproc),
                hInstance: hinst.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassExW(&wc);
        }
    }

    fn create_drag_source_window(
        class_name: PCWSTR,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    ) -> Option<HWND> {
        unsafe {
            let hinst =
                windows::Win32::System::LibraryLoader::GetModuleHandleW(None).unwrap_or_default();
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                class_name,
                w!(""),
                WS_POPUP,
                x,
                y,
                w,
                h,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .ok()?;
            let _ = SetLayeredWindowAttributes(
                hwnd,
                windows::Win32::Foundation::COLORREF(0),
                1,
                LWA_ALPHA,
            );
            Some(hwnd)
        }
    }

    impl IDropSource_Impl for DropSource_Impl {
        fn QueryContinueDrag(
            &self,
            fescapepressed: windows::core::BOOL,
            grfkeystate: MODIFIERKEYS_FLAGS,
        ) -> windows::core::HRESULT {
            let elapsed_ms = self.ctrl.note_query_continue();
            let physical_down = (grfkeystate.0 & self.expected_mk) != 0;
            let remote_down = self.ctrl.remote_button_down.load(Ordering::Acquire);
            let neither_ms = self
                .ctrl
                .update_neither_down(physical_down, remote_down, elapsed_ms);
            let decision = decide_target_drag_continue(TargetDragQueryInput {
                escape_pressed: fescapepressed.as_bool(),
                cancel_reason: self.ctrl.effective_cancel_reason(),
                track_remote_button: self.ctrl.track_remote_button,
                remote_button_down: remote_down,
                physical_button_down: physical_down,
                seen_button_down: self.ctrl.seen_button_down.load(Ordering::Acquire),
                elapsed_ms,
                neither_down_ms: neither_ms,
                max_duration_ms: TARGET_DRAG_MAX_DURATION_MS,
                neither_down_timeout_ms: TARGET_DRAG_NEITHER_BTN_CANCEL_MS,
            });
            match decision {
                TargetDragDecision::Continue => S_OK,
                TargetDragDecision::Drop => {
                    self.ctrl.set_decision_reason("drop");
                    DRAGDROP_S_DROP
                }
                TargetDragDecision::Cancel(reason) => {
                    self.ctrl.set_decision_reason(reason);
                    DRAGDROP_S_CANCEL
                }
            }
        }

        fn GiveFeedback(&self, _dweffect: DROPEFFECT) -> windows::core::HRESULT {
            DRAGDROP_S_USEDEFAULTCURSORS
        }
    }

    struct TargetDragCleanupGuard {
        ctrl: Arc<TargetDragControl>,
        hwnd: Option<HWND>,
        ole_initialized: bool,
        timer_armed: bool,
        forwarded_button_inject: bool,
        injected_down: bool,
        staging_root: PathBuf,
        on_finished: Option<Box<dyn FnOnce(TargetDragResult) + Send>>,
        dropped: bool,
        reason: &'static str,
        hr_u32: u32,
        effect_u32: u32,
        done_logged: bool,
        cleaned: bool,
    }

    impl TargetDragCleanupGuard {
        fn run_cleanup(&mut self) {
            if self.cleaned {
                return;
            }
            self.cleaned = true;
            let (outcome, notice) =
                classify_target_drag_outcome(self.hr_u32, self.effect_u32, self.reason);
            self.dropped = outcome == DragOutcome::Done;
            unsafe {
                CURRENT_DRAG_CTRL.with(|cell| {
                    *cell.borrow_mut() = None;
                });
                if !self.done_logged {
                    self.done_logged = true;
                    crate::kvm::debug_log::note(|| {
                        format!(
                            "TARGET_DND_DONE id={} hr=0x{:08X} effect={} dropped={} outcome={} reason={}",
                            self.ctrl.drag_id,
                            self.hr_u32,
                            self.effect_u32,
                            self.dropped,
                            outcome.as_str(),
                            self.reason,
                        )
                    });
                }

                if should_release_target_drag_on_exit(
                    self.forwarded_button_inject,
                    self.injected_down,
                ) {
                    self.injected_down = false;
                    let (sent, err) =
                        super::super::injector::inject_mouse_button(self.ctrl.button, false);
                    crate::kvm::debug_log::note(|| {
                        format!(
                            "TARGET_DND_BTN_UP id={} button={} sent={sent} err={err}",
                            self.ctrl.drag_id, self.ctrl.button,
                        )
                    });
                }

                let _ = windows::Win32::UI::Input::KeyboardAndMouse::ReleaseCapture();

                if let Some(h) = self.hwnd.take() {
                    if self.timer_armed {
                        let _ = KillTimer(Some(h), DRAG_WATCHDOG_TIMER_ID);
                        self.timer_armed = false;
                    }
                    let _ = ShowWindow(h, SW_HIDE);
                    let _ = DestroyWindow(h);
                }

                if self.ole_initialized {
                    OleUninitialize();
                    self.ole_initialized = false;
                }

                let mut cleared_globals = false;
                if let Ok(mut g) = active_target_drag_slot().lock() {
                    if g.as_ref().is_some_and(|c| c.seq == self.ctrl.seq) {
                        *g = None;
                        cleared_globals = true;
                    }
                }
                if cleared_globals {
                    TARGET_DRAG_HWND.store(0, Ordering::SeqCst);
                    TARGET_DRAG_TID.store(0, Ordering::SeqCst);
                    REMOTE_DRAG_BUTTON_DOWN.store(false, Ordering::Release);
                    LOCAL_DRAG_SOURCE_ACTIVE.store(false, Ordering::Release);
                }
                self.ctrl.mark_finished();

                super::super::staging::cleanup_staging_entry(
                    &self.staging_root,
                    &self.ctrl.drag_id,
                );
                crate::kvm::debug_log::note(|| {
                    format!(
                        "TARGET_DND_CLEANUP id={} dropped={} outcome={} reason={}",
                        self.ctrl.drag_id,
                        self.dropped,
                        outcome.as_str(),
                        self.reason
                    )
                });
            }

            if let Some(cb) = self.on_finished.take() {
                cb(TargetDragResult {
                    outcome,
                    effect: self.effect_u32,
                    reason: self.reason,
                    notice,
                });
            }
        }
    }

    impl Drop for TargetDragCleanupGuard {
        fn drop(&mut self) {
            self.run_cleanup();
        }
    }

    unsafe fn pump_drag_window_messages() {
        let mut msg = MSG::default();
        for _ in 0..4 {
            let mut drained = false;
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                drained = true;
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if !drained {
                std::thread::sleep(Duration::from_millis(2));
            }
        }
    }

    fn spawn_target_drag_watchdog(
        ctrl: Arc<TargetDragControl>,
    ) -> Option<std::thread::JoinHandle<()>> {
        std::thread::Builder::new()
            .name(format!("drag-wd-{}", ctrl.seq))
            .spawn(move || unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{
                    GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
                    GUITHREADINFO, WM_NULL,
                };
                let mut nudges = 0u32;
                while !ctrl.wait_watchdog_tick(Duration::from_millis(40)) {
                    let tid = ctrl.tid.load(Ordering::Acquire);
                    if tid == 0 {
                        continue;
                    }
                    let hwnd_raw = ctrl.hwnd.load(Ordering::Acquire);
                    let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
                    let elapsed_ms = ctrl.started_at.elapsed().as_millis() as u64;
                    let in_drag_loop = ctrl.in_drag_loop.load(Ordering::Acquire);
                    let last_query = ctrl.last_query_ms.load(Ordering::Acquire);
                    let since_last_query_ms = elapsed_ms.saturating_sub(last_query);

                    let remote_down = ctrl.remote_button_down.load(Ordering::Acquire);
                    let physical_down = if ctrl.track_remote_button {
                        if crate::kvm::hooks::is_hook_active() {
                            crate::kvm::hooks::is_physical_button_down(ctrl.button)
                        } else {
                            false
                        }
                    } else {
                        is_physical_mouse_button_down(ctrl.button)
                    };
                    let remote_up_ms = ctrl.update_remote_up(remote_down, elapsed_ms);
                    let neither_ms = ctrl.update_neither_down(physical_down, remote_down, elapsed_ms);

                    let reason_opt = decide_watchdog_cancel(WatchdogEvalInput {
                        elapsed_ms,
                        in_drag_loop,
                        since_last_query_ms,
                        cancel_reason: ctrl.effective_cancel_reason(),
                        track_remote_button: ctrl.track_remote_button,
                        remote_button_down: remote_down,
                        remote_up_ms,
                        physical_button_down: physical_down,
                        neither_down_ms: neither_ms,
                        max_duration_ms: TARGET_DRAG_MAX_DURATION_MS,
                        neither_down_timeout_ms: TARGET_DRAG_NEITHER_BTN_CANCEL_MS,
                        query_stall_timeout_ms: TARGET_DRAG_QUERY_STALL_MS,
                    });

                    let Some(reason) = reason_opt else {
                        continue;
                    };

                    ctrl.set_decision_reason(reason);
                    ctrl.request_cancel(reason);

                    let mut tid_info = GUITHREADINFO {
                        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                        ..Default::default()
                    };
                    let tid_cap = if GetGUIThreadInfo(tid, &mut tid_info).is_ok() {
                        tid_info.hwndCapture
                    } else {
                        HWND::default()
                    };
                    let mut fg_info = GUITHREADINFO {
                        cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
                        ..Default::default()
                    };
                    let fg_cap = if GetGUIThreadInfo(0, &mut fg_info).is_ok() {
                        fg_info.hwndCapture
                    } else {
                        HWND::default()
                    };

                    if nudges == 0 || nudges.is_multiple_of(5) {
                        crate::kvm::debug_log::note(|| {
                            format!(
                                "TARGET_DND_WATCHDOG_FORCE id={} tid={tid} reason={reason} nudge={nudges} elapsed_ms={elapsed_ms} since_query_ms={since_last_query_ms} tid_cap=0x{:X}",
                                ctrl.drag_id,
                                tid_cap.0 as usize,
                            )
                        });
                    }

                    // Never post synthetic mouse button-up here: forced termination must cancel
                    // DoDragDrop cleanly rather than dropping onto whatever window is underneath.
                    for act in watchdog_cancel_messages() {
                        if !hwnd.0.is_null() {
                            let _ = PostMessageW(Some(hwnd), act.msg, WPARAM(act.wparam), LPARAM(0));
                        }
                        if !tid_cap.0.is_null() {
                            let _ =
                                PostMessageW(Some(tid_cap), act.msg, WPARAM(act.wparam), LPARAM(0));
                        }
                        let _ = PostThreadMessageW(tid, act.msg, WPARAM(act.wparam), LPARAM(0));
                    }
                    let _ = PostThreadMessageW(tid, WM_NULL, WPARAM(0), LPARAM(0));

                    if nudges >= 2 {
                        let fg_hwnd = GetForegroundWindow();
                        let fg_tid = if !fg_hwnd.0.is_null() {
                            GetWindowThreadProcessId(fg_hwnd, None)
                        } else {
                            0
                        };
                        let drag_is_fg_or_cap = fg_tid == tid
                            || !tid_cap.0.is_null()
                            || (!hwnd.0.is_null() && fg_cap == hwnd);
                        if should_inject_escape_on_drag_timeout(drag_is_fg_or_cap) {
                            inject_escape_key();
                        }
                    }

                    nudges = nudges.saturating_add(1);
                }
            })
            .ok()
    }

    pub(super) fn run_target_drag_drop(
        drag_id: String,
        paths: Vec<PathBuf>,
        button: u8,
        forwarded_button_inject: bool,
        staging_root: PathBuf,
        on_finished: Box<dyn FnOnce(TargetDragResult) + Send>,
    ) {
        let seq = NEXT_TARGET_DRAG_SEQ.fetch_add(1, Ordering::AcqRel);
        let ctrl = Arc::new(TargetDragControl::new(
            seq,
            drag_id.clone(),
            button,
            forwarded_button_inject,
        ));
        let prev = if let Ok(mut g) = active_target_drag_slot().lock() {
            let old = g.take();
            if let Some(ref p) = old {
                p.request_cancel("superseded");
                let ptid = p.tid.load(Ordering::Acquire);
                let phwnd = p.hwnd.load(Ordering::Acquire);
                crate::kvm::debug_log::note(|| {
                    format!(
                        "TARGET_DND_SUPERSEDE old_id={} old_tid={} new_id={}",
                        p.drag_id, ptid, drag_id
                    )
                });
                wake_target_drag(ptid, phwnd);
            }
            *g = Some(Arc::clone(&ctrl));
            old
        } else {
            None
        };

        std::thread::spawn(move || unsafe {
            if let Some(p) = prev {
                if !p.wait_finished(Duration::from_millis(800)) {
                    crate::kvm::debug_log::note(|| {
                        format!(
                            "TARGET_DND_SUPERSEDE_TIMEOUT old_id={} new_id={}",
                            p.drag_id, ctrl.drag_id
                        )
                    });
                }
            }

            LOCAL_DRAG_SOURCE_ACTIVE.store(true, Ordering::Release);
            TARGET_DRAG_CANCELLED.store(false, Ordering::Release);
            hide_edge_catcher();

            let mut guard = TargetDragCleanupGuard {
                ctrl: Arc::clone(&ctrl),
                hwnd: None,
                ole_initialized: false,
                timer_armed: false,
                forwarded_button_inject,
                injected_down: false,
                staging_root,
                on_finished: Some(on_finished),
                dropped: false,
                reason: "panic",
                hr_u32: 0x0004_0101, // DRAGDROP_S_CANCEL
                effect_u32: 0,
                done_logged: false,
                cleaned: false,
            };

            let early_hook_release = take_pending_target_drag_early_release();
            if forwarded_button_inject && crate::kvm::hooks::is_hook_active() {
                let phys_held = crate::kvm::hooks::is_physical_button_down(button);
                if !phys_held {
                    ctrl.remote_button_down.store(false, Ordering::Release);
                    REMOTE_DRAG_BUTTON_DOWN.store(false, Ordering::Release);
                }
            }
            if let Some(cancel_reason) = ctrl.effective_cancel_reason() {
                guard.reason = cancel_reason;
                guard.run_cleanup();
                return;
            }
            if early_hook_release
                || (forwarded_button_inject && !ctrl.remote_button_down.load(Ordering::Acquire))
            {
                guard.reason = "released_before_start";
                guard.run_cleanup();
                return;
            }

            crate::kvm::debug_log::note(|| {
                format!(
                    "TARGET_DND_START id={} files={} button={} fwd_inject={}",
                    ctrl.drag_id,
                    paths.len(),
                    button,
                    forwarded_button_inject,
                )
            });

            guard.ole_initialized = OleInitialize(None).is_ok();
            let tid = GetCurrentThreadId();
            ctrl.tid.store(tid, Ordering::Release);
            TARGET_DRAG_TID.store(tid, Ordering::Release);

            let (pre_cap_hwnd, _, pre_cap_proc, _, _) =
                crate::kvm::debug_log::query_capture_snapshot();
            if pre_cap_hwnd != 0 {
                crate::kvm::debug_log::note(|| {
                    format!(
                        "TARGET_DND_PRE_CAPTURE id={} cap_hwnd=0x{:X} cap_proc={}",
                        ctrl.drag_id, pre_cap_hwnd, pre_cap_proc
                    )
                });
            }

            let class_name = w!("AnyLeapDragSource");
            register_drag_source_class(class_name);
            let (init_cx, init_cy) = super::super::injector::get_cursor_pos().unwrap_or((0, 0));
            let hwnd = create_drag_source_window(class_name, init_cx - 32, init_cy - 32, 64, 64);
            guard.hwnd = hwnd;

            let mut cx = init_cx;
            let mut cy = init_cy;
            let mut hit_ok = false;
            if let Some(h) = hwnd {
                use windows::Win32::Foundation::POINT;
                use windows::Win32::UI::WindowsAndMessaging::{
                    WindowFromPoint, HWND_TOPMOST, SWP_NOACTIVATE, SWP_SHOWWINDOW,
                };
                let raw = h.0 as isize;
                ctrl.hwnd.store(raw, Ordering::Release);
                TARGET_DRAG_HWND.store(raw, Ordering::Release);
                for attempt in 0..2 {
                    let (px, py) = super::super::injector::get_cursor_pos().unwrap_or((cx, cy));
                    cx = px;
                    cy = py;
                    let _ = SetWindowPos(
                        h,
                        Some(HWND_TOPMOST),
                        cx - 32,
                        cy - 32,
                        64,
                        64,
                        SWP_SHOWWINDOW | SWP_NOACTIVATE,
                    );
                    pump_drag_window_messages();
                    let hit = WindowFromPoint(POINT { x: cx, y: cy });
                    if hit == h {
                        hit_ok = true;
                        break;
                    }
                    if attempt == 1 {
                        crate::kvm::debug_log::note(|| {
                            format!(
                                "TARGET_DND_HITTEST_FAIL id={} expected=0x{:X} actual=0x{:X} cx={cx} cy={cy}",
                                ctrl.drag_id,
                                h.0 as usize,
                                hit.0 as usize,
                            )
                        });
                    }
                }
            }

            if forwarded_button_inject && !hit_ok {
                guard.reason = "hittest_blocked";
                guard.run_cleanup();
                return;
            }

            if forwarded_button_inject {
                use windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState;
                let early_after_win = take_pending_target_drag_early_release();
                let hook_released = crate::kvm::hooks::is_hook_active()
                    && !crate::kvm::hooks::is_physical_button_down(button);
                if let Some(cancel_reason) = ctrl.effective_cancel_reason() {
                    guard.reason = cancel_reason;
                    guard.run_cleanup();
                    return;
                }
                if early_after_win
                    || hook_released
                    || !ctrl.remote_button_down.load(Ordering::Acquire)
                {
                    guard.reason = "released_before_start";
                    guard.run_cleanup();
                    return;
                }

                REMOTE_DRAG_BUTTON.store(button, Ordering::Relaxed);
                REMOTE_DRAG_BUTTON_DOWN.store(true, Ordering::Release);
                let (sent, err) = super::super::injector::inject_mouse_button(button, true);
                guard.injected_down = sent > 0;
                let vk = vk_for_mouse_button(button) as i32;
                let mut msg = MSG::default();
                let mut thread_key_down = false;
                for _ in 0..20 {
                    while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                    if (GetKeyState(vk) as u16 & 0x8000) != 0 {
                        thread_key_down = true;
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(4));
                }
                crate::kvm::debug_log::note(|| {
                    format!(
                        "TARGET_DND_BTN_DOWN id={} button={} sent={sent} err={err} cx={cx} cy={cy} hit_ok={hit_ok} thread_key_down={thread_key_down}",
                        ctrl.drag_id, button,
                    )
                });
            }

            // Take explicit capture on our drag window before DoDragDrop, logging if another
            // window held capture; then mark the window click-through for drop targets beneath.
            if let Some(h) = hwnd {
                use windows::Win32::UI::Input::KeyboardAndMouse::{GetCapture, SetCapture};
                let (other_cap, _, other_proc, _, _) =
                    crate::kvm::debug_log::query_capture_snapshot();
                if other_cap != 0 && other_cap != h.0 as isize {
                    crate::kvm::debug_log::note(|| {
                        format!(
                            "TARGET_DND_OTHER_CAPTURE id={} cap_hwnd=0x{:X} cap_proc={}",
                            ctrl.drag_id, other_cap, other_proc
                        )
                    });
                }
                let _ = SetCapture(h);
                let self_capture = GetCapture() == h;
                crate::kvm::debug_log::note(|| {
                    format!(
                        "TARGET_DND_CAPTURE id={} self_capture={self_capture}",
                        ctrl.drag_id
                    )
                });

                let ex = GetWindowLongW(h, GWL_EXSTYLE);
                let _ = SetWindowLongW(h, GWL_EXSTYLE, ex | WS_EX_TRANSPARENT.0 as i32);
                let _ = SetWindowPos(
                    h,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_NOZORDER | SWP_FRAMECHANGED,
                );
                guard.timer_armed = SetTimer(Some(h), DRAG_WATCHDOG_TIMER_ID, 50, None) != 0;
            }

            CURRENT_DRAG_CTRL.with(|cell| {
                *cell.borrow_mut() = Some(Arc::clone(&ctrl));
            });

            ctrl.mark_drag_loop_entered();
            let watchdog_handle = spawn_target_drag_watchdog(Arc::clone(&ctrl));

            let data_obj: IDataObject = DropDataObject::new(&paths).into();
            let drop_src: IDropSource = DropSource {
                expected_mk: mk_flag_for_mouse_button(button),
                ctrl: Arc::clone(&ctrl),
            }
            .into();
            let mut effect = DROPEFFECT_NONE;
            let hr = DoDragDrop(&data_obj, &drop_src, DROPEFFECT_COPY, &mut effect);
            ctrl.in_drag_loop.store(false, Ordering::Release);
            ctrl.stop_watchdog();
            if let Some(jh) = watchdog_handle {
                let _ = jh.join();
            }
            let dropped = hr == DRAGDROP_S_DROP;
            let reason = if dropped {
                "drop"
            } else {
                ctrl.take_decision_reason()
                    .or_else(|| ctrl.effective_cancel_reason())
                    .unwrap_or(if (hr.0 as u32 & 0x8000_0000) != 0 {
                        "ole_error"
                    } else {
                        "ole_cancel"
                    })
            };

            guard.dropped = dropped;
            guard.hr_u32 = hr.0 as u32;
            guard.effect_u32 = effect.0;
            guard.reason = reason;
            guard.run_cleanup();
        });
    }
}

#[cfg(windows)]
use win_ole::edge_catcher_thread_main;

/// Start the target-side `DoDragDrop` from a hidden window under the cursor.
pub fn start_target_drag_drop(
    drag_id: String,
    paths: Vec<PathBuf>,
    button: u8,
    forwarded_button_inject: bool,
    staging_root: PathBuf,
    on_finished: Box<dyn FnOnce(TargetDragResult) + Send>,
) {
    #[cfg(windows)]
    {
        win_ole::run_target_drag_drop(
            drag_id,
            paths,
            button,
            forwarded_button_inject,
            staging_root,
            on_finished,
        );
    }
    #[cfg(not(windows))]
    {
        let _ = (paths, button, forwarded_button_inject);
        super::staging::cleanup_staging_entry(&staging_root, &drag_id);
        on_finished(TargetDragResult {
            outcome: DragOutcome::Cancelled,
            effect: 0,
            reason: "unsupported_platform",
            notice: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_catcher_trigger_decision_respects_all_guards() {
        assert!(should_show_edge_catcher(true, true, false, 1, false, false));
        assert!(!should_show_edge_catcher(
            false, true, false, 1, false, false
        ));
        assert!(!should_show_edge_catcher(
            true, false, false, 1, false, false
        ));
        assert!(!should_show_edge_catcher(true, true, true, 1, false, false));
        assert!(!should_show_edge_catcher(
            true, true, false, 0, false, false
        ));
        assert!(!should_show_edge_catcher(true, true, false, 1, true, false));
        assert!(!should_show_edge_catcher(true, true, false, 1, false, true));
    }

    #[test]
    fn compute_seam_strip_builds_2px_edge_windows() {
        let mon = MonitorDisplay {
            id: "m1".into(),
            device_name: "m1".into(),
            x: 1920,
            y: -420,
            width: 1080,
            height: 1920,
            is_primary: false,
        };
        assert_eq!(
            compute_seam_strip(&mon, Edge::Left, 0, 1080, 2),
            Some(SeamStrip {
                x: 1920,
                y: 0,
                width: 2,
                height: 1080,
                side: Edge::Left,
            })
        );
        assert_eq!(
            compute_seam_strip(&mon, Edge::Right, -500, 2000, 2),
            Some(SeamStrip {
                x: 1920 + 1080 - 2,
                y: -420,
                width: 2,
                height: 1920,
                side: Edge::Right,
            })
        );
        assert_eq!(
            compute_seam_strip(&mon, Edge::Bottom, 1920, 3000, 2),
            Some(SeamStrip {
                x: 1920,
                y: -420 + 1920 - 2,
                width: 1080,
                height: 2,
                side: Edge::Bottom,
            })
        );
        assert_eq!(compute_seam_strip(&mon, Edge::Left, 500, 500, 2), None);
    }

    #[test]
    fn cursor_near_seam_strip_and_button_mapping() {
        let s = SeamStrip {
            x: 1918,
            y: 100,
            width: 2,
            height: 800,
            side: Edge::Right,
        };
        assert!(is_cursor_near_seam_strip((1919, 500), &s, 0));
        assert!(is_cursor_near_seam_strip((1902, 500), &s, 16));
        assert!(!is_cursor_near_seam_strip((1901, 500), &s, 16));
        assert!(!is_cursor_near_seam_strip((1919, 50), &s, 16));

        assert_eq!(button_from_grf_key_state(0x0001), 1);
        assert_eq!(button_from_grf_key_state(0x0002), 2);
        assert_eq!(button_from_grf_key_state(0x0010), 3);
        assert_eq!(vk_for_mouse_button(2), 0x02);
        assert_eq!(mk_flag_for_mouse_button(2), 0x0002);
    }

    #[test]
    fn target_drag_button_decisions_enforce_single_owner() {
        assert!(should_use_forwarded_drag_button_inject(true));
        assert!(!should_use_forwarded_drag_button_inject(false));

        assert!(is_drag_button_held_in_query(true, true, 0, 0x0001));
        assert!(!is_drag_button_held_in_query(true, false, 0x0001, 0x0001));
        assert!(is_drag_button_held_in_query(false, false, 0x0001, 0x0001));
        assert!(!is_drag_button_held_in_query(false, true, 0, 0x0001));

        assert!(should_release_target_drag_on_exit(true, true));
        assert!(!should_release_target_drag_on_exit(true, false));
        assert!(!should_release_target_drag_on_exit(false, true));
    }

    fn base_query() -> TargetDragQueryInput {
        TargetDragQueryInput {
            escape_pressed: false,
            cancel_reason: None,
            track_remote_button: true,
            remote_button_down: true,
            physical_button_down: true,
            seen_button_down: true,
            elapsed_ms: 50,
            neither_down_ms: None,
            max_duration_ms: TARGET_DRAG_MAX_DURATION_MS,
            neither_down_timeout_ms: TARGET_DRAG_NEITHER_BTN_CANCEL_MS,
        }
    }

    #[test]
    fn target_drag_termination_continues_while_buttons_held() {
        assert_eq!(
            decide_target_drag_continue(base_query()),
            TargetDragDecision::Continue
        );
        // Remote still held even if OS physical state is transiently 0.
        assert_eq!(
            decide_target_drag_continue(TargetDragQueryInput {
                physical_button_down: false,
                ..base_query()
            }),
            TargetDragDecision::Continue
        );
        // Local-only mode while physical button is down.
        assert_eq!(
            decide_target_drag_continue(TargetDragQueryInput {
                track_remote_button: false,
                remote_button_down: false,
                physical_button_down: true,
                ..base_query()
            }),
            TargetDragDecision::Continue
        );
    }

    #[test]
    fn target_drag_termination_drops_on_button_release() {
        // Tracked remote button released while synthetic OS press is active -> Drop.
        assert_eq!(
            decide_target_drag_continue(TargetDragQueryInput {
                remote_button_down: false,
                physical_button_down: true,
                ..base_query()
            }),
            TargetDragDecision::Drop
        );
        // Local-only mode after button was seen down and physical button released -> Drop.
        assert_eq!(
            decide_target_drag_continue(TargetDragQueryInput {
                track_remote_button: false,
                remote_button_down: false,
                physical_button_down: false,
                seen_button_down: true,
                neither_down_ms: Some(0),
                ..base_query()
            }),
            TargetDragDecision::Drop
        );
    }

    #[test]
    fn target_drag_termination_cancels_on_signals_neither_button_and_timeout() {
        assert_eq!(
            decide_target_drag_continue(TargetDragQueryInput {
                escape_pressed: true,
                ..base_query()
            }),
            TargetDragDecision::Cancel("escape")
        );
        for reason in [
            "drag_cancel",
            "transfer_cancel",
            "release_all",
            "emergency_release",
            "disconnect",
            "superseded",
        ] {
            assert_eq!(
                decide_target_drag_continue(TargetDragQueryInput {
                    cancel_reason: Some(reason),
                    ..base_query()
                }),
                TargetDragDecision::Cancel(reason)
            );
        }

        // Neither physical nor tracked remote button down: <= 300ms waits, > 300ms cancels.
        assert_eq!(
            decide_target_drag_continue(TargetDragQueryInput {
                remote_button_down: false,
                physical_button_down: false,
                neither_down_ms: Some(300),
                ..base_query()
            }),
            TargetDragDecision::Continue
        );
        assert_eq!(
            decide_target_drag_continue(TargetDragQueryInput {
                remote_button_down: false,
                physical_button_down: false,
                neither_down_ms: Some(301),
                ..base_query()
            }),
            TargetDragDecision::Cancel("no_buttons_down_timeout")
        );

        // Overall 60s timeout cancels even if remote button appears stuck down.
        assert_eq!(
            decide_target_drag_continue(TargetDragQueryInput {
                elapsed_ms: 59_999,
                ..base_query()
            }),
            TargetDragDecision::Continue
        );
        assert_eq!(
            decide_target_drag_continue(TargetDragQueryInput {
                elapsed_ms: 60_000,
                ..base_query()
            }),
            TargetDragDecision::Cancel("timeout")
        );
    }

    #[test]
    fn target_drag_control_tracks_neither_down_duration_and_cancel() {
        let ctrl = TargetDragControl::new(1, "d-1".into(), 1, true);
        assert_eq!(ctrl.update_neither_down(false, true, 100), None);
        assert_eq!(ctrl.update_neither_down(false, false, 150), Some(0));
        assert_eq!(ctrl.update_neither_down(false, false, 451), Some(301));
        assert_eq!(ctrl.update_neither_down(true, false, 460), None);
        assert_eq!(ctrl.update_neither_down(false, false, 470), Some(0));

        assert_eq!(ctrl.effective_cancel_reason(), None);
        ctrl.request_cancel("disconnect");
        assert_eq!(ctrl.effective_cancel_reason(), Some("disconnect"));
    }

    #[test]
    fn watchdog_cancel_covers_remote_up_query_stall_timeout_and_escape_guard() {
        let base = WatchdogEvalInput {
            elapsed_ms: 500,
            in_drag_loop: true,
            since_last_query_ms: 50,
            cancel_reason: None,
            track_remote_button: true,
            remote_button_down: true,
            remote_up_ms: None,
            physical_button_down: false,
            neither_down_ms: None,
            max_duration_ms: TARGET_DRAG_MAX_DURATION_MS,
            neither_down_timeout_ms: TARGET_DRAG_NEITHER_BTN_CANCEL_MS,
            query_stall_timeout_ms: TARGET_DRAG_QUERY_STALL_MS,
        };
        assert_eq!(decide_watchdog_cancel(base), None);

        // Remote button held up > 300ms cancels even if QueryContinueDrag has not run yet.
        assert_eq!(
            decide_watchdog_cancel(WatchdogEvalInput {
                remote_button_down: false,
                remote_up_ms: Some(300),
                ..base
            }),
            None
        );
        assert_eq!(
            decide_watchdog_cancel(WatchdogEvalInput {
                remote_button_down: false,
                remote_up_ms: Some(301),
                ..base
            }),
            Some("remote_button_up_timeout")
        );

        // QueryContinueDrag not called for > 1000ms while inside DoDragDrop forces cancel.
        assert_eq!(
            decide_watchdog_cancel(WatchdogEvalInput {
                since_last_query_ms: 1_000,
                ..base
            }),
            None
        );
        assert_eq!(
            decide_watchdog_cancel(WatchdogEvalInput {
                since_last_query_ms: 1_001,
                ..base
            }),
            Some("query_stall_timeout")
        );
        // Before DoDragDrop begins, query stall check stays inactive.
        assert_eq!(
            decide_watchdog_cancel(WatchdogEvalInput {
                in_drag_loop: false,
                since_last_query_ms: 5_000,
                ..base
            }),
            None
        );

        // 60s timeout and explicit cancel.
        assert_eq!(
            decide_watchdog_cancel(WatchdogEvalInput {
                elapsed_ms: 60_000,
                ..base
            }),
            Some("timeout")
        );
        assert_eq!(
            decide_watchdog_cancel(WatchdogEvalInput {
                cancel_reason: Some("release_all"),
                ..base
            }),
            Some("release_all")
        );

        // Escape SendInput guard: only when drag window/thread is foreground or owns capture.
        assert!(should_inject_escape_on_drag_timeout(true));
        assert!(!should_inject_escape_on_drag_timeout(false));
    }

    #[test]
    fn drag_drop_defaults_to_disabled_and_gates_edge_catcher() {
        let _lock = test_files_settings_lock();
        const {
            assert!(!DEFAULT_DRAG_DROP_ENABLED);
        }
        struct RestoreGuard;
        impl Drop for RestoreGuard {
            fn drop(&mut self) {
                set_files_settings(DEFAULT_DRAG_DROP_ENABLED, true, 2048, false, 100);
            }
        }
        let _restore = RestoreGuard;

        // Initial default state must have drag_drop disabled and file_clipboard enabled.
        assert!(!drag_drop_enabled());
        assert!(file_clipboard_enabled());

        // With default settings applied, drag_drop_enabled() must be false and edge catcher off.
        set_files_settings(DEFAULT_DRAG_DROP_ENABLED, true, 2048, false, 100);
        assert!(!drag_drop_enabled());
        assert!(file_clipboard_enabled());
        assert!(!should_show_edge_catcher(
            true,
            drag_drop_enabled(),
            false,
            1,
            false,
            false
        ));

        // Explicitly enabling turns it on; setting back off disables it cleanly.
        set_files_settings(true, true, 2048, false, 100);
        assert!(drag_drop_enabled());
        set_files_settings(false, true, 2048, false, 100);
        assert!(!drag_drop_enabled());
        assert!(!is_edge_catcher_visible());
    }

    #[test]
    fn watchdog_never_posts_mouse_up_or_produces_drop() {
        const WM_LBUTTONUP: u32 = 0x0202;
        const WM_RBUTTONUP: u32 = 0x0205;
        const WM_MBUTTONUP: u32 = 0x0208;
        let msgs = watchdog_cancel_messages();
        for m in &msgs {
            assert_ne!(m.msg, WM_LBUTTONUP);
            assert_ne!(m.msg, WM_RBUTTONUP);
            assert_ne!(m.msg, WM_MBUTTONUP);
        }

        // Whenever the watchdog produces a cancel reason, passing it to decide_target_drag_continue
        // must result in Cancel(...) even if remote/physical button states are already released.
        for reason in [
            "remote_button_up_timeout",
            "no_buttons_down_timeout",
            "query_stall_timeout",
            "timeout",
            "release_all",
        ] {
            assert_eq!(
                decide_target_drag_continue(TargetDragQueryInput {
                    cancel_reason: Some(reason),
                    remote_button_down: false,
                    physical_button_down: false,
                    seen_button_down: true,
                    ..base_query()
                }),
                TargetDragDecision::Cancel(reason)
            );
        }
    }

    #[test]
    fn classify_target_drag_outcome_maps_all_terminal_states() {
        const DRAGDROP_S_DROP_U32: u32 = 0x0004_0100;
        const DRAGDROP_S_CANCEL_U32: u32 = 0x0004_0101;
        const E_FAIL_U32: u32 = 0x8000_4005;

        assert_eq!(
            classify_target_drag_outcome(DRAGDROP_S_DROP_U32, 1, "drop"),
            (DragOutcome::Done, None)
        );
        assert_eq!(
            classify_target_drag_outcome(DRAGDROP_S_DROP_U32, 0, "drop"),
            (
                DragOutcome::Failed,
                Some("Drop was not accepted by the target app".to_string()),
            )
        );
        assert_eq!(
            classify_target_drag_outcome(E_FAIL_U32, 0, "ole_cancel"),
            (
                DragOutcome::Failed,
                Some("Drag failed (OLE error 0x80004005)".to_string()),
            )
        );
        assert_eq!(
            classify_target_drag_outcome(DRAGDROP_S_CANCEL_U32, 0, "remote_button_up_timeout"),
            (
                DragOutcome::Failed,
                Some("Drag failed: mouse button state was lost before drop".to_string()),
            )
        );
        assert_eq!(
            classify_target_drag_outcome(DRAGDROP_S_CANCEL_U32, 0, "query_stall_timeout"),
            (
                DragOutcome::Failed,
                Some("Drag timed out before drop".to_string()),
            )
        );
        for reason in [
            "drag_cancel",
            "transfer_cancel",
            "escape",
            "disabled",
            "ole_cancel",
        ] {
            assert_eq!(
                classify_target_drag_outcome(DRAGDROP_S_CANCEL_U32, 0, reason),
                (DragOutcome::Cancelled, None)
            );
        }
    }

    #[test]
    fn drag_transfer_registry_supports_terminal_states_and_cancel_at_any_stage() {
        let reg = DragTransferRegistry::new();

        // 1. Cancel during staging emits terminal cancelled once.
        let init = reg.register_staging("d-stage", "photo.png", 1024);
        assert_eq!(init.pct, 0);
        assert_eq!(init.state, "active");
        assert_eq!(reg.get_stage("d-stage"), Some(DragStage::Staging));
        assert_eq!(
            reg.cancel_at_any_stage("d-stage"),
            CancelDragAction::Cancelled(DragProgressPayload {
                id: "d-stage".into(),
                name: "photo.png".into(),
                pct: 0,
                state: "cancelled",
                size_bytes: 1024,
            })
        );
        assert!(reg.is_finished("d-stage"));
        assert!(reg
            .complete_terminal("d-stage", DragOutcome::Done)
            .is_none());

        // 2. Cancel during waiting-for-drop emits terminal cancelled once.
        reg.register_staging("d-wait", "doc.pdf", 2048);
        let wait = reg.mark_waiting_for_drop("d-wait").unwrap();
        assert_eq!(wait.pct, 99);
        assert_eq!(wait.state, "active");
        assert_eq!(reg.get_stage("d-wait"), Some(DragStage::WaitingForDrop));
        assert_eq!(
            reg.cancel_at_any_stage("d-wait"),
            CancelDragAction::Cancelled(DragProgressPayload {
                id: "d-wait".into(),
                name: "doc.pdf".into(),
                pct: 0,
                state: "cancelled",
                size_bytes: 2048,
            })
        );

        // 3. Done -> cancel_at_any_stage clears finished entry without downgrading outcome.
        reg.register_staging("d-done", "clip.mp4", 4096);
        let done_ev = reg.complete_terminal("d-done", DragOutcome::Done).unwrap();
        assert_eq!(done_ev.pct, 100);
        assert_eq!(done_ev.state, "done");
        assert_eq!(
            reg.cancel_at_any_stage("d-done"),
            CancelDragAction::ClearedFinished
        );
        assert_eq!(
            reg.cancel_at_any_stage("d-done"),
            CancelDragAction::NotTracked
        );

        // 4. Failed -> cancel_at_any_stage clears finished entry without re-emitting.
        reg.register_staging("d-fail", "bad.png", 512);
        let fail_ev = reg
            .complete_terminal("d-fail", DragOutcome::Failed)
            .unwrap();
        assert_eq!(fail_ev.pct, 0);
        assert_eq!(fail_ev.state, "failed");
        assert_eq!(
            reg.cancel_at_any_stage("d-fail"),
            CancelDragAction::ClearedFinished
        );
        assert_eq!(
            reg.cancel_at_any_stage("d-fail"),
            CancelDragAction::NotTracked
        );
    }

    #[cfg(windows)]
    #[ignore = "Requires an interactive Windows desktop session"]
    #[test]
    fn target_drag_terminates_within_2s_when_wedged_or_capture_owned() {
        use std::sync::{
            atomic::{AtomicBool, Ordering},
            mpsc, Arc,
        };
        use windows::core::w;
        use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, PeekMessageW,
            RegisterClassExW, SetLayeredWindowAttributes, ShowWindow, TranslateMessage, LWA_ALPHA,
            MSG, PM_REMOVE, SW_SHOWNOACTIVATE, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
            WS_EX_TOPMOST, WS_POPUP,
        };

        unsafe extern "system" fn helper_wndproc(
            hwnd: HWND,
            msg: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }

        let tmp_dir =
            std::env::temp_dir().join(format!("anyleap-drag-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp_dir);
        let log_path = tmp_dir.join("gate-debug.log");
        crate::kvm::debug_log::set_enabled(true, log_path.clone());
        let staging_a = tmp_dir.join("staging-a");
        let entry_a = staging_a.join("gate-cap-owned");
        std::fs::create_dir_all(&entry_a).unwrap();
        let file_a = entry_a.join("sample.txt");
        std::fs::write(&file_a, b"spec25a-r5-gate").unwrap();

        let (cx, cy) = crate::kvm::injector::get_cursor_pos().unwrap_or((200, 200));

        // Failure condition 1a: another thread owns mouse capture while off-cursor so
        // DoDragDrop enters and stalls/conflicts -> watchdog must force exit <= 2s.
        let stop_helper = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_helper);
        let (cap_tx, cap_rx) = mpsc::channel::<()>();
        let helper = std::thread::spawn(move || unsafe {
            let class_name = w!("AnyLeapTestCaptureOwner");
            let hinst = GetModuleHandleW(None).unwrap_or_default();
            let wc = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(helper_wndproc),
                hInstance: hinst.into(),
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
                class_name,
                w!(""),
                WS_POPUP,
                cx + 300,
                cy + 300,
                32,
                32,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .unwrap();
            let _ = SetLayeredWindowAttributes(
                hwnd,
                windows::Win32::Foundation::COLORREF(0),
                1,
                LWA_ALPHA,
            );
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetCapture(hwnd);
            let _ = cap_tx.send(());
            let mut msg = MSG::default();
            while !stop_clone.load(Ordering::Acquire) {
                while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = ReleaseCapture();
            let _ = DestroyWindow(hwnd);
        });
        cap_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let (done_a_tx, done_a_rx) = mpsc::channel::<TargetDragResult>();
        let t_a = Instant::now();
        start_target_drag_drop(
            "gate-cap-owned".to_string(),
            vec![file_a],
            1,
            true,
            staging_a,
            Box::new(move |res| {
                let _ = done_a_tx.send(res);
            }),
        );
        let res_a = done_a_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("target drag with external capture must exit <= 2s");
        let elapsed_a = t_a.elapsed();
        stop_helper.store(true, Ordering::Release);
        let _ = helper.join();
        assert!(
            elapsed_a <= Duration::from_secs(2),
            "elapsed_a={elapsed_a:?} exceeded 2s"
        );
        assert_ne!(res_a.outcome, DragOutcome::Done);
        assert!(!is_local_drag_source_active());

        // Failure condition 1b: tracked remote button releases while DoDragDrop receives no input
        // -> watchdog must force exit via remote_button_up_timeout (>300ms) within <= 2s.
        let staging_b = tmp_dir.join("staging-b");
        let entry_b = staging_b.join("gate-remote-up-stalled");
        std::fs::create_dir_all(&entry_b).unwrap();
        let file_b = entry_b.join("sample.txt");
        std::fs::write(&file_b, b"spec25a-r5-gate").unwrap();

        let (done_b_tx, done_b_rx) = mpsc::channel::<TargetDragResult>();
        let t_b = Instant::now();
        start_target_drag_drop(
            "gate-remote-up-stalled".to_string(),
            vec![file_b],
            1,
            true,
            staging_b,
            Box::new(move |res| {
                let _ = done_b_tx.send(res);
            }),
        );
        std::thread::sleep(Duration::from_millis(200));
        set_remote_drag_button_state(1, false);
        let res_b = done_b_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("target drag with released button must exit <= 2s");
        let elapsed_b = t_b.elapsed();
        assert!(elapsed_b <= Duration::from_secs(2));
        assert_ne!(res_b.outcome, DragOutcome::Done);
        assert!(!is_local_drag_source_active());

        // Failure condition 2: no button state visible (forwarded_button_inject=false, physical
        // button up, no input to DoDragDrop) -> watchdog terminates <= 2s.
        let staging_c = tmp_dir.join("staging-c");
        let entry_c = staging_c.join("gate-no-btn");
        std::fs::create_dir_all(&entry_c).unwrap();
        let file_c = entry_c.join("sample.txt");
        std::fs::write(&file_c, b"spec25a-r5-gate").unwrap();

        let (done_c_tx, done_c_rx) = mpsc::channel::<TargetDragResult>();
        let t_c = Instant::now();
        start_target_drag_drop(
            "gate-no-btn".to_string(),
            vec![file_c],
            1,
            false,
            staging_c,
            Box::new(move |res| {
                let _ = done_c_tx.send(res);
            }),
        );
        let res_c = done_c_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("target drag with no button state visible must exit <= 2s");
        let elapsed_c = t_c.elapsed();
        assert!(
            elapsed_c <= Duration::from_secs(2),
            "elapsed_c={elapsed_c:?} exceeded 2s"
        );
        assert_ne!(res_c.outcome, DragOutcome::Done);
        assert!(!is_local_drag_source_active());
        let snap = dnd_state_snapshot();
        assert_eq!(snap.target_tid, 0);
        assert!(!snap.local_src_active);
        assert!(!snap.remote_btn_down);

        eprintln!(
            "[gate] target_drag_terminates_within_2s: cap_owned={elapsed_a:?} ({:?}/{}) remote_up_stalled={elapsed_b:?} ({:?}/{}) no_btn_state={elapsed_c:?} ({:?}/{}) local_src_active={}",
            res_a.outcome, res_a.reason,
            res_b.outcome, res_b.reason,
            res_c.outcome, res_c.reason,
            is_local_drag_source_active()
        );
        crate::kvm::debug_log::set_enabled(false, log_path);
        let _ = std::fs::remove_dir_all(&tmp_dir);
    }
}
