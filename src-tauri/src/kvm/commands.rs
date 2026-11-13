use super::client::{handshake, send_delta, KvmClientHandle};
use super::clipboard::{set_clipboard_image, set_clipboard_text, start_clipboard_sync};
use super::discovery::DiscoveredNode;
use super::files::{send_file, send_image};
use super::identity::{Identity, TrustStore};
use super::injector;
use super::monitors::{get_local_monitors, MonitorDisplay};
use super::protocol::{ComputerNode, KvmPacket, KvmStatus, Outbound, ScreenRect, TopologyConfig, TransferEvent, UdpMouseDelta};
use super::server::start_kvm_server;
use crate::state::AppState;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

const KVM_PORT: u16 = 4177;

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

fn get_local_machine_info() -> (String, String, i32, i32, Vec<MonitorDisplay>) {
    let name = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "AnyLeap-PC".to_string());
    let id = format!("{}-host", name.to_lowercase());
    let monitors = get_local_monitors();
    let primary = monitors.iter().find(|m| m.is_primary).or_else(|| monitors.first());
    let sw = primary.map(|m| m.width).unwrap_or(1920);
    let sh = primary.map(|m| m.height).unwrap_or(1080);
    (id, name, sw, sh, monitors)
}

#[tauri::command]
pub async fn kvm_get_status(state: State<'_, AppState>) -> Result<KvmStatus, String> {
    let (id, name, sw, sh, monitors) = get_local_machine_info();
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
    })
}

