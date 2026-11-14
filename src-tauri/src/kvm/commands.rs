use super::client::{handshake, send_delta, KvmClientHandle};
use super::clipboard::{set_clipboard_image, set_clipboard_text, start_clipboard_sync};
use super::debug_log;
use super::discovery::DiscoveredNode;
use super::files::{send_file, send_image};
use super::identity::{Identity, TrustStore};
use super::injector;
use super::input_logic::{
    map_cursor_enter, normalize_in_union, receiver_step_with_dwell, union_rect, Edge,
    PressedInputTracker, ReceiverStep,
};
use super::monitors::{get_local_monitors, MonitorDisplay};
use super::protocol::{
    ComputerNode, KvmPacket, KvmStatus, LinkedPeerInfo, Outbound, ScreenRect, TopologyConfig,
    TransferEvent, TrustedPeerInfo, UdpMouseDelta,
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
    session_epoch: Instant,
    monitors: Vec<MonitorDisplay>,
    union: ScreenRect,
    last_pos_emit: Option<Instant>,
    tracker: PressedInputTracker,
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
            session_epoch: Instant::now(),
            monitors,
            union,
            last_pos_emit: None,
            tracker: PressedInputTracker::new(),
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
        .map(|p| std::path::PathBuf::from(p).join("Downloads").join("AnyLeap_Transfers"))
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
}

/// Identity + trust store, created on first use under the app data dir.
fn get_security(app: &AppHandle, state: &AppState) -> Result<(Arc<Identity>, Arc<TrustStore>), String> {
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
    };
    let _ = app.emit("kvm-activity", item);
}

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
    let primary = monitors.iter().find(|m| m.is_primary).or_else(|| monitors.first());
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
    if let Some(id) = removed_id {
        kvm.topology.computers.retain(|c| c.is_local || c.id != id);
        kvm.topology
            .links
            .retain(|l| l.from_id != id && l.to_id != id);
    }
    super::hooks::restore_cursor_visibility();
}

