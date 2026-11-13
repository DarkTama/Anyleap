use super::client::{handshake, send_delta, KvmClientHandle};
use super::clipboard::{set_clipboard_image, set_clipboard_text, start_clipboard_sync};
use super::discovery::DiscoveredNode;
use super::files::{send_file, send_image};
use super::identity::{Identity, TrustStore};
use super::injector;
use super::input_logic::{
    map_cursor_enter, normalize_in_union, receiver_step, union_rect, Edge, PressedInputTracker,
    ReceiverStep,
};
use super::monitors::{get_local_monitors, MonitorDisplay};
use super::protocol::{
    ComputerNode, KvmPacket, KvmStatus, Outbound, ScreenRect, TopologyConfig, TransferEvent,
    UdpMouseDelta,
};
use super::server::start_kvm_server;
use crate::state::AppState;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

const KVM_PORT: u16 = 4177;

struct ReceiverSessionState {
    is_entered: bool,
    entry_edge: Edge,
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
            monitors,
            union,
            last_pos_emit: None,
            tracker: PressedInputTracker::new(),
        }
    }
}

fn release_tracked_inputs(keys: Vec<super::input_logic::TrackedKey>, buttons: Vec<u8>) {
    for b in buttons {
        injector::inject_mouse_button(b, false);
    }
    for k in keys {
        injector::inject_key(k.key_code, k.scan_code, false, k.extended);
    }
}

