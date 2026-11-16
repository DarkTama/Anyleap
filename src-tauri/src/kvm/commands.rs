use super::client::{handshake, send_delta, KvmClientHandle};
use super::clipboard::{set_clipboard_image, set_clipboard_text, start_clipboard_sync};
use super::debug_log;
use super::discovery::DiscoveredNode;
use super::files::{send_file, send_image};
use super::identity::{Identity, TrustStore};
use super::injector;
use super::input_logic::{
    compute_linked_seams, handle_monitor_change_fallback, has_any_overlap, map_cursor_enter_spec24,
    monitors_changed, normalize_in_union, receiver_step_with_dwell, receiver_step_with_seams,
    resolve_remote_origin, union_rect, Edge, PressedInputTracker, ReceiverSeamStep, ReceiverStep,
};
use super::monitors::{get_local_monitors, MonitorDisplay};
use super::protocol::{
    ComputerNode, KvmPacket, KvmStatus, LinkedPeerInfo, LinkedSeam, Outbound, ScreenRect,
    TopologyConfig, TransferEvent, TrustedPeerInfo, UdpMouseDelta,
};
use super::server::start_kvm_server;
use crate::state::{AppState, KvmState};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

const KVM_PORT: u16 = 4177;

/// Receiver side: a remote controller currently drives this PC's cursor (UI status only).
static RECEIVER_ENTERED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct ReceiverSessionState {
    is_entered: bool,
    entry_edge: Edge,
    dwell_ms: u32,
    deadzone_px: i32,
    dwell_start_ms: Option<u64>,
    dwell_seam: Option<usize>,
    seams: Vec<LinkedSeam>,
    has_seams: bool,
    session_epoch: Instant,
    monitors: Vec<MonitorDisplay>,
    union: ScreenRect,
    last_pos_emit: Option<Instant>,
    tracker: PressedInputTracker,
    // --- SPEC-25a ---
    buttons_mask: u8,
    drag_edge_hold_until: Option<Instant>,
    // --- end SPEC-25a ---
}

impl ReceiverSessionState {
    fn new(monitors: Vec<MonitorDisplay>) -> Self {
        let union = union_rect(&monitors);
        Self {
            is_entered: false,
            entry_edge: Edge::Left,
            dwell_ms: 0,
            deadzone_px: 0,
            dwell_start_ms: None,
            dwell_seam: None,
            seams: Vec::new(),
            has_seams: false,
            session_epoch: Instant::now(),
            monitors,
            union,
            last_pos_emit: None,
            tracker: PressedInputTracker::new(),
            // --- SPEC-25a ---
            buttons_mask: 0,
            drag_edge_hold_until: None,
            // --- end SPEC-25a ---
        }
    }
}

fn release_tracked_inputs(keys: Vec<super::input_logic::TrackedKey>, buttons: Vec<u8>) {
    for b in buttons {
        let _ = injector::inject_mouse_button(b, false);
    }
    for k in keys {
        let _ = injector::inject_key(k.key_code, k.scan_code, false, k.extended);
    }
}

fn release_receiver_inputs(rx_state: &Arc<Mutex<ReceiverSessionState>>, reason: &'static str) {
    let released = if let Ok(mut st) = rx_state.lock() {
        st.is_entered = false;
        RECEIVER_ENTERED.store(false, std::sync::atomic::Ordering::Relaxed);
        st.dwell_start_ms = None;
        st.last_pos_emit = None;
        Some(st.tracker.release_all())
    } else {
        None
    };
    let (released_keys, released_buttons) = match released {
        Some((keys, buttons)) => {
            let kc = keys.len();
            let bc = buttons.len();
            release_tracked_inputs(keys, buttons);
            (kc, bc)
        }
        None => (0, 0),
    };
    if debug_log::is_enabled() {
        debug_log::record_event(debug_log::DebugEvent::ReleaseAll {
            ts_us: debug_log::now_us(),
            reason,
            released_keys,
            released_buttons,
        });
    }
}

fn get_download_dir() -> std::path::PathBuf {
    std::env::var("USERPROFILE")
        .map(|p| {
            std::path::PathBuf::from(p)
                .join("Downloads")
                .join("AnyLeap_Transfers")
        })
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Identity + trust store, created on first use under the app data dir.
fn get_security(
    app: &AppHandle,
    state: &AppState,
) -> Result<(Arc<Identity>, Arc<TrustStore>), String> {
    let mut kvm = state.kvm.lock().unwrap();
    if kvm.security.is_none() {
        let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
        kvm.security = Some((
            Arc::new(Identity::load_or_create(&dir)?),
            Arc::new(TrustStore::load(Some(&dir))),
        ));
    }
    Ok(kvm.security.clone().unwrap())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn emit_clipboard_text(app: &AppHandle, text: &str) {
    if !super::clipboard::sync_text_enabled() {
        return;
    }
    let _ = set_clipboard_text(text);
    let preview = if text.len() > 32 {
        format!("{}...", text.chars().take(32).collect::<String>())
    } else {
        text.to_string()
    };
    let _ = app.emit(
        "kvm-activity",
        serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "timestamp": now_ms(),
            "type": "text",
            "name": format!("Text: \"{}\"", preview),
            "sizeBytes": text.len(),
            "progressPercent": 100,
            "status": "completed"
        }),
    );
}

pub(crate) fn emit_transfer_progress(
    app: &AppHandle,
    id: &str,
    kind: &str,
    name: &str,
    pct: u8,
    state: &str,
    size_bytes: u64,
) {
    let _ = app.emit(
        "kvm-transfer-progress",
        serde_json::json!({
            "id": id,
            "kind": kind,
            "name": name,
            "pct": pct.min(100),
            "state": state,
            "sizeBytes": size_bytes,
        }),
    );
}

fn emit_drag_progress_payload(app: &AppHandle, ev: &super::dragdrop::DragProgressPayload) {
    emit_transfer_progress(
        app,
        &ev.id,
        "drag",
        &ev.name,
        ev.pct,
        ev.state,
        ev.size_bytes,
    );
}

fn finish_target_drag_and_notify(
    app: &AppHandle,
    drag_id: &str,
    res: super::dragdrop::TargetDragResult,
) {
    if let Some(ev) =
        super::dragdrop::global_drag_registry().complete_terminal(drag_id, res.outcome)
    {
        emit_drag_progress_payload(app, &ev);
    }
    if let Some(ref msg) = res.notice {
        emit_continuity_notice(app, msg);
    }
    send_pkt_from_app(
        app,
        KvmPacket::DragResult {
            drag_id: drag_id.to_string(),
            outcome: res.outcome,
            notice: res.notice,
        },
    );
}

fn emit_transfer(app: &AppHandle, ev: TransferEvent) {
    let item = match ev {
        TransferEvent::ClipboardImage { data } => {
            if !super::clipboard::sync_image_enabled() {
                return;
            }
            let _ = set_clipboard_image(&data);
            serde_json::json!({
                "id": uuid::Uuid::new_v4().to_string(),
                "timestamp": now_ms(),
                "type": "image",
                "name": format!("Image ({:.1} MB)", data.len() as f32 / 1_000_000.0),
                "sizeBytes": data.len(),
                "progressPercent": 100,
                "status": "completed"
            })
        }
        TransferEvent::File { name, size } => serde_json::json!({
            "id": uuid::Uuid::new_v4().to_string(),
            "timestamp": now_ms(),
            "type": "file",
            "name": name,
            "sizeBytes": size,
            "progressPercent": 100,
            "status": "completed",
            "savedPath": get_download_dir().display().to_string()
        }),
        // --- SPEC-25c ---
        TransferEvent::OpenFile {
            transfer_id,
            name,
            size,
            saved_path,
        } => {
            super::open_on_linked::handle_open_file_transfer_ready(
                app,
                &transfer_id,
                saved_path.clone(),
            );
            serde_json::json!({
                "id": transfer_id,
                "timestamp": now_ms(),
                "type": "file",
                "name": name,
                "sizeBytes": size,
                "progressPercent": 100,
                "status": "completed",
                "savedPath": saved_path.display().to_string()
            })
        }
        TransferEvent::Progress {
            id,
            kind,
            name,
            pct,
            state,
            size_bytes,
        } => {
            if kind == "drag" && super::dragdrop::global_drag_registry().is_finished(&id) {
                return;
            }
            let emit_pct = if kind == "drag" && state == "active" {
                pct.min(99)
            } else {
                pct
            };
            emit_transfer_progress(app, &id, &kind, &name, emit_pct, &state, size_bytes);
            return;
        }
        TransferEvent::Cancelled {
            id,
            kind,
            name,
            size_bytes,
        } => {
            if kind == "drag" {
                return;
            }
            emit_transfer_progress(app, &id, &kind, &name, 0, "cancelled", size_bytes);
            return;
        }
        TransferEvent::Failed {
            id,
            kind,
            name,
            reason,
        } => {
            if kind == "drag" {
                return;
            }
            emit_transfer_progress(app, &id, &kind, &name, 0, "failed", 0);
            if kind == "open" {
                let _ = id;
                emit_continuity_notice(
                    app,
                    &format!("Open-on-Linked transfer failed ({name}): {reason}"),
                );
            }
            return;
        }
        // --- end SPEC-25c ---
        // --- SPEC-25a ---
        TransferEvent::DragFile { .. }
        | TransferEvent::ClipFile { .. }
        | TransferEvent::CursorFile { .. } => return,
        // --- end SPEC-25a ---
    };
    let _ = app.emit("kvm-activity", item);
}

// --- SPEC-25a ---
#[derive(Debug, Clone)]
struct PendingTargetDrag {
    drag_id: String,
    button: u8,
    forwarded_button_inject: bool,
    released_early: bool,
    is_fallback: bool,
}

struct PendingOutboundDrag {
    drag_id: String,
    files: Vec<super::protocol::FileMeta>,
    sources: Vec<(String, std::path::PathBuf)>,
    conn: quinn::Connection,
    pkt_tx: tokio::sync::mpsc::Sender<KvmPacket>,
    app: AppHandle,
}

static PENDING_TARGET_DRAG: std::sync::OnceLock<Mutex<Option<PendingTargetDrag>>> =
    std::sync::OnceLock::new();
static PENDING_OUTBOUND_DRAG: std::sync::OnceLock<Mutex<Option<PendingOutboundDrag>>> =
    std::sync::OnceLock::new();

fn pending_target_drag() -> &'static Mutex<Option<PendingTargetDrag>> {
    PENDING_TARGET_DRAG.get_or_init(|| Mutex::new(None))
}

fn pending_outbound_drag() -> &'static Mutex<Option<PendingOutboundDrag>> {
    PENDING_OUTBOUND_DRAG.get_or_init(|| Mutex::new(None))
}

pub(crate) fn emit_continuity_notice(app: &AppHandle, text: &str) {
    let _ = app.emit("kvm-continuity-notice", serde_json::json!({ "text": text }));
}

fn summarize_offer_name(files: &[super::protocol::FileMeta]) -> String {
    let count = files.iter().filter(|f| !f.is_dir).count().max(files.len());
    match files.first() {
        Some(first) if count <= 1 => first.rel_path.clone(),
        Some(first) => format!("{} (+{} more)", first.rel_path, count.saturating_sub(1)),
        None => "Files".to_string(),
    }
}

fn handle_incoming_drag_offer(
    app: &AppHandle,
    drag_id: String,
    files: Vec<super::protocol::FileMeta>,
    button: u8,
    forwarded_button_inject: bool,
) {
    if !super::dragdrop::drag_drop_enabled() {
        super::debug_log::note(|| format!("DRAG_OFFER_RECV_IGNORED id={drag_id} reason=disabled"));
        send_pkt_from_app(
            app,
            KvmPacket::DragResult {
                drag_id,
                outcome: super::protocol::DragOutcome::Cancelled,
                notice: Some("Drag-and-drop is disabled on the linked PC".to_string()),
            },
        );
        return;
    }
    let button = button.clamp(1, 3);
    let staging_root = super::staging::default_staging_root();
    let download_dir = get_download_dir();
    let limit_mb = super::staging::staging_limit_mb();
    let summary = summarize_offer_name(&files);
    let mut initial_released_early = false;
    if forwarded_button_inject {
        let held = if super::hooks::is_hook_active() {
            super::hooks::is_physical_button_down(button)
        } else {
            true
        };
        initial_released_early = !held;
        super::dragdrop::set_pending_target_drag_early_release(!held);
        super::dragdrop::set_remote_drag_button_state(button, held);
    } else {
        super::dragdrop::set_pending_target_drag_early_release(false);
    }

    match super::staging::register_transfer_offer(
        &drag_id,
        super::staging::TransferKind::Drag,
        &files,
        &staging_root,
        &download_dir,
        limit_mb,
    ) {
        Ok(plan) => {
            super::debug_log::note(|| {
                format!(
                    "DRAG_OFFER_RECV id={drag_id} files={} expected_files={} bytes={} button={button} fwd_inject={forwarded_button_inject} released_early={initial_released_early} dest={:?}",
                    files.len(),
                    plan.expected_files,
                    plan.total_bytes,
                    plan.destination,
                )
            });
            let is_fallback =
                plan.destination == super::staging::TransferDestination::DownloadsFallback;
            let reg_ev = super::dragdrop::global_drag_registry().register_staging(
                &drag_id,
                &summary,
                plan.total_bytes,
            );
            if is_fallback {
                if forwarded_button_inject {
                    super::dragdrop::set_remote_drag_button_state(button, false);
                }
                emit_continuity_notice(
                    app,
                    &format!(
                        "Drag exceeds {limit_mb} MB staging limit — saving \"{summary}\" to Downloads\\AnyLeap_Transfers"
                    ),
                );
            } else {
                emit_drag_progress_payload(app, &reg_ev);
            }
            if let Ok(mut g) = pending_target_drag().lock() {
                *g = Some(PendingTargetDrag {
                    drag_id: drag_id.clone(),
                    button,
                    forwarded_button_inject,
                    released_early: initial_released_early && !is_fallback,
                    is_fallback,
                });
            }
            if plan.expected_files == 0 && !is_fallback {
                if let Ok(mut g) = pending_target_drag().lock() {
                    *g = None;
                }
                if let Some(ev) =
                    super::dragdrop::global_drag_registry().mark_waiting_for_drop(&drag_id)
                {
                    emit_drag_progress_payload(app, &ev);
                }
                send_pkt_from_app(
                    app,
                    KvmPacket::DragReady {
                        drag_id: drag_id.clone(),
                    },
                );
                let app_cb = app.clone();
                let drag_id_cb = drag_id.clone();
                super::dragdrop::start_target_drag_drop(
                    drag_id,
                    plan.top_level_paths,
                    button,
                    forwarded_button_inject,
                    staging_root,
                    Box::new(move |res| {
                        finish_target_drag_and_notify(&app_cb, &drag_id_cb, res);
                    }),
                );
            }
        }
        Err(e) => {
            super::debug_log::note(|| format!("DRAG_OFFER_RECV_ERR id={drag_id} err={e}"));
            let msg = format!("Drag rejected: {e}");
            emit_continuity_notice(app, &msg);
            send_pkt_from_app(
                app,
                KvmPacket::DragResult {
                    drag_id,
                    outcome: super::protocol::DragOutcome::Failed,
                    notice: Some(msg),
                },
            );
        }
    }
}

fn send_pkt_from_app(app: &AppHandle, pkt: KvmPacket) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(kvm) = state.kvm.lock() else {
        return;
    };
    if let Some(ref client) = kvm.client {
        if client
            .is_connected
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            let _ = client.send_packet_tx.try_send(pkt);
            return;
        }
    }
    if let Some(ref server) = kvm.server {
        if server.is_running.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = server.outgoing_tx.send(Outbound::Packet(pkt));
        }
    }
}

fn replace_previous_clip_offer(app: &AppHandle, new_offer_id: &str) {
    let Some(prev_id) = super::staging::swap_previous_clip_offer(new_offer_id) else {
        return;
    };
    super::debug_log::note(|| format!("CLIP_REPLACE_PREV prev={prev_id} next={new_offer_id}"));
    let mut emitted_cancel = false;
    if let Some(pending) = super::staging::take_pending_inbound_clip(&prev_id) {
        emit_transfer_progress(
            app,
            &prev_id,
            "clip",
            &pending.summary,
            0,
            "cancelled",
            pending.total_bytes,
        );
        emitted_cancel = true;
    }
    let _ = super::staging::take_pending_outbound_clip(&prev_id);
    let cancelled = super::files::global_cancel_registry().cancel_any_conn(&prev_id);
    if !cancelled.is_empty() {
        for (_, meta) in &cancelled {
            if !emitted_cancel {
                emit_transfer_progress(
                    app,
                    &meta.id,
                    &meta.kind,
                    &meta.name,
                    0,
                    "cancelled",
                    meta.size_bytes,
                );
                emitted_cancel = true;
            }
        }
        send_pkt_from_app(
            app,
            KvmPacket::TransferCancel {
                id: prev_id.clone(),
            },
        );
    }
    if let Some((kind, total_bytes)) = super::staging::cancel_transfer_with_info(&prev_id) {
        if !emitted_cancel {
            emit_transfer_progress(
                app,
                &prev_id,
                match kind {
                    super::staging::TransferKind::Drag => "drag",
                    super::staging::TransferKind::Clip => "clip",
                },
                "Files",
                0,
                "cancelled",
                total_bytes,
            );
        }
    }
    let staging_root = super::staging::default_staging_root();
    super::staging::cleanup_staging_entry(&staging_root, &prev_id);
}

