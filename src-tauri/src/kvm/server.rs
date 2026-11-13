use super::files::{receive, send_image};
use super::identity::{ct_eq, peer_fingerprint, server_config, Identity, TrustStore, TrustedPeer};
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
    download_dir: PathBuf,
    incoming_tx: broadcast::Sender<KvmPacket>,
    outgoing_tx: broadcast::Sender<Outbound>,
    mouse_delta_tx: broadcast::Sender<UdpMouseDelta>,
    transfer_tx: broadcast::Sender<TransferEvent>,
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
    pub shutdown_tx: watch::Sender<bool>,
}

impl KvmServerHandle {
    pub fn pin(&self) -> String {
        self.security_pin.lock().unwrap().clone()
    }
}

pub async fn start_kvm_server(
    port: u16,
    identity: &Identity,
    trust: Arc<TrustStore>,
    local_id: String,
    local_name: String,
    download_dir: PathBuf,
) -> Result<KvmServerHandle, String> {
    let (incoming_tx, _) = broadcast::channel(512);
    let (outgoing_tx, _) = broadcast::channel(512);
    let (mouse_delta_tx, _) = broadcast::channel(1024);
    let (transfer_tx, _) = broadcast::channel(64);
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
        download_dir,
        incoming_tx: incoming_tx.clone(),
        outgoing_tx: outgoing_tx.clone(),
        mouse_delta_tx: mouse_delta_tx.clone(),
        transfer_tx: transfer_tx.clone(),
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
        shutdown_tx,
    })
}

async fn handle_connection(incoming: quinn::Incoming, ctx: ServerContext) {
    let Ok(conn) = incoming.await else { return };
    let Some(fp) = peer_fingerprint(&conn) else {
        conn.close(1u32.into(), b"no certificate");
        return;
    };
    let trusted = ctx.trust.contains(&fp);

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
    match first {
        KvmPacket::Pair { computer_id, name, pin } => {
            let accepted = trusted || ctx.check_pin(&pin);
            let ack = KvmPacket::PairAck {
                accepted,
                computer_id: ctx.local_id.clone(),
                name: ctx.local_name.clone(),
            };
            let _ = write_frame(&mut send, &ack).await;
            if !accepted {
                let _ = send.finish();
                let _ = timeout(Duration::from_secs(2), send.stopped()).await;
                conn.close(2u32.into(), b"pairing rejected");
                return;
            }
            if !trusted {
                ctx.trust.add(TrustedPeer {
                    computer_id: computer_id.clone(),
                    name: name.clone(),
                    fingerprint: fp,
                    address: conn.remote_address().ip().to_string(),
                });
            }
            let _ = ctx.incoming_tx.send(KvmPacket::Pair { computer_id, name, pin: String::new() });
        }
        other if trusted => {
            let _ = ctx.incoming_tx.send(other);
        }
        _ => {
            conn.close(3u32.into(), b"not authorised");
            return;
        }
    }

    // Authorised session from here on.
    let mut tasks = Vec::new();

    let mut out_rx = ctx.outgoing_tx.subscribe();
    let out_conn = conn.clone();
    tasks.push(tokio::spawn(async move {
        loop {
            match out_rx.recv().await {
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

    while let Ok(Some(packet)) = read_frame::<_, KvmPacket>(&mut recv).await {
        if !matches!(packet, KvmPacket::Pair { .. } | KvmPacket::PairAck { .. }) {
            let _ = ctx.incoming_tx.send(packet);
        }
    }

    // Connection closed / idle timeout: emit SessionDisconnect.
    for t in tasks {
        t.abort();
    }
    conn.close(0u32.into(), b"bye");
    let _ = ctx.incoming_tx.send(KvmPacket::SessionDisconnect);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kvm::client::{handshake, send_delta, KvmClientHandle};
    use crate::kvm::files::send_file;
    use crate::kvm::identity::client_config;

    struct Rig {
        srv: KvmServerHandle,
        srv_trust: Arc<TrustStore>,
        cli_id: Identity,
        cli_trust: TrustStore,
        dir: PathBuf,
    }

    async fn rig(tag: &str) -> Rig {
        let dir = std::env::temp_dir().join(format!("anyleap-quic-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let srv_id = Identity::load_or_create(&dir.join("srv")).unwrap();
        let cli_id = Identity::load_or_create(&dir.join("cli")).unwrap();
        let srv_trust = Arc::new(TrustStore::load(None));
        let srv = start_kvm_server(0, &srv_id, srv_trust.clone(), "srv".into(), "Server".into(), dir.join("dl"))
            .await
            .unwrap();
        Rig { srv, srv_trust, cli_id, cli_trust: TrustStore::load(None), dir }
    }

    fn key(code: u16) -> KvmPacket {
        KvmPacket::KeyEvent { key_code: code, scan_code: 0, is_down: true, extended: false }
    }

    async fn connect(r: &Rig, pin: Option<String>) -> Result<KvmClientHandle, String> {
        KvmClientHandle::connect("127.0.0.1", r.srv.port, "cli".into(), "Client".into(), pin, &r.cli_id, &r.cli_trust, r.dir.join("cli-dl")).await
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
        let other = start_kvm_server(0, &other_id, Arc::new(TrustStore::load(None)), "o".into(), "O".into(), r.dir.join("dl2"))
            .await
            .unwrap();
        let res = handshake("127.0.0.1", other.port, "cli".into(), "Client".into(), None, &r.cli_id, &r.cli_trust).await;
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
}