/// Role-aware unlink: if this PC has an outgoing client link, close it and stop
/// hooks; otherwise close every incoming controller session on the server.
fn disconnect_peer_in_state(kvm: &mut KvmState) {
    if kvm.client.is_some() {
        disconnect_client_in_state(kvm);
    } else if let Some(ref srv) = kvm.server {
        srv.disconnect_sessions(None);
        kvm.connected_peers.clear();
        kvm.topology.computers.retain(|c| c.is_local);
        kvm.topology.links.clear();
        RECEIVER_ENTERED.store(false, std::sync::atomic::Ordering::Relaxed);
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
    let kvm = state.kvm.lock().unwrap();
    let port = kvm.server.as_ref().map(|s| s.port).unwrap_or(KVM_PORT);

    // Auto-start discovery listener if not already running
    if !kvm.discovery.is_running() {
        kvm.discovery.start(id.clone(), name.clone(), port, sw, sh);
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

    let server_handle = start_kvm_server(
        port.unwrap_or(KVM_PORT),
        &identity,
        trust,
        id.clone(),
        name.clone(),
        monitors.clone(),
        get_download_dir(),
    )
    .await?;
    let port = server_handle.port;
    let mut rx = server_handle.incoming_tx.subscribe();
    let mut transfer_rx = server_handle.transfer_tx.subscribe();
    let mut delta_rx = server_handle.mouse_delta_tx.subscribe();
    let outgoing_tx = server_handle.outgoing_tx.clone();

    // Start server-side clipboard monitor
    let (clip_tx, mut clip_rx) = tokio::sync::mpsc::channel::<Outbound>(32);
    start_clipboard_sync(clip_tx, Arc::clone(&server_handle.is_running));
    let out_tx = outgoing_tx.clone();
    tokio::spawn(async move {
        while let Some(pkt) = clip_rx.recv().await {
            let _ = out_tx.send(pkt);
        }
    });

    let rx_state = Arc::new(Mutex::new(ReceiverSessionState::new(monitors.clone())));
    let rx_state_pkt = Arc::clone(&rx_state);
    let pkt_out_tx = outgoing_tx.clone();

    let app_h = app_handle.clone();
    tokio::spawn(async move {
        while let Ok(packet) = rx.recv().await {
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
                }
                KvmPacket::CursorEnter {
                    edge,
                    relative_y,
                    dwell_ms,
                    deadzone_px,
                } => {
                    let fresh_monitors = get_local_monitors();
                    let entry_edge = Edge::from_str(&edge).unwrap_or(Edge::Left);
                    let pos = map_cursor_enter(&fresh_monitors, entry_edge, relative_y);
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
                        st.monitors = fresh_monitors;
                        st.union = union;
                        st.last_pos_emit = None;
                    }

                    // Keep the receiver's current keyboard focus: the entry point is the
                    // screen edge (desktop/taskbar), so focusing the window under it would
                    // send typed keys to the desktop until the user clicks something.
                    injector::set_cursor_position(pos.0, pos.1);

                    let _ = pkt_out_tx.send(Outbound::Packet(KvmPacket::CursorPosition {
                        norm_x,
                        norm_y,
                    }));
                    let _ = app_h.emit("kvm-cursor-enter", &edge);
                }
                KvmPacket::EmergencyRelease => {
                    release_receiver_inputs(&rx_state_pkt, "emergency_release");
                    super::hooks::restore_cursor_visibility();
                }
                KvmPacket::ReleaseAll => {
                    release_receiver_inputs(&rx_state_pkt, "release_all");
                    super::hooks::restore_cursor_visibility();
                }
                KvmPacket::PeerDisconnected { computer_id } => {
                    release_receiver_inputs(&rx_state_pkt, "peer_disconnected");
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
                        }
                    }
                    super::hooks::restore_cursor_visibility();
                    let _ = app_h.emit("kvm-peer-disconnected", &computer_id);
                }
                KvmPacket::MouseButton { button, is_down } => {
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
                _ => {}
            }
        }
    });

    let app_t = app_handle.clone();
    tokio::spawn(async move {
        while let Ok(ev) = transfer_rx.recv().await {
            emit_transfer(&app_t, ev);
        }
    });

    // Spawn datagram delta worker (authoritative receiver cursor)
    let delta_out_tx = outgoing_tx.clone();
    let app_d = app_handle.clone();
    tokio::spawn(async move {
        while let Ok(delta) = delta_rx.recv().await {
            enum DeltaAction {
                Ignore,
                Move {
                    pos: (i32, i32),
                    emit_pos: Option<(f32, f32)>,
                },
                Return {
                    edge: Edge,
                    relative_y: f32,
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
                        ref monitors,
                        ..
                    } = *st;
                    match receiver_step_with_dwell(
                        cur,
                        (delta.dx, delta.dy),
                        monitors,
                        entry_edge,
                        dwell_ms,
                        deadzone_px,
                        dwell_start_ms,
                        now_ms,
                    ) {
                        ReceiverStep::Move(pos) => {
                            let emit_pos = if st
                                .last_pos_emit
                                .is_none_or(|t| t.elapsed() >= Duration::from_millis(50))
                            {
                                st.last_pos_emit = Some(Instant::now());
                                Some(normalize_in_union(pos, &st.union))
                            } else {
                                None
                            };
                            DeltaAction::Move { pos, emit_pos }
                        }
                        ReceiverStep::Return { edge, relative_y } => {
                            st.is_entered = false;
                            RECEIVER_ENTERED.store(false, std::sync::atomic::Ordering::Relaxed);
                            st.dwell_start_ms = None;
                            st.last_pos_emit = None;
                            let (keys, buttons) = st.tracker.release_all();
                            DeltaAction::Return {
                                edge,
                                relative_y,
                                keys,
                                buttons,
                            }
                        }
                    }
                } else {
                    DeltaAction::Ignore
                }
            };

            match action {
                DeltaAction::Ignore => {}
                DeltaAction::Move { pos, emit_pos } => {
                    injector::set_cursor_position(pos.0, pos.1);
                    if let Some((norm_x, norm_y)) = emit_pos {
                        let _ = delta_out_tx.send(Outbound::Packet(KvmPacket::CursorPosition {
                            norm_x,
                            norm_y,
                        }));
                    }
                }
                DeltaAction::Return {
                    edge,
                    relative_y,
                    keys,
                    buttons,
                } => {
                    let released_keys = keys.len();
                    let released_buttons = buttons.len();
                    release_tracked_inputs(keys, buttons);
                    let edge_str = edge.as_str().to_string();
                    let _ = delta_out_tx.send(Outbound::Packet(KvmPacket::CursorReturn {
                        edge: edge_str.clone(),
                        relative_y,
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
    kvm.discovery
        .start(id.clone(), name.clone(), port, sw, sh);

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
    let (_ep, conn, _send, _recv, _peer_info) =
        handshake(&ip, port.unwrap_or(KVM_PORT), id, name, monitors, Some(pin), &identity, &trust).await?;
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
    tokio::spawn(async move {
        while let Some(item) = clip_rx.recv().await {
            match item {
                Outbound::Packet(pkt) => {
                    let _ = client_send.send(pkt).await;
                }
                Outbound::Image { format, data } => {
                    let _ = send_image(&clip_conn, format, data).await;
                }
            }
        }
    });

    let peer_id_for_rx = client.peer_id.clone();
    let client_connected_flag = Arc::clone(&client.is_connected);
    let mut in_rx = client.incoming_tx.subscribe();
    let app_h = app_handle.clone();
    tokio::spawn(async move {
        while let Ok(pkt) = in_rx.recv().await {
            match pkt {
                KvmPacket::ClipboardText { text } => emit_clipboard_text(&app_h, &text),
                KvmPacket::CursorReturn { relative_y, .. } => {
                    if let Some(app_state) = app_h.try_state::<AppState>() {
                        if let Ok(kvm) = app_state.kvm.lock() {
                            kvm.hooks.handle_cursor_return(relative_y);
                        }
                    }
                    let _ = app_h.emit("kvm-cursor-return", "");
                }
                KvmPacket::CursorPosition { norm_x, norm_y } => {
                    let _ = app_h.emit(
                        "kvm-cursor-pos",
                        serde_json::json!({ "norm_x": norm_x, "norm_y": norm_y }),
                    );
                }
                KvmPacket::SessionDisconnect => {
                    if let Some(app_state) = app_h.try_state::<AppState>() {
                        if let Ok(mut kvm) = app_state.kvm.lock() {
                            if kvm
                                .client
                                .as_ref()
                                .is_some_and(|c| Arc::ptr_eq(&c.is_connected, &client_connected_flag))
                            {
                                kvm.client = None;
                                kvm.hooks.stop_hooks();
                                kvm.topology
                                    .computers
                                    .retain(|c| c.is_local || c.id != peer_id_for_rx);
                                kvm.topology
                                    .links
                                    .retain(|l| l.from_id != peer_id_for_rx && l.to_id != peer_id_for_rx);
                                super::hooks::restore_cursor_visibility();
                            }
                        }
                    }
                    let _ = app_h.emit("kvm-peer-disconnected", &peer_id_for_rx);
                }
                _ => {}
            }
        }
    });

    let mut transfer_rx = client.transfer_tx.subscribe();
    let app_t = app_handle.clone();
    tokio::spawn(async move {
        while let Ok(ev) = transfer_rx.recv().await {
            emit_transfer(&app_t, ev);
        }
    });

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
    Ok(())
}

#[tauri::command]
pub async fn kvm_disconnect_peer(state: State<'_, AppState>) -> Result<(), String> {
    let mut kvm = state.kvm.lock().unwrap();
    disconnect_peer_in_state(&mut kvm);
    Ok(())
}

#[tauri::command]
pub async fn kvm_send_file(
    file_path: String,
    state: State<'_, AppState>,
) -> Result<(), String> {
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

#[tauri::command]
pub async fn kvm_update_topology(
    mut topology: TopologyConfig,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let mut kvm = state.kvm.lock().unwrap();
    if let Some(ref client) = kvm.client {
        let peer_bounds = union_rect(&client.peer_monitors);
        if let Some(node) = topology
            .computers
            .iter_mut()
            .find(|c| !c.is_local && c.id == client.peer_id)
        {
            if node.monitors.is_empty() && !client.peer_monitors.is_empty() {
                node.monitors = client.peer_monitors.clone();
                node.bounds = peer_bounds;
            }
        }
    }
    kvm.topology = topology.clone();

    let (packet_tx, mut packet_rx) = tokio::sync::mpsc::channel::<KvmPacket>(256);
    let (delta_tx, mut delta_rx) = tokio::sync::mpsc::channel::<(i32, i32)>(512);

    if let Some(ref client) = kvm.client {
        let client_sender = client.send_packet_tx.clone();
        let conn = client.conn.clone();
        let app_h = app_handle.clone();

        tokio::spawn(async move {
            while let Some(packet) = packet_rx.recv().await {
                if let KvmPacket::CursorEnter { ref edge, .. } = packet {
                    let _ = app_h.emit("kvm-cursor-enter", edge);
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

        kvm.hooks.start_hooks(topology, packet_tx, delta_tx);
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvm::input_logic::Edge;

    fn test_mons() -> Vec<MonitorDisplay> {
        vec![MonitorDisplay {
            id: "mon-1".into(),
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
            test_mons(),
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
            test_mons(),
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

        let mut kvm = KvmState::default();
        kvm.server = Some(local_srv);
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

        let st = build_status("local".into(), "Local".into(), 1920, 1080, test_mons(), &kvm);
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
        let st_after = build_status("local".into(), "Local".into(), 1920, 1080, test_mons(), &kvm);
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
            test_mons(),
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

        let mut kvm = KvmState::default();
        kvm.server = Some(srv);
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
        let st_unlinked = build_status("srv".into(), "Server".into(), 1920, 1080, test_mons(), &kvm);
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
        assert!(saw_disc, "server must observe disconnect when Main PC forgets peer");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
