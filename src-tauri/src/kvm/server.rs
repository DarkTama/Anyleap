use super::files::{receive, send_image};
use super::identity::{ct_eq, peer_fingerprint, server_config, Identity, TrustStore, TrustedPeer};
use super::monitors::MonitorDisplay;
use super::protocol::{decode_delta, read_frame, read_frame_capped, write_frame, KvmPacket, Outbound, TransferEvent, UdpMouseDelta};
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

#[derive(Clone)]
struct ServerContext {
    security_pin: Arc<Mutex<String>>,
    /// Failure count + lockout end; only touched while holding the `security_pin` lock.
    failures: Arc<AtomicU32>,
    locked_until: Arc<Mutex<Option<Instant>>>,
    trust: Arc<TrustStore>,
    local_id: String,
    local_name: String,
    local_monitors: Vec<MonitorDisplay>,
    download_dir: PathBuf,
    incoming_tx: broadcast::Sender<KvmPacket>,
    outgoing_tx: broadcast::Sender<Outbound>,
    mouse_delta_tx: broadcast::Sender<UdpMouseDelta>,
    transfer_tx: broadcast::Sender<TransferEvent>,
    disconnect_tx: broadcast::Sender<Option<String>>,
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
    local_monitors: Vec<MonitorDisplay>,
    download_dir: PathBuf,
) -> Result<KvmServerHandle, String> {
    let (incoming_tx, _) = broadcast::channel(512);
    let (outgoing_tx, _) = broadcast::channel(512);
    let (mouse_delta_tx, _) = broadcast::channel(1024);
    let (transfer_tx, _) = broadcast::channel(64);
    let (disconnect_tx, _) = broadcast::channel(32);
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
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
        local_monitors,
        download_dir,
        incoming_tx: incoming_tx.clone(),
        outgoing_tx: outgoing_tx.clone(),
        mouse_delta_tx: mouse_delta_tx.clone(),
        transfer_tx: transfer_tx.clone(),
        disconnect_tx: disconnect_tx.clone(),
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

    let Ok(Ok((mut send, mut recv))) = timeout(Duration::from_secs(10), conn.accept_bi()).await else {
        conn.close(1u32.into(), b"no control stream");
        return;
    };

    // Authorisation: unknown peers may only send `Pair`; anything else closes the connection.
    let first = match timeout(Duration::from_secs(10), read_frame_capped::<_, KvmPacket>(&mut recv, MAX_HELLO)).await {
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
                monitors: ctx.local_monitors.clone(),
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
            } else if !matches!(other, KvmPacket::PairAck { .. } | KvmPacket::PeerDisconnected { .. }) {
                let _ = ctx.incoming_tx.send(other);
            }
        }
        _ => {
            conn.close(3u32.into(), b"not authorised");
            return;
        }
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
                            let _ = send_image(&out_conn, format, data).await;
                        }
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

        let (uni_conn, transfer_tx, dir) = (conn.clone(), ctx.transfer_tx.clone(), ctx.download_dir.clone());
        tasks.push(tokio::spawn(async move {
            while let Ok(mut s) = uni_conn.accept_uni().await {
                let (tx, dir) = (transfer_tx.clone(), dir.clone());
                tokio::spawn(async move {
                    match receive(&mut s, &dir).await {
                        Ok(ev) => {
                            let _ = tx.send(ev);
                        }
                        Err(e) => eprintln!("[KVM-Server] transfer rejected: {e}"),
                    }
                });
            }
        }));

        let mut disconnected_by_server = false;
        loop {
            tokio::select! {
                res = read_frame::<_, KvmPacket>(&mut recv) => match res {
                    Ok(Some(packet)) => {
                        if matches!(packet, KvmPacket::SessionDisconnect) {
                            break;
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
        let _ = ctx.incoming_tx.send(KvmPacket::PeerDisconnected { computer_id });
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
            default_mons(),
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
    async fn correct_pin_pairs_and_delivers_input_then_reconnects_without_pin() {
        let r = rig("pair").await;
        let mut rx = r.srv.incoming_tx.subscribe();
        let c = connect(&r, Some(r.srv.pin())).await.unwrap();
        assert_eq!(c.peer_monitors, default_mons());
        c.send_packet(key(65)).await.unwrap();
        assert_eq!(next_key(&mut rx).await, Some(65));
        assert_eq!(r.srv_trust.by_address("127.0.0.1").unwrap().computer_id, "cli");
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
            let wrong = if first == "100000" { "100001" } else { "100000" };
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
        let wrong = if first == "100000" { "100001" } else { "100000" };
        for _ in 0..5 {
            assert!(connect(&r, Some(wrong.into())).await.is_err());
        }
        let err = connect(&r, Some(r.srv.pin())).await.err().unwrap();
        assert!(err.contains("Incorrect PIN"), "locked: {err}");
        tokio::time::sleep(PAIR_LOCKOUT + Duration::from_millis(200)).await;
        connect(&r, Some(r.srv.pin())).await.expect("paired after lockout");
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn oversize_hello_closes_connection() {
        let r = rig("bighello").await;
        let mut ep = quinn::Endpoint::client(([0, 0, 0, 0], 0).into()).unwrap();
        ep.set_default_client_config(client_config(&r.cli_id).unwrap());
        let conn = ep.connect(([127, 0, 0, 1], r.srv.port).into(), "anyleap-kvm").unwrap().await.unwrap();
        let (mut send, _recv) = conn.open_bi().await.unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut send, &(1_000_000u32).to_be_bytes()).await.unwrap();
        assert!(timeout(Duration::from_secs(3), conn.closed()).await.is_ok(), "server must close");
        let _ = std::fs::remove_dir_all(&r.dir);
    }

    #[tokio::test]
    async fn unpaired_key_event_closes_connection() {
        let r = rig("unpaired").await;
        let mut rx = r.srv.incoming_tx.subscribe();
        let mut ep = quinn::Endpoint::client(([0, 0, 0, 0], 0).into()).unwrap();
        ep.set_default_client_config(client_config(&r.cli_id).unwrap());
        let conn = ep.connect(([127, 0, 0, 1], r.srv.port).into(), "anyleap-kvm").unwrap().await.unwrap();
        let (mut send, _recv) = conn.open_bi().await.unwrap();
        write_frame(&mut send, &key(65)).await.unwrap();
        assert!(timeout(Duration::from_secs(3), conn.closed()).await.is_ok(), "server must close");
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
            default_mons(),
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
        let (mut rx, mut trx, mut drx) =
            (r.srv.incoming_tx.subscribe(), r.srv.transfer_tx.subscribe(), r.srv.mouse_delta_tx.subscribe());
        let c = connect(&r, Some(r.srv.pin())).await.unwrap();

        let data: Vec<u8> = (0..5 * 1024 * 1024u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect();
        let src = r.dir.join("big.bin");
        std::fs::write(&src, &data).unwrap();
        let conn = c.conn.clone();
        let sender = tokio::spawn(async move { send_file(&conn, &src).await });

        for code in 0..20u16 {
            c.send_packet(key(code)).await.unwrap();
            send_delta(&c.conn, &UdpMouseDelta { dx: 1, dy: 2, seq: code as u32 });
        }
        for code in 0..20u16 {
            assert_eq!(next_key(&mut rx).await, Some(code));
        }
        assert!(timeout(Duration::from_secs(3), drx.recv()).await.is_ok(), "datagram delivered");

        sender.await.unwrap().unwrap();
        match timeout(Duration::from_secs(10), trx.recv()).await.unwrap().unwrap() {
            TransferEvent::File { name, size } => {
                assert_eq!((name.as_str(), size), ("big.bin", data.len() as u64));
            }
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(std::fs::read(r.dir.join("dl").join("big.bin")).unwrap(), data);
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
            if let Ok(KvmPacket::Pair { computer_id, .. }) = timeout(Duration::from_secs(3), srv_in_rx.recv()).await.unwrap() {
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

        let (entry_edge, mut cursor_pos) = match timeout(Duration::from_secs(3), srv_in_rx.recv()).await.unwrap().unwrap() {
            KvmPacket::CursorEnter { edge, relative_y, .. } => {
                let e = Edge::from_str(&edge).unwrap();
                let pos = map_cursor_enter(&srv_mons, e, relative_y);
                (e, pos)
            }
            other => panic!("expected CursorEnter, got {other:?}"),
        };
        assert_eq!(cursor_pos, (2, 540));

        // Controller sends move right, then move left past the entry edge
        c1.send_mouse_delta(10, 0);
        let d1 = timeout(Duration::from_secs(3), srv_delta_rx.recv()).await.unwrap().unwrap();
        match receiver_step(cursor_pos, (d1.dx, d1.dy), &srv_mons, entry_edge) {
            ReceiverStep::Move(p) => cursor_pos = p,
            other => panic!("expected Move, got {other:?}"),
        }
        assert_eq!(cursor_pos, (12, 540));

        c1.send_mouse_delta(-20, 0);
        let d2 = timeout(Duration::from_secs(3), srv_delta_rx.recv()).await.unwrap().unwrap();
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

        match timeout(Duration::from_secs(3), c1_in_rx.recv()).await.unwrap().unwrap() {
            KvmPacket::CursorReturn { edge, relative_y, .. } => {
                assert_eq!(edge, "left");
                assert!((relative_y - 0.5).abs() < 1e-4);
            }
            other => panic!("expected CursorReturn on client, got {other:?}"),
        }

        // ReleaseAll delivered
        c1.send_packet(KvmPacket::ReleaseAll).await.unwrap();
        assert_eq!(
            timeout(Duration::from_secs(3), srv_in_rx.recv()).await.unwrap().unwrap(),
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
        let c2 = connect(&r, None).await.expect("reconnect by fingerprint after IP change");
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

        let dir = std::env::temp_dir().join(format!("anyleap-quic-spec24_seams-{}", std::process::id()));
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
            linked_mons.clone(),
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
}