fn spawn_outbound_pending_clip(app: &AppHandle, pending: super::staging::PendingOutboundClip) {
    super::files::register_outbound_total(&pending.offer_id, pending.total_bytes);
    super::debug_log::note(|| {
        format!(
            "CLIP_FETCH_REQ side=sender id={} files={} bytes={}",
            pending.offer_id,
            pending.files.len(),
            pending.total_bytes
        )
    });
    emit_transfer_progress(
        app,
        &pending.offer_id,
        "clip",
        &pending.summary,
        0,
        "active",
        pending.total_bytes,
    );
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(kvm) = state.kvm.lock() else {
        return;
    };
    if let Some(ref client) = kvm.client {
        if client
            .is_connected
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            let conn = client.conn.clone();
            for (rel_path, disk_path) in pending.sources {
                let size = std::fs::metadata(&disk_path).map(|m| m.len()).unwrap_or(0);
                spawn_client_bulk_transfer(
                    &conn,
                    app,
                    super::files::BulkTransferKind::Clip,
                    pending.offer_id.clone(),
                    rel_path,
                    disk_path,
                    size,
                );
            }
            return;
        }
    }
    if let Some(ref server) = kvm.server {
        if server.is_running.load(std::sync::atomic::Ordering::Relaxed) {
            let out_tx = server.outgoing_tx.clone();
            let offer_id = pending.offer_id.clone();
            let sources = pending.sources;
            tokio::task::spawn_blocking(move || {
                for (rel_path, disk_path) in sources {
                    let size = std::fs::metadata(&disk_path).map(|m| m.len()).unwrap_or(0);
                    let _ = out_tx.send(Outbound::ClipFile {
                        offer_id: offer_id.clone(),
                        rel_path,
                        path: disk_path,
                        size,
                    });
                }
            });
        }
    }
}

fn handle_clip_fetch_request(app: &AppHandle, offer_id: &str) {
    if let Some(pending) = super::staging::take_pending_outbound_clip(offer_id) {
        spawn_outbound_pending_clip(app, pending);
    }
}

fn handle_transfer_cancel_packet(app: &AppHandle, id: &str) {
    let mut emitted = false;
    if let super::dragdrop::CancelDragAction::Cancelled(ev) =
        super::dragdrop::global_drag_registry().cancel_at_any_stage(id)
    {
        emit_drag_progress_payload(app, &ev);
        emitted = true;
    }
    if let Some(pending) = super::staging::take_pending_inbound_clip(id) {
        if !emitted {
            emit_transfer_progress(
                app,
                id,
                "clip",
                &pending.summary,
                0,
                "cancelled",
                pending.total_bytes,
            );
            emitted = true;
        }
    }
    let _ = super::staging::take_pending_outbound_clip(id);
    if let Some((kind, total_bytes)) = super::staging::cancel_transfer_with_info(id) {
        if !emitted && kind != super::staging::TransferKind::Drag {
            emit_transfer_progress(
                app,
                id,
                match kind {
                    super::staging::TransferKind::Drag => "drag",
                    super::staging::TransferKind::Clip => "clip",
                },
                "Files",
                0,
                "cancelled",
                total_bytes,
            );
        }
    }
    super::staging::cleanup_staging_entry(&super::staging::default_staging_root(), id);
    super::dragdrop::cancel_target_drag_for_id(id, "transfer_cancel");
    if let Ok(mut g) = pending_target_drag().lock() {
        if g.as_ref().is_some_and(|p| p.drag_id == id) {
            *g = None;
        }
    }
    if let Ok(mut g) = pending_outbound_drag().lock() {
        if g.as_ref().is_some_and(|p| p.drag_id == id) {
            *g = None;
        }
    }
}

fn handle_incoming_file_clip_offer(
    app: &AppHandle,
    offer_id: String,
    files: Vec<super::protocol::FileMeta>,
) {
    if !super::dragdrop::file_clipboard_enabled() || !super::clipboard::sync_files_enabled() {
        super::debug_log::note(|| format!("CLIP_OFFER_RECV_IGNORED id={offer_id} reason=disabled"));
        return;
    }
    replace_previous_clip_offer(app, &offer_id);
    let staging_root = super::staging::default_staging_root();
    let download_dir = get_download_dir();
    let limit_mb = super::staging::staging_limit_mb();
    let total_bytes: u64 = files
        .iter()
        .filter(|f| !f.is_dir)
        .fold(0u64, |acc, f| acc.saturating_add(f.size));
    let summary = summarize_offer_name(&files);

    if super::staging::decide_clip_offer_mode(total_bytes, super::staging::clip_prefetch_limit_mb())
        == super::staging::ClipOfferMode::AwaitFetch
    {
        super::debug_log::note(|| {
            format!(
                "CLIP_AWAIT_FETCH id={offer_id} files={} bytes={total_bytes}",
                files.len()
            )
        });
        super::staging::insert_pending_inbound_clip(super::staging::PendingInboundClip {
            offer_id: offer_id.clone(),
            files,
            summary: summary.clone(),
            total_bytes,
            created_at: Instant::now(),
        });
        emit_transfer_progress(
            app,
            &offer_id,
            "clip",
            &summary,
            0,
            "awaiting_fetch",
            total_bytes,
        );
        let app_exp = app.clone();
        tokio::spawn(async move {
            tokio::time::sleep(super::staging::AWAITING_FETCH_TTL + Duration::from_millis(50))
                .await;
            let expired = super::staging::sweep_expired_pending_inbound_clips(
                super::staging::AWAITING_FETCH_TTL,
            );
            for item in expired {
                emit_transfer_progress(
                    &app_exp,
                    &item.offer_id,
                    "clip",
                    &item.summary,
                    0,
                    "cancelled",
                    item.total_bytes,
                );
            }
            let _ = super::staging::sweep_expired_pending_outbound_clips(
                super::staging::AWAITING_FETCH_TTL,
            );
        });
        return;
    }

    match super::staging::register_transfer_offer(
        &offer_id,
        super::staging::TransferKind::Clip,
        &files,
        &staging_root,
        &download_dir,
        limit_mb,
    ) {
        Ok(plan) => {
            super::debug_log::note(|| {
                format!(
                    "CLIP_OFFER_RECV id={offer_id} files={} expected_files={} bytes={} dest={:?}",
                    files.len(),
                    plan.expected_files,
                    plan.total_bytes,
                    plan.destination,
                )
            });
            if plan.destination == super::staging::TransferDestination::DownloadsFallback {
                emit_continuity_notice(
                    app,
                    &format!(
                        "Copied files exceed {limit_mb} MB staging limit — saving \"{summary}\" to Downloads\\AnyLeap_Transfers"
                    ),
                );
            } else {
                emit_transfer_progress(
                    app,
                    &offer_id,
                    "clip",
                    &summary,
                    0,
                    "active",
                    plan.total_bytes,
                );
            }
            // If sender held `ClipFile` because its own prefetch threshold was lower, request it now.
            send_pkt_from_app(
                app,
                KvmPacket::ClipFetchRequest {
                    offer_id: offer_id.clone(),
                },
            );
        }
        Err(e) => {
            super::debug_log::note(|| format!("CLIP_OFFER_RECV_ERR id={offer_id} err={e}"));
            emit_continuity_notice(app, &format!("File clipboard offer rejected: {e}"));
            send_pkt_from_app(app, KvmPacket::TransferCancel { id: offer_id });
        }
    }
}

/// Returns `true` if the button event was absorbed by a pending/active target drag.
fn handle_drag_mouse_button(button: u8, is_down: bool) -> bool {
    if is_down {
        if let Ok(mut g) = pending_target_drag().lock() {
            if let Some(ref mut pending) = *g {
                if !pending.is_fallback {
                    pending.button = button;
                    return true;
                }
            }
        }
        false
    } else {
        super::dragdrop::set_remote_drag_button_state(button, false);
        if let Ok(mut g) = pending_target_drag().lock() {
            if let Some(ref mut pending) = *g {
                if !pending.is_fallback {
                    pending.released_early = true;
                    return true;
                }
            }
        }
        false
    }
}

fn handle_spec25a_transfer_event<F>(app: &AppHandle, ev: TransferEvent, send_packet: F)
where
    F: Fn(KvmPacket) + Send + 'static,
{
    let staging_root = super::staging::default_staging_root();
    match ev {
        TransferEvent::DragFile {
            drag_id,
            rel_path,
            size,
            ..
        } => {
            super::debug_log::note(|| {
                format!("DRAG_STAGING_FILE_DONE id={drag_id} rel={rel_path} size={size}")
            });
            let (done_opt, progress) = super::staging::record_file_received(&drag_id, size);
            if done_opt.is_none() {
                if let Some((rec, total)) = progress {
                    let pct = super::files::pct_for_bytes(rec, total).min(99);
                    emit_transfer_progress(app, &drag_id, "drag", &rel_path, pct, "active", total);
                }
            }
            if let Some(done) = done_opt {
                let pending = pending_target_drag().lock().ok().and_then(|mut g| g.take());
                let early_hook_release = super::dragdrop::take_pending_target_drag_early_release();
                if done.destination == super::staging::TransferDestination::DownloadsFallback {
                    if let Some(ev) = super::dragdrop::global_drag_registry()
                        .complete_terminal(&drag_id, super::protocol::DragOutcome::Done)
                    {
                        emit_drag_progress_payload(app, &ev);
                    }
                    send_packet(KvmPacket::DragResult {
                        drag_id,
                        outcome: super::protocol::DragOutcome::Done,
                        notice: None,
                    });
                } else if done.destination == super::staging::TransferDestination::Staging {
                    if let Some(p) = pending {
                        if p.released_early || early_hook_release {
                            super::debug_log::note(|| {
                                format!("DRAG_CANCEL_SEND id={drag_id} reason=released_early")
                            });
                            super::staging::cleanup_staging_entry(&staging_root, &drag_id);
                            if let Some(ev) = super::dragdrop::global_drag_registry()
                                .complete_terminal(
                                    &drag_id,
                                    super::protocol::DragOutcome::Cancelled,
                                )
                            {
                                emit_drag_progress_payload(app, &ev);
                            }
                            let msg = "Drag cancelled before transfer finished".to_string();
                            emit_continuity_notice(app, &msg);
                            send_packet(KvmPacket::DragResult {
                                drag_id,
                                outcome: super::protocol::DragOutcome::Cancelled,
                                notice: Some(msg),
                            });
                        } else {
                            super::debug_log::note(|| {
                                format!(
                                    "DRAG_READY_SEND id={drag_id} paths={} button={} fwd_inject={}",
                                    done.top_level_paths.len(),
                                    p.button,
                                    p.forwarded_button_inject,
                                )
                            });
                            if let Some(ev) = super::dragdrop::global_drag_registry()
                                .mark_waiting_for_drop(&drag_id)
                            {
                                emit_drag_progress_payload(app, &ev);
                            }
                            send_packet(KvmPacket::DragReady {
                                drag_id: drag_id.clone(),
                            });
                            let app_cb = app.clone();
                            let drag_id_cb = drag_id.clone();
                            super::dragdrop::start_target_drag_drop(
                                drag_id,
                                done.top_level_paths,
                                p.button,
                                p.forwarded_button_inject,
                                staging_root,
                                Box::new(move |res| {
                                    finish_target_drag_and_notify(&app_cb, &drag_id_cb, res);
                                }),
                            );
                        }
                    } else {
                        super::debug_log::note(|| {
                            format!("DRAG_STAGING_ORPHAN_CLEANUP id={drag_id}")
                        });
                        super::staging::cleanup_staging_entry(&staging_root, &drag_id);
                        if let Some(ev) = super::dragdrop::global_drag_registry()
                            .complete_terminal(&drag_id, super::protocol::DragOutcome::Cancelled)
                        {
                            emit_drag_progress_payload(app, &ev);
                        }
                        send_packet(KvmPacket::DragResult {
                            drag_id,
                            outcome: super::protocol::DragOutcome::Cancelled,
                            notice: None,
                        });
                    }
                }
            }
        }
        TransferEvent::ClipFile {
            offer_id,
            rel_path,
            size,
            ..
        } => {
            super::debug_log::note(|| {
                format!("CLIP_STAGING_FILE_DONE id={offer_id} rel={rel_path} size={size}")
            });
            let (done_opt, progress) = super::staging::record_file_received(&offer_id, size);
            if done_opt.is_none() {
                if let Some((rec, total)) = progress {
                    let pct = super::files::pct_for_bytes(rec, total);
                    emit_transfer_progress(app, &offer_id, "clip", &rel_path, pct, "active", total);
                }
            }
            if let Some(done) = done_opt {
                emit_transfer_progress(
                    app,
                    &offer_id,
                    "clip",
                    &rel_path,
                    100,
                    "done",
                    done.total_bytes,
                );
                if done.destination == super::staging::TransferDestination::Staging {
                    match super::clipboard::set_clipboard_files(&done.top_level_paths) {
                        Ok(()) => {
                            super::debug_log::note(|| {
                                format!(
                                    "CLIP_SET_FILES id={offer_id} count={} ok=true",
                                    done.top_level_paths.len()
                                )
                            });
                        }
                        Err(e) => {
                            super::debug_log::note(|| {
                                format!(
                                    "CLIP_SET_FILES id={offer_id} count={} ok=false err={e}",
                                    done.top_level_paths.len()
                                )
                            });
                            emit_continuity_notice(
                                app,
                                &format!("Failed to place staged files on clipboard: {e}"),
                            );
                        }
                    }
                }
                let _ = app.emit(
                    "kvm-activity",
                    serde_json::json!({
                        "id": uuid::Uuid::new_v4().to_string(),
                        "timestamp": now_ms(),
                        "type": "file",
                        "name": rel_path,
                        "sizeBytes": done.total_bytes,
                        "progressPercent": 100,
                        "status": "completed",
                        "savedPath": done.base_dir.display().to_string()
                    }),
                );
            }
        }
        TransferEvent::Cancelled { id, kind, .. } => {
            if kind == "drag" {
                if let Some(ev) = super::dragdrop::global_drag_registry()
                    .complete_terminal(&id, super::protocol::DragOutcome::Cancelled)
                {
                    emit_drag_progress_payload(app, &ev);
                }
            }
            super::dragdrop::cancel_target_drag_for_id(&id, "transfer_cancel");
            super::staging::cancel_transfer(&id);
            super::staging::cleanup_staging_entry(&staging_root, &id);
            let _ = super::staging::take_pending_inbound_clip(&id);
            let _ = super::staging::take_pending_outbound_clip(&id);
            if let Ok(mut g) = pending_target_drag().lock() {
                if g.as_ref().is_some_and(|p| p.drag_id == id) {
                    *g = None;
                }
            }
            if let Ok(mut g) = pending_outbound_drag().lock() {
                if g.as_ref().is_some_and(|p| p.drag_id == id) {
                    *g = None;
                }
            }
        }
        TransferEvent::Failed {
            id,
            kind,
            name,
            reason,
        } => {
            if kind == "drag" || kind == "clip" {
                super::debug_log::note(|| {
                    format!("STAGING_FAIL kind={kind} id={id} name={name} err={reason}")
                });
                if kind == "drag" {
                    super::dragdrop::cancel_target_drag_for_id(&id, "transfer_failed");
                }
                super::staging::cancel_transfer(&id);
                super::staging::cleanup_staging_entry(&staging_root, &id);
                if kind == "drag" {
                    if let Ok(mut g) = pending_target_drag().lock() {
                        if g.as_ref().is_some_and(|p| p.drag_id == id) {
                            *g = None;
                        }
                    }
                    if let Some(ev) = super::dragdrop::global_drag_registry()
                        .complete_terminal(&id, super::protocol::DragOutcome::Failed)
                    {
                        emit_drag_progress_payload(app, &ev);
                    }
                    let msg = format!("Drag transfer failed ({name}): {reason}");
                    emit_continuity_notice(app, &msg);
                    send_packet(KvmPacket::DragResult {
                        drag_id: id,
                        outcome: super::protocol::DragOutcome::Failed,
                        notice: Some(msg),
                    });
                } else {
                    emit_continuity_notice(app, &format!("File copy failed ({name}): {reason}"));
                }
            }
        }
        TransferEvent::CursorFile { role, name, data } => {
            let _ =
                super::cursor_scheme::store_remote_cursor_file(&staging_root, &role, &name, &data);
        }
        _ => {}
    }
}