#[tauri::command]
pub async fn kvm_start_server(
    port: Option<u16>,
    state: State<'_, AppState>,
    app_handle: AppHandle,
) -> Result<KvmStatus, String> {
    let (id, name, sw, sh, monitors) = get_local_machine_info();
    let (identity, trust) = get_security(&app_handle, &state)?;

    let server_handle = start_kvm_server(
        port.unwrap_or(KVM_PORT),
        &identity,
        trust,
        id.clone(),
        name.clone(),
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

    let app_h = app_handle.clone();
    tokio::spawn(async move {
        while let Ok(packet) = rx.recv().await {
            match packet {
                KvmPacket::Pair {
                    computer_id,
                    name: peer_name,
                    ..
                } => {
                    if let Some(app_state) = app_h.try_state::<AppState>() {
                        if let Ok(mut kvm) = app_state.kvm.lock() {
                            if !kvm.connected_peers.contains(&peer_name) {
                                kvm.connected_peers.push(peer_name.clone());
                            }
                            if !kvm.topology.computers.iter().any(|c| c.id == computer_id) {
                                kvm.topology.computers.push(ComputerNode {
                                    id: computer_id.clone(),
                                    name: peer_name.clone(),
                                    is_local: false,
                                    address: None,
                                    bounds: ScreenRect {
                                        x: 1920,
                                        y: 0,
                                        width: 1920,
                                        height: 1080,
                                    },
                                });
                            }
                        }
                        let _ = app_h.emit("kvm-peer-connected", &peer_name);
                    }
                }
                KvmPacket::CursorEnter { edge, relative_y } => {
                    #[cfg(windows)]
                    unsafe {
                        let sw = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                            windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN,
                        );
                        let sh = windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(
                            windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN,
                        );
                        let target_x = if edge == "left" { 15 } else { sw - 15 };
                        let target_y = ((relative_y * sh as f32) as i32).clamp(10, sh - 10);
                        let _ = windows::Win32::UI::WindowsAndMessaging::SetCursorPos(
                            target_x, target_y,
                        );
                        let pt = windows::Win32::Foundation::POINT { x: target_x, y: target_y };
                        let hwnd = windows::Win32::UI::WindowsAndMessaging::WindowFromPoint(pt);
                        if !hwnd.is_invalid() {
                            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(hwnd);
                        }
                    }
                    let _ = app_h.emit("kvm-cursor-enter", &edge);
                }
                KvmPacket::CursorPosition { norm_x, norm_y } => {
                    let _ = app_h.emit("kvm-cursor-pos", serde_json::json!({ "norm_x": norm_x, "norm_y": norm_y }));
                }
                KvmPacket::EmergencyRelease => {
                    super::hooks::restore_cursor_visibility();
                }
                KvmPacket::SessionDisconnect => {
                    if let Some(app_state) = app_h.try_state::<AppState>() {
                        if let Ok(mut kvm) = app_state.kvm.lock() {
                            kvm.connected_peers.clear();
                            kvm.client = None;
                        }
                    }
                    super::hooks::restore_cursor_visibility();
                    let _ = app_h.emit("kvm-peer-disconnected", ());
                }
                KvmPacket::MouseButton { button, is_down } => {
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

    // Spawn datagram delta worker
    tokio::spawn(async move {
        while let Ok(delta) = delta_rx.recv().await {
            injector::inject_mouse_delta(delta.dx, delta.dy);
        }
    });

    let mut kvm = state.kvm.lock().unwrap();
    let pin = server_handle.pin();
    let connected_peers = kvm.connected_peers.clone();
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
    let (id, name, _sw, _sh, _monitors) = get_local_machine_info();
    let (identity, trust) = get_security(&app_handle, &state)?;
    // Re-pairing replaces any stale fingerprint recorded for this address.
    if let Some(old) = trust.by_address(&ip) {
        trust.remove(&old.computer_id);
    }
    let (_ep, conn, _send, _recv) =
        handshake(&ip, port.unwrap_or(KVM_PORT), id, name, Some(pin), &identity, &trust).await?;
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
    let (id, name, _sw, _sh, _monitors) = get_local_machine_info();
    let (identity, trust) = get_security(&app_handle, &state)?;

    let client = KvmClientHandle::connect(
        &ip,
        port.unwrap_or(KVM_PORT),
        id,
        name,
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

    let mut in_rx = client.incoming_tx.subscribe();
    let app_h = app_handle.clone();
    tokio::spawn(async move {
        while let Ok(pkt) = in_rx.recv().await {
            match pkt {
                KvmPacket::ClipboardText { text } => emit_clipboard_text(&app_h, &text),
                KvmPacket::SessionDisconnect => {
                    if let Some(app_state) = app_h.try_state::<AppState>() {
                        if let Ok(mut kvm) = app_state.kvm.lock() {
                            kvm.client = None;
                            kvm.hooks.stop_hooks();
                        }
                    }
                    super::hooks::restore_cursor_visibility();
                    let _ = app_h.emit("kvm-peer-disconnected", ());
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

    let mut kvm = state.kvm.lock().unwrap();
    kvm.client = Some(client);
    Ok(())
}

#[tauri::command]
pub async fn kvm_disconnect_peer(state: State<'_, AppState>) -> Result<(), String> {
    let mut kvm = state.kvm.lock().unwrap();
    if let Some(client) = kvm.client.take() {
        let _ = client.send_packet_tx.try_send(KvmPacket::SessionDisconnect);
        client.is_connected.store(false, std::sync::atomic::Ordering::Relaxed);
        client.conn.close(0u32.into(), b"disconnect");
    }
    kvm.hooks.stop_hooks();
    kvm.connected_peers.clear();
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
) -> Result<(), String> {
    let mut kvm = state.kvm.lock().unwrap();
    kvm.topology = topology.clone();

    let (packet_tx, mut packet_rx) = tokio::sync::mpsc::channel::<KvmPacket>(256);
    let (delta_tx, mut delta_rx) = tokio::sync::mpsc::channel::<(i32, i32)>(512);

    if let Some(ref client) = kvm.client {
        let client_sender = client.send_packet_tx.clone();
        let conn = client.conn.clone();

        tokio::spawn(async move {
            while let Some(packet) = packet_rx.recv().await {
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