fn release_receiver_inputs(rx_state: &Arc<Mutex<ReceiverSessionState>>) {
    let released = if let Ok(mut st) = rx_state.lock() {
        st.is_entered = false;
        st.last_pos_emit = None;
        Some(st.tracker.release_all())
    } else {
        None
    };
    if let Some((keys, buttons)) = released {
        release_tracked_inputs(keys, buttons);
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

#[tauri::command]
pub async fn kvm_get_status(
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<KvmStatus, String> {
    let (id, name, sw, sh, monitors) = get_local_machine_info(&app_handle, &state)?;
    let kvm = state.kvm.lock().unwrap();
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

    // Auto-start discovery listener if not already running
    if !kvm.discovery.is_running() {
        kvm.discovery.start(id.clone(), name.clone(), port, sw, sh);
    }

    Ok(KvmStatus {
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
    })
}

#[tauri::command]
pub async fn kvm_start_server(
    port: Option<u16>,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<KvmStatus, String> {
    let (id, name, sw, sh, monitors) = get_local_machine_info(&app_handle, &state)?;
    let (identity, trust) = get_security(&app_handle, &state)?;

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
                KvmPacket::CursorEnter { edge, relative_y } => {
                    let fresh_monitors = get_local_monitors();
                    let entry_edge = Edge::from_str(&edge).unwrap_or(Edge::Left);
                    let pos = map_cursor_enter(&fresh_monitors, entry_edge, relative_y);
                    let union = union_rect(&fresh_monitors);
                    let (norm_x, norm_y) = normalize_in_union(pos, &union);

                    if let Ok(mut st) = rx_state_pkt.lock() {
                        st.is_entered = true;
                        st.entry_edge = entry_edge;
                        st.monitors = fresh_monitors;
                        st.union = union;
                        st.last_pos_emit = None;
                    }

                    injector::set_cursor_position(pos.0, pos.1);
                    #[cfg(windows)]
                    unsafe {
                        let pt = windows::Win32::Foundation::POINT { x: pos.0, y: pos.1 };
                        let hwnd = windows::Win32::UI::WindowsAndMessaging::WindowFromPoint(pt);
                        if !hwnd.is_invalid() {
                            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
                        }
                    }

                    let _ = pkt_out_tx.send(Outbound::Packet(KvmPacket::CursorPosition {
                        norm_x,
                        norm_y,
                    }));
                    let _ = app_h.emit("kvm-cursor-enter", &edge);
                }
                KvmPacket::EmergencyRelease | KvmPacket::ReleaseAll => {
                    release_receiver_inputs(&rx_state_pkt);
                    super::hooks::restore_cursor_visibility();
                }
                KvmPacket::PeerDisconnected { computer_id } => {
                    release_receiver_inputs(&rx_state_pkt);
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
                    injector::inject_mouse_button(button, is_down);
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
                    injector::inject_key(key_code, scan_code, is_down, extended);
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
                    match receiver_step(cur, (delta.dx, delta.dy), &st.monitors, st.entry_edge) {
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
                    release_tracked_inputs(keys, buttons);
                    let edge_str = edge.as_str().to_string();
                    let _ = delta_out_tx.send(Outbound::Packet(KvmPacket::CursorReturn {
                        edge: edge_str.clone(),
                        relative_y,
                    }));
                    let _ = app_d.emit("kvm-cursor-return", &edge_str);
                }
            }
        }
    });

    let mut kvm = state.kvm.lock().unwrap();
    let pin = server_handle.pin();
    let connected_peers = kvm.connected_peers.clone();
    let peers: Vec<ComputerNode> = kvm
        .topology
        .computers
        .iter()
        .filter(|c| !c.is_local)
        .cloned()
        .collect();
    kvm.server = Some(server_handle);

    // Ensure discovery is broadcasting
    kvm.discovery
        .start(id.clone(), name.clone(), port, sw, sh);

    Ok(KvmStatus {
        is_host_running: true,
        port,
        security_pin: pin,
        connected_peers,
        is_input_trapped: false,
        active_target_id: None,
        local_name: name,
        local_id: id,
        screen_width: sw,
        screen_height: sh,
        monitors,
        peers,
    })
}

#[tauri::command]
pub async fn kvm_stop_server(state: State<'_, AppState>) -> Result<(), String> {
    let mut kvm = state.kvm.lock().unwrap();
    if let Some(srv) = kvm.server.take() {
        srv.is_running
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = srv.shutdown_tx.send(true);
    }
    kvm.hooks.stop_hooks();
    kvm.connected_peers.clear();
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
    // Re-pairing replaces any stale fingerprint recorded for this address.
    if let Some(old) = trust.by_address(&ip) {
        trust.remove(&old.computer_id);
    }
    let (_ep, conn, _send, _recv, _peer_info) =
        handshake(&ip, port.unwrap_or(KVM_PORT), id, name, monitors, Some(pin), &identity, &trust).await?;
    conn.close(0u32.into(), b"paired");
    Ok(())
}

#[tauri::command]
pub async fn kvm_forget_peer(
    computer_id: String,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let (_, trust) = get_security(&app_handle, &state)?;
    trust.remove(&computer_id);
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
                            kvm.client = None;
                            kvm.hooks.stop_hooks();
                            kvm.topology
                                .computers
                                .retain(|c| c.is_local || c.id != peer_id_for_rx);
                            kvm.topology
                                .links
                                .retain(|l| l.from_id != peer_id_for_rx && l.to_id != peer_id_for_rx);
                        }
                    }
                    super::hooks::restore_cursor_visibility();
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
    let mut removed_id = None;
    if let Some(client) = kvm.client.take() {
        removed_id = Some(client.peer_id.clone());
        let _ = client.send_packet_tx.try_send(KvmPacket::SessionDisconnect);
        client.is_connected.store(false, std::sync::atomic::Ordering::Relaxed);
        client.conn.close(0u32.into(), b"disconnect");
    }
    kvm.hooks.stop_hooks();
    if let Some(id) = removed_id {
        kvm.topology.computers.retain(|c| c.is_local || c.id != id);
        kvm.topology.links.retain(|l| l.from_id != id && l.to_id != id);
    }
    super::hooks::restore_cursor_visibility();
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
    topology: TopologyConfig,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<(), String> {
    let mut kvm = state.kvm.lock().unwrap();
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