fn spawn_send_cursor_scheme(
    send_tx: tokio::sync::mpsc::Sender<KvmPacket>,
    conn: quinn::Connection,
) {
    tokio::spawn(async move {
        let items = tokio::task::spawn_blocking(super::cursor_scheme::read_local_cursor_scheme)
            .await
            .unwrap_or_default();
        let roles: Vec<String> = items.iter().map(|i| i.role.clone()).collect();
        let _ = send_tx.send(KvmPacket::CursorSchemeOffer { roles }).await;
        for item in items {
            let _ = super::files::send_cursor_file(&conn, item.role, item.name, &item.data).await;
        }
    });
}

pub(crate) fn on_pointer_scheme_setting_changed(
    state: &AppState,
    _app_handle: &AppHandle,
    enabled: bool,
) {
    let kvm = state.kvm.lock().unwrap();
    if let Some(ref client) = kvm.client {
        if enabled {
            spawn_send_cursor_scheme(client.send_packet_tx.clone(), client.conn.clone());
        } else {
            let tx = client.send_packet_tx.clone();
            tokio::spawn(async move {
                let _ = tx
                    .send(KvmPacket::CursorSchemeOffer { roles: Vec::new() })
                    .await;
            });
        }
    }
    if !enabled {
        super::cursor_scheme::restore_system_cursors(&super::staging::default_staging_root());
    }
}

fn cleanup_spec25a_on_disconnect_with_app(app: Option<&AppHandle>) {
    if cfg!(test) {
        return;
    }
    let staging_root = super::staging::default_staging_root();
    super::dragdrop::clear_edge_catcher();
    super::dragdrop::cancel_active_target_drag_with_reason("disconnect");
    super::dragdrop::set_drag_leap_in_progress(false);
    if let Ok(mut g) = pending_target_drag().lock() {
        *g = None;
    }
    if let Ok(mut g) = pending_outbound_drag().lock() {
        *g = None;
    }
    let expired_drags = super::dragdrop::global_drag_registry().drain_active();
    let expired_inbound = super::staging::drain_pending_inbound_clips();
    if let Some(app_h) = app {
        for ev in expired_drags {
            emit_drag_progress_payload(app_h, &ev);
        }
        for item in expired_inbound {
            emit_transfer_progress(
                app_h,
                &item.offer_id,
                "clip",
                &item.summary,
                0,
                "cancelled",
                item.total_bytes,
            );
        }
    }
    super::staging::clear_pending_outbound_clips();
    let _ = super::staging::take_previous_clip_offer();
    super::staging::cancel_all_transfers();
    super::staging::cleanup_all_staging(&staging_root);
    super::cursor_scheme::clear_remote_cursors(&staging_root);
}

fn cleanup_spec25a_on_disconnect() {
    cleanup_spec25a_on_disconnect_with_app(None);
}

fn spawn_client_bulk_transfer(
    conn: &quinn::Connection,
    app: &AppHandle,
    kind: super::files::BulkTransferKind,
    id: String,
    rel_path: String,
    path: std::path::PathBuf,
    size: u64,
) {
    let c = conn.clone();
    let app_handle = app.clone();
    tokio::spawn(async move {
        let id_prog = id.clone();
        let name_prog = rel_path.clone();
        let app_prog = app_handle.clone();
        let kind_str = kind.as_str();
        let res = super::files::send_tracked_bulk_file(
            &c,
            kind,
            "client",
            id.clone(),
            rel_path.clone(),
            &path,
            size,
            |pct| {
                emit_transfer_progress(
                    &app_prog,
                    &id_prog,
                    kind_str,
                    &name_prog,
                    pct,
                    if pct >= 100 { "done" } else { "active" },
                    size,
                )
            },
        )
        .await;
        if let Err(e) = res {
            if e == super::files::TRANSFER_CANCELLED_ERR {
                let meta = super::files::TransferCancelMeta {
                    id,
                    kind: kind_str.to_string(),
                    name: rel_path,
                    size_bytes: size,
                };
                if super::files::emit_cancelled_once(c.stable_id(), &meta, None) {
                    emit_transfer_progress(
                        &app_handle,
                        &meta.id,
                        &meta.kind,
                        &meta.name,
                        0,
                        "cancelled",
                        size,
                    );
                }
            } else {
                let label = match kind {
                    super::files::BulkTransferKind::Drag => "Drag transfer",
                    super::files::BulkTransferKind::Clip => "File copy",
                };
                emit_continuity_notice(&app_handle, &format!("{label} failed ({rel_path}): {e}"));
            }
        }
    });
}

fn drag_return_hold_ms(drag_drop_enabled: bool) -> u64 {
    if drag_drop_enabled {
        350
    } else {
        0
    }
}

fn should_hold_drag_return(
    drag_edge_hold_until: &mut Option<Instant>,
    buttons_mask: u8,
    drag_drop_enabled: bool,
    now: Instant,
) -> bool {
    if buttons_mask == 0 {
        *drag_edge_hold_until = None;
        return false;
    }
    let hold_ms = drag_return_hold_ms(drag_drop_enabled);
    if hold_ms == 0 {
        *drag_edge_hold_until = None;
        return false;
    }
    let hold = drag_edge_hold_until.get_or_insert_with(|| now + Duration::from_millis(hold_ms));
    now < *hold
}

fn handle_drag_ready(app: &AppHandle, drag_id: &str) {
    super::debug_log::note(|| format!("DRAG_READY_RECV id={drag_id}"));
    if let Ok(mut g) = pending_outbound_drag().lock() {
        if g.as_ref().is_some_and(|p| p.drag_id == drag_id) {
            *g = None;
        }
    }
    if let Some(ev) = super::dragdrop::global_drag_registry().mark_waiting_for_drop(drag_id) {
        emit_drag_progress_payload(app, &ev);
    }
}

fn handle_drag_cancel(app: &AppHandle, drag_id: &str) {
    super::debug_log::note(|| format!("DRAG_CANCEL_RECV id={drag_id}"));
    let staging_root = super::staging::default_staging_root();
    if let Ok(mut g) = pending_outbound_drag().lock() {
        if g.as_ref().is_some_and(|p| p.drag_id == drag_id) {
            *g = None;
        }
    }
    let mut cancelled_target = false;
    if let Ok(mut g) = pending_target_drag().lock() {
        if g.as_ref().is_some_and(|p| p.drag_id == drag_id) {
            *g = None;
            cancelled_target = true;
        }
    }
    if let Some(ev) = super::dragdrop::global_drag_registry()
        .complete_terminal(drag_id, super::protocol::DragOutcome::Cancelled)
    {
        emit_drag_progress_payload(app, &ev);
    }
    super::dragdrop::cancel_target_drag_for_id(drag_id, "drag_cancel");
    super::dragdrop::set_drag_leap_in_progress(false);
    super::staging::cancel_transfer(drag_id);
    super::staging::cleanup_staging_entry(&staging_root, drag_id);
    if cancelled_target {
        emit_continuity_notice(app, "Drag transfer cancelled");
    }
}

fn handle_drag_result(
    app: &AppHandle,
    drag_id: &str,
    outcome: super::protocol::DragOutcome,
    notice: Option<String>,
) {
    super::debug_log::note(|| {
        format!(
            "DRAG_RESULT_RECV id={drag_id} outcome={} notice={}",
            outcome.as_str(),
            notice.as_deref().unwrap_or("")
        )
    });
    if let Ok(mut g) = pending_outbound_drag().lock() {
        if g.as_ref().is_some_and(|p| p.drag_id == drag_id) {
            *g = None;
        }
    }
    if let Some(ev) = super::dragdrop::global_drag_registry().complete_terminal(drag_id, outcome) {
        emit_drag_progress_payload(app, &ev);
    }
    if let Some(msg) = notice {
        emit_continuity_notice(app, &msg);
    } else if outcome == super::protocol::DragOutcome::Failed {
        emit_continuity_notice(app, "Drop was not accepted by the target app");
    }
}

fn handle_cursor_scheme_offer(roles: Vec<String>) {
    let staging_root = super::staging::default_staging_root();
    super::cursor_scheme::set_offered_roles(&staging_root, roles.clone());
    if roles.is_empty() || !super::dragdrop::pointer_scheme_enabled() {
        super::cursor_scheme::restore_system_cursors(&staging_root);
    } else if RECEIVER_ENTERED.load(std::sync::atomic::Ordering::Relaxed) {
        super::cursor_scheme::apply_remote_cursors(&staging_root);
    }
}

fn spawn_outbound_drag_files_client(
    drag_id: String,
    sources: Vec<(String, std::path::PathBuf)>,
    total: u64,
    conn: quinn::Connection,
    pkt_tx: tokio::sync::mpsc::Sender<KvmPacket>,
    app: AppHandle,
) {
    tauri::async_runtime::spawn(async move {
        let count = sources.len();
        super::debug_log::note(|| {
            format!("DRAG_STAGING_START side=client id={drag_id} files={count} total_bytes={total}")
        });
        let mut sent_bytes: u64 = 0;
        let mut throttle = super::files::ProgressThrottle::new();
        for (rel_path, src_path) in sources {
            let size = std::fs::metadata(&src_path).map(|m| m.len()).unwrap_or(0);
            let res = super::files::send_drag_file_with_progress(
                &conn,
                drag_id.clone(),
                rel_path.clone(),
                &src_path,
                size,
                |delta, _| {
                    sent_bytes = sent_bytes.saturating_add(delta);
                    let pct = super::files::pct_for_bytes(sent_bytes, total).min(99);
                    if let Some(p) = throttle.should_emit(pct, false) {
                        emit_transfer_progress(
                            &app, &drag_id, "drag", &rel_path, p, "active", total,
                        );
                    }
                },
            )
            .await;
            match res {
                Ok(()) => {
                    super::debug_log::note(|| {
                        format!(
                        "DRAG_FILE_SEND_DONE side=client id={drag_id} rel={rel_path} size={size}"
                    )
                    });
                }
                Err(e) if e == super::files::TRANSFER_CANCELLED_ERR => {
                    let meta = super::files::TransferCancelMeta {
                        id: drag_id.clone(),
                        kind: "drag".to_string(),
                        name: rel_path,
                        size_bytes: total,
                    };
                    let _ = super::files::emit_cancelled_once(conn.stable_id(), &meta, None);
                    if let Some(ev) = super::dragdrop::global_drag_registry()
                        .complete_terminal(&drag_id, super::protocol::DragOutcome::Cancelled)
                    {
                        emit_drag_progress_payload(&app, &ev);
                    }
                    return;
                }
                Err(e) => {
                    super::debug_log::note(|| {
                        format!(
                            "DRAG_FILE_SEND_ERR side=client id={drag_id} rel={rel_path} err={e}"
                        )
                    });
                    super::debug_log::note(|| {
                        format!("DRAG_CANCEL_SEND side=client id={drag_id} reason={e}")
                    });
                    if let Some(ev) = super::dragdrop::global_drag_registry()
                        .complete_terminal(&drag_id, super::protocol::DragOutcome::Failed)
                    {
                        emit_drag_progress_payload(&app, &ev);
                    }
                    let msg = format!("Drag transfer failed: {e}");
                    emit_continuity_notice(&app, &msg);
                    let _ = pkt_tx
                        .send(KvmPacket::DragCancel {
                            drag_id: drag_id.clone(),
                        })
                        .await;
                    return;
                }
            }
        }
        // Outbound files finished uploading; stay active at 99% until the target PC reports the
        // terminal DragResult (drop accepted / rejected / cancelled).
        if let Some(ev) = super::dragdrop::global_drag_registry().mark_waiting_for_drop(&drag_id) {
            emit_drag_progress_payload(&app, &ev);
        }
    });
}

fn compute_linked_drag_return(
    cur: (i32, i32),
    monitors: &[MonitorDisplay],
    union: &super::protocol::ScreenRect,
    seams: &[super::protocol::LinkedSeam],
) -> Option<(Edge, f32, Option<String>, Option<i32>)> {
    let mut best: Option<(i32, Edge, i32, &super::protocol::LinkedSeam)> = None;
    for seam in seams {
        let Some(side) = Edge::from_str(&seam.side) else {
            continue;
        };
        let Some(mon) = monitors
            .iter()
            .find(|m| m.effective_id() == seam.linked_monitor_id)
        else {
            continue;
        };
        let perp = match side {
            Edge::Left | Edge::Right => cur.1.clamp(seam.start, (seam.end - 1).max(seam.start)),
            Edge::Top | Edge::Bottom => cur.0.clamp(seam.start, (seam.end - 1).max(seam.start)),
        };
        let dist = match side {
            Edge::Left => (cur.0 - mon.x).abs(),
            Edge::Right => (mon.x + mon.width - 1 - cur.0).abs(),
            Edge::Top => (cur.1 - mon.y).abs(),
            Edge::Bottom => (mon.y + mon.height - 1 - cur.1).abs(),
        } + match side {
            Edge::Left | Edge::Right => (cur.1 - perp).abs(),
            Edge::Top | Edge::Bottom => (cur.0 - perp).abs(),
        };
        if best.is_none_or(|(d, _, _, _)| dist < d) {
            best = Some((dist, side, perp, seam));
        }
    }
    let (_, edge, perp, seam) = best?;
    let relative_y = match edge {
        Edge::Left | Edge::Right => {
            ((perp - union.y) as f32 / union.height.max(1) as f32).clamp(0.0, 1.0)
        }
        Edge::Top | Edge::Bottom => {
            ((perp - union.x) as f32 / union.width.max(1) as f32).clamp(0.0, 1.0)
        }
    };
    Some((
        edge,
        relative_y,
        Some(seam.main_monitor_id.clone()),
        Some(perp + seam.main_offset),
    ))
}
// --- end SPEC-25a ---

fn get_local_machine_info(
    app: &AppHandle,
    state: &AppState,
) -> Result<(String, String, i32, i32, Vec<MonitorDisplay>), String> {
    let (identity, _) = get_security(app, state)?;
    let id = identity.computer_id();
    let name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "AnyLeap-PC".to_string());
    let monitors = get_local_monitors();
    let union = union_rect(&monitors);
    let primary = monitors
        .iter()
        .find(|m| m.is_primary)
        .or_else(|| monitors.first());
    let sw = if union.width > 0 {
        union.width
    } else {
        primary.map(|m| m.width).unwrap_or(0)
    };
    let sh = if union.height > 0 {
        union.height
    } else {
        primary.map(|m| m.height).unwrap_or(0)
    };
    Ok((id, name, sw, sh, monitors))
}

fn build_status(
    id: String,
    name: String,
    sw: i32,
    sh: i32,
    monitors: Vec<MonitorDisplay>,
    kvm: &KvmState,
) -> KvmStatus {
    let is_host_running = kvm
        .server
        .as_ref()
        .map(|s| s.is_running.load(std::sync::atomic::Ordering::Relaxed))
        .unwrap_or(false);
    let port = kvm.server.as_ref().map(|s| s.port).unwrap_or(KVM_PORT);
    let pin = kvm.server.as_ref().map(|s| s.pin()).unwrap_or_default();
    let is_trapped = kvm.hooks.is_trapped();
    let connected_peers = kvm.connected_peers.clone();
    let peers: Vec<ComputerNode> = kvm
        .topology
        .computers
        .iter()
        .filter(|c| !c.is_local)
        .cloned()
        .collect();
    let linked_peer = if let Some(ref client) = kvm.client {
        client
            .is_connected
            .load(std::sync::atomic::Ordering::Relaxed)
            .then(|| LinkedPeerInfo {
                computer_id: client.peer_id.clone(),
                name: client.peer_name.clone(),
                role: "main".to_string(),
            })
    } else if is_host_running {
        peers.first().map(|p| LinkedPeerInfo {
            computer_id: p.id.clone(),
            name: p.name.clone(),
            role: "linked".to_string(),
        })
    } else {
        None
    };

    KvmStatus {
        is_host_running,
        port,
        security_pin: pin,
        connected_peers,
        is_input_trapped: is_trapped,
        active_target_id: None,
        local_name: name,
        local_id: id,
        screen_width: sw,
        screen_height: sh,
        monitors,
        peers,
        controller_edge: kvm.hooks.active_link_edge().map(String::from),
        receiver_entered: RECEIVER_ENTERED.load(std::sync::atomic::Ordering::Relaxed),
        allow_control: is_host_running,
        linked_peer,
    }
}

/// Stops the receiver server without touching any active controller client or hooks.
fn stop_server_in_state(kvm: &mut KvmState) {
    if let Some(srv) = kvm.server.take() {
        srv.disconnect_sessions(None);
        srv.is_running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = srv.shutdown_tx.send(true);
    }
    // --- SPEC-25b ---
    super::audio::on_linked_server_stopped();
    // --- /SPEC-25b ---
    RECEIVER_ENTERED.store(false, std::sync::atomic::Ordering::Relaxed);
    kvm.connected_peers.clear();
    let client_peer_id = kvm.client.as_ref().map(|c| c.peer_id.clone());
    kvm.topology
        .computers
        .retain(|c| c.is_local || client_peer_id.as_deref() == Some(c.id.as_str()));
    kvm.topology.links.retain(|l| {
        client_peer_id
            .as_deref()
            .is_some_and(|pid| l.from_id == pid || l.to_id == pid)
    });
    // --- SPEC-25a ---
    super::cursor_scheme::restore_system_cursors(&super::staging::default_staging_root());
    if kvm.client.is_none() {
        cleanup_spec25a_on_disconnect();
    }
    // --- end SPEC-25a ---
}

