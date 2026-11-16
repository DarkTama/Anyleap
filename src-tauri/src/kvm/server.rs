use super::files::send_image;
use super::identity::{ct_eq, peer_fingerprint, server_config, Identity, TrustStore, TrustedPeer};
use super::monitors::MonitorDisplay;
use super::protocol::{
    decode_delta, read_frame, read_frame_capped, write_frame, KvmPacket, Outbound, TransferEvent,
    UdpMouseDelta,
};
use rand::{rngs::OsRng, Rng};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU32, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};
use tokio::sync::{broadcast, watch};
use tokio::time::timeout;

const MAX_PIN_FAILURES: u32 = 5;
#[cfg(not(test))]
const PAIR_LOCKOUT: Duration = Duration::from_secs(60);
#[cfg(test)]
const PAIR_LOCKOUT: Duration = Duration::from_millis(500);
/// Cap for the first (pre-auth) frame of a connection.
const MAX_HELLO: usize = 4096;

/// Returns the current local monitor layout; queried on every pairing.
pub type MonitorProvider = Arc<dyn Fn() -> Vec<MonitorDisplay> + Send + Sync>;

#[derive(Clone)]
struct ServerContext {
    security_pin: Arc<Mutex<String>>,
    /// Failure count + lockout end; only touched while holding the `security_pin` lock.
    failures: Arc<AtomicU32>,
    locked_until: Arc<Mutex<Option<Instant>>>,
    trust: Arc<TrustStore>,
    local_id: String,
    local_name: String,
    monitor_provider: MonitorProvider,
    download_dir: PathBuf,
    incoming_tx: broadcast::Sender<KvmPacket>,
    outgoing_tx: broadcast::Sender<Outbound>,
    mouse_delta_tx: broadcast::Sender<UdpMouseDelta>,
    transfer_tx: broadcast::Sender<TransferEvent>,
    disconnect_tx: broadcast::Sender<Option<String>>,
    // --- SPEC-25b ---
    audio_out_tx: broadcast::Sender<bytes::Bytes>,
    // --- /SPEC-25b ---
}

impl ServerContext {
    fn check_pin(&self, attempt: &str) -> bool {
        // Whole check runs under the PIN lock so parallel attempts cannot exceed the limit.
        let mut pin = self.security_pin.lock().unwrap();
        let mut locked = self.locked_until.lock().unwrap();
        if locked.is_some_and(|t| Instant::now() < t) {
            return false;
        }
        *locked = None;
        if ct_eq(attempt.as_bytes(), pin.as_bytes()) {
            return true;
        }
        if self.failures.fetch_add(1, Ordering::Relaxed) + 1 >= MAX_PIN_FAILURES {
            self.failures.store(0, Ordering::Relaxed);
            *pin = generate_pin();
            *locked = Some(Instant::now() + PAIR_LOCKOUT);
        }
        false
    }
}

pub fn generate_pin() -> String {
    OsRng.gen_range(100_000..=999_999).to_string()
}

pub struct KvmServerHandle {
    pub port: u16,
    pub security_pin: Arc<Mutex<String>>,
    pub is_running: Arc<AtomicBool>,
    pub incoming_tx: broadcast::Sender<KvmPacket>,
    pub outgoing_tx: broadcast::Sender<Outbound>,
    pub mouse_delta_tx: broadcast::Sender<UdpMouseDelta>,
    pub transfer_tx: broadcast::Sender<TransferEvent>,
    pub disconnect_tx: broadcast::Sender<Option<String>>,
    pub shutdown_tx: watch::Sender<bool>,
    // --- SPEC-25b ---
    pub audio_out_tx: broadcast::Sender<bytes::Bytes>,
    // --- /SPEC-25b ---
}

impl KvmServerHandle {
    pub fn pin(&self) -> String {
        self.security_pin.lock().unwrap().clone()
    }

    /// Close every active incoming session (`None`) or a specific peer (`Some(id)`).
    pub fn disconnect_sessions(&self, computer_id: Option<&str>) {
        let _ = self.disconnect_tx.send(computer_id.map(str::to_string));
    }
}

pub async fn start_kvm_server(
    port: u16,
    identity: &Identity,
    trust: Arc<TrustStore>,
    local_id: String,
    local_name: String,
    monitor_provider: MonitorProvider,
    download_dir: PathBuf,
) -> Result<KvmServerHandle, String> {
    let (incoming_tx, _) = broadcast::channel(512);
    let (outgoing_tx, _) = broadcast::channel(512);
    let (mouse_delta_tx, _) = broadcast::channel(1024);
    let (transfer_tx, _) = broadcast::channel(256);
    let (disconnect_tx, _) = broadcast::channel(32);
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    // --- SPEC-25b ---
    let (audio_out_tx, _) = broadcast::channel(256);
    // --- /SPEC-25b ---
    let security_pin = Arc::new(Mutex::new(generate_pin()));
    let is_running = Arc::new(AtomicBool::new(true));

    let addr: SocketAddr = ([0, 0, 0, 0], port).into();
    let endpoint = quinn::Endpoint::server(server_config(identity)?, addr)
        .map_err(|e| format!("Failed to bind UDP port {port}: {e}"))?;
    let port = endpoint.local_addr().map_err(|e| e.to_string())?.port();

    let ctx = ServerContext {
        security_pin: Arc::clone(&security_pin),
        failures: Arc::new(AtomicU32::new(0)),
        locked_until: Arc::new(Mutex::new(None)),
        trust,
        local_id,
        local_name,
        monitor_provider,
        download_dir,
        incoming_tx: incoming_tx.clone(),
        outgoing_tx: outgoing_tx.clone(),
        mouse_delta_tx: mouse_delta_tx.clone(),
        transfer_tx: transfer_tx.clone(),
        disconnect_tx: disconnect_tx.clone(),
        // --- SPEC-25b ---
        audio_out_tx: audio_out_tx.clone(),
        // --- /SPEC-25b ---
    };

    tokio::spawn(async move {
        loop {
            tokio::select! {
                inc = endpoint.accept() => match inc {
                    Some(inc) => { tokio::spawn(handle_connection(inc, ctx.clone())); }
                    None => break,
                },
                _ = shutdown_rx.changed() => {
                    endpoint.close(0u32.into(), b"shutdown");
                    break;
                }
            }
        }
    });

    Ok(KvmServerHandle {
        port,
        security_pin,
        is_running,
        incoming_tx,
        outgoing_tx,
        mouse_delta_tx,
        transfer_tx,
        disconnect_tx,
        shutdown_tx,
        // --- SPEC-25b ---
        audio_out_tx,
        // --- /SPEC-25b ---
    })
}

