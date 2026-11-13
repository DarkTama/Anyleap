use super::files::receive;
use super::identity::{client_config, peer_fingerprint, Identity, TrustStore, TrustedPeer};
use super::protocol::{encode_delta, read_frame, write_frame, KvmPacket, TransferEvent, UdpMouseDelta};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, mpsc};
use tokio::time::timeout;

pub struct KvmClientHandle {
    pub target_addr: String,
    pub is_connected: Arc<AtomicBool>,
    pub send_packet_tx: mpsc::Sender<KvmPacket>,
    pub incoming_tx: broadcast::Sender<KvmPacket>,
    pub transfer_tx: broadcast::Sender<TransferEvent>,
    pub conn: quinn::Connection,
    pub delta_seq: Arc<AtomicU32>,
    _endpoint: quinn::Endpoint,
}

pub fn send_delta(conn: &quinn::Connection, d: &UdpMouseDelta) {
    let _ = conn.send_datagram(bytes::Bytes::copy_from_slice(&encode_delta(d)));
}

/// Connect, present our certificate, and complete the `Pair`/`PairAck` exchange.
/// `pin = Some(..)` pairs (and pins the server fingerprint); `None` requires a
/// previously paired peer whose fingerprint must still match.
pub async fn handshake(
    ip: &str,
    port: u16,
    local_id: String,
    local_name: String,
    pin: Option<String>,
    identity: &Identity,
    trust: &TrustStore,
) -> Result<(quinn::Endpoint, quinn::Connection, quinn::SendStream, quinn::RecvStream), String> {
    let stored = trust.by_address(ip);
    if stored.is_none() && pin.is_none() {
        return Err("Peer not paired — enter its PIN first".into());
    }
    let addr: SocketAddr = format!("{ip}:{port}").parse().map_err(|e| format!("Invalid peer address: {e}"))?;
    let mut endpoint = quinn::Endpoint::client(([0, 0, 0, 0], 0).into()).map_err(|e| e.to_string())?;
    endpoint.set_default_client_config(client_config(identity)?);

    let connecting = endpoint.connect(addr, "anyleap-kvm").map_err(|e| e.to_string())?;
    let conn = timeout(Duration::from_secs(5), connecting)
        .await
        .map_err(|_| format!("Connection to {addr} timed out"))?
        .map_err(|e| format!("Failed to connect to peer at {addr}: {e}"))?;

    let fp = peer_fingerprint(&conn).ok_or("Peer presented no certificate")?;
    if pin.is_none() && stored.is_some_and(|s| s.fingerprint != fp) {
        conn.close(4u32.into(), b"identity changed");
        return Err("Peer identity changed — re-pair".into());
    }

    let (mut send, mut recv) = conn.open_bi().await.map_err(|e| e.to_string())?;
    let hello = KvmPacket::Pair { computer_id: local_id, name: local_name, pin: pin.clone().unwrap_or_default() };
    write_frame(&mut send, &hello).await.map_err(|e| e.to_string())?;
    let ack = timeout(Duration::from_secs(10), read_frame::<_, KvmPacket>(&mut recv)).await;
    match ack {
        Ok(Ok(Some(KvmPacket::PairAck { accepted: true, computer_id, name }))) => {
            if pin.is_some() {
                trust.add(TrustedPeer { computer_id, name, fingerprint: fp, address: ip.to_string() });
            }
            Ok((endpoint, conn, send, recv))
        }
        Ok(Ok(Some(KvmPacket::PairAck { .. }))) => Err("Incorrect PIN".into()),
        _ => Err("Peer closed the connection during pairing".into()),
    }
}

impl KvmClientHandle {
    pub async fn connect(
        ip: &str,
        port: u16,
        local_id: String,
        local_name: String,
        pin: Option<String>,
        identity: &Identity,
        trust: &TrustStore,
        download_dir: PathBuf,
    ) -> Result<Self, String> {
        let (endpoint, conn, mut send, mut recv) =
            handshake(ip, port, local_id, local_name, pin, identity, trust).await?;

        let (send_packet_tx, mut send_packet_rx) = mpsc::channel::<KvmPacket>(512);
        let (incoming_tx, _) = broadcast::channel::<KvmPacket>(512);
        let (transfer_tx, _) = broadcast::channel::<TransferEvent>(64);
        let is_connected = Arc::new(AtomicBool::new(true));

        let out_connected = Arc::clone(&is_connected);
        tokio::spawn(async move {
            while let Some(packet) = send_packet_rx.recv().await {
                if write_frame(&mut send, &packet).await.is_err() {
                    out_connected.store(false, Ordering::Relaxed);
                    break;
                }
            }
        });

        let (in_tx, in_connected) = (incoming_tx.clone(), Arc::clone(&is_connected));
        tokio::spawn(async move {
            while let Ok(Some(packet)) = read_frame::<_, KvmPacket>(&mut recv).await {
                let _ = in_tx.send(packet);
            }
            in_connected.store(false, Ordering::Relaxed);
            let _ = in_tx.send(KvmPacket::SessionDisconnect);
        });

        let (uni_conn, tx) = (conn.clone(), transfer_tx.clone());
        tokio::spawn(async move {
            while let Ok(mut s) = uni_conn.accept_uni().await {
                let (tx, dir) = (tx.clone(), download_dir.clone());
                tokio::spawn(async move {
                    match receive(&mut s, &dir).await {
                        Ok(ev) => {
                            let _ = tx.send(ev);
                        }
                        Err(e) => eprintln!("[KVM-Client] transfer rejected: {e}"),
                    }
                });
            }
        });

        Ok(Self {
            target_addr: ip.to_string(),
            is_connected,
            send_packet_tx,
            incoming_tx,
            transfer_tx,
            conn,
            delta_seq: Arc::new(AtomicU32::new(0)),
            _endpoint: endpoint,
        })
    }

    pub async fn send_packet(&self, packet: KvmPacket) -> Result<(), String> {
        self.send_packet_tx.send(packet).await.map_err(|e| format!("Send failed: {e}"))
    }

    pub fn send_mouse_delta(&self, dx: i32, dy: i32) {
        let seq = self.delta_seq.fetch_add(1, Ordering::Relaxed);
        send_delta(&self.conn, &UdpMouseDelta { dx, dy, seq });
    }
}