fn disconnect_client_in_state(kvm: &mut KvmState) {
    let mut removed_id = None;
    if let Some(client) = kvm.client.take() {
        removed_id = Some(client.peer_id.clone());
        let _ = client.send_packet_tx.try_send(KvmPacket::SessionDisconnect);
        client
            .is_connected
            .store(false, std::sync::atomic::Ordering::Relaxed);
        client.conn.close(0u32.into(), b"disconnect");
    }
    kvm.hooks.stop_hooks();
    // --- SPEC-25b ---
    super::audio::on_main_disconnected();
    // --- /SPEC-25b ---
    if let Some(id) = removed_id {
        kvm.topology.computers.retain(|c| c.is_local || c.id != id);
        kvm.topology
            .links
            .retain(|l| l.from_id != id && l.to_id != id);
    }
    super::hooks::restore_cursor_visibility();
    // --- SPEC-25a ---
    cleanup_spec25a_on_disconnect();
    // --- end SPEC-25a ---
}

/// Role-aware unlink: if this PC has an outgoing client link, close it and stop
/// hooks; otherwise close every incoming controller session on the server.
fn disconnect_peer_in_state(kvm: &mut KvmState) {
    if kvm.client.is_some() {
        disconnect_client_in_state(kvm);
    } else if let Some(ref srv) = kvm.server {
        srv.disconnect_sessions(None);
        // --- SPEC-25b ---
        super::audio::on_linked_peer_disconnected(0);
        // --- /SPEC-25b ---
        kvm.connected_peers.clear();
        kvm.topology.computers.retain(|c| c.is_local);
        kvm.topology.links.clear();
        RECEIVER_ENTERED.store(false, std::sync::atomic::Ordering::Relaxed);
        // --- SPEC-25a ---
        super::cursor_scheme::restore_system_cursors(&super::staging::default_staging_root());
        cleanup_spec25a_on_disconnect();
        // --- end SPEC-25a ---
    }
}

/// Remove trust entry and close any active session (either role) with `computer_id`.
fn forget_peer_in_state(kvm: &mut KvmState, trust: &TrustStore, computer_id: &str) {
    trust.remove(computer_id);
    if kvm
        .client
        .as_ref()
        .is_some_and(|c| c.peer_id == computer_id)
    {
        disconnect_client_in_state(kvm);
    }
    if let Some(ref srv) = kvm.server {
        srv.disconnect_sessions(Some(computer_id));
    }
    let removed_name = kvm
        .topology
        .computers
        .iter()
        .find(|c| !c.is_local && c.id == computer_id)
        .map(|c| c.name.clone());
    kvm.topology
        .computers
        .retain(|c| c.is_local || c.id != computer_id);
    kvm.topology
        .links
        .retain(|l| l.from_id != computer_id && l.to_id != computer_id);
    if let Some(name) = removed_name {
        let still_present = kvm
            .topology
            .computers
            .iter()
            .any(|c| !c.is_local && c.name == name);
        if !still_present {
            kvm.connected_peers.retain(|p| p != &name);
        }
    }
}