async fn handle_connection(incoming: quinn::Incoming, ctx: ServerContext) {
    let Ok(conn) = incoming.await else { return };
    let Some(fp) = peer_fingerprint(&conn) else {
        conn.close(1u32.into(), b"no certificate");
        return;
    };
    let remote_ip = conn.remote_address().ip().to_string();
    let trusted_entry = ctx.trust.by_fingerprint(&fp);
    let trusted = trusted_entry.is_some();

    let Ok(Ok((mut send, mut recv))) = timeout(Duration::from_secs(10), conn.accept_bi()).await
    else {
        conn.close(1u32.into(), b"no control stream");
        return;
    };

    // Authorisation: unknown peers may only send `Pair`; anything else closes the connection.
    let first = match timeout(
        Duration::from_secs(10),
        read_frame_capped::<_, KvmPacket>(&mut recv, MAX_HELLO),
    )
    .await
    {
        Ok(Ok(Some(p))) => p,
        _ => {
            conn.close(1u32.into(), b"no hello");
            return;
        }
    };

    let peer_computer_id: Option<String>;
    let mut disconnected_early = false;

    match first {
        KvmPacket::Pair {
            computer_id,
            name,
            pin,
            monitors,
        } => {
            let accepted = trusted || ctx.check_pin(&pin);
            let ack = KvmPacket::PairAck {
                accepted,
                computer_id: ctx.local_id.clone(),
                name: ctx.local_name.clone(),
                monitors: (ctx.monitor_provider)(),
            };
            let _ = write_frame(&mut send, &ack).await;
            if !accepted {
                let _ = send.finish();
                let _ = timeout(Duration::from_secs(2), send.stopped()).await;
                conn.close(2u32.into(), b"pairing rejected");
                return;
            }
            peer_computer_id = Some(computer_id.clone());
            if let Some(mut entry) = trusted_entry {
                if entry.address != remote_ip || entry.name != name {
                    entry.address = remote_ip;
                    entry.name = name.clone();
                    ctx.trust.add(entry);
                }
            } else {
                ctx.trust.add(TrustedPeer {
                    computer_id: computer_id.clone(),
                    name: name.clone(),
                    fingerprint: fp,
                    address: remote_ip,
                });
            }
            let _ = ctx.incoming_tx.send(KvmPacket::Pair {
                computer_id,
                name,
                pin: String::new(),
                monitors,
            });
        }
        other if trusted => {
            peer_computer_id = trusted_entry.as_ref().map(|p| p.computer_id.clone());
            if let Some(mut entry) = trusted_entry {
                if entry.address != remote_ip {
                    entry.address = remote_ip;
                    ctx.trust.add(entry);
                }
            }
            if matches!(other, KvmPacket::SessionDisconnect) {
                disconnected_early = true;
            } else if !matches!(
                other,
                KvmPacket::PairAck { .. } | KvmPacket::PeerDisconnected { .. }
            ) {
                let _ = ctx.incoming_tx.send(other);
            }
        }
        _ => {
            conn.close(3u32.into(), b"not authorised");
            return;
        }
    }

    fn spawn_server_bulk_transfer(
        conn: &quinn::Connection,
        xfer_tx: &broadcast::Sender<TransferEvent>,
        kind: super::files::BulkTransferKind,
        id: String,
        rel_path: String,
        path: std::path::PathBuf,
        size: u64,
    ) {
        let c = conn.clone();
        let tx = xfer_tx.clone();
        tokio::spawn(async move {
            let id_prog = id.clone();
            let name_prog = rel_path.clone();
            let tx_prog = tx.clone();
            let kind_str = kind.as_str();
            let res = super::files::send_tracked_bulk_file(
                &c,
                kind,
                "server",
                id.clone(),
                rel_path.clone(),
                &path,
                size,
                |pct| {
                    let (emit_pct, emit_state) = match kind {
                        super::files::BulkTransferKind::Drag => (pct.min(99), "active"),
                        super::files::BulkTransferKind::Clip => {
                            (pct, if pct >= 100 { "done" } else { "active" })
                        }
                    };
                    let _ = tx_prog.send(TransferEvent::Progress {
                        id: id_prog.clone(),
                        kind: kind_str.to_string(),
                        name: name_prog.clone(),
                        pct: emit_pct,
                        state: emit_state.to_string(),
                        size_bytes: size,
                    });
                },
            )
            .await;
            if let Err(reason) = res {
                if reason == super::files::TRANSFER_CANCELLED_ERR {
                    let meta = super::files::TransferCancelMeta {
                        id,
                        kind: kind_str.to_string(),
                        name: rel_path,
                        size_bytes: size,
                    };
                    super::files::emit_cancelled_once(c.stable_id(), &meta, Some(&tx));
                } else {
                    let _ = tx.send(TransferEvent::Failed {
                        id,
                        kind: kind_str.to_string(),
                        name: rel_path,
                        reason,
                    });
                }
            }
        });
    }

    if !disconnected_early {
        // Authorised session from here on.
        let mut tasks = Vec::new();
        let mut disc_rx = ctx.disconnect_tx.subscribe();
        let mut out_disc_rx = ctx.disconnect_tx.subscribe();
        let (disc_done_tx, mut disc_done_rx) = tokio::sync::oneshot::channel::<()>();
        let out_peer_id = peer_computer_id.clone();

        let mut out_rx = ctx.outgoing_tx.subscribe();
        let out_conn = conn.clone();
        let out_xfer_tx = ctx.transfer_tx.clone();
        let out_pkt_tx = ctx.outgoing_tx.clone();
        tasks.push(tokio::spawn(async move {
            let mut disc_done_tx = Some(disc_done_tx);
            loop {
                tokio::select! {
                    res = out_rx.recv() => match res {
                        Ok(Outbound::Packet(p)) => {
                            if write_frame(&mut send, &p).await.is_err() {
                                break;
                            }
                        }
                        Ok(Outbound::Image { format, data }) => {
                            let c = out_conn.clone();
                            tokio::spawn(async move {
                                let _ = send_image(&c, format, data).await;
                            });
                        }
                        // --- SPEC-25c ---
                        Ok(Outbound::OpenFile { transfer_id, path }) => {
                            let c = out_conn.clone();
                            let xfer_tx = out_xfer_tx.clone();
                            let pkt_tx = out_pkt_tx.clone();
                            tokio::spawn(async move {
                                if super::open_on_linked::send_open_file_with_events(
                                    &c,
                                    &path,
                                    &transfer_id,
                                    None,
                                    Some(&xfer_tx),
                                )
                                .await
                                .is_ok()
                                {
                                    let _ = pkt_tx.send(Outbound::Packet(KvmPacket::OpenFile { transfer_id }));
                                }
                            });
                        }
                        // --- end SPEC-25c ---
                        // --- SPEC-25a ---
                        Ok(Outbound::DragFile { drag_id, rel_path, path, size }) => {
                            spawn_server_bulk_transfer(
                                &out_conn,
                                &out_xfer_tx,
                                super::files::BulkTransferKind::Drag,
                                drag_id,
                                rel_path,
                                path,
                                size,
                            );
                        }
                        Ok(Outbound::ClipFile { offer_id, rel_path, path, size }) => {
                            spawn_server_bulk_transfer(
                                &out_conn,
                                &out_xfer_tx,
                                super::files::BulkTransferKind::Clip,
                                offer_id,
                                rel_path,
                                path,
                                size,
                            );
                        }
                        Ok(Outbound::CursorFile { role, name, data }) => {
                            let c = out_conn.clone();
                            tokio::spawn(async move {
                                let _ = super::files::send_cursor_file(&c, role, name, &data).await;
                            });
                        }
                        // --- end SPEC-25a ---
                        Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Err(_) => break,
                    },
                    res = out_disc_rx.recv() => match res {
                        Ok(target) => {
                            let matches = match (&target, &out_peer_id) {
                                (None, _) => true,
                                (Some(tid), Some(pid)) => tid == pid,
                                (Some(_), None) => false,
                            };
                            if matches {
                                let _ = write_frame(&mut send, &KvmPacket::SessionDisconnect).await;
                                let _ = send.finish();
                                if let Some(tx) = disc_done_tx.take() {
                                    let _ = tx.send(());
                                }
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(_)) => {}
                        Err(_) => break,
                    }
                }
            }
        }));

        let (dg_conn, delta_tx) = (conn.clone(), ctx.mouse_delta_tx.clone());
        tasks.push(tokio::spawn(async move {
            while let Ok(dg) = dg_conn.read_datagram().await {
                if let Some(d) = decode_delta(&dg) {
                    let _ = delta_tx.send(d);
                }
            }
        }));

        // --- SPEC-25b ---
        let (audio_dg_conn, mut audio_out_rx) = (conn.clone(), ctx.audio_out_tx.subscribe());
        tasks.push(tokio::spawn(async move {
            loop {
                match audio_out_rx.recv().await {
                    Ok(dg) => {
                        let _ = audio_dg_conn.send_datagram(dg);
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        }));
        // --- /SPEC-25b ---

        let (uni_conn, transfer_tx, dir) = (
            conn.clone(),
            ctx.transfer_tx.clone(),
            ctx.download_dir.clone(),
        );
        tasks.push(tokio::spawn(async move {
            let conn_id = uni_conn.stable_id();
            while let Ok(mut s) = uni_conn.accept_uni().await {
                let (tx, dir) = (transfer_tx.clone(), dir.clone());
                tokio::spawn(async move {
                    match super::files::receive_with_events_for_conn(
                        &mut s,
                        &dir,
                        Some(&tx),
                        conn_id,
                    )
                    .await
                    {
                        Ok(ev) => {
                            let _ = tx.send(ev);
                        }
                        Err(e) if e == super::files::TRANSFER_CANCELLED_ERR => {
                            let _ = s.stop(super::files::quic_cancel_varint());
                        }
                        Err(e) => {
                            super::debug_log::note(|| format!("STAGING_FAIL side=server err={e}"));
                            eprintln!("[KVM-Server] transfer rejected: {e}");
                        }
                    }
                });
            }
        }));

        let conn_id = conn.stable_id();
        let mut disconnected_by_server = false;
        loop {
            tokio::select! {
                res = read_frame::<_, KvmPacket>(&mut recv) => match res {
                    Ok(Some(packet)) => {
                        if matches!(packet, KvmPacket::SessionDisconnect) {
                            break;
                        }
                        if let KvmPacket::TransferCancel { ref id } = packet {
                            super::debug_log::note(|| format!("TRANSFER_CANCEL_RECV id={id}"));
                            if let Some(meta) =
                                super::files::global_cancel_registry().cancel_for_conn(conn_id, id)
                            {
                                let _ = ctx.transfer_tx.send(TransferEvent::Cancelled {
                                    id: meta.id,
                                    kind: meta.kind,
                                    name: meta.name,
                                    size_bytes: meta.size_bytes,
                                });
                            }
                            super::open_on_linked::cancel_pending_open(id);
                        }
                        if !matches!(
                            packet,
                            KvmPacket::Pair { .. } | KvmPacket::PairAck { .. } | KvmPacket::PeerDisconnected { .. }
                        ) {
                            let _ = ctx.incoming_tx.send(packet);
                        }
                    }
                    _ => break,
                },
                res = disc_rx.recv() => match res {
                    Ok(target) => {
                        let matches = match (&target, &peer_computer_id) {
                            (None, _) => true,
                            (Some(tid), Some(pid)) => tid == pid,
                            (Some(_), None) => false,
                        };
                        if matches {
                            disconnected_by_server = true;
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(_) => break,
                }
            }
        }

        if disconnected_by_server {
            let _ = timeout(Duration::from_millis(100), &mut disc_done_rx).await;
        }
        for t in tasks {
            t.abort();
        }
    }

    conn.close(0u32.into(), b"bye");
    if let Some(computer_id) = peer_computer_id {
        let _ = ctx
            .incoming_tx
            .send(KvmPacket::PeerDisconnected { computer_id });
    } else {
        let _ = ctx.incoming_tx.send(KvmPacket::SessionDisconnect);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvm::client::{handshake, send_delta, KvmClientHandle};
    use crate::kvm::files::send_file;
    use crate::kvm::identity::client_config;
    use crate::kvm::input_logic::{map_cursor_enter, receiver_step, Edge, ReceiverStep};

    struct Rig {
        srv: KvmServerHandle,
        srv_trust: Arc<TrustStore>,
        cli_id: Identity,
        cli_trust: TrustStore,
        dir: PathBuf,
    }

    fn default_mons() -> Vec<MonitorDisplay> {
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

    async fn rig(tag: &str) -> Rig {
        let dir = std::env::temp_dir().join(format!("anyleap-quic-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let srv_id = Identity::load_or_create(&dir.join("srv")).unwrap();
        let cli_id = Identity::load_or_create(&dir.join("cli")).unwrap();
        let srv_trust = Arc::new(TrustStore::load(None));
        let srv = start_kvm_server(
            0,
            &srv_id,
            srv_trust.clone(),
            "srv".into(),
            "Server".into(),
            Arc::new(default_mons),
            dir.join("dl"),
        )
        .await
        .unwrap();
        Rig {
            srv,
            srv_trust,
            cli_id,
            cli_trust: TrustStore::load(None),
            dir,
        }
    }

    fn key(code: u16) -> KvmPacket {
        KvmPacket::KeyEvent {
            key_code: code,
            scan_code: 0,
            is_down: true,
            extended: false,
        }
    }

    async fn connect(r: &Rig, pin: Option<String>) -> Result<KvmClientHandle, String> {
        KvmClientHandle::connect(
            "127.0.0.1",
            r.srv.port,
            "cli".into(),
            "Client".into(),
            default_mons(),
            pin,
            &r.cli_id,
            &r.cli_trust,
            r.dir.join("cli-dl"),
        )
        .await
    }

    async fn next_key(rx: &mut broadcast::Receiver<KvmPacket>) -> Option<u16> {
        loop {
            match timeout(Duration::from_secs(3), rx.recv()).await {
                Ok(Ok(KvmPacket::KeyEvent { key_code, .. })) => return Some(key_code),
                Ok(Ok(_)) => continue,
                _ => return None,
            }
        }
    }

    #[tokio::test]
    async fn pair_ack_carries_monitor_layout_at_pairing_time() {
        let dir = std::env::temp_dir().join(format!("anyleap-quic-fresh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let srv_id = Identity::load_or_create(&dir.join("srv")).unwrap();
        let cli_id = Identity::load_or_create(&dir.join("cli")).unwrap();
        let mon = |w: i32, h: i32| MonitorDisplay {
            id: "m".into(),
            device_name: "m".into(),
            x: 0,
            y: 0,
            width: w,
            height: h,
            is_primary: true,
        };
        let layout_a = vec![mon(1920, 1080)];
        let layout_b = vec![mon(1920, 1080), {
            let mut m = mon(2560, 1440);
            m.id = "m2".into();
            m.x = 1920;
            m.is_primary = false;
            m
        }];
        let current = Arc::new(Mutex::new(layout_a.clone()));
        let cur = Arc::clone(&current);
        let srv = start_kvm_server(
            0,
            &srv_id,
            Arc::new(TrustStore::load(None)),
            "srv".into(),
            "Server".into(),
            Arc::new(move || cur.lock().unwrap().clone()),
            dir.join("dl"),
        )
        .await
        .unwrap();

        let c1 = KvmClientHandle::connect(
            "127.0.0.1",
            srv.port,
            "cli-1".into(),
            "C1".into(),
            default_mons(),
            Some(srv.pin()),
            &cli_id,
            &TrustStore::load(None),
            dir.join("c1-dl"),
        )
        .await
        .unwrap();
        assert_eq!(c1.peer_monitors, layout_a);
        drop(c1);

        *current.lock().unwrap() = layout_b.clone();

        let cli2_id = Identity::load_or_create(&dir.join("cli2")).unwrap();
        let c2 = KvmClientHandle::connect(
            "127.0.0.1",
            srv.port,
            "cli-2".into(),
            "C2".into(),
            default_mons(),
            Some(srv.pin()),
            &cli2_id,
            &TrustStore::load(None),
            dir.join("c2-dl"),
        )
        .await
        .unwrap();
        assert_eq!(c2.peer_monitors, layout_b);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn correct_pin_pairs_and_delivers_input_then_reconnects_without_pin() {
        let r = rig("pair").await;
        let mut rx = r.srv.incoming_tx.subscribe();
        let c = connect(&r, Some(r.srv.pin())).await.unwrap();
        assert_eq!(c.peer_monitors, default_mons());
        c.send_packet(key(65)).await.unwrap();
        assert_eq!(next_key(&mut rx).await, Some(65));
        assert_eq!(
            r.srv_trust.by_address("127.0.0.1").unwrap().computer_id,
            "cli"
        );
        drop(c);

        let c2 = connect(&r, None).await.expect("reconnect without PIN");
        c2.send_packet(key(66)).await.unwrap();
        assert_eq!(next_key(&mut rx).await, Some(66));
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn wrong_pin_rejected_and_regenerates_after_five_failures() {
        let r = rig("wrong").await;
        let mut rx = r.srv.incoming_tx.subscribe();
        let first = r.srv.pin();
        for _ in 0..5 {
            let wrong = if first == "100000" {
                "100001"
            } else {
                "100000"
            };
            let err = connect(&r, Some(wrong.into())).await.err().unwrap();
            assert!(err.contains("Incorrect PIN"), "{err}");
        }
        assert_ne!(r.srv.pin(), first);
        assert!(r.srv_trust.by_address("127.0.0.1").is_none());
        assert_eq!(next_key(&mut rx).await, None);
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn lockout_rejects_even_correct_pin_then_expires() {
        let r = rig("lockout").await;
        let first = r.srv.pin();
        let wrong = if first == "100000" {
            "100001"
        } else {
            "100000"
        };
        for _ in 0..5 {
            assert!(connect(&r, Some(wrong.into())).await.is_err());
        }
        let err = connect(&r, Some(r.srv.pin())).await.err().unwrap();
        assert!(err.contains("Incorrect PIN"), "locked: {err}");
        tokio::time::sleep(PAIR_LOCKOUT + Duration::from_millis(200)).await;
        connect(&r, Some(r.srv.pin()))
            .await
            .expect("paired after lockout");
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn oversize_hello_closes_connection() {
        let r = rig("bighello").await;
        let mut ep = quinn::Endpoint::client(([0, 0, 0, 0], 0).into()).unwrap();
        ep.set_default_client_config(client_config(&r.cli_id).unwrap());
        let conn = ep
            .connect(([127, 0, 0, 1], r.srv.port).into(), "anyleap-kvm")
            .unwrap()
            .await
            .unwrap();
        let (mut send, _recv) = conn.open_bi().await.unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut send, &(1_000_000u32).to_be_bytes())
            .await
            .unwrap();
        assert!(
            timeout(Duration::from_secs(3), conn.closed()).await.is_ok(),
            "server must close"
        );
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn unpaired_key_event_closes_connection() {
        let r = rig("unpaired").await;
        let mut rx = r.srv.incoming_tx.subscribe();
        let mut ep = quinn::Endpoint::client(([0, 0, 0, 0], 0).into()).unwrap();
        ep.set_default_client_config(client_config(&r.cli_id).unwrap());
        let conn = ep
            .connect(([127, 0, 0, 1], r.srv.port).into(), "anyleap-kvm")
            .unwrap()
            .await
            .unwrap();
        let (mut send, _recv) = conn.open_bi().await.unwrap();
        write_frame(&mut send, &key(65)).await.unwrap();
        assert!(
            timeout(Duration::from_secs(3), conn.closed()).await.is_ok(),
            "server must close"
        );
        assert_eq!(next_key(&mut rx).await, None);
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn changed_server_identity_is_refused() {
        let r = rig("changed").await;
        drop(connect(&r, Some(r.srv.pin())).await.unwrap());
        let other_id = Identity::load_or_create(&r.dir.join("other")).unwrap();
        let other = start_kvm_server(
            0,
            &other_id,
            Arc::new(TrustStore::load(None)),
            "o".into(),
            "O".into(),
            Arc::new(default_mons),
            r.dir.join("dl2"),
        )
        .await
        .unwrap();
        let res = handshake(
            "127.0.0.1",
            other.port,
            "cli".into(),
            "Client".into(),
            default_mons(),
            None,
            &r.cli_id,
            &r.cli_trust,
        )
        .await;
        assert!(res.err().unwrap().contains("identity changed"));
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn five_mb_file_arrives_identical_while_control_flows() {
        let r = rig("file").await;
        let (mut rx, mut trx, mut drx) = (
            r.srv.incoming_tx.subscribe(),
            r.srv.transfer_tx.subscribe(),
            r.srv.mouse_delta_tx.subscribe(),
        );
        let c = connect(&r, Some(r.srv.pin())).await.unwrap();

        let data: Vec<u8> = (0..5 * 1024 * 1024u32)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        let src = r.dir.join("big.bin");
        std::fs::write(&src, &data).unwrap();
        let conn = c.conn.clone();
        let sender = tokio::spawn(async move { send_file(&conn, &src).await });

        for code in 0..20u16 {
            c.send_packet(key(code)).await.unwrap();
            send_delta(
                &c.conn,
                &UdpMouseDelta {
                    dx: 1,
                    dy: 2,
                    seq: code as u32,
                },
            );
        }
        for code in 0..20u16 {
            assert_eq!(next_key(&mut rx).await, Some(code));
        }
        assert!(
            timeout(Duration::from_secs(3), drx.recv()).await.is_ok(),
            "datagram delivered"
        );

        sender.await.unwrap().unwrap();
        match timeout(Duration::from_secs(10), trx.recv())
            .await
            .unwrap()
            .unwrap()
        {
            TransferEvent::File { name, size } => {
                assert_eq!((name.as_str(), size), ("big.bin", data.len() as u64));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(
            std::fs::read(r.dir.join("dl").join("big.bin")).unwrap(),
            data
        );
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    // Verification 4.5: Loopback QUIC input flow, CursorReturn, ReleaseAll, PeerDisconnected
    #[tokio::test]
    async fn loopback_cursor_enter_deltas_return_release_all_and_peer_disconnect() {
        let r = rig("loopback-kvm").await;
        let mut srv_in_rx = r.srv.incoming_tx.subscribe();
        let mut srv_delta_rx = r.srv.mouse_delta_tx.subscribe();
        let srv_out_tx = r.srv.outgoing_tx.clone();
        let srv_mons = default_mons();

        let c1 = KvmClientHandle::connect(
            "127.0.0.1",
            r.srv.port,
            "cli-1".into(),
            "Client1".into(),
            default_mons(),
            Some(r.srv.pin()),
            &r.cli_id,
            &r.cli_trust,
            r.dir.join("cli1-dl"),
        )
        .await
        .unwrap();
        let mut c1_in_rx = c1.incoming_tx.subscribe();

        let cli2_id = Identity::load_or_create(&r.dir.join("cli2")).unwrap();
        let cli2_trust = TrustStore::load(None);
        let c2 = KvmClientHandle::connect(
            "127.0.0.1",
            r.srv.port,
            "cli-2".into(),
            "Client2".into(),
            default_mons(),
            Some(r.srv.pin()),
            &cli2_id,
            &cli2_trust,
            r.dir.join("cli2-dl"),
        )
        .await
        .unwrap();

        // Drain the two Pair notifications
        let mut paired_ids = Vec::new();
        for _ in 0..2 {
            if let Ok(KvmPacket::Pair { computer_id, .. }) =
                timeout(Duration::from_secs(3), srv_in_rx.recv())
                    .await
                    .unwrap()
            {
                paired_ids.push(computer_id);
            }
        }
        assert!(paired_ids.contains(&"cli-1".to_string()));
        assert!(paired_ids.contains(&"cli-2".to_string()));

        // Controller enters receiver from left edge
        c1.send_packet(KvmPacket::CursorEnter {
            edge: "left".into(),
            relative_y: 0.5,
            dwell_ms: 0,
            deadzone_px: 0,
            target_monitor: None,
            along_px: None,
        })
        .await
        .unwrap();

        let (entry_edge, mut cursor_pos) = match timeout(Duration::from_secs(3), srv_in_rx.recv())
            .await
            .unwrap()
            .unwrap()
        {
            KvmPacket::CursorEnter {
                edge, relative_y, ..
            } => {
                let e = Edge::from_str(&edge).unwrap();
                let pos = map_cursor_enter(&srv_mons, e, relative_y);
                (e, pos)
            }
            other => panic!("expected CursorEnter, got {other:?}"),
        };
        assert_eq!(cursor_pos, (2, 540));

        // Controller sends move right, then move left past the entry edge
        c1.send_mouse_delta(10, 0);
        let d1 = timeout(Duration::from_secs(3), srv_delta_rx.recv())
            .await
            .unwrap()
            .unwrap();
        match receiver_step(cursor_pos, (d1.dx, d1.dy), &srv_mons, entry_edge) {
            ReceiverStep::Move(p) => cursor_pos = p,
            other => panic!("expected Move, got {other:?}"),
        }
        assert_eq!(cursor_pos, (12, 540));

        c1.send_mouse_delta(-20, 0);
        let d2 = timeout(Duration::from_secs(3), srv_delta_rx.recv())
            .await
            .unwrap()
            .unwrap();
        match receiver_step(cursor_pos, (d2.dx, d2.dy), &srv_mons, entry_edge) {
            ReceiverStep::Return { edge, relative_y } => {
                srv_out_tx
                    .send(Outbound::Packet(KvmPacket::CursorReturn {
                        edge: edge.as_str().to_string(),
                        relative_y,
                        target_monitor: None,
                        along_px: None,
                    }))
                    .unwrap();
            }
            other => panic!("expected Return, got {other:?}"),
        }

        match timeout(Duration::from_secs(3), c1_in_rx.recv())
            .await
            .unwrap()
            .unwrap()
        {
            KvmPacket::CursorReturn {
                edge, relative_y, ..
            } => {
                assert_eq!(edge, "left");
                assert!((relative_y - 0.5).abs() < 1e-4);
            }
            other => panic!("expected CursorReturn on client, got {other:?}"),
        }

        // ReleaseAll delivered
        c1.send_packet(KvmPacket::ReleaseAll).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(3), srv_in_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            KvmPacket::ReleaseAll
        );

        // Disconnecting cli-1 emits PeerDisconnected { computer_id: "cli-1" } and leaves cli-2 intact
        c1.send_packet(KvmPacket::SessionDisconnect).await.unwrap();
        drop(c1);
        let disc = loop {
            match timeout(Duration::from_secs(3), srv_in_rx.recv()).await {
                Ok(Ok(KvmPacket::PeerDisconnected { computer_id })) => break computer_id,
                Ok(Ok(_)) => continue,
                other => panic!("expected PeerDisconnected, got {other:?}"),
            }
        };
        assert_eq!(disc, "cli-1");

        // cli-2 still works
        c2.send_packet(key(77)).await.unwrap();
        assert_eq!(next_key(&mut srv_in_rx).await, Some(77));

        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn fingerprint_reconnect_succeeds_when_ip_changes_and_updates_address() {
        let r = rig("fp-ip-change").await;
        let mut rx = r.srv.incoming_tx.subscribe();
        let c = connect(&r, Some(r.srv.pin())).await.unwrap();
        c.send_packet(key(65)).await.unwrap();
        assert_eq!(next_key(&mut rx).await, Some(65));
        drop(c);

        // Simulate both sides having a stale IP for the paired peer while keeping its fingerprint.
        let mut cli_entry = r.cli_trust.list().into_iter().next().unwrap();
        cli_entry.address = "10.99.99.1".into();
        r.cli_trust.add(cli_entry);

        let mut srv_entry = r.srv_trust.list().into_iter().next().unwrap();
        srv_entry.address = "10.99.99.2".into();
        r.srv_trust.add(srv_entry);

        // Reconnect without PIN at the new address (127.0.0.1); fingerprint match must succeed and update address.
        let c2 = connect(&r, None)
            .await
            .expect("reconnect by fingerprint after IP change");
        c2.send_packet(key(66)).await.unwrap();
        assert_eq!(next_key(&mut rx).await, Some(66));

        assert_eq!(r.cli_trust.list()[0].address, "127.0.0.1");
        assert_eq!(r.srv_trust.list()[0].address, "127.0.0.1");
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn loopback_seams_enter_specific_monitor_and_return_non_primary_seam() {
        use crate::kvm::input_logic::{
            compute_linked_seams, detect_controller_edge_seams, map_controller_return_pos,
            map_cursor_enter_spec24, receiver_step_with_seams, ReceiverSeamStep,
        };
        use crate::kvm::protocol::Point;

        let main_mons = vec![
            MonitorDisplay {
                id: "\\\\.\\DISPLAY1".into(),
                device_name: "\\\\.\\DISPLAY1".into(),
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
                is_primary: true,
            },
            MonitorDisplay {
                id: "\\\\.\\DISPLAY2".into(),
                device_name: "\\\\.\\DISPLAY2".into(),
                x: 1920,
                y: 0,
                width: 1920,
                height: 1080,
                is_primary: false,
            },
        ];
        let linked_mons = vec![
            MonitorDisplay {
                id: "\\\\.\\DISPLAY_L1".into(),
                device_name: "\\\\.\\DISPLAY_L1".into(),
                x: 0,
                y: 0,
                width: 1920,
                height: 1200,
                is_primary: true,
            },
            MonitorDisplay {
                id: "\\\\.\\DISPLAY_L2".into(),
                device_name: "\\\\.\\DISPLAY_L2".into(),
                x: 1920,
                y: 0,
                width: 1280,
                height: 1080,
                is_primary: false,
            },
        ];
        let origin = Point { x: 0, y: 1080 };

        let dir =
            std::env::temp_dir().join(format!("anyleap-quic-spec24_seams-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let srv_id = Identity::load_or_create(&dir.join("srv")).unwrap();
        let cli_id = Identity::load_or_create(&dir.join("cli")).unwrap();
        let srv_trust = Arc::new(TrustStore::load(None));
        let cli_trust = TrustStore::load(None);
        let srv = start_kvm_server(
            0,
            &srv_id,
            srv_trust,
            "srv".into(),
            "Server".into(),
            {
                let m = linked_mons.clone();
                Arc::new(move || m.clone())
            },
            dir.join("dl-srv"),
        )
        .await
        .unwrap();
        let mut srv_in_rx = srv.incoming_tx.subscribe();
        let mut srv_delta_rx = srv.mouse_delta_tx.subscribe();
        let srv_out_tx = srv.outgoing_tx.clone();

        let cli = KvmClientHandle::connect(
            "127.0.0.1",
            srv.port,
            "cli".into(),
            "Client".into(),
            main_mons.clone(),
            Some(srv.pin()),
            &cli_id,
            &cli_trust,
            dir.join("dl-cli"),
        )
        .await
        .unwrap();
        let mut cli_in_rx = cli.incoming_tx.subscribe();

        // 1. Main computes seams and delivers KvmPacket::Seams to Linked.
        let seams = compute_linked_seams(&main_mons, &linked_mons, origin);
        assert_eq!(seams.len(), 2);
        cli.send_packet(KvmPacket::Seams {
            seams: seams.clone(),
        })
        .await
        .unwrap();

        let rx_seams = loop {
            match timeout(Duration::from_secs(3), srv_in_rx.recv())
                .await
                .unwrap()
                .unwrap()
            {
                KvmPacket::Seams { seams } => break seams,
                _ => continue,
            }
        };
        assert_eq!(rx_seams, seams);

        // 2. Main leaps from non-primary monitor \\.\DISPLAY2 bottom edge (x=2400).
        let main_seams = crate::kvm::input_logic::compute_seams(&main_mons, &linked_mons, origin);
        let hit = detect_controller_edge_seams(
            (2400, 1079),
            &main_mons,
            &linked_mons,
            origin,
            &main_seams,
            0,
        )
        .expect("Main non-primary bottom edge should hit seam to L2");
        assert_eq!(hit.target_monitor, "\\\\.\\DISPLAY_L2");
        assert_eq!(hit.along_px, 480);

        cli.send_packet(KvmPacket::CursorEnter {
            edge: hit.seam.side.opposite().as_str().to_string(),
            relative_y: hit.relative_y,
            dwell_ms: 0,
            deadzone_px: 0,
            target_monitor: Some(hit.target_monitor.clone()),
            along_px: Some(hit.along_px),
        })
        .await
        .unwrap();

        let (entry_edge, cursor_pos) = loop {
            match timeout(Duration::from_secs(3), srv_in_rx.recv())
                .await
                .unwrap()
                .unwrap()
            {
                KvmPacket::CursorEnter {
                    edge,
                    relative_y,
                    target_monitor,
                    along_px,
                    ..
                } => {
                    let e = Edge::from_str(&edge).unwrap();
                    let pos = map_cursor_enter_spec24(
                        &linked_mons,
                        e,
                        relative_y,
                        target_monitor.as_deref(),
                        along_px,
                    );
                    break (e, pos);
                }
                _ => continue,
            }
        };
        assert_eq!(entry_edge, Edge::Top);
        // Should enter specific non-primary monitor \\.\DISPLAY_L2 (x >= 1920), not L1!
        assert_eq!(cursor_pos, (2400, 2));

        // 3. Push past L2 top seam (which is on non-primary L2) -> returns to Main non-primary \\.\DISPLAY2!
        let mut dwell_seam = None;
        let mut dwell_start = None;
        cli.send_mouse_delta(0, -10);
        let d = timeout(Duration::from_secs(3), srv_delta_rx.recv())
            .await
            .unwrap()
            .unwrap();
        match receiver_step_with_seams(
            cursor_pos,
            (d.dx, d.dy),
            &linked_mons,
            &rx_seams,
            0,
            0,
            &mut dwell_seam,
            &mut dwell_start,
            0,
        ) {
            ReceiverSeamStep::Return {
                edge,
                relative_y,
                target_monitor,
                along_px,
            } => {
                srv_out_tx
                    .send(Outbound::Packet(KvmPacket::CursorReturn {
                        edge: edge.as_str().to_string(),
                        relative_y,
                        target_monitor,
                        along_px,
                    }))
                    .unwrap();
            }
            other => panic!("expected ReceiverSeamStep::Return, got {other:?}"),
        }

        // Also check non-seam edge clamping on L2 right edge (x = 3199).
        assert_eq!(
            receiver_step_with_seams(
                (3195, 500),
                (20, 0),
                &linked_mons,
                &rx_seams,
                0,
                0,
                &mut None,
                &mut None,
                0,
            ),
            ReceiverSeamStep::Move((3199, 500))
        );

        let landed = match timeout(Duration::from_secs(3), cli_in_rx.recv())
            .await
            .unwrap()
            .unwrap()
        {
            KvmPacket::CursorReturn {
                edge,
                relative_y,
                target_monitor,
                along_px,
            } => {
                assert_eq!(edge, "top");
                assert_eq!(target_monitor.as_deref(), Some("\\\\.\\DISPLAY2"));
                assert_eq!(along_px, Some(480));
                map_controller_return_pos(
                    &main_mons,
                    target_monitor.as_deref(),
                    Edge::Top,
                    along_px,
                    &main_mons[0],
                    &crate::kvm::input_logic::union_rect(&linked_mons),
                    Edge::Bottom,
                    0,
                    0,
                    relative_y,
                )
            }
            other => panic!("expected CursorReturn, got {other:?}"),
        };
        assert_eq!(landed, (2400, 1077));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // --- SPEC-25c ---
    #[tokio::test]
    async fn loopback_session_lock_open_url_and_open_file() {
        let r = rig("spec25c").await;
        let mut srv_in_rx = r.srv.incoming_tx.subscribe();
        let mut srv_trx = r.srv.transfer_tx.subscribe();
        let srv_out_tx = r.srv.outgoing_tx.clone();

        let c = connect(&r, Some(r.srv.pin())).await.unwrap();
        let mut cli_in_rx = c.incoming_tx.subscribe();

        // Drain initial Pair packet
        let _ = timeout(Duration::from_secs(3), srv_in_rx.recv())
            .await
            .unwrap()
            .unwrap();

        // 1. SessionLock (client -> server)
        c.send_packet(KvmPacket::SessionLock).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(3), srv_in_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            KvmPacket::SessionLock
        );

        // 2. OpenUrl (client -> server, and server -> client)
        let url = "https://example.com/anyleap".to_string();
        c.send_packet(KvmPacket::OpenUrl { url: url.clone() })
            .await
            .unwrap();
        assert_eq!(
            timeout(Duration::from_secs(3), srv_in_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            KvmPacket::OpenUrl { url: url.clone() }
        );
        srv_out_tx
            .send(Outbound::Packet(KvmPacket::OpenUrl { url: url.clone() }))
            .unwrap();
        assert_eq!(
            timeout(Duration::from_secs(3), cli_in_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            KvmPacket::OpenUrl { url }
        );

        let open_dir = crate::kvm::open_on_linked::open_staging_dir_for(
            &crate::kvm::open_on_linked::open_staging_root(),
            "tid-1",
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&open_dir);

        // 3. OpenFile stream + OpenFile packet (client -> server)
        let src = r.dir.join("note.txt");
        std::fs::write(&src, b"hello linked pc").unwrap();
        crate::kvm::open_on_linked::send_open_file(&c.conn, &src, "tid-1", None)
            .await
            .unwrap();
        c.send_packet(KvmPacket::OpenFile {
            transfer_id: "tid-1".into(),
        })
        .await
        .unwrap();

        let mut saw_100 = false;
        loop {
            match timeout(Duration::from_secs(3), srv_trx.recv())
                .await
                .unwrap()
                .unwrap()
            {
                TransferEvent::Progress {
                    id,
                    kind,
                    pct,
                    state,
                    size_bytes,
                    ..
                } => {
                    assert_eq!(id, "tid-1");
                    assert_eq!(kind, "open");
                    assert!(pct <= 100);
                    assert_eq!(size_bytes, 15);
                    if pct == 100 {
                        assert_eq!(state, "done");
                        saw_100 = true;
                    } else {
                        assert_eq!(state, "active");
                    }
                }
                TransferEvent::OpenFile {
                    transfer_id,
                    name,
                    size,
                    saved_path,
                } => {
                    assert_eq!(transfer_id, "tid-1");
                    assert_eq!(name, "note.txt");
                    assert_eq!(size, 15);
                    assert!(saved_path.starts_with(&open_dir));
                    assert_eq!(std::fs::read(&saved_path).unwrap(), b"hello linked pc");
                    break;
                }
                other => panic!("expected TransferEvent::OpenFile, got {other:?}"),
            }
        }
        assert!(saw_100);
        assert_eq!(
            timeout(Duration::from_secs(3), srv_in_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            KvmPacket::OpenFile {
                transfer_id: "tid-1".into(),
            }
        );

        let _ = std::fs::remove_dir_all(&open_dir);
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn open_file_transfer_cancels_cleanly_from_both_sides() {
        let r = rig("open-cancel").await;
        let pin = r.srv.security_pin.lock().unwrap().clone();
        let c = connect(&r, Some(pin)).await.unwrap();
        let mut srv_trx = r.srv.transfer_tx.subscribe();
        let mut cli_trx = c.transfer_tx.subscribe();

        // Large enough payload that the transfer is still running when cancelled.
        let src = r.dir.join("big-open.bin");
        let payload = vec![0xABu8; 48 * 1024 * 1024];
        std::fs::write(&src, &payload).unwrap();

        // Case 1: Receiver initiates cancel mid-transfer.
        let id_rx = format!("tid-rx-cancel-{}", std::process::id());
        let open_dir_rx = crate::kvm::open_on_linked::open_staging_dir_for(
            &crate::kvm::open_on_linked::open_staging_root(),
            &id_rx,
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&open_dir_rx);

        let send_conn = c.conn.clone();
        let send_path = src.clone();
        let send_tx = c.transfer_tx.clone();
        let send_id = id_rx.clone();
        let send_task = tokio::spawn(async move {
            crate::kvm::open_on_linked::send_open_file_with_events(
                &send_conn,
                &send_path,
                &send_id,
                None,
                Some(&send_tx),
            )
            .await
        });

        // Wait until receiver starts receiving progress.
        loop {
            match timeout(Duration::from_secs(5), srv_trx.recv())
                .await
                .unwrap()
                .unwrap()
            {
                TransferEvent::Progress { id, .. } if id == id_rx => break,
                _ => {}
            }
        }
        // Simulate receiver cancelling locally on server (conn != client conn) + sending TransferCancel to client.
        let srv_conns = crate::kvm::files::global_cancel_registry()
            .cancel_except_conn(c.conn.stable_id(), &id_rx);
        for (_, meta) in &srv_conns {
            let _ = r.srv.transfer_tx.send(TransferEvent::Cancelled {
                id: meta.id.clone(),
                kind: meta.kind.clone(),
                name: meta.name.clone(),
                size_bytes: meta.size_bytes,
            });
        }
        assert!(
            !srv_conns.is_empty(),
            "server must have active receiver registration"
        );
        r.srv
            .outgoing_tx
            .send(Outbound::Packet(KvmPacket::TransferCancel {
                id: id_rx.clone(),
            }))
            .unwrap();

        let send_res = timeout(Duration::from_secs(5), send_task)
            .await
            .unwrap()
            .unwrap();
        assert!(send_res.is_err(), "sender must abort on cancel");

        // Both sides must observe TransferEvent::Cancelled, and no partial files remain.
        let mut saw_cli_cancel = false;
        while let Ok(Ok(ev)) = timeout(Duration::from_millis(500), cli_trx.recv()).await {
            if matches!(ev, TransferEvent::Cancelled { ref id, .. } if id == &id_rx) {
                saw_cli_cancel = true;
                break;
            }
        }
        assert!(saw_cli_cancel, "sender (client) must observe Cancelled");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !open_dir_rx.exists(),
            "partial open staging dir must be removed"
        );

        // Case 2: Sender initiates cancel mid-transfer.
        let id_tx = format!("tid-tx-cancel-{}", std::process::id());
        let open_dir_tx = crate::kvm::open_on_linked::open_staging_dir_for(
            &crate::kvm::open_on_linked::open_staging_root(),
            &id_tx,
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&open_dir_tx);

        let send_conn2 = c.conn.clone();
        let send_path2 = src.clone();
        let send_tx2 = c.transfer_tx.clone();
        let send_id2 = id_tx.clone();
        let send_task2 = tokio::spawn(async move {
            crate::kvm::open_on_linked::send_open_file_with_events(
                &send_conn2,
                &send_path2,
                &send_id2,
                None,
                Some(&send_tx2),
            )
            .await
        });

        loop {
            match timeout(Duration::from_secs(5), srv_trx.recv())
                .await
                .unwrap()
                .unwrap()
            {
                TransferEvent::Progress { id, .. } if id == id_tx => break,
                _ => {}
            }
        }
        if let Some(meta) =
            crate::kvm::files::global_cancel_registry().cancel_for_conn(c.conn.stable_id(), &id_tx)
        {
            let _ = c.transfer_tx.send(TransferEvent::Cancelled {
                id: meta.id,
                kind: meta.kind,
                name: meta.name,
                size_bytes: meta.size_bytes,
            });
        }
        c.send_packet(KvmPacket::TransferCancel { id: id_tx.clone() })
            .await
            .unwrap();

        let send_res2 = timeout(Duration::from_secs(5), send_task2)
            .await
            .unwrap()
            .unwrap();
        assert!(send_res2.is_err());

        let mut saw_srv_cancel = false;
        while let Ok(Ok(ev)) = timeout(Duration::from_millis(500), srv_trx.recv()).await {
            if matches!(ev, TransferEvent::Cancelled { ref id, .. } if id == &id_tx) {
                saw_srv_cancel = true;
                break;
            }
        }
        assert!(saw_srv_cancel, "receiver (server) must observe Cancelled");
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(
            !open_dir_tx.exists(),
            "partial open staging dir must be removed on sender cancel"
        );

        let _ = std::fs::remove_dir_all(&open_dir_rx);
        let _ = std::fs::remove_dir_all(&open_dir_tx);
        let _ = std::fs::remove_dir_all(&r.dir);
    }
    // --- end SPEC-25c ---

    // --- SPEC-25b ---
    #[tokio::test]
    async fn loopback_audio_start_and_audio_datagrams() {
        use crate::kvm::audio::{
            JitterBuffer, AUDIO_PACKET_BYTES, JITTER_MAX_FRAMES, JITTER_TARGET_FRAMES,
        };
        use crate::kvm::protocol::encode_audio_datagram;

        let r = rig("audio-25b").await;
        let mut srv_in_rx = r.srv.incoming_tx.subscribe();
        let cli = connect(&r, Some(r.srv.pin())).await.unwrap();
        let mut cli_in_rx = cli.incoming_tx.subscribe();
        let mut cli_audio_rx = cli.audio_tx.subscribe();

        // Drain Pair
        loop {
            match timeout(Duration::from_secs(3), srv_in_rx.recv())
                .await
                .unwrap()
                .unwrap()
            {
                KvmPacket::Pair { .. } => break,
                _ => continue,
            }
        }

        // Main PC -> Linked PC: AudioStart
        cli.send_packet(KvmPacket::AudioStart {
            sample_rate: 48_000,
            channels: 2,
            frame_ms: 5,
            mute_local: true,
        })
        .await
        .unwrap();

        match timeout(Duration::from_secs(3), srv_in_rx.recv())
            .await
            .unwrap()
            .unwrap()
        {
            KvmPacket::AudioStart {
                sample_rate,
                channels,
                frame_ms,
                mute_local,
            } => {
                assert_eq!(
                    (sample_rate, channels, frame_ms, mute_local),
                    (48_000, 2, 5, true)
                );
            }
            other => panic!("expected AudioStart, got {other:?}"),
        }

        // Linked PC -> Main PC: AudioMuteState reporting
        r.srv
            .outgoing_tx
            .send(Outbound::Packet(KvmPacket::AudioMuteState {
                state: "muted".to_string(),
            }))
            .unwrap();
        assert_eq!(
            timeout(Duration::from_secs(3), cli_in_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            KvmPacket::AudioMuteState {
                state: "muted".to_string(),
            }
        );

        // Linked PC -> Main PC: 8 x 5 ms (960-byte) audio datagrams
        for seq in 0u32..8 {
            let pcm = vec![(seq + 10) as u8; AUDIO_PACKET_BYTES];
            let dg = encode_audio_datagram(seq, &pcm).unwrap();
            assert_ne!(dg.len(), 12);
            r.srv.audio_out_tx.send(bytes::Bytes::from(dg)).unwrap();
        }

        let mut jb = JitterBuffer::new(JITTER_TARGET_FRAMES, JITTER_MAX_FRAMES);
        for _ in 0..8 {
            let dg = timeout(Duration::from_secs(3), cli_audio_rx.recv())
                .await
                .unwrap()
                .unwrap();
            assert_eq!(dg.pcm.len(), AUDIO_PACKET_BYTES);
            jb.push(dg.seq, dg.pcm);
        }
        for seq in 0u8..8 {
            let out = jb.pop();
            assert_eq!(out[0], seq + 10);
        }

        // AudioActivity (Linked -> Main) and MediaKey + AudioStop (Main -> Linked)
        r.srv
            .outgoing_tx
            .send(Outbound::Packet(KvmPacket::AudioActivity { active: true }))
            .unwrap();
        assert_eq!(
            timeout(Duration::from_secs(3), cli_in_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            KvmPacket::AudioActivity { active: true }
        );

        cli.send_packet(KvmPacket::MediaKey {
            vk: 0xB3,
            is_down: true,
        })
        .await
        .unwrap();
        assert_eq!(
            timeout(Duration::from_secs(3), srv_in_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            KvmPacket::MediaKey {
                vk: 0xB3,
                is_down: true
            }
        );

        cli.send_packet(KvmPacket::AudioStop).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(3), srv_in_rx.recv())
                .await
                .unwrap()
                .unwrap(),
            KvmPacket::AudioStop
        );

        let _ = std::fs::remove_dir_all(&r.dir);
    }
    // --- /SPEC-25b ---

    // --- SPEC-25a ---
    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // Serializes global files settings across sync and async tests.
    async fn loopback_drag_offer_drag_file_and_drag_ready() {
        let _settings_lock = crate::kvm::dragdrop::test_files_settings_lock();
        struct RestoreDragSettings;
        impl Drop for RestoreDragSettings {
            fn drop(&mut self) {
                crate::kvm::dragdrop::set_files_settings(
                    crate::kvm::dragdrop::DEFAULT_DRAG_DROP_ENABLED,
                    true,
                    2048,
                    false,
                    100,
                );
            }
        }
        let _restore_settings = RestoreDragSettings;
        crate::kvm::dragdrop::set_files_settings(true, true, 2048, false, 100);

        use crate::kvm::files::send_drag_file;
        use crate::kvm::protocol::{FileMeta, TransferEvent};
        use crate::kvm::staging::{
            cleanup_staging_entry, default_staging_root, record_file_received,
            register_transfer_offer, TransferDestination, TransferKind,
        };

        let r = rig("drag-offer-file-ready").await;
        let mut srv_pkt_rx = r.srv.incoming_tx.subscribe();
        let mut srv_xfer_rx = r.srv.transfer_tx.subscribe();
        let c = connect(&r, Some(r.srv.pin())).await.unwrap();
        let mut cli_pkt_rx = c.incoming_tx.subscribe();

        let src_file = r.dir.join("hello.txt");
        let payload = b"Cross-PC SPEC-25a QUIC loopback drag payload!";
        std::fs::write(&src_file, payload).unwrap();

        let drag_id = format!("drag-loopback-{}", std::process::id());
        let offer_files = vec![FileMeta {
            rel_path: "hello.txt".into(),
            size: payload.len() as u64,
            is_dir: false,
        }];

        // Client -> Server: DragOffer control packet
        c.send_packet(KvmPacket::DragOffer {
            drag_id: drag_id.clone(),
            files: offer_files.clone(),
            button: 1,
        })
        .await
        .unwrap();

        let (rx_drag_id, rx_files) = loop {
            match timeout(Duration::from_secs(3), srv_pkt_rx.recv()).await {
                Ok(Ok(KvmPacket::DragOffer { drag_id, files, .. })) => break (drag_id, files),
                Ok(Ok(_)) => continue,
                other => panic!("expected DragOffer on server, got {other:?}"),
            }
        };
        assert_eq!(rx_drag_id, drag_id);
        assert_eq!(rx_files, offer_files);

        // Register staging offer on server side (with Downloads dir inside test temp)
        let staging_root = default_staging_root();
        let plan = register_transfer_offer(
            &rx_drag_id,
            TransferKind::Drag,
            &rx_files,
            &staging_root,
            &r.dir.join("downloads"),
            512,
        )
        .unwrap();
        assert_eq!(plan.destination, TransferDestination::Staging);

        // Client -> Server: StreamHeader::DragFile uni stream
        send_drag_file(
            &c.conn,
            drag_id.clone(),
            "hello.txt".to_string(),
            &src_file,
            payload.len() as u64,
        )
        .await
        .unwrap();

        let staged_path = loop {
            match timeout(Duration::from_secs(3), srv_xfer_rx.recv()).await {
                Ok(Ok(TransferEvent::DragFile {
                    drag_id: ev_id,
                    rel_path,
                    path,
                    size,
                })) if ev_id == drag_id => {
                    assert_eq!(rel_path, "hello.txt");
                    assert_eq!(size, payload.len() as u64);
                    let (completed, _) = record_file_received(&ev_id, size);
                    assert!(completed.is_some());
                    break path;
                }
                Ok(Ok(_)) => continue,
                other => panic!("expected TransferEvent::DragFile, got {other:?}"),
            }
        };
        assert_eq!(std::fs::read(&staged_path).unwrap(), payload);

        // Server -> Client: DragReady control packet
        r.srv
            .outgoing_tx
            .send(Outbound::Packet(KvmPacket::DragReady {
                drag_id: drag_id.clone(),
            }))
            .unwrap();
        loop {
            match timeout(Duration::from_secs(3), cli_pkt_rx.recv()).await {
                Ok(Ok(KvmPacket::DragReady { drag_id: ready_id })) => {
                    assert_eq!(ready_id, drag_id);
                    break;
                }
                Ok(Ok(_)) => continue,
                other => panic!("expected DragReady on client, got {other:?}"),
            }
        }

        cleanup_staging_entry(&staging_root, &drag_id);
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn loopback_large_clip_file_400mb_succeeds() {
        use crate::kvm::files::send_clip_file;
        use crate::kvm::protocol::{FileMeta, TransferEvent};
        use crate::kvm::staging::{
            cleanup_staging_entry, default_staging_root, record_file_received,
            register_transfer_offer, TransferDestination, TransferKind,
        };

        let r = rig("clip-file-400mb").await;
        let mut srv_pkt_rx = r.srv.incoming_tx.subscribe();
        let mut srv_xfer_rx = r.srv.transfer_tx.subscribe();
        let c = connect(&r, Some(r.srv.pin())).await.unwrap();

        let size: u64 = 400 * 1024 * 1024;
        let src_file = r.dir.join("large-video.mp4");
        {
            let f = std::fs::File::create(&src_file).unwrap();
            f.set_len(size).unwrap();
        }

        let offer_id = format!("clip-400mb-{}", std::process::id());
        let offer_files = vec![FileMeta {
            rel_path: "large-video.mp4".into(),
            size,
            is_dir: false,
        }];

        c.send_packet(KvmPacket::FileClipOffer {
            offer_id: offer_id.clone(),
            files: offer_files.clone(),
        })
        .await
        .unwrap();

        let (rx_offer_id, rx_files) = loop {
            match timeout(Duration::from_secs(3), srv_pkt_rx.recv()).await {
                Ok(Ok(KvmPacket::FileClipOffer { offer_id, files })) => break (offer_id, files),
                Ok(Ok(_)) => continue,
                other => panic!("expected FileClipOffer on server, got {other:?}"),
            }
        };

        let staging_root = default_staging_root();
        let plan = register_transfer_offer(
            &rx_offer_id,
            TransferKind::Clip,
            &rx_files,
            &staging_root,
            &r.dir.join("downloads"),
            2048,
        )
        .unwrap();
        assert_eq!(plan.destination, TransferDestination::Staging);

        let t0 = std::time::Instant::now();
        let conn_clone = c.conn.clone();
        let id_clone = offer_id.clone();
        let src_clone = src_file.clone();
        let send_task = tokio::spawn(async move {
            send_clip_file(
                &conn_clone,
                id_clone,
                "large-video.mp4".to_string(),
                &src_clone,
                size,
            )
            .await
        });

        // Control stream must stay responsive while a 400 MB ClipFile is streaming.
        c.send_packet(KvmPacket::MouseWheel { delta: 42 })
            .await
            .unwrap();
        loop {
            match timeout(Duration::from_secs(3), srv_pkt_rx.recv()).await {
                Ok(Ok(KvmPacket::MouseWheel { delta: 42 })) => break,
                Ok(Ok(_)) => continue,
                other => panic!("control packet stalled during 400 MB clip send: {other:?}"),
            }
        }

        let mut progress_pcts = Vec::new();
        let staged_path = loop {
            match timeout(Duration::from_secs(25), srv_xfer_rx.recv()).await {
                Ok(Ok(TransferEvent::Progress { id, kind, pct, .. })) if id == offer_id => {
                    assert_eq!(kind, "clip");
                    assert!(pct <= 100);
                    if let Some(&prev) = progress_pcts.last() {
                        assert!(pct >= prev);
                    }
                    progress_pcts.push(pct);
                }
                Ok(Ok(TransferEvent::ClipFile {
                    offer_id: ev_id,
                    rel_path,
                    path,
                    size: rx_size,
                })) if ev_id == offer_id => {
                    assert_eq!(rel_path, "large-video.mp4");
                    assert_eq!(rx_size, size);
                    let (completed, _) = record_file_received(&ev_id, rx_size);
                    assert!(completed.is_some());
                    break path;
                }
                Ok(Ok(_)) => continue,
                other => panic!(
                    "expected TransferEvent::ClipFile after {:?}, got {other:?}",
                    t0.elapsed()
                ),
            }
        };
        send_task.await.unwrap().unwrap();
        assert_eq!(progress_pcts.last().copied(), Some(100));
        let elapsed = t0.elapsed();
        eprintln!("[loopback-400mb] elapsed={elapsed:?}, path={staged_path:?}");
        assert_eq!(std::fs::metadata(&staged_path).unwrap().len(), size);

        cleanup_staging_entry(&staging_root, &offer_id);
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn loopback_clip_file_failure_cleans_part_and_emits_failed() {
        use crate::kvm::protocol::{write_frame, FileMeta, StreamHeader, TransferEvent};
        use crate::kvm::staging::{
            cleanup_staging_entry, default_staging_root, register_transfer_offer, TransferKind,
        };

        let r = rig("clip-file-fail").await;
        let mut srv_pkt_rx = r.srv.incoming_tx.subscribe();
        let mut srv_xfer_rx = r.srv.transfer_tx.subscribe();
        let c = connect(&r, Some(r.srv.pin())).await.unwrap();

        let offer_id = format!("clip-fail-{}", std::process::id());
        c.send_packet(KvmPacket::FileClipOffer {
            offer_id: offer_id.clone(),
            files: vec![FileMeta {
                rel_path: "broken.bin".into(),
                size: 64 * 1024,
                is_dir: false,
            }],
        })
        .await
        .unwrap();

        let (rx_id, rx_files) = loop {
            match timeout(Duration::from_secs(3), srv_pkt_rx.recv()).await {
                Ok(Ok(KvmPacket::FileClipOffer { offer_id, files })) => break (offer_id, files),
                Ok(Ok(_)) => continue,
                other => panic!("expected FileClipOffer, got {other:?}"),
            }
        };
        let staging_root = default_staging_root();
        let plan = register_transfer_offer(
            &rx_id,
            TransferKind::Clip,
            &rx_files,
            &staging_root,
            &r.dir.join("downloads"),
            2048,
        )
        .unwrap();

        // Stream a valid header + truncated payload and finish early (EOF before declared 64 KiB).
        let mut s = c.conn.open_uni().await.unwrap();
        write_frame(
            &mut s,
            &StreamHeader::ClipFile {
                offer_id: offer_id.clone(),
                rel_path: "broken.bin".into(),
                size: 64 * 1024,
            },
        )
        .await
        .unwrap();
        s.write_all(&vec![0xAB; 512]).await.unwrap();
        s.finish().unwrap();
        let _ = s.stopped().await;

        loop {
            match timeout(Duration::from_secs(5), srv_xfer_rx.recv()).await {
                Ok(Ok(TransferEvent::Failed {
                    id,
                    kind,
                    name,
                    reason,
                })) if id == offer_id => {
                    assert_eq!(kind, "clip");
                    assert_eq!(name, "broken.bin");
                    assert!(!reason.is_empty());
                    break;
                }
                Ok(Ok(TransferEvent::ClipFile {
                    offer_id: ev_id, ..
                })) if ev_id == offer_id => {
                    panic!("truncated ClipFile must never complete");
                }
                Ok(Ok(_)) => continue,
                other => panic!("expected TransferEvent::Failed, got {other:?}"),
            }
        }

        let part_path = plan.base_dir.join("broken.bin.part");
        assert!(
            !part_path.exists(),
            "failed transfer must remove .part file"
        );
        cleanup_staging_entry(&staging_root, &offer_id);
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn loopback_mouse_datagram_latency_during_300mb_clip_transfer() {
        use crate::kvm::files::send_clip_file;
        use crate::kvm::protocol::{encode_delta, FileMeta, TransferEvent, UdpMouseDelta};
        use crate::kvm::staging::{
            cleanup_staging_entry, default_staging_root, record_file_received,
            register_transfer_offer, TransferKind,
        };
        use tokio::sync::oneshot;

        let r = rig("clip-300mb-dg-lat").await;
        let mut srv_pkt_rx = r.srv.incoming_tx.subscribe();
        let mut srv_xfer_rx = r.srv.transfer_tx.subscribe();
        let mut srv_dg_rx = r.srv.mouse_delta_tx.subscribe();
        let c = connect(&r, Some(r.srv.pin())).await.unwrap();

        let size: u64 = 300 * 1024 * 1024;
        let src_file = r.dir.join("bulk-300mb.bin");
        {
            let f = std::fs::File::create(&src_file).unwrap();
            f.set_len(size).unwrap();
        }

        let offer_id = format!("clip-300mb-dg-{}", std::process::id());
        let offer_files = vec![FileMeta {
            rel_path: "bulk-300mb.bin".into(),
            size,
            is_dir: false,
        }];
        c.send_packet(KvmPacket::FileClipOffer {
            offer_id: offer_id.clone(),
            files: offer_files,
        })
        .await
        .unwrap();

        let (rx_offer_id, rx_files) = loop {
            match timeout(Duration::from_secs(3), srv_pkt_rx.recv()).await {
                Ok(Ok(KvmPacket::FileClipOffer { offer_id, files })) => break (offer_id, files),
                Ok(Ok(_)) => continue,
                other => panic!("expected FileClipOffer, got {other:?}"),
            }
        };
        let staging_root = default_staging_root();
        let _plan = register_transfer_offer(
            &rx_offer_id,
            TransferKind::Clip,
            &rx_files,
            &staging_root,
            &r.dir.join("downloads"),
            2048,
        )
        .unwrap();

        let epoch = std::time::Instant::now();
        let (stop_tx, mut stop_rx) = oneshot::channel::<()>();
        let (rx_stop_tx, mut rx_stop_rx) = oneshot::channel::<()>();

        let dg_conn = c.conn.clone();
        let sender_task = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(4));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut seq = 0u32;
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stop_rx => break seq,
                    _ = tick.tick() => {
                        let ts_us = epoch.elapsed().as_micros() as u64;
                        let delta = UdpMouseDelta {
                            dx: (ts_us & 0xFFFF_FFFF) as i32,
                            dy: (ts_us >> 32) as i32,
                            seq,
                        };
                        let _ = dg_conn.send_datagram(bytes::Bytes::copy_from_slice(&encode_delta(&delta)));
                        seq = seq.wrapping_add(1);
                    }
                }
            }
        });

        let recv_task = tokio::spawn(async move {
            let mut latencies_us = Vec::<u64>::new();
            loop {
                tokio::select! {
                    biased;
                    _ = &mut rx_stop_rx => break latencies_us,
                    res = srv_dg_rx.recv() => {
                        match res {
                            Ok(d) => {
                                let sent_us = ((d.dy as u32 as u64) << 32) | (d.dx as u32 as u64);
                                let now_us = epoch.elapsed().as_micros() as u64;
                                latencies_us.push(now_us.saturating_sub(sent_us));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(_) => break latencies_us,
                        }
                    }
                }
            }
        });

        let t0 = std::time::Instant::now();
        let conn_clone = c.conn.clone();
        let id_clone = offer_id.clone();
        let src_clone = src_file.clone();
        let send_task = tokio::spawn(async move {
            send_clip_file(
                &conn_clone,
                id_clone,
                "bulk-300mb.bin".to_string(),
                &src_clone,
                size,
            )
            .await
        });

        loop {
            match timeout(Duration::from_secs(60), srv_xfer_rx.recv()).await {
                Ok(Ok(TransferEvent::ClipFile {
                    offer_id: ev_id,
                    size: rx_size,
                    ..
                })) if ev_id == offer_id => {
                    let (completed, _) = record_file_received(&ev_id, rx_size);
                    assert!(completed.is_some());
                    break;
                }
                Ok(Ok(_)) => continue,
                other => panic!("ClipFile did not complete cleanly: {other:?}"),
            }
        }
        send_task.await.unwrap().unwrap();
        let elapsed = t0.elapsed();
        let _ = stop_tx.send(());
        let sent_count = sender_task.await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let _ = rx_stop_tx.send(());
        let mut latencies = recv_task.await.unwrap();
        assert!(
            !latencies.is_empty(),
            "must receive mouse datagrams during transfer"
        );
        latencies.sort_unstable();
        let n = latencies.len();
        let p50 = latencies[n / 2];
        let p95 = latencies[((n - 1) * 95) / 100];
        let max = *latencies.last().unwrap();
        let mb_per_sec = (size as f64 / (1024.0 * 1024.0)) / elapsed.as_secs_f64();
        eprintln!(
            "[r2-bench] 300MB elapsed={elapsed:?} throughput={mb_per_sec:.1} MB/s datagrams_sent={sent_count} recv={n} p50={p50}us p95={p95}us max={max}us"
        );
        // Loose CI safety sanity bound only (non-flaky on busy loopback runners).
        assert!(
            max < 10_000_000,
            "unexpected >10s datagram stall: max={max}us"
        );

        cleanup_staging_entry(&staging_root, &offer_id);

        // Phase 2: Linked -> Main (Server -> Client 300MB transfer while Client -> Server mouse datagrams flow).
        let offer_id_s2c = format!("clip-300mb-dg-s2c-{}", std::process::id());
        let offer_files_s2c = vec![FileMeta {
            rel_path: "bulk-300mb.bin".into(),
            size,
            is_dir: false,
        }];
        let _plan_s2c = register_transfer_offer(
            &offer_id_s2c,
            TransferKind::Clip,
            &offer_files_s2c,
            &staging_root,
            &r.dir.join("downloads-s2c"),
            2048,
        )
        .unwrap();

        let mut cli_xfer_rx = c.transfer_tx.subscribe();
        let mut srv_dg_rx_s2c = r.srv.mouse_delta_tx.subscribe();
        let epoch_s2c = std::time::Instant::now();
        let (stop_tx_s2c, mut stop_rx_s2c) = oneshot::channel::<()>();
        let (rx_stop_tx_s2c, mut rx_stop_rx_s2c) = oneshot::channel::<()>();

        let dg_conn_s2c = c.conn.clone();
        let sender_task_s2c = tokio::spawn(async move {
            let mut tick = tokio::time::interval(Duration::from_millis(4));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut seq = 0u32;
            loop {
                tokio::select! {
                    biased;
                    _ = &mut stop_rx_s2c => break seq,
                    _ = tick.tick() => {
                        let ts_us = epoch_s2c.elapsed().as_micros() as u64;
                        let delta = UdpMouseDelta {
                            dx: (ts_us & 0xFFFF_FFFF) as i32,
                            dy: (ts_us >> 32) as i32,
                            seq,
                        };
                        let _ = dg_conn_s2c.send_datagram(bytes::Bytes::copy_from_slice(&encode_delta(&delta)));
                        seq = seq.wrapping_add(1);
                    }
                }
            }
        });

        let recv_task_s2c = tokio::spawn(async move {
            let mut latencies_us = Vec::<u64>::new();
            loop {
                tokio::select! {
                    biased;
                    _ = &mut rx_stop_rx_s2c => break latencies_us,
                    res = srv_dg_rx_s2c.recv() => {
                        match res {
                            Ok(d) => {
                                let sent_us = ((d.dy as u32 as u64) << 32) | (d.dx as u32 as u64);
                                let now_us = epoch_s2c.elapsed().as_micros() as u64;
                                latencies_us.push(now_us.saturating_sub(sent_us));
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                            Err(_) => break latencies_us,
                        }
                    }
                }
            }
        });

        let t0_s2c = std::time::Instant::now();
        r.srv
            .outgoing_tx
            .send(Outbound::ClipFile {
                offer_id: offer_id_s2c.clone(),
                rel_path: "bulk-300mb.bin".to_string(),
                path: src_file.clone(),
                size,
            })
            .unwrap();

        loop {
            match timeout(Duration::from_secs(60), cli_xfer_rx.recv()).await {
                Ok(Ok(TransferEvent::ClipFile {
                    offer_id: ev_id,
                    size: rx_size,
                    ..
                })) if ev_id == offer_id_s2c => {
                    let (completed, _) = record_file_received(&ev_id, rx_size);
                    assert!(completed.is_some());
                    break;
                }
                Ok(Ok(_)) => continue,
                other => panic!("s2c ClipFile did not complete cleanly: {other:?}"),
            }
        }
        let elapsed_s2c = t0_s2c.elapsed();
        let _ = stop_tx_s2c.send(());
        let sent_count_s2c = sender_task_s2c.await.unwrap();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let _ = rx_stop_tx_s2c.send(());
        let mut latencies_s2c = recv_task_s2c.await.unwrap();
        assert!(
            !latencies_s2c.is_empty(),
            "must receive mouse datagrams during server->client transfer"
        );
        latencies_s2c.sort_unstable();
        let n_s2c = latencies_s2c.len();
        let p50_s2c = latencies_s2c[n_s2c / 2];
        let p95_s2c = latencies_s2c[((n_s2c - 1) * 95) / 100];
        let max_s2c = *latencies_s2c.last().unwrap();
        let mb_per_sec_s2c = (size as f64 / (1024.0 * 1024.0)) / elapsed_s2c.as_secs_f64();
        eprintln!(
            "[r2-bench] s2c 300MB elapsed={elapsed_s2c:?} throughput={mb_per_sec_s2c:.1} MB/s datagrams_sent={sent_count_s2c} recv={n_s2c} p50={p50_s2c}us p95={p95_s2c}us max={max_s2c}us"
        );
        assert!(
            max_s2c < 10_000_000,
            "unexpected >10s datagram stall during s2c: max={max_s2c}us"
        );

        cleanup_staging_entry(&staging_root, &offer_id_s2c);
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn loopback_clip_file_cancels_cleanly_from_both_sides() {
        use crate::kvm::files::{global_cancel_registry, send_clip_file, TRANSFER_CANCELLED_ERR};
        use crate::kvm::protocol::{FileMeta, TransferEvent};
        use crate::kvm::staging::{
            cleanup_staging_entry, default_staging_root, register_transfer_offer, TransferKind,
        };

        let r = rig("clip-cancel-both").await;
        let pin = r.srv.security_pin.lock().unwrap().clone();
        let c = connect(&r, Some(pin)).await.unwrap();
        let mut srv_pkt_rx = r.srv.incoming_tx.subscribe();
        let mut srv_trx = r.srv.transfer_tx.subscribe();
        let mut cli_trx = c.transfer_tx.subscribe();

        let size: u64 = 128 * 1024 * 1024;
        let src_file = r.dir.join("clip-big.bin");
        {
            let f = std::fs::File::create(&src_file).unwrap();
            f.set_len(size).unwrap();
        }
        let staging_root = default_staging_root();

        // Phase 1: Receiver-initiated cancel mid-way.
        let id_rx = format!("clip-rx-cancel-{}", std::process::id());
        let offer_files = vec![FileMeta {
            rel_path: "clip-big.bin".into(),
            size,
            is_dir: false,
        }];
        c.send_packet(KvmPacket::FileClipOffer {
            offer_id: id_rx.clone(),
            files: offer_files.clone(),
        })
        .await
        .unwrap();
        let (rx_id, rx_files) = loop {
            match timeout(Duration::from_secs(3), srv_pkt_rx.recv()).await {
                Ok(Ok(KvmPacket::FileClipOffer { offer_id, files })) if offer_id == id_rx => {
                    break (offer_id, files)
                }
                Ok(Ok(_)) => continue,
                other => panic!("expected FileClipOffer, got {other:?}"),
            }
        };
        let plan_rx = register_transfer_offer(
            &rx_id,
            TransferKind::Clip,
            &rx_files,
            &staging_root,
            &r.dir.join("downloads"),
            2048,
        )
        .unwrap();

        let conn_clone = c.conn.clone();
        let id_send1 = id_rx.clone();
        let src1 = src_file.clone();
        let send_task1 = tokio::spawn(async move {
            send_clip_file(
                &conn_clone,
                id_send1,
                "clip-big.bin".to_string(),
                &src1,
                size,
            )
            .await
        });

        // Wait for server receiver progress event.
        loop {
            match timeout(Duration::from_secs(5), srv_trx.recv()).await {
                Ok(Ok(TransferEvent::Progress { id, state, .. })) if id == id_rx => {
                    assert_eq!(state, "active");
                    break;
                }
                Ok(Ok(TransferEvent::ClipFile { offer_id, .. })) if offer_id == id_rx => {
                    panic!("transfer finished before receiver cancel triggered");
                }
                Ok(Ok(_)) => continue,
                other => panic!("expected Progress on receiver before cancel, got {other:?}"),
            }
        }

        // Receiver cancels locally + sends TransferCancel to sender.
        let srv_cancelled = global_cancel_registry().cancel_except_conn(c.conn.stable_id(), &id_rx);
        assert!(
            !srv_cancelled.is_empty(),
            "receiver must have registered active transfer"
        );
        for (_, meta) in &srv_cancelled {
            let _ = r.srv.transfer_tx.send(TransferEvent::Cancelled {
                id: meta.id.clone(),
                kind: meta.kind.clone(),
                name: meta.name.clone(),
                size_bytes: meta.size_bytes,
            });
        }
        r.srv
            .outgoing_tx
            .send(Outbound::Packet(KvmPacket::TransferCancel {
                id: id_rx.clone(),
            }))
            .unwrap();

        let send_res1 = timeout(Duration::from_secs(5), send_task1)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            send_res1.as_ref().map_err(String::as_str),
            Err(TRANSFER_CANCELLED_ERR)
        );

        let mut srv_saw_cancel = false;
        let mut cli_saw_cancel = false;
        for _ in 0..30 {
            if let Ok(Ok(TransferEvent::Cancelled { id, kind, .. })) =
                timeout(Duration::from_millis(200), srv_trx.recv()).await
            {
                if id == id_rx && kind == "clip" {
                    srv_saw_cancel = true;
                }
            }
            if let Ok(Ok(TransferEvent::Cancelled { id, kind, .. })) =
                timeout(Duration::from_millis(200), cli_trx.recv()).await
            {
                if id == id_rx && kind == "clip" {
                    cli_saw_cancel = true;
                }
            }
            if srv_saw_cancel && cli_saw_cancel {
                break;
            }
        }
        assert!(
            srv_saw_cancel && cli_saw_cancel,
            "both sides must see cancelled (srv={srv_saw_cancel}, cli={cli_saw_cancel})"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !plan_rx.base_dir.exists(),
            "cancelled clip staging dir + .part must be removed"
        );

        // Phase 2: Sender-initiated cancel mid-way.
        let id_tx = format!("clip-tx-cancel-{}", std::process::id());
        c.send_packet(KvmPacket::FileClipOffer {
            offer_id: id_tx.clone(),
            files: offer_files,
        })
        .await
        .unwrap();
        let (rx_id2, rx_files2) = loop {
            match timeout(Duration::from_secs(3), srv_pkt_rx.recv()).await {
                Ok(Ok(KvmPacket::FileClipOffer { offer_id, files })) if offer_id == id_tx => {
                    break (offer_id, files)
                }
                Ok(Ok(_)) => continue,
                other => panic!("expected second FileClipOffer, got {other:?}"),
            }
        };
        let plan_tx = register_transfer_offer(
            &rx_id2,
            TransferKind::Clip,
            &rx_files2,
            &staging_root,
            &r.dir.join("downloads"),
            2048,
        )
        .unwrap();

        let conn_clone2 = c.conn.clone();
        let id_send2 = id_tx.clone();
        let src2 = src_file.clone();
        let send_task2 = tokio::spawn(async move {
            send_clip_file(
                &conn_clone2,
                id_send2,
                "clip-big.bin".to_string(),
                &src2,
                size,
            )
            .await
        });

        loop {
            match timeout(Duration::from_secs(5), srv_trx.recv()).await {
                Ok(Ok(TransferEvent::Progress { id, .. })) if id == id_tx => break,
                Ok(Ok(TransferEvent::ClipFile { offer_id, .. })) if offer_id == id_tx => {
                    panic!("transfer finished before sender cancel triggered");
                }
                Ok(Ok(_)) => continue,
                other => panic!("expected Progress before sender cancel, got {other:?}"),
            }
        }

        // Sender cancels locally + sends TransferCancel to receiver.
        if let Some(meta) = global_cancel_registry().cancel_for_conn(c.conn.stable_id(), &id_tx) {
            let _ = c.transfer_tx.send(TransferEvent::Cancelled {
                id: meta.id,
                kind: meta.kind,
                name: meta.name,
                size_bytes: meta.size_bytes,
            });
        }
        c.send_packet(KvmPacket::TransferCancel { id: id_tx.clone() })
            .await
            .unwrap();

        let send_res2 = timeout(Duration::from_secs(5), send_task2)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            send_res2.as_ref().map_err(String::as_str),
            Err(TRANSFER_CANCELLED_ERR)
        );

        let mut srv_saw_cancel2 = false;
        let mut cli_saw_cancel2 = false;
        for _ in 0..30 {
            if let Ok(Ok(TransferEvent::Cancelled { id, kind, .. })) =
                timeout(Duration::from_millis(200), srv_trx.recv()).await
            {
                if id == id_tx && kind == "clip" {
                    srv_saw_cancel2 = true;
                }
            }
            if let Ok(Ok(TransferEvent::Cancelled { id, kind, .. })) =
                timeout(Duration::from_millis(200), cli_trx.recv()).await
            {
                if id == id_tx && kind == "clip" {
                    cli_saw_cancel2 = true;
                }
            }
            if srv_saw_cancel2 && cli_saw_cancel2 {
                break;
            }
        }
        assert!(srv_saw_cancel2 && cli_saw_cancel2, "both sides must see cancelled in phase 2 (srv={srv_saw_cancel2}, cli={cli_saw_cancel2})");
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(
            !plan_tx.base_dir.exists(),
            "cancelled phase 2 staging dir + .part must be removed"
        );

        cleanup_staging_entry(&staging_root, &id_rx);
        cleanup_staging_entry(&staging_root, &id_tx);
        let _ = std::fs::remove_dir_all(&r.dir);
    }
    // --- end SPEC-25a ---
}