#[tauri::command]
pub async fn kvm_get_status(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<KvmStatus, String> {
    let (id, name, sw, sh, monitors) = get_local_machine_info(&app_handle, &state)?;
    let mut kvm = state.kvm.lock().unwrap();
    let port = kvm.server.as_ref().map(|s| s.port).unwrap_or(KVM_PORT);

    // Auto-start discovery listener if not already running
    if !kvm.discovery.is_running() {
        kvm.discovery.start(id.clone(), name.clone(), port, sw, sh);
    }

    if let Some(prev_origin) = kvm.topology.remote_origin {
        let prev_main = kvm
            .topology
            .computers
            .iter()
            .find(|c| c.is_local)
            .map(|c| c.monitors.clone())
            .unwrap_or_default();
        if !prev_main.is_empty() && monitors_changed(&prev_main, &monitors) {
            let linked_mons = kvm
                .client
                .as_ref()
                .map(|c| c.peer_monitors.clone())
                .or_else(|| {
                    kvm.topology
                        .computers
                        .iter()
                        .find(|c| !c.is_local)
                        .map(|c| c.monitors.clone())
                })
                .unwrap_or_default();
            if let Some((fallback_origin, side)) = handle_monitor_change_fallback(
                &prev_main,
                &monitors,
                &linked_mons,
                &linked_mons,
                prev_origin,
                None,
            ) {
                let mut updated = kvm.topology.clone();
                if let Some(local_node) = updated.computers.iter_mut().find(|c| c.is_local) {
                    local_node.monitors = monitors.clone();
                    local_node.bounds = union_rect(&monitors);
                }
                updated.remote_origin = Some(fallback_origin);
                let _ = app_handle.emit(
                    "kvm-layout-notice",
                    serde_json::json!({
                        "reason": "monitors_changed",
                        "side": side.as_str(),
                        "remoteOrigin": fallback_origin,
                    }),
                );
                apply_controller_topology(&mut kvm, updated, &app_handle);
            }
        }
    }

    Ok(build_status(id, name, sw, sh, monitors, &kvm))
}

async fn start_server_inner(
    port: Option<u16>,
    state: &AppState,
    app_handle: &AppHandle,
) -> Result<KvmStatus, String> {
    let (id, name, sw, sh, monitors) = get_local_machine_info(app_handle, state)?;
    {
        let kvm = state.kvm.lock().unwrap();
        if kvm
            .server
            .as_ref()
            .is_some_and(|s| s.is_running.load(std::sync::atomic::Ordering::Relaxed))
        {
            return Ok(build_status(id, name, sw, sh, monitors, &kvm));
        }
    }
    let (identity, trust) = get_security(app_handle, state)?;
    // --- SPEC-25c ---
    super::open_on_linked::cleanup_expired_open_staging(
        &super::open_on_linked::open_staging_root(),
        super::open_on_linked::OPEN_TEMP_MAX_AGE,
        std::time::SystemTime::now(),
    );
    // --- end SPEC-25c ---
    // --- SPEC-25a ---
    super::cursor_scheme::restore_if_crash_marker(&super::staging::default_staging_root());
    super::staging::cleanup_expired_staging(
        &super::staging::default_staging_root(),
        super::staging::STAGING_MAX_AGE,
        std::time::SystemTime::now(),
    );
    // --- end SPEC-25a ---

    let server_handle = start_kvm_server(
        port.unwrap_or(KVM_PORT),
        &identity,
        trust,
        id.clone(),
        name.clone(),
        Arc::new(get_local_monitors),
        get_download_dir(),
    )
    .await?;
    let port = server_handle.port;
    let mut rx = server_handle.incoming_tx.subscribe();
    let mut transfer_rx = server_handle.transfer_tx.subscribe();
    let mut delta_rx = server_handle.mouse_delta_tx.subscribe();
    let outgoing_tx = server_handle.outgoing_tx.clone();
    // --- SPEC-25b ---
    let audio_out_tx = server_handle.audio_out_tx.clone();
    super::audio::on_linked_server_started(outgoing_tx.clone(), app_handle);
    // --- /SPEC-25b ---

    // Start server-side clipboard monitor
    let (clip_tx, mut clip_rx) = tokio::sync::mpsc::channel::<Outbound>(32);
    start_clipboard_sync(clip_tx, Arc::clone(&server_handle.is_running));
    let out_tx = outgoing_tx.clone();
    let srv_clip_app = app_handle.clone();
    tokio::spawn(async move {
        while let Some(pkt) = clip_rx.recv().await {
            if let Outbound::Packet(KvmPacket::FileClipOffer {
                ref offer_id,
                ref files,
            }) = pkt
            {
                replace_previous_clip_offer(&srv_clip_app, offer_id);
                let _ = super::staging::sweep_expired_pending_outbound_clips(
                    super::staging::AWAITING_FETCH_TTL,
                );
                let total_bytes: u64 = files.iter().filter(|f| !f.is_dir).map(|f| f.size).sum();
                let summary = summarize_offer_name(files);
                if super::staging::decide_clip_offer_mode(
                    total_bytes,
                    super::staging::clip_prefetch_limit_mb(),
                ) == super::staging::ClipOfferMode::Prefetch
                {
                    super::files::register_outbound_total(offer_id, total_bytes);
                    super::debug_log::note(|| {
                        format!(
                        "CLIP_OFFER_SEND side=server id={offer_id} files={} total_bytes={total_bytes}",
                        files.len()
                    )
                    });
                    emit_transfer_progress(
                        &srv_clip_app,
                        offer_id,
                        "clip",
                        &summary,
                        0,
                        "active",
                        total_bytes,
                    );
                } else {
                    super::debug_log::note(|| {
                        format!(
                        "CLIP_OFFER_SEND side=server id={offer_id} files={} total_bytes={total_bytes} mode=awaiting_fetch",
                        files.len()
                    )
                    });
                }
            }
            let _ = out_tx.send(pkt);
        }
    });

    let rx_state = Arc::new(Mutex::new(ReceiverSessionState::new(monitors.clone())));
    let rx_state_pkt = Arc::clone(&rx_state);
    let pkt_out_tx = outgoing_tx.clone();
    {
        let blocked_tx = outgoing_tx.clone();
        injector::set_input_blocked_sink(move |proc_name| {
            let _ = blocked_tx.send(Outbound::Packet(KvmPacket::InputBlocked {
                foreground_process: proc_name,
            }));
        });
    }

    let app_h = app_handle.clone();
    tokio::spawn(async move {
        loop {
            let packet = match rx.recv().await {
                Ok(p) => p,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            match packet {
                KvmPacket::Pair {
                    computer_id,
                    name: peer_name,
                    monitors: peer_monitors,
                    ..
                } => {
                    let bounds = union_rect(&peer_monitors);
                    if let Some(app_state) = app_h.try_state::<AppState>() {
                        if let Ok(mut kvm) = app_state.kvm.lock() {
                            if !kvm.connected_peers.contains(&peer_name) {
                                kvm.connected_peers.push(peer_name.clone());
                            }
                            if let Some(existing) = kvm
                                .topology
                                .computers
                                .iter_mut()
                                .find(|c| c.id == computer_id)
                            {
                                existing.name = peer_name.clone();
                                existing.bounds = bounds;
                                existing.monitors = peer_monitors;
                            } else {
                                kvm.topology.computers.push(ComputerNode {
                                    id: computer_id.clone(),
                                    name: peer_name.clone(),
                                    is_local: false,
                                    address: None,
                                    bounds,
                                    monitors: peer_monitors,
                                });
                            }
                        }
                        let _ = app_h.emit("kvm-peer-connected", &peer_name);
                    }
                    // --- SPEC-25b ---
                    super::audio::on_linked_peer_connected();
                    // --- /SPEC-25b ---
                }
                KvmPacket::Seams { seams } => {
                    let fresh_monitors = get_local_monitors();
                    let union = union_rect(&fresh_monitors);
                    // --- SPEC-25a ---
                    let strips =
                        super::dragdrop::compute_linked_seam_strips(&fresh_monitors, &seams);
                    // --- end SPEC-25a ---
                    if let Ok(mut st) = rx_state_pkt.lock() {
                        st.seams = seams;
                        st.has_seams = true;
                        st.dwell_seam = None;
                        st.dwell_start_ms = None;
                        st.monitors = fresh_monitors;
                        st.union = union;
                    }
                    // --- SPEC-25a ---
                    let rx_state_drag = Arc::clone(&rx_state_pkt);
                    let drag_out_tx = pkt_out_tx.clone();
                    let app_drag = app_h.clone();
                    let on_linked_drag_caught: super::dragdrop::DragCaughtCallback = Arc::new(
                        move |paths, button, drop_pt| {
                            super::dragdrop::set_drag_leap_in_progress(false);
                            if !super::dragdrop::drag_drop_enabled() {
                                super::debug_log::note(|| {
                                    "DRAG_OFFER_SKIPPED side=linked reason=disabled".to_string()
                                });
                                return;
                            }
                            let collected = match super::staging::collect_paths_to_file_meta(&paths)
                            {
                                Ok(v) => v,
                                Err(e) => {
                                    super::debug_log::note(|| {
                                        format!(
                                            "DRAG_CANCEL_SEND side=linked reason=collect_err:{e}"
                                        )
                                    });
                                    emit_continuity_notice(
                                        &app_drag,
                                        &format!("Drag rejected: {e}"),
                                    );
                                    return;
                                }
                            };
                            let (files, sources) = collected;
                            if files.is_empty() || sources.is_empty() {
                                super::debug_log::note(|| {
                                    "DRAG_CANCEL_SEND side=linked reason=empty_files".to_string()
                                });
                                return;
                            }
                            let drag_id = uuid::Uuid::new_v4().to_string();
                            let total_bytes: u64 =
                                files.iter().filter(|f| !f.is_dir).map(|f| f.size).sum();
                            super::files::register_outbound_total(&drag_id, total_bytes);
                            let ret_info =
                                if let Ok(mut st) = rx_state_drag.lock() {
                                    if st.is_entered {
                                        st.is_entered = false;
                                        RECEIVER_ENTERED
                                            .store(false, std::sync::atomic::Ordering::Relaxed);
                                        st.buttons_mask = 0;
                                        st.drag_edge_hold_until = None;
                                        st.dwell_seam = None;
                                        st.dwell_start_ms = None;
                                        let (keys, buttons) = st.tracker.release_all();
                                        let ret = compute_linked_drag_return(
                                            drop_pt,
                                            &st.monitors,
                                            &st.union,
                                            &st.seams,
                                        )
                                        .unwrap_or((st.entry_edge, 0.5, None, None));
                                        Some((ret, keys, buttons))
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                };

                            if let Some((
                                (edge, relative_y, target_monitor, along_px),
                                keys,
                                buttons,
                            )) = ret_info
                            {
                                release_tracked_inputs(keys, buttons);
                                super::cursor_scheme::restore_system_cursors(
                                    &super::staging::default_staging_root(),
                                );
                                super::dragdrop::set_edge_catcher_poll_state(true, false);
                                let _ =
                                    drag_out_tx.send(Outbound::Packet(KvmPacket::CursorReturn {
                                        edge: edge.as_str().to_string(),
                                        relative_y,
                                        target_monitor,
                                        along_px,
                                    }));
                            }

                            super::debug_log::note(|| {
                                format!(
                                "DRAG_OFFER_SEND side=linked id={drag_id} files={} total_bytes={total_bytes} button={button}",
                                files.len()
                            )
                            });
                            let _ = drag_out_tx.send(Outbound::Packet(KvmPacket::DragOffer {
                                drag_id: drag_id.clone(),
                                files: files.clone(),
                                button,
                            }));
                            let summary = summarize_offer_name(&files);
                            let reg_ev = super::dragdrop::global_drag_registry().register_staging(
                                &drag_id,
                                &summary,
                                total_bytes,
                            );
                            emit_drag_progress_payload(&app_drag, &reg_ev);
                            for (rel_path, src_path) in sources {
                                let size =
                                    std::fs::metadata(&src_path).map(|m| m.len()).unwrap_or(0);
                                let _ = drag_out_tx.send(Outbound::DragFile {
                                    drag_id: drag_id.clone(),
                                    rel_path,
                                    path: src_path,
                                    size,
                                });
                            }
                        },
                    );
                    super::dragdrop::configure_edge_catcher(strips, Some(on_linked_drag_caught));
                    super::dragdrop::set_edge_catcher_poll_state(
                        true,
                        RECEIVER_ENTERED.load(std::sync::atomic::Ordering::Relaxed),
                    );
                    // --- end SPEC-25a ---
                }
                KvmPacket::CursorEnter {
                    edge,
                    relative_y,
                    dwell_ms,
                    deadzone_px,
                    target_monitor,
                    along_px,
                } => {
                    let fresh_monitors = get_local_monitors();
                    let entry_edge = Edge::from_str(&edge).unwrap_or(Edge::Left);
                    let pos = map_cursor_enter_spec24(
                        &fresh_monitors,
                        entry_edge,
                        relative_y,
                        target_monitor.as_deref(),
                        along_px,
                    );
                    let union = union_rect(&fresh_monitors);
                    let (norm_x, norm_y) = normalize_in_union(pos, &union);

                    if debug_log::is_enabled() {
                        debug_log::record_event(debug_log::DebugEvent::ReceiverEnter {
                            ts_us: debug_log::now_us(),
                            edge: entry_edge,
                            relative_y,
                            pos_x: pos.0,
                            pos_y: pos.1,
                            hwnd: debug_log::foreground_hwnd_raw(),
                        });
                    }

                    if let Ok(mut st) = rx_state_pkt.lock() {
                        st.is_entered = true;
                        RECEIVER_ENTERED.store(true, std::sync::atomic::Ordering::Relaxed);
                        st.entry_edge = entry_edge;
                        st.dwell_ms = dwell_ms;
                        st.deadzone_px = deadzone_px;
                        st.dwell_start_ms = None;
                        st.dwell_seam = None;
                        st.monitors = fresh_monitors;
                        st.union = union;
                        st.last_pos_emit = None;
                        // --- SPEC-25a ---
                        st.buttons_mask = 0;
                        st.drag_edge_hold_until = None;
                        // --- end SPEC-25a ---
                    }

                    // Keep the receiver's current keyboard focus: the entry point is the
                    // screen edge (desktop/taskbar), so focusing the window under it would
                    // send typed keys to the desktop until the user clicks something.
                    injector::set_cursor_position(pos.0, pos.1);
                    // --- SPEC-25c ---
                    injector::ensure_cursor_visible_with_nudge("receiver_cursor_enter");
                    // --- end SPEC-25c ---

                    // --- SPEC-25a ---
                    super::dragdrop::set_edge_catcher_poll_state(true, true);
                    if super::dragdrop::pointer_scheme_enabled() {
                        super::cursor_scheme::apply_remote_cursors(
                            &super::staging::default_staging_root(),
                        );
                    }
                    // --- end SPEC-25a ---

                    let _ = pkt_out_tx.send(Outbound::Packet(KvmPacket::CursorPosition {
                        norm_x,
                        norm_y,
                    }));
                    let _ = app_h.emit("kvm-cursor-enter", &edge);
                }
                KvmPacket::EmergencyRelease => {
                    release_receiver_inputs(&rx_state_pkt, "emergency_release");
                    super::hooks::restore_cursor_visibility();
                    // --- SPEC-25a ---
                    if let Ok(mut st) = rx_state_pkt.lock() {
                        st.buttons_mask = 0;
                        st.drag_edge_hold_until = None;
                    }
                    super::dragdrop::cancel_active_target_drag_with_reason("emergency_release");
                    super::dragdrop::hide_edge_catcher();
                    super::dragdrop::set_edge_catcher_poll_state(true, false);
                    super::cursor_scheme::restore_system_cursors(
                        &super::staging::default_staging_root(),
                    );
                    // --- end SPEC-25a ---
                }
                KvmPacket::ReleaseAll => {
                    release_receiver_inputs(&rx_state_pkt, "release_all");
                    super::hooks::restore_cursor_visibility();
                    // --- SPEC-25a ---
                    if let Ok(mut st) = rx_state_pkt.lock() {
                        st.buttons_mask = 0;
                        st.drag_edge_hold_until = None;
                    }
                    super::dragdrop::cancel_active_target_drag_with_reason("release_all");
                    super::dragdrop::hide_edge_catcher();
                    super::dragdrop::set_edge_catcher_poll_state(true, false);
                    super::cursor_scheme::restore_system_cursors(
                        &super::staging::default_staging_root(),
                    );
                    // --- end SPEC-25a ---
                }
                KvmPacket::PeerDisconnected { computer_id } => {
                    release_receiver_inputs(&rx_state_pkt, "peer_disconnected");
                    if let Ok(mut st) = rx_state_pkt.lock() {
                        st.seams.clear();
                        st.has_seams = false;
                        st.dwell_seam = None;
                        // --- SPEC-25a ---
                        st.buttons_mask = 0;
                        st.drag_edge_hold_until = None;
                        // --- end SPEC-25a ---
                    }
                    if let Some(app_state) = app_h.try_state::<AppState>() {
                        if let Ok(mut kvm) = app_state.kvm.lock() {
                            let removed_name = kvm
                                .topology
                                .computers
                                .iter()
                                .find(|c| !c.is_local && c.id == computer_id)
                                .map(|c| c.name.clone());
                            kvm.topology
                                .computers
                                .retain(|c| c.is_local || c.id != computer_id);
                            kvm.topology
                                .links
                                .retain(|l| l.from_id != computer_id && l.to_id != computer_id);
                            if let Some(name) = removed_name {
                                let still_present = kvm
                                    .topology
                                    .computers
                                    .iter()
                                    .any(|c| !c.is_local && c.name == name);
                                if !still_present {
                                    kvm.connected_peers.retain(|p| p != &name);
                                }
                            }
                            // --- SPEC-25b ---
                            super::audio::on_linked_peer_disconnected(kvm.connected_peers.len());
                            // --- /SPEC-25b ---
                        }
                    }
                    super::hooks::restore_cursor_visibility();
                    // --- SPEC-25a ---
                    super::cursor_scheme::restore_system_cursors(
                        &super::staging::default_staging_root(),
                    );
                    cleanup_spec25a_on_disconnect_with_app(Some(&app_h));
                    // --- end SPEC-25a ---
                    let _ = app_h.emit("kvm-peer-disconnected", &computer_id);
                }
                KvmPacket::MouseButton { button, is_down } => {
                    // --- SPEC-25a ---
                    let bit = 1u8 << (button.saturating_sub(1).min(7));
                    if handle_drag_mouse_button(button, is_down) {
                        if !is_down {
                            let mask_zero = if let Ok(mut st) = rx_state_pkt.lock() {
                                st.buttons_mask &= !bit;
                                st.tracker.record_button(button, false);
                                st.buttons_mask == 0
                            } else {
                                true
                            };
                            if mask_zero {
                                super::dragdrop::hide_edge_catcher();
                            }
                        }
                        continue;
                    }
                    let mask_zero = if let Ok(mut st) = rx_state_pkt.lock() {
                        if is_down {
                            st.buttons_mask |= bit;
                            false
                        } else {
                            st.buttons_mask &= !bit;
                            st.buttons_mask == 0
                        }
                    } else {
                        !is_down
                    };
                    if !is_down && mask_zero {
                        super::dragdrop::hide_edge_catcher();
                    }
                    // --- end SPEC-25a ---
                    if let Ok(mut st) = rx_state_pkt.lock() {
                        st.tracker.record_button(button, is_down);
                    }
                    let log_enabled = debug_log::is_enabled();
                    let (fg_hwnd, focus_hwnd) = if log_enabled {
                        let fg = debug_log::foreground_hwnd_raw();
                        let focus = debug_log::foreground_focus_hwnd_raw(fg);
                        (fg, focus)
                    } else {
                        (0, None)
                    };
                    let (sent, last_err) = injector::inject_mouse_button(button, is_down);
                    if log_enabled {
                        debug_log::record_event(debug_log::DebugEvent::RemoteMouseButton {
                            ts_us: debug_log::now_us(),
                            button,
                            is_down,
                            sent,
                            last_err,
                            hwnd: fg_hwnd,
                            focus_hwnd,
                        });
                    }
                }
                KvmPacket::MouseWheel { delta } => {
                    injector::inject_mouse_wheel(delta);
                }
                KvmPacket::KeyEvent {
                    key_code,
                    scan_code,
                    is_down,
                    extended,
                } => {
                    if let Ok(mut st) = rx_state_pkt.lock() {
                        st.tracker
                            .record_key(key_code, scan_code, is_down, extended, false);
                    }
                    let log_enabled = debug_log::is_enabled();
                    let (fg_hwnd, focus_hwnd) = if log_enabled {
                        let fg = debug_log::foreground_hwnd_raw();
                        let focus = debug_log::foreground_focus_hwnd_raw(fg);
                        (fg, focus)
                    } else {
                        (0, None)
                    };
                    let (sent, last_err) =
                        injector::inject_key(key_code, scan_code, is_down, extended);
                    if log_enabled {
                        debug_log::record_event(debug_log::DebugEvent::RemoteKey {
                            ts_us: debug_log::now_us(),
                            vk: key_code,
                            scan_code,
                            is_down,
                            extended,
                            sent,
                            last_err,
                            hwnd: fg_hwnd,
                            focus_hwnd,
                        });
                    }
                }
                KvmPacket::ClipboardText { text } => emit_clipboard_text(&app_h, &text),
                // --- SPEC-25c ---
                KvmPacket::SessionLock => {
                    release_receiver_inputs(&rx_state_pkt, "session_lock");
                    super::session_sync::handle_inbound_session_packet(&app_h, true);
                }
                KvmPacket::SessionSleep => {
                    release_receiver_inputs(&rx_state_pkt, "session_sleep");
                    super::session_sync::handle_inbound_session_packet(&app_h, false);
                }
                KvmPacket::OpenUrl { url } => {
                    super::open_on_linked::handle_open_url_packet(&app_h, &url);
                }
                KvmPacket::OpenFile { transfer_id } => {
                    super::open_on_linked::handle_open_file_packet(&app_h, &transfer_id);
                }
                // --- end SPEC-25c ---
                // --- SPEC-25b ---
                KvmPacket::AudioStart {
                    sample_rate,
                    channels,
                    frame_ms,
                    mute_local,
                } => {
                    super::audio::on_linked_audio_start(
                        sample_rate,
                        channels,
                        frame_ms,
                        mute_local,
                        audio_out_tx.clone(),
                        pkt_out_tx.clone(),
                        &app_h,
                    );
                }
                KvmPacket::AudioStop => {
                    super::audio::on_linked_audio_stop();
                }
                KvmPacket::MediaKey { vk, is_down } => {
                    super::media_keys::inject_media_key(vk, is_down);
                }
                // --- /SPEC-25b ---
                // --- SPEC-25a ---
                KvmPacket::DragOffer {
                    drag_id,
                    files,
                    button,
                } => {
                    handle_incoming_drag_offer(
                        &app_h,
                        drag_id,
                        files,
                        button,
                        super::dragdrop::should_use_forwarded_drag_button_inject(true),
                    );
                }
                KvmPacket::DragReady { drag_id } => {
                    handle_drag_ready(&app_h, &drag_id);
                }
                KvmPacket::DragCancel { drag_id } => {
                    handle_drag_cancel(&app_h, &drag_id);
                }
                KvmPacket::DragResult {
                    drag_id,
                    outcome,
                    notice,
                } => {
                    handle_drag_result(&app_h, &drag_id, outcome, notice);
                }
                KvmPacket::FileClipOffer { offer_id, files } => {
                    handle_incoming_file_clip_offer(&app_h, offer_id, files);
                }
                KvmPacket::ClipFetchRequest { offer_id } => {
                    handle_clip_fetch_request(&app_h, &offer_id);
                }
                KvmPacket::CursorSchemeOffer { roles } => {
                    handle_cursor_scheme_offer(roles);
                }
                // --- end SPEC-25a ---
                // --- SPEC-25c ---
                KvmPacket::TransferCancel { id } => {
                    handle_transfer_cancel_packet(&app_h, &id);
                }
                // --- end SPEC-25c ---
                _ => {}
            }
        }
    });

    let app_t = app_handle.clone();
    // --- SPEC-25a ---
    let transfer_out_tx = outgoing_tx.clone();
    // --- end SPEC-25a ---
    tokio::spawn(async move {
        loop {
            match transfer_rx.recv().await {
                Ok(ev) => {
                    // --- SPEC-25a ---
                    let tx_pkt = transfer_out_tx.clone();
                    handle_spec25a_transfer_event(&app_t, ev.clone(), move |pkt| {
                        let _ = tx_pkt.send(Outbound::Packet(pkt));
                    });
                    // --- end SPEC-25a ---
                    emit_transfer(&app_t, ev);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // Spawn datagram delta worker (authoritative receiver cursor) on a dedicated OS thread
    // so bulk file transfers or Tokio runtime tasks never starve cursor injection on Linked PC.
    let delta_out_tx = outgoing_tx.clone();
    let app_d = app_handle.clone();
    let _ =
        std::thread::Builder::new()
            .name("kvm-delta-inject".into())
            .spawn(move || {
                #[cfg(windows)]
                unsafe {
                    use windows::Win32::System::Threading::{
                        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL,
                    };
                    let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL);
                }
                loop {
                    let first_delta = match delta_rx.blocking_recv() {
                        Ok(d) => d,
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    };
                    let (mut agg_dx, mut agg_dy) = (first_delta.dx, first_delta.dy);
                    loop {
                        match delta_rx.try_recv() {
                            Ok(more) => {
                                agg_dx = agg_dx.saturating_add(more.dx);
                                agg_dy = agg_dy.saturating_add(more.dy);
                            }
                            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_)) => continue,
                            Err(_) => break,
                        }
                    }
                    enum DeltaAction {
                        Ignore,
                        Move {
                            pos: (i32, i32),
                            emit_pos: Option<(f32, f32)>,
                            // --- SPEC-25a ---
                            buttons_mask: u8,
                            // --- end SPEC-25a ---
                        },
                        Return {
                            edge: Edge,
                            relative_y: f32,
                            target_monitor: Option<String>,
                            along_px: Option<i32>,
                            keys: Vec<super::input_logic::TrackedKey>,
                            buttons: Vec<u8>,
                        },
                    }

                    let action = {
                        let Ok(mut st) = rx_state.lock() else {
                            continue;
                        };
                        if !st.is_entered {
                            DeltaAction::Ignore
                        } else if let Some(cur) = injector::get_cursor_pos() {
                            let now_ms = st.session_epoch.elapsed().as_millis() as u64;
                            let ReceiverSessionState {
                                entry_edge,
                                dwell_ms,
                                deadzone_px,
                                ref mut dwell_start_ms,
                                ref mut dwell_seam,
                                ref seams,
                                has_seams,
                                ref monitors,
                                buttons_mask,
                                ref mut drag_edge_hold_until,
                                ..
                            } = *st;
                            if has_seams {
                                match receiver_step_with_seams(
                                    cur,
                                    (agg_dx, agg_dy),
                                    monitors,
                                    seams,
                                    dwell_ms,
                                    deadzone_px,
                                    dwell_seam,
                                    dwell_start_ms,
                                    now_ms,
                                ) {
                                    ReceiverSeamStep::Move(pos) => {
                                        *drag_edge_hold_until = None;
                                        let emit_pos = if st.last_pos_emit.is_none_or(|t| {
                                            t.elapsed() >= Duration::from_millis(50)
                                        }) {
                                            st.last_pos_emit = Some(Instant::now());
                                            Some(normalize_in_union(pos, &st.union))
                                        } else {
                                            None
                                        };
                                        DeltaAction::Move {
                                            pos,
                                            emit_pos,
                                            buttons_mask,
                                        }
                                    }
                                    ReceiverSeamStep::Return {
                                        edge,
                                        relative_y,
                                        target_monitor,
                                        along_px,
                                    } => {
                                        // --- SPEC-25a ---
                                        if should_hold_drag_return(
                                            drag_edge_hold_until,
                                            buttons_mask,
                                            super::dragdrop::drag_drop_enabled(),
                                            Instant::now(),
                                        ) {
                                            DeltaAction::Move {
                                                pos: cur,
                                                emit_pos: None,
                                                buttons_mask,
                                            }
                                        } else {
                                            *drag_edge_hold_until = None;
                                            st.buttons_mask = 0;
                                            st.is_entered = false;
                                            RECEIVER_ENTERED
                                                .store(false, std::sync::atomic::Ordering::Relaxed);
                                            st.dwell_seam = None;
                                            st.dwell_start_ms = None;
                                            st.last_pos_emit = None;
                                            let (keys, buttons) = st.tracker.release_all();
                                            DeltaAction::Return {
                                                edge,
                                                relative_y,
                                                target_monitor,
                                                along_px,
                                                keys,
                                                buttons,
                                            }
                                        }
                                        // --- end SPEC-25a ---
                                    }
                                }
                            } else {
                                match receiver_step_with_dwell(
                                    cur,
                                    (agg_dx, agg_dy),
                                    monitors,
                                    entry_edge,
                                    dwell_ms,
                                    deadzone_px,
                                    dwell_start_ms,
                                    now_ms,
                                ) {
                                    ReceiverStep::Move(pos) => {
                                        *drag_edge_hold_until = None;
                                        let emit_pos = if st.last_pos_emit.is_none_or(|t| {
                                            t.elapsed() >= Duration::from_millis(50)
                                        }) {
                                            st.last_pos_emit = Some(Instant::now());
                                            Some(normalize_in_union(pos, &st.union))
                                        } else {
                                            None
                                        };
                                        DeltaAction::Move {
                                            pos,
                                            emit_pos,
                                            buttons_mask,
                                        }
                                    }
                                    ReceiverStep::Return { edge, relative_y } => {
                                        *drag_edge_hold_until = None;
                                        st.buttons_mask = 0;
                                        st.is_entered = false;
                                        RECEIVER_ENTERED
                                            .store(false, std::sync::atomic::Ordering::Relaxed);
                                        st.dwell_seam = None;
                                        st.dwell_start_ms = None;
                                        st.last_pos_emit = None;
                                        let (keys, buttons) = st.tracker.release_all();
                                        DeltaAction::Return {
                                            edge,
                                            relative_y,
                                            target_monitor: None,
                                            along_px: None,
                                            keys,
                                            buttons,
                                        }
                                    }
                                }
                            }
                        } else {
                            DeltaAction::Ignore
                        }
                    };

                    match action {
                        DeltaAction::Ignore => {}
                        DeltaAction::Move {
                            pos,
                            emit_pos,
                            buttons_mask,
                        } => {
                            if !injector::inject_mouse_move_absolute(pos.0, pos.1) {
                                // Never freeze the cursor: fall back to a plain warp.
                                injector::set_cursor_position(pos.0, pos.1);
                            }
                            injector::maybe_nudge_first_delta_after_unlock();
                            // --- SPEC-25a ---
                            if buttons_mask != 0 {
                                super::dragdrop::on_mouse_drag_probe(pos, buttons_mask, false);
                            } else if super::dragdrop::is_edge_catcher_visible() {
                                super::dragdrop::hide_edge_catcher();
                            }
                            // --- end SPEC-25a ---
                            if let Some((norm_x, norm_y)) = emit_pos {
                                let _ = delta_out_tx.send(Outbound::Packet(
                                    KvmPacket::CursorPosition { norm_x, norm_y },
                                ));
                            }
                        }
                        DeltaAction::Return {
                            edge,
                            relative_y,
                            target_monitor,
                            along_px,
                            keys,
                            buttons,
                        } => {
                            let released_keys = keys.len();
                            let released_buttons = buttons.len();
                            release_tracked_inputs(keys, buttons);
                            // --- SPEC-25a ---
                            super::dragdrop::hide_edge_catcher();
                            super::dragdrop::set_edge_catcher_poll_state(true, false);
                            super::cursor_scheme::restore_system_cursors(
                                &super::staging::default_staging_root(),
                            );
                            // --- end SPEC-25a ---
                            let edge_str = edge.as_str().to_string();
                            let _ = delta_out_tx.send(Outbound::Packet(KvmPacket::CursorReturn {
                                edge: edge_str.clone(),
                                relative_y,
                                target_monitor,
                                along_px,
                            }));
                            if debug_log::is_enabled() {
                                debug_log::record_event(debug_log::DebugEvent::ReturnSent {
                                    ts_us: debug_log::now_us(),
                                    edge,
                                    relative_y,
                                    released_keys,
                                    released_buttons,
                                });
                            }
                            let _ = app_d.emit("kvm-cursor-return", &edge_str);
                        }
                    }
                }
            });

    let mut kvm = state.kvm.lock().unwrap();
    kvm.server = Some(server_handle);

    // Ensure discovery is broadcasting
    kvm.discovery.start(id.clone(), name.clone(), port, sw, sh);

    Ok(build_status(id, name, sw, sh, monitors, &kvm))
}

#[tauri::command]
pub async fn kvm_start_server(
    port: Option<u16>,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<KvmStatus, String> {
    start_server_inner(port, &state, &app_handle).await
}

#[tauri::command]
pub async fn kvm_set_allow_control(
    enabled: bool,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<KvmStatus, String> {
    if enabled {
        start_server_inner(None, &state, &app_handle).await
    } else {
        let (id, name, sw, sh, monitors) = get_local_machine_info(&app_handle, &state)?;
        let mut kvm = state.kvm.lock().unwrap();
        stop_server_in_state(&mut kvm);
        Ok(build_status(id, name, sw, sh, monitors, &kvm))
    }
}

#[tauri::command]
pub async fn kvm_stop_server(state: State<'_, AppState>) -> Result<(), String> {
    let mut kvm = state.kvm.lock().unwrap();
    stop_server_in_state(&mut kvm);
    if kvm.client.is_none() {
        kvm.hooks.stop_hooks();
    }
    Ok(())
}

#[tauri::command]
pub async fn kvm_get_discovered_peers(
    state: State<'_, AppState>,
) -> Result<Vec<DiscoveredNode>, String> {
    let kvm = state.kvm.lock().unwrap();
    Ok(kvm.discovery.list_discovered())
}

#[tauri::command]
pub async fn kvm_pair_peer(
    ip: String,
    port: Option<u16>,
    pin: String,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let (id, name, _sw, _sh, monitors) = get_local_machine_info(&app_handle, &state)?;
    let (identity, trust) = get_security(&app_handle, &state)?;
    let (_ep, conn, _send, _recv, _peer_info) = handshake(
        &ip,
        port.unwrap_or(KVM_PORT),
        id,
        name,
        monitors,
        Some(pin),
        &identity,
        &trust,
    )
    .await?;
    conn.close(0u32.into(), b"paired");
    Ok(())
}

#[tauri::command]
pub async fn kvm_list_trusted(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<Vec<TrustedPeerInfo>, String> {
    let (_, trust) = get_security(&app_handle, &state)?;
    Ok(trust
        .list()
        .into_iter()
        .map(|p| TrustedPeerInfo {
            computer_id: p.computer_id,
            name: p.name,
            address: p.address,
        })
        .collect())
}

#[tauri::command]
pub async fn kvm_forget_peer(
    computer_id: String,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let (_, trust) = get_security(&app_handle, &state)?;
    let mut kvm = state.kvm.lock().unwrap();
    forget_peer_in_state(&mut kvm, &trust, &computer_id);
    Ok(())
}

#[tauri::command]
pub async fn kvm_connect_peer(
    ip: String,
    port: Option<u16>,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let (id, name, _sw, _sh, monitors) = get_local_machine_info(&app_handle, &state)?;
    let (identity, trust) = get_security(&app_handle, &state)?;
    // --- SPEC-25c ---
    super::open_on_linked::cleanup_expired_open_staging(
        &super::open_on_linked::open_staging_root(),
        super::open_on_linked::OPEN_TEMP_MAX_AGE,
        std::time::SystemTime::now(),
    );
    // --- end SPEC-25c ---
    // --- SPEC-25a ---
    super::staging::cleanup_expired_staging(
        &super::staging::default_staging_root(),
        super::staging::STAGING_MAX_AGE,
        std::time::SystemTime::now(),
    );
    // --- end SPEC-25a ---

    let client = KvmClientHandle::connect(
        &ip,
        port.unwrap_or(KVM_PORT),
        id,
        name,
        monitors,
        None,
        &identity,
        &trust,
        get_download_dir(),
    )
    .await?;

    let (clip_tx, mut clip_rx) = tokio::sync::mpsc::channel::<Outbound>(32);
    start_clipboard_sync(clip_tx, Arc::clone(&client.is_connected));
    let client_send = client.send_packet_tx.clone();
    let clip_conn = client.conn.clone();
    let clip_app = app_handle.clone();
    // --- SPEC-25a ---
    if super::dragdrop::pointer_scheme_enabled() {
        spawn_send_cursor_scheme(client.send_packet_tx.clone(), client.conn.clone());
    }
    let transfer_pkt_tx = client.send_packet_tx.clone();
    // --- end SPEC-25a ---
    tokio::spawn(async move {
        while let Some(item) = clip_rx.recv().await {
            match item {
                Outbound::Packet(pkt) => {
                    if let KvmPacket::FileClipOffer {
                        ref offer_id,
                        ref files,
                    } = pkt
                    {
                        replace_previous_clip_offer(&clip_app, offer_id);
                        let _ = super::staging::sweep_expired_pending_outbound_clips(
                            super::staging::AWAITING_FETCH_TTL,
                        );
                        let total_bytes: u64 =
                            files.iter().filter(|f| !f.is_dir).map(|f| f.size).sum();
                        let summary = summarize_offer_name(files);
                        if super::staging::decide_clip_offer_mode(
                            total_bytes,
                            super::staging::clip_prefetch_limit_mb(),
                        ) == super::staging::ClipOfferMode::Prefetch
                        {
                            super::files::register_outbound_total(offer_id, total_bytes);
                            super::debug_log::note(|| {
                                format!(
                                "CLIP_OFFER_SEND side=client id={offer_id} files={} total_bytes={total_bytes}",
                                files.len()
                            )
                            });
                            emit_transfer_progress(
                                &clip_app,
                                offer_id,
                                "clip",
                                &summary,
                                0,
                                "active",
                                total_bytes,
                            );
                        } else {
                            super::debug_log::note(|| {
                                format!(
                                "CLIP_OFFER_SEND side=client id={offer_id} files={} total_bytes={total_bytes} mode=awaiting_fetch",
                                files.len()
                            )
                            });
                        }
                    }
                    let _ = client_send.send(pkt).await;
                }
                Outbound::Image { format, data } => {
                    let conn = clip_conn.clone();
                    tokio::spawn(async move {
                        let _ = send_image(&conn, format, data).await;
                    });
                }
                // --- SPEC-25c ---
                Outbound::OpenFile { .. } => {}
                // --- end SPEC-25c ---
                // --- SPEC-25a ---
                Outbound::DragFile {
                    drag_id,
                    rel_path,
                    path,
                    size,
                } => {
                    spawn_client_bulk_transfer(
                        &clip_conn,
                        &clip_app,
                        super::files::BulkTransferKind::Drag,
                        drag_id,
                        rel_path,
                        path,
                        size,
                    );
                }
                Outbound::ClipFile {
                    offer_id,
                    rel_path,
                    path,
                    size,
                } => {
                    spawn_client_bulk_transfer(
                        &clip_conn,
                        &clip_app,
                        super::files::BulkTransferKind::Clip,
                        offer_id,
                        rel_path,
                        path,
                        size,
                    );
                }
                Outbound::CursorFile { role, name, data } => {
                    let conn = clip_conn.clone();
                    tokio::spawn(async move {
                        let _ = super::files::send_cursor_file(&conn, role, name, &data).await;
                    });
                } // --- end SPEC-25a ---
            }
        }
    });

    let peer_id_for_rx = client.peer_id.clone();
    let client_connected_flag = Arc::clone(&client.is_connected);
    let mut in_rx = client.incoming_tx.subscribe();
    let app_h = app_handle.clone();
    tokio::spawn(async move {
        loop {
            let pkt = match in_rx.recv().await {
                Ok(p) => p,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            };
            match pkt {
                KvmPacket::ClipboardText { text } => emit_clipboard_text(&app_h, &text),
                KvmPacket::CursorReturn {
                    edge,
                    relative_y,
                    target_monitor,
                    along_px,
                } => {
                    if let Some(app_state) = app_h.try_state::<AppState>() {
                        if let Ok(kvm) = app_state.kvm.lock() {
                            kvm.hooks.handle_cursor_return(
                                &edge,
                                relative_y,
                                target_monitor.as_deref(),
                                along_px,
                            );
                        }
                    }
                    let _ = app_h.emit("kvm-cursor-return", "");
                    // --- SPEC-25a ---
                    if let Ok(mut g) = pending_target_drag().lock() {
                        if let Some(ref mut pending) = *g {
                            if !pending.is_fallback && pending.forwarded_button_inject {
                                let held = if super::hooks::is_hook_active() {
                                    super::hooks::is_physical_button_down(pending.button)
                                } else {
                                    true
                                };
                                super::dragdrop::set_remote_drag_button_state(pending.button, held);
                                if !held {
                                    pending.released_early = true;
                                    super::dragdrop::set_pending_target_drag_early_release(true);
                                }
                            }
                        }
                    }
                    super::dragdrop::set_drag_leap_in_progress(false);
                    super::dragdrop::hide_edge_catcher();
                    // --- end SPEC-25a ---
                }
                // --- SPEC-25a ---
                KvmPacket::DragOffer {
                    drag_id,
                    files,
                    button,
                } => {
                    handle_incoming_drag_offer(
                        &app_h,
                        drag_id,
                        files,
                        button,
                        super::dragdrop::should_use_forwarded_drag_button_inject(true),
                    );
                }
                KvmPacket::DragReady { drag_id } => {
                    handle_drag_ready(&app_h, &drag_id);
                }
                KvmPacket::DragCancel { drag_id } => {
                    handle_drag_cancel(&app_h, &drag_id);
                }
                KvmPacket::DragResult {
                    drag_id,
                    outcome,
                    notice,
                } => {
                    handle_drag_result(&app_h, &drag_id, outcome, notice);
                }
                KvmPacket::FileClipOffer { offer_id, files } => {
                    handle_incoming_file_clip_offer(&app_h, offer_id, files);
                }
                KvmPacket::ClipFetchRequest { offer_id } => {
                    handle_clip_fetch_request(&app_h, &offer_id);
                }
                KvmPacket::CursorSchemeOffer { roles } => {
                    handle_cursor_scheme_offer(roles);
                }
                // --- end SPEC-25a ---
                // --- SPEC-25c ---
                KvmPacket::TransferCancel { id } => {
                    handle_transfer_cancel_packet(&app_h, &id);
                }
                // --- end SPEC-25c ---
                KvmPacket::CursorPosition { norm_x, norm_y } => {
                    let _ = app_h.emit(
                        "kvm-cursor-pos",
                        serde_json::json!({ "norm_x": norm_x, "norm_y": norm_y }),
                    );
                }
                KvmPacket::SessionDisconnect => {
                    if let Some(app_state) = app_h.try_state::<AppState>() {
                        if let Ok(mut kvm) = app_state.kvm.lock() {
                            if kvm.client.as_ref().is_some_and(|c| {
                                Arc::ptr_eq(&c.is_connected, &client_connected_flag)
                            }) {
                                kvm.client = None;
                                kvm.hooks.stop_hooks();
                                kvm.topology
                                    .computers
                                    .retain(|c| c.is_local || c.id != peer_id_for_rx);
                                kvm.topology.links.retain(|l| {
                                    l.from_id != peer_id_for_rx && l.to_id != peer_id_for_rx
                                });
                                super::hooks::restore_cursor_visibility();
                                // --- SPEC-25b ---
                                super::audio::on_main_disconnected();
                                // --- /SPEC-25b ---
                                // --- SPEC-25a ---
                                cleanup_spec25a_on_disconnect_with_app(Some(&app_h));
                                // --- end SPEC-25a ---
                            }
                        }
                    }
                    let _ = app_h.emit("kvm-peer-disconnected", &peer_id_for_rx);
                }
                // --- SPEC-25c ---
                KvmPacket::SessionLock => {
                    super::session_sync::handle_inbound_session_packet(&app_h, true);
                }
                KvmPacket::SessionSleep => {
                    super::session_sync::handle_inbound_session_packet(&app_h, false);
                }
                KvmPacket::OpenUrl { url } => {
                    super::open_on_linked::handle_open_url_packet(&app_h, &url);
                }
                KvmPacket::OpenFile { transfer_id } => {
                    super::open_on_linked::handle_open_file_packet(&app_h, &transfer_id);
                }
                // --- end SPEC-25c ---
                KvmPacket::InputBlocked { foreground_process } => {
                    super::debug_log::note(|| {
                        format!("INPUT_BLOCKED_RECV side=main fg_process={foreground_process}")
                    });
                    let _ = app_h.emit(
                        "kvm-input-blocked",
                        serde_json::json!({ "proc": foreground_process }),
                    );
                }
                // --- SPEC-25b ---
                KvmPacket::AudioActivity { active } => {
                    super::media_keys::record_linked_audio_activity(active);
                }
                KvmPacket::AudioMuteState { state } => {
                    super::audio::on_main_audio_mute_state(&state);
                }
                // --- /SPEC-25b ---
                _ => {}
            }
        }
    });

    let mut transfer_rx = client.transfer_tx.subscribe();
    let app_t = app_handle.clone();
    tokio::spawn(async move {
        loop {
            match transfer_rx.recv().await {
                Ok(ev) => {
                    // --- SPEC-25a ---
                    let tx_pkt = transfer_pkt_tx.clone();
                    handle_spec25a_transfer_event(&app_t, ev.clone(), move |pkt| {
                        let _ = tx_pkt.try_send(pkt);
                    });
                    // --- end SPEC-25a ---
                    emit_transfer(&app_t, ev);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    // --- SPEC-25b ---
    let mut audio_rx = client.audio_tx.subscribe();
    tokio::spawn(async move {
        loop {
            match audio_rx.recv().await {
                Ok(dg) => super::audio::on_main_audio_datagram(dg),
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    let main_audio_pkt_tx = client.send_packet_tx.clone();
    // --- /SPEC-25b ---

    let peer_node = ComputerNode {
        id: client.peer_id.clone(),
        name: client.peer_name.clone(),
        is_local: false,
        address: Some(ip),
        bounds: union_rect(&client.peer_monitors),
        monitors: client.peer_monitors.clone(),
    };

    let mut kvm = state.kvm.lock().unwrap();
    if kvm.client.is_some() {
        disconnect_client_in_state(&mut kvm);
    }

    let local_monitors = get_local_monitors();
    if let Some(prev_origin) = kvm.topology.remote_origin {
        let prev_main = kvm
            .topology
            .computers
            .iter()
            .find(|c| c.is_local)
            .map(|c| c.monitors.clone())
            .unwrap_or_else(|| local_monitors.clone());
        let prev_linked = kvm
            .topology
            .computers
            .iter()
            .find(|c| !c.is_local && c.id == peer_node.id)
            .map(|c| c.monitors.clone())
            .unwrap_or_default();
        if let Some((fallback_origin, side)) = handle_monitor_change_fallback(
            &prev_main,
            &local_monitors,
            &prev_linked,
            &peer_node.monitors,
            prev_origin,
            None,
        ) {
            kvm.topology.remote_origin = Some(fallback_origin);
            let _ = app_handle.emit(
                "kvm-layout-notice",
                serde_json::json!({
                    "reason": "monitors_changed",
                    "side": side.as_str(),
                    "remoteOrigin": fallback_origin,
                }),
            );
        }
    }

    if let Some(existing) = kvm
        .topology
        .computers
        .iter_mut()
        .find(|c| !c.is_local && c.id == peer_node.id)
    {
        *existing = peer_node;
    } else {
        kvm.topology.computers.push(peer_node);
    }
    kvm.client = Some(client);
    // --- SPEC-25b ---
    super::audio::on_main_connected(main_audio_pkt_tx, &app_handle);
    // --- /SPEC-25b ---

    if kvm.topology.remote_origin.is_some() || !kvm.topology.links.is_empty() {
        let mut updated = kvm.topology.clone();
        if let Some(local_node) = updated.computers.iter_mut().find(|c| c.is_local) {
            if !local_monitors.is_empty() {
                local_node.monitors = local_monitors.clone();
                local_node.bounds = union_rect(&local_monitors);
            }
        }
        apply_controller_topology(&mut kvm, updated, &app_handle);
    }
    Ok(())
}

#[tauri::command]
pub async fn kvm_disconnect_peer(state: State<'_, AppState>) -> Result<(), String> {
    let mut kvm = state.kvm.lock().unwrap();
    disconnect_peer_in_state(&mut kvm);
    Ok(())
}

#[tauri::command]
pub async fn kvm_send_file(file_path: String, state: State<'_, AppState>) -> Result<(), String> {
    let path_buf = std::path::PathBuf::from(file_path);
    if !path_buf.is_file() {
        return Err("File does not exist".to_string());
    }
    let conn = {
        let kvm = state.kvm.lock().unwrap();
        kvm.client.as_ref().ok_or("No connected peer")?.conn.clone()
    };
    tokio::spawn(async move {
        if let Err(e) = send_file(&conn, &path_buf).await {
            eprintln!("[KVM] file send failed: {e}");
        }
    });
    Ok(())
}

pub(crate) fn prepare_topology_update(
    prev_topology: &TopologyConfig,
    mut next_topology: TopologyConfig,
    live_local_monitors: &[MonitorDisplay],
    live_peer: Option<(&str, &[MonitorDisplay])>,
) -> Result<(TopologyConfig, Option<serde_json::Value>), String> {
    if let Some((peer_id, peer_monitors)) = live_peer {
        let peer_bounds = union_rect(peer_monitors);
        if let Some(node) = next_topology
            .computers
            .iter_mut()
            .find(|c| !c.is_local && c.id == peer_id)
        {
            if node.monitors.is_empty() && !peer_monitors.is_empty() {
                node.monitors = peer_monitors.to_vec();
                node.bounds = peer_bounds;
            }
        }
    }

    let local_id = next_topology
        .computers
        .iter()
        .find(|c| c.is_local)
        .map(|c| c.id.clone());

    let prev_main = prev_topology
        .computers
        .iter()
        .find(|c| c.is_local)
        .map(|c| c.monitors.clone())
        .unwrap_or_default();
    let prev_linked = prev_topology
        .computers
        .iter()
        .find(|c| !c.is_local)
        .map(|c| c.monitors.clone())
        .unwrap_or_default();

    let next_main: Vec<MonitorDisplay> = if !live_local_monitors.is_empty() {
        live_local_monitors.to_vec()
    } else {
        next_topology
            .computers
            .iter()
            .find(|c| c.is_local)
            .map(|c| c.monitors.clone())
            .unwrap_or_default()
    };

    let next_linked: Vec<MonitorDisplay> = if let Some((peer_id, peer_monitors)) = live_peer {
        if !peer_monitors.is_empty() {
            peer_monitors.to_vec()
        } else {
            next_topology
                .computers
                .iter()
                .find(|c| !c.is_local && c.id == peer_id)
                .map(|c| c.monitors.clone())
                .unwrap_or_default()
        }
    } else {
        next_topology
            .computers
            .iter()
            .find(|c| !c.is_local)
            .map(|c| c.monitors.clone())
            .unwrap_or_default()
    };

    let mut notice = None;
    let changed = (!prev_main.is_empty() && monitors_changed(&prev_main, &next_main))
        || (!prev_linked.is_empty() && monitors_changed(&prev_linked, &next_linked));

    if changed {
        if let Some(prev_origin) = prev_topology.remote_origin.or(next_topology.remote_origin) {
            if let Some((fallback_origin, side)) = handle_monitor_change_fallback(
                &prev_main,
                &next_main,
                &prev_linked,
                &next_linked,
                prev_origin,
                None,
            ) {
                next_topology.remote_origin = Some(fallback_origin);
                notice = Some(serde_json::json!({
                    "reason": "monitors_changed",
                    "side": side.as_str(),
                    "remoteOrigin": fallback_origin,
                }));
            }
        }
    } else if let Some(origin) = next_topology.remote_origin {
        if !next_main.is_empty()
            && !next_linked.is_empty()
            && has_any_overlap(&next_main, &next_linked, origin)
        {
            return Err("Monitor layout has overlapping displays".to_string());
        }
    }

    if next_topology.remote_origin.is_none() {
        next_topology.remote_origin = resolve_remote_origin(
            &next_topology,
            &next_main,
            &next_linked,
            local_id.as_deref(),
        );
    }

    if !next_main.is_empty() {
        if let Some(local_node) = next_topology.computers.iter_mut().find(|c| c.is_local) {
            local_node.monitors = next_main.clone();
            local_node.bounds = union_rect(&next_main);
        }
    }

    Ok((next_topology, notice))
}

fn apply_controller_topology(kvm: &mut KvmState, topology: TopologyConfig, app_handle: &AppHandle) {
    kvm.topology = topology.clone();

    let (packet_tx, mut packet_rx) = tokio::sync::mpsc::channel::<KvmPacket>(256);
    let (delta_tx, mut delta_rx) = tokio::sync::mpsc::channel::<(i32, i32)>(512);

    if let Some(ref client) = kvm.client {
        let main_monitors = topology
            .computers
            .iter()
            .find(|c| c.is_local)
            .map(|c| c.monitors.clone())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(get_local_monitors);
        let linked_monitors = topology
            .computers
            .iter()
            .find(|c| !c.is_local && c.id == client.peer_id)
            .map(|c| c.monitors.clone())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| client.peer_monitors.clone());
        let local_id = topology
            .computers
            .iter()
            .find(|c| c.is_local)
            .map(|c| c.id.as_str());
        let origin = resolve_remote_origin(&topology, &main_monitors, &linked_monitors, local_id);
        let seams = origin
            .map(|o| compute_linked_seams(&main_monitors, &linked_monitors, o))
            .unwrap_or_default();
        // --- SPEC-25a ---
        let main_seams = origin
            .map(|o| super::input_logic::compute_seams(&main_monitors, &linked_monitors, o))
            .unwrap_or_default();
        let mut main_strips =
            super::dragdrop::compute_main_seam_strips(&main_monitors, &main_seams);
        if main_strips.is_empty() {
            if let Some(link) = local_id.and_then(|lid| {
                topology
                    .links
                    .iter()
                    .find(|l| l.from_id == lid || l.to_id == lid)
            }) {
                let edge_str = if local_id == Some(link.from_id.as_str()) {
                    link.from_edge.as_str()
                } else {
                    link.to_edge.as_str()
                };
                if let (Some(side), Some(mon)) = (
                    Edge::from_str(edge_str),
                    main_monitors
                        .iter()
                        .find(|m| m.is_primary)
                        .or(main_monitors.first()),
                ) {
                    let (start, end) = match side {
                        Edge::Left | Edge::Right => (mon.y, mon.y + mon.height),
                        Edge::Top | Edge::Bottom => (mon.x, mon.x + mon.width),
                    };
                    if let Some(s) = super::dragdrop::compute_seam_strip(mon, side, start, end, 2) {
                        main_strips.push(s);
                    }
                }
            }
        }
        let ec_conn = client.conn.clone();
        let ec_packet_tx = packet_tx.clone();
        let ec_app = app_handle.clone();
        // --- end SPEC-25a ---

        let seam_tx = client.send_packet_tx.clone();
        tokio::spawn(async move {
            let _ = seam_tx.send(KvmPacket::Seams { seams }).await;
        });

        let client_sender = client.send_packet_tx.clone();
        let conn = client.conn.clone();
        let app_h = app_handle.clone();

        tokio::spawn(async move {
            while let Some(packet) = packet_rx.recv().await {
                match &packet {
                    KvmPacket::CursorEnter { edge, .. } => {
                        let _ = app_h.emit("kvm-cursor-enter", edge);
                    }
                    KvmPacket::EmergencyRelease => {
                        let _ = app_h.emit("kvm-cursor-return", "");
                    }
                    _ => {}
                }
                let _ = client_sender.send(packet).await;
            }
        });

        tokio::spawn(async move {
            let mut seq = 0u32;
            while let Some((dx, dy)) = delta_rx.recv().await {
                send_delta(&conn, &UdpMouseDelta { dx, dy, seq });
                seq = seq.wrapping_add(1);
            }
        });

        let app_for_blocked = app_handle.clone();
        let on_leap_blocked: super::hooks::LeapBlockedCallback =
            Arc::new(move |proc_name: &str| {
                let _ = app_for_blocked.emit(
                    "kvm-leap-blocked",
                    serde_json::json!({
                        "reason": "elevated_foreground",
                        "proc": proc_name,
                    }),
                );
            });
        // --- SPEC-25a ---
        let on_drag_engaged: super::hooks::DragModeEngagedCallback = Arc::new(move |button| {
            let pending = pending_outbound_drag()
                .lock()
                .ok()
                .and_then(|mut g| g.take());
            if !super::dragdrop::drag_drop_enabled() {
                super::debug_log::note(|| {
                    format!("DRAG_OFFER_SKIPPED side=main reason=disabled button={button}")
                });
                return;
            }
            if let Some(p) = pending {
                let total_bytes: u64 = p.files.iter().filter(|f| !f.is_dir).map(|f| f.size).sum();
                let summary = summarize_offer_name(&p.files);
                super::debug_log::note(|| {
                    format!(
                    "DRAG_OFFER_SEND side=main id={} files={} total_bytes={total_bytes} button={button}",
                    p.drag_id,
                    p.files.len(),
                )
                });
                let reg_ev = super::dragdrop::global_drag_registry().register_staging(
                    &p.drag_id,
                    &summary,
                    total_bytes,
                );
                emit_drag_progress_payload(&p.app, &reg_ev);
                let _ = p.pkt_tx.try_send(KvmPacket::DragOffer {
                    drag_id: p.drag_id.clone(),
                    files: p.files,
                    button,
                });
                spawn_outbound_drag_files_client(
                    p.drag_id,
                    p.sources,
                    total_bytes,
                    p.conn,
                    p.pkt_tx,
                    p.app,
                );
            } else {
                super::debug_log::note(|| {
                    format!(
                        "DRAG_OFFER_SKIPPED side=main reason=no_pending_outbound button={button}"
                    )
                });
            }
        });
        kvm.hooks
            .set_drag_mode_engaged_callback(Some(on_drag_engaged));

        let on_main_drag_caught: super::dragdrop::DragCaughtCallback =
            Arc::new(move |paths, button, drop_pt| {
                if !super::dragdrop::drag_drop_enabled() {
                    super::dragdrop::set_drag_leap_in_progress(false);
                    return;
                }
                let collected = match super::staging::collect_paths_to_file_meta(&paths) {
                    Ok(v) => v,
                    Err(e) => {
                        super::dragdrop::set_drag_leap_in_progress(false);
                        emit_continuity_notice(&ec_app, &format!("Drag rejected: {e}"));
                        return;
                    }
                };
                let (files, sources) = collected;
                if files.is_empty() || sources.is_empty() {
                    super::dragdrop::set_drag_leap_in_progress(false);
                    return;
                }
                let drag_id = uuid::Uuid::new_v4().to_string();
                if let Ok(mut g) = pending_outbound_drag().lock() {
                    *g = Some(PendingOutboundDrag {
                        drag_id: drag_id.clone(),
                        files,
                        sources,
                        conn: ec_conn.clone(),
                        pkt_tx: ec_packet_tx.clone(),
                        app: ec_app.clone(),
                    });
                }
                let engaged = ec_app
                    .try_state::<AppState>()
                    .and_then(|st| {
                        st.kvm
                            .lock()
                            .ok()
                            .map(|kvm| kvm.hooks.engage_drag_leap(drop_pt, button))
                    })
                    .unwrap_or(false);
                if !engaged {
                    if let Ok(mut g) = pending_outbound_drag().lock() {
                        *g = None;
                    }
                    super::dragdrop::set_drag_leap_in_progress(false);
                }
            });
        super::dragdrop::set_edge_catcher_poll_state(false, false);
        super::dragdrop::configure_edge_catcher(main_strips, Some(on_main_drag_caught));
        // --- end SPEC-25a ---
        kvm.hooks
            .start_hooks(topology, packet_tx, delta_tx, Some(on_leap_blocked));
    }
}

#[tauri::command]
pub async fn kvm_update_topology(
    topology: TopologyConfig,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let live_local_monitors = get_local_monitors();
    let mut kvm = state.kvm.lock().unwrap();
    let live_peer_pair = kvm
        .client
        .as_ref()
        .map(|c| (c.peer_id.clone(), c.peer_monitors.clone()));
    let live_peer_ref = live_peer_pair
        .as_ref()
        .map(|(id, mons)| (id.as_str(), mons.as_slice()));

    let (prepared, notice) =
        prepare_topology_update(&kvm.topology, topology, &live_local_monitors, live_peer_ref)?;

    if let Some(payload) = notice {
        let _ = app_handle.emit("kvm-layout-notice", payload);
    }

    apply_controller_topology(&mut kvm, prepared, &app_handle);

    Ok(())
}

#[tauri::command]
pub async fn kvm_emergency_release(state: State<'_, AppState>) -> Result<(), String> {
    let kvm = state.kvm.lock().unwrap();
    kvm.hooks.emergency_release();
    Ok(())
}

#[tauri::command]
pub async fn kvm_set_debug_log(enabled: bool, app_handle: AppHandle) -> Result<String, String> {
    let log_dir = app_handle.path().app_log_dir().map_err(|e| e.to_string())?;
    let log_path = debug_log::log_path_for_dir(&log_dir);
    debug_log::set_enabled(enabled, log_path.clone());
    Ok(log_path.to_string_lossy().into_owned())
}

#[tauri::command]
pub async fn kvm_get_debug_log_path(app_handle: AppHandle) -> Result<String, String> {
    let log_dir = app_handle.path().app_log_dir().map_err(|e| e.to_string())?;
    Ok(debug_log::log_path_for_dir(&log_dir)
        .to_string_lossy()
        .into_owned())
}

#[tauri::command]
pub async fn kvm_open_debug_log_folder(app_handle: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let log_dir = app_handle.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&log_dir).map_err(|e| e.to_string())?;
    app_handle
        .opener()
        .open_path(log_dir.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn kvm_debug_note(text: String) -> Result<(), String> {
    if !debug_log::is_enabled() {
        return Ok(());
    }
    debug_log::record_event(debug_log::DebugEvent::Note {
        ts_us: debug_log::now_us(),
        hwnd: debug_log::foreground_hwnd_raw(),
        text,
    });
    Ok(())
}

// --- SPEC-25c ---
#[tauri::command]
pub fn kvm_set_session_settings(
    lock_together: bool,
    sleep_together: bool,
    open_on_linked_menu: bool,
) -> Result<(), String> {
    super::session_sync::set_session_sync_settings(lock_together, sleep_together);
    super::open_on_linked::set_open_on_linked_menu_enabled(open_on_linked_menu);
    super::open_on_linked::sync_context_menu(open_on_linked_menu)
}

#[tauri::command]
pub async fn kvm_open_url_on_linked(url: String, state: State<'_, AppState>) -> Result<(), String> {
    super::open_on_linked::send_open_url_from_state(&state, &url)
}

#[tauri::command]
pub async fn kvm_open_file_on_linked(path: String, app_handle: AppHandle) -> Result<(), String> {
    super::open_on_linked::send_and_open_file_from_app(&app_handle, std::path::PathBuf::from(path))
        .await
}

#[tauri::command]
pub async fn kvm_cancel_transfer(
    id: String,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    super::debug_log::note(|| format!("TRANSFER_CANCEL_SEND id={id}"));
    let mut emitted = false;
    match super::dragdrop::global_drag_registry().cancel_at_any_stage(&id) {
        super::dragdrop::CancelDragAction::Cancelled(ev) => {
            emit_drag_progress_payload(&app_handle, &ev);
            emitted = true;
        }
        super::dragdrop::CancelDragAction::ClearedFinished => {
            super::staging::cleanup_staging_entry(&super::staging::default_staging_root(), &id);
            return Ok(());
        }
        super::dragdrop::CancelDragAction::NotTracked => {}
    }
    let cancelled_entries = super::files::global_cancel_registry().cancel_any_conn(&id);
    for (_, meta) in &cancelled_entries {
        if !emitted || meta.kind != "drag" {
            emit_transfer_progress(
                &app_handle,
                &meta.id,
                &meta.kind,
                &meta.name,
                0,
                "cancelled",
                meta.size_bytes,
            );
            emitted = true;
        }
    }
    if let Some(pending) = super::staging::take_pending_inbound_clip(&id) {
        if !emitted {
            emit_transfer_progress(
                &app_handle,
                &id,
                "clip",
                &pending.summary,
                0,
                "cancelled",
                pending.total_bytes,
            );
            emitted = true;
        }
    }
    let _ = super::staging::take_pending_outbound_clip(&id);
    if let Some((kind, total_bytes)) = super::staging::cancel_transfer_with_info(&id) {
        if !emitted && kind != super::staging::TransferKind::Drag {
            emit_transfer_progress(
                &app_handle,
                &id,
                match kind {
                    super::staging::TransferKind::Drag => "drag",
                    super::staging::TransferKind::Clip => "clip",
                },
                "Files",
                0,
                "cancelled",
                total_bytes,
            );
        }
    }
    super::staging::cleanup_staging_entry(&super::staging::default_staging_root(), &id);
    super::dragdrop::cancel_target_drag_for_id(&id, "transfer_cancel");
    if let Ok(mut g) = pending_target_drag().lock() {
        if g.as_ref().is_some_and(|p| p.drag_id == id) {
            *g = None;
        }
    }
    if let Ok(mut g) = pending_outbound_drag().lock() {
        if g.as_ref().is_some_and(|p| p.drag_id == id) {
            *g = None;
        }
    }
    super::open_on_linked::cancel_pending_open(&id);
    let kvm = state
        .kvm
        .lock()
        .map_err(|e| format!("Mutex lock poisoned: {e}"))?;
    if let Some(ref client) = kvm.client {
        if client
            .is_connected
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            let _ = client
                .send_packet_tx
                .try_send(KvmPacket::TransferCancel { id });
            return Ok(());
        }
    }
    if let Some(ref server) = kvm.server {
        if server.is_running.load(std::sync::atomic::Ordering::Relaxed) {
            let _ = server
                .outgoing_tx
                .send(Outbound::Packet(KvmPacket::TransferCancel { id }));
            return Ok(());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn kvm_fetch_clip(
    offer_id: String,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    if let Some(pending) = super::staging::take_pending_inbound_clip(&offer_id) {
        let staging_root = super::staging::default_staging_root();
        let download_dir = get_download_dir();
        let limit_mb = super::staging::staging_limit_mb();
        match super::staging::register_transfer_offer(
            &pending.offer_id,
            super::staging::TransferKind::Clip,
            &pending.files,
            &staging_root,
            &download_dir,
            limit_mb,
        ) {
            Ok(plan) => {
                super::debug_log::note(|| {
                    format!(
                        "CLIP_FETCH_REQ side=receiver id={offer_id} bytes={}",
                        pending.total_bytes
                    )
                });
                if plan.destination == super::staging::TransferDestination::DownloadsFallback {
                    emit_continuity_notice(
                        &app_handle,
                        &format!(
                            "Copied files exceed {limit_mb} MB staging limit — saving \"{}\" to Downloads\\AnyLeap_Transfers",
                            pending.summary
                        ),
                    );
                } else {
                    emit_transfer_progress(
                        &app_handle,
                        &pending.offer_id,
                        "clip",
                        &pending.summary,
                        0,
                        "active",
                        pending.total_bytes,
                    );
                }
                let pkt = KvmPacket::ClipFetchRequest { offer_id };
                let kvm = state
                    .kvm
                    .lock()
                    .map_err(|e| format!("Mutex lock poisoned: {e}"))?;
                if let Some(ref client) = kvm.client {
                    if client
                        .is_connected
                        .load(std::sync::atomic::Ordering::Relaxed)
                    {
                        let _ = client.send_packet_tx.try_send(pkt);
                        return Ok(());
                    }
                }
                if let Some(ref server) = kvm.server {
                    if server.is_running.load(std::sync::atomic::Ordering::Relaxed) {
                        let _ = server.outgoing_tx.send(Outbound::Packet(pkt));
                        return Ok(());
                    }
                }
                return Ok(());
            }
            Err(e) => {
                emit_continuity_notice(&app_handle, &format!("File clipboard offer rejected: {e}"));
                emit_transfer_progress(
                    &app_handle,
                    &pending.offer_id,
                    "clip",
                    &pending.summary,
                    0,
                    "cancelled",
                    pending.total_bytes,
                );
                send_pkt_from_app(&app_handle, KvmPacket::TransferCancel { id: offer_id });
                return Err(e);
            }
        }
    }

    if let Some(pending) = super::staging::take_pending_outbound_clip(&offer_id) {
        spawn_outbound_pending_clip(&app_handle, pending);
        return Ok(());
    }

    Err(format!("Clip offer {offer_id} is no longer pending"))
}
// --- end SPEC-25c ---

// --- SPEC-25b ---
#[tauri::command]
pub async fn kvm_set_audio_settings(
    audio_bridge: bool,
    audio_volume: u8,
    media_keys_follow_audio: bool,
    mute_linked_while_bridging: Option<bool>,
    app_handle: AppHandle,
) -> Result<(), String> {
    super::audio::set_audio_settings(
        audio_bridge,
        audio_volume,
        media_keys_follow_audio,
        mute_linked_while_bridging.unwrap_or(true),
        &app_handle,
    );
    Ok(())
}

#[tauri::command]
pub async fn kvm_get_audio_state() -> Result<super::audio::AudioStatePayload, String> {
    Ok(super::audio::get_audio_state())
}
// --- /SPEC-25b ---

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvm::input_logic::Edge;

    fn test_mons() -> Vec<MonitorDisplay> {
        vec![MonitorDisplay {
            id: "mon-1".into(),
            device_name: "mon-1".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
            is_primary: true,
        }]
    }

    #[tokio::test]
    async fn allow_control_off_keeps_controller_hooks() {
        let dir = std::env::temp_dir().join(format!("anyleap-cmd-allow-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let local_id = Identity::load_or_create(&dir.join("local")).unwrap();
        let remote_id = Identity::load_or_create(&dir.join("remote")).unwrap();

        let local_srv = start_kvm_server(
            0,
            &local_id,
            Arc::new(TrustStore::load(None)),
            "local".into(),
            "Local".into(),
            Arc::new(test_mons),
            dir.join("dl-local"),
        )
        .await
        .unwrap();

        let remote_srv = start_kvm_server(
            0,
            &remote_id,
            Arc::new(TrustStore::load(None)),
            "remote".into(),
            "Remote".into(),
            Arc::new(test_mons),
            dir.join("dl-remote"),
        )
        .await
        .unwrap();

        let cli_trust = TrustStore::load(None);
        let client = KvmClientHandle::connect(
            "127.0.0.1",
            remote_srv.port,
            "local".into(),
            "Local".into(),
            test_mons(),
            Some(remote_srv.pin()),
            &local_id,
            &cli_trust,
            dir.join("dl-cli"),
        )
        .await
        .unwrap();

        let mut kvm = KvmState {
            server: Some(local_srv),
            ..Default::default()
        };
        kvm.topology.computers.push(ComputerNode {
            id: "remote".into(),
            name: "Remote".into(),
            is_local: false,
            address: Some("127.0.0.1".into()),
            bounds: union_rect(&test_mons()),
            monitors: test_mons(),
        });
        kvm.client = Some(client);
        kvm.hooks.set_active_for_test(true, Edge::Right);

        // Turning allow-control OFF stops receiver server, but MUST keep client link & controller hooks.
        stop_server_in_state(&mut kvm);
        assert!(kvm.server.is_none());
        assert!(kvm.client.is_some());
        assert!(kvm.hooks.is_active());
        assert_eq!(kvm.hooks.active_link_edge(), Some("right"));

        let st = build_status(
            "local".into(),
            "Local".into(),
            1920,
            1080,
            test_mons(),
            &kvm,
        );
        assert!(!st.allow_control);
        assert!(!st.is_host_running);
        assert_eq!(st.controller_edge.as_deref(), Some("right"));
        assert_eq!(
            st.linked_peer,
            Some(LinkedPeerInfo {
                computer_id: "remote".into(),
                name: "Remote".into(),
                role: "main".into(),
            })
        );

        // Role-aware unlink on Main PC closes client link and stops hooks.
        disconnect_peer_in_state(&mut kvm);
        assert!(kvm.client.is_none());
        assert!(!kvm.hooks.is_active());
        let st_after = build_status(
            "local".into(),
            "Local".into(),
            1920,
            1080,
            test_mons(),
            &kvm,
        );
        assert!(st_after.linked_peer.is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn forget_peer_and_linked_unlink_close_active_sessions() {
        let dir = std::env::temp_dir().join(format!("anyleap-cmd-forget-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let srv_id = Identity::load_or_create(&dir.join("srv")).unwrap();
        let cli_id = Identity::load_or_create(&dir.join("cli")).unwrap();
        let srv_trust = Arc::new(TrustStore::load(None));
        let cli_trust = TrustStore::load(None);

        let srv = start_kvm_server(
            0,
            &srv_id,
            srv_trust.clone(),
            "srv".into(),
            "Server".into(),
            Arc::new(test_mons),
            dir.join("dl-srv"),
        )
        .await
        .unwrap();

        // Part A: Linked PC (server role) forgets an active controller -> session closes & trust removed.
        let c1 = KvmClientHandle::connect(
            "127.0.0.1",
            srv.port,
            "cli".into(),
            "Client".into(),
            test_mons(),
            Some(srv.pin()),
            &cli_id,
            &cli_trust,
            dir.join("dl-c1"),
        )
        .await
        .unwrap();

        let mut kvm = KvmState {
            server: Some(srv),
            ..Default::default()
        };
        kvm.connected_peers.push("Client".into());
        kvm.topology.computers.push(ComputerNode {
            id: "cli".into(),
            name: "Client".into(),
            is_local: false,
            address: None,
            bounds: union_rect(&test_mons()),
            monitors: test_mons(),
        });

        let st_linked = build_status("srv".into(), "Server".into(), 1920, 1080, test_mons(), &kvm);
        assert_eq!(
            st_linked.linked_peer,
            Some(LinkedPeerInfo {
                computer_id: "cli".into(),
                name: "Client".into(),
                role: "linked".into(),
            })
        );

        forget_peer_in_state(&mut kvm, &srv_trust, "cli");
        assert!(srv_trust.list().is_empty());
        assert!(kvm.connected_peers.is_empty());
        assert!(kvm.topology.computers.iter().all(|c| c.is_local));
        assert!(
            tokio::time::timeout(Duration::from_secs(3), c1.conn.closed())
                .await
                .is_ok(),
            "forgotten peer session must close on server"
        );

        // Part B: Linked PC (server role) role-aware Unlink closes incoming session while keeping allow_control ON.
        let srv_port = kvm.server.as_ref().unwrap().port;
        let srv_pin = kvm.server.as_ref().unwrap().pin();
        let c2 = KvmClientHandle::connect(
            "127.0.0.1",
            srv_port,
            "cli".into(),
            "Client".into(),
            test_mons(),
            Some(srv_pin),
            &cli_id,
            &cli_trust,
            dir.join("dl-c2"),
        )
        .await
        .unwrap();
        kvm.connected_peers.push("Client".into());
        kvm.topology.computers.push(ComputerNode {
            id: "cli".into(),
            name: "Client".into(),
            is_local: false,
            address: None,
            bounds: union_rect(&test_mons()),
            monitors: test_mons(),
        });

        disconnect_peer_in_state(&mut kvm);
        assert!(kvm.server.is_some());
        assert!(
            tokio::time::timeout(Duration::from_secs(3), c2.conn.closed())
                .await
                .is_ok(),
            "role-aware unlink on Linked PC must close incoming session"
        );
        let st_unlinked =
            build_status("srv".into(), "Server".into(), 1920, 1080, test_mons(), &kvm);
        assert!(st_unlinked.allow_control);
        assert!(st_unlinked.linked_peer.is_none());

        // Part C: Main PC (client role) forgets peer -> trust removed & client session closed.
        let mut srv_rx = kvm.server.as_ref().unwrap().incoming_tx.subscribe();
        let srv_pin2 = kvm.server.as_ref().unwrap().pin();
        let c3 = KvmClientHandle::connect(
            "127.0.0.1",
            srv_port,
            "cli".into(),
            "Client".into(),
            test_mons(),
            Some(srv_pin2),
            &cli_id,
            &cli_trust,
            dir.join("dl-c3"),
        )
        .await
        .unwrap();
        let mut main_kvm = KvmState::default();
        main_kvm.topology.computers.push(ComputerNode {
            id: "srv".into(),
            name: "Server".into(),
            is_local: false,
            address: Some("127.0.0.1".into()),
            bounds: union_rect(&test_mons()),
            monitors: test_mons(),
        });
        main_kvm.client = Some(c3);

        forget_peer_in_state(&mut main_kvm, &cli_trust, "srv");
        assert!(cli_trust.list().is_empty());
        assert!(main_kvm.client.is_none());
        let saw_disc = loop {
            match tokio::time::timeout(Duration::from_secs(3), srv_rx.recv()).await {
                Ok(Ok(KvmPacket::PeerDisconnected { computer_id })) if computer_id == "cli" => {
                    break true;
                }
                Ok(Ok(_)) => continue,
                _ => break false,
            }
        };
        assert!(
            saw_disc,
            "server must observe disconnect when Main PC forgets peer"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prepare_topology_update_rejects_overlaps() {
        let main_mons = test_mons();
        let linked_mons = vec![MonitorDisplay {
            id: "\\\\.\\DISPLAY1".into(),
            device_name: "\\\\.\\DISPLAY1".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1200,
            is_primary: true,
        }];
        let topo = TopologyConfig {
            computers: vec![
                ComputerNode {
                    id: "main".into(),
                    name: "Main".into(),
                    is_local: true,
                    address: None,
                    bounds: union_rect(&main_mons),
                    monitors: main_mons.clone(),
                },
                ComputerNode {
                    id: "linked".into(),
                    name: "Linked".into(),
                    is_local: false,
                    address: None,
                    bounds: union_rect(&linked_mons),
                    monitors: linked_mons.clone(),
                },
            ],
            links: vec![],
            remote_origin: Some(super::super::protocol::Point { x: 1000, y: 0 }),
        };
        let err = prepare_topology_update(
            &topo,
            topo.clone(),
            &main_mons,
            Some(("linked", &linked_mons)),
        )
        .unwrap_err();
        assert!(err.contains("overlapping"));
    }

    #[test]
    fn prepare_topology_update_falls_back_on_monitor_change() {
        let prev_main = test_mons();
        let next_main = vec![MonitorDisplay {
            id: "mon-1".into(),
            device_name: "mon-1".into(),
            x: 0,
            y: 0,
            width: 2560,
            height: 1440,
            is_primary: true,
        }];
        let linked_mons = vec![MonitorDisplay {
            id: "\\\\.\\DISPLAY1".into(),
            device_name: "\\\\.\\DISPLAY1".into(),
            x: 0,
            y: 0,
            width: 1920,
            height: 1200,
            is_primary: true,
        }];
        let prev_topo = TopologyConfig {
            computers: vec![
                ComputerNode {
                    id: "main".into(),
                    name: "Main".into(),
                    is_local: true,
                    address: None,
                    bounds: union_rect(&prev_main),
                    monitors: prev_main.clone(),
                },
                ComputerNode {
                    id: "linked".into(),
                    name: "Linked".into(),
                    is_local: false,
                    address: None,
                    bounds: union_rect(&linked_mons),
                    monitors: linked_mons.clone(),
                },
            ],
            links: vec![],
            remote_origin: Some(super::super::protocol::Point { x: 1920, y: 100 }),
        };
        let mut next_topo = prev_topo.clone();
        next_topo.computers[0].monitors = next_main.clone();
        next_topo.computers[0].bounds = union_rect(&next_main);

        let (prepared, notice) = prepare_topology_update(
            &prev_topo,
            next_topo,
            &next_main,
            Some(("linked", &linked_mons)),
        )
        .unwrap();
        assert_eq!(
            prepared.remote_origin,
            Some(super::super::protocol::Point { x: 2560, y: 0 })
        );
        let notice = notice.expect("should emit layout notice on monitor change");
        assert_eq!(notice["reason"], "monitors_changed");
        assert_eq!(notice["side"], "right");
    }

    #[test]
    fn drag_return_hold_ms_gives_edge_catcher_time_when_enabled() {
        assert_eq!(drag_return_hold_ms(true), 350);
        assert_eq!(drag_return_hold_ms(false), 0);

        let now = Instant::now();
        let mut hold = None;
        // Disabled drag-and-drop: never holds, leaves deadline unset.
        assert!(!should_hold_drag_return(&mut hold, 1, false, now));
        assert_eq!(hold, None);

        // Enabled drag-and-drop with button pressed: holds until deadline passes.
        assert!(should_hold_drag_return(&mut hold, 1, true, now));
        assert!(hold.is_some());
        assert!(should_hold_drag_return(
            &mut hold,
            1,
            true,
            now + Duration::from_millis(349)
        ));
        assert!(!should_hold_drag_return(
            &mut hold,
            1,
            true,
            now + Duration::from_millis(350)
        ));
    }
}
