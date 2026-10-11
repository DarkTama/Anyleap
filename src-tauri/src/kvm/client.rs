use super::protocol::{KvmPacket, UdpMouseDelta};
use futures_util::{SinkExt, StreamExt};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::protocol::Message;

pub struct KvmClientHandle {
    pub target_addr: String,
    pub is_connected: Arc<AtomicBool>,
    pub send_packet_tx: mpsc::Sender<KvmPacket>,
    pub incoming_tx: broadcast::Sender<KvmPacket>,
    pub udp_socket: Arc<UdpSocket>,
    pub target_udp_addr: SocketAddr,
    pub delta_seq: Arc<AtomicU32>,
}

impl KvmClientHandle {
    pub async fn connect(
        target_ip: &str,
        tcp_port: u16,
        udp_port: u16,
        local_id: String,
        local_name: String,
    ) -> Result<Self, String> {
        let ws_url = format!("ws://{target_ip}:{tcp_port}/ws");
        let (ws_stream, _) = connect_async(&ws_url)
            .await
            .map_err(|e| format!("Failed to connect to peer at {ws_url}: {e}"))?;

        let (mut ws_sender, mut ws_receiver) = ws_stream.split();
        let (send_packet_tx, mut send_packet_rx) = mpsc::channel::<KvmPacket>(512);
        let (incoming_tx, _) = broadcast::channel::<KvmPacket>(512);
        let is_connected = Arc::new(AtomicBool::new(true));

        // UDP socket for mouse delta streaming
        let udp_socket = UdpSocket::bind("0.0.0.0:0")
            .await
            .map_err(|e| format!("Failed to bind client UDP socket: {e}"))?;
        let target_udp_addr: SocketAddr = format!("{target_ip}:{udp_port}")
            .parse()
            .map_err(|e| format!("Invalid target UDP addr: {e}"))?;

        // Send handshake packet immediately
        let handshake = KvmPacket::Handshake {
            computer_id: local_id,
            name: local_name,
            pin: String::new(),
        };
        let _ = send_packet_tx.send(handshake).await;

        // Outbound WS task
        let out_connected = Arc::clone(&is_connected);
        tokio::spawn(async move {
            while let Some(packet) = send_packet_rx.recv().await {
                if let Ok(text) = serde_json::to_string(&packet) {
                    if ws_sender.send(Message::Text(text.into())).await.is_err() {
                        out_connected.store(false, Ordering::Relaxed);
                        break;
                    }
                }
            }
        });

        // Inbound WS task
        let in_tx = incoming_tx.clone();
        let in_connected = Arc::clone(&is_connected);
        tokio::spawn(async move {
            while let Some(Ok(msg)) = ws_receiver.next().await {
                if let Message::Text(text) = msg {
                    if let Ok(packet) = serde_json::from_str::<KvmPacket>(&text) {
                        let _ = in_tx.send(packet);
                    }
                }
            }
            in_connected.store(false, Ordering::Relaxed);
            let _ = in_tx.send(KvmPacket::SessionDisconnect);
        });

        Ok(Self {
            target_addr: target_ip.to_string(),
            is_connected,
            send_packet_tx,
            incoming_tx,
            udp_socket: Arc::new(udp_socket),
            target_udp_addr,
            delta_seq: Arc::new(AtomicU32::new(0)),
        })
    }

    pub async fn send_packet(&self, packet: KvmPacket) -> Result<(), String> {
        self.send_packet_tx
            .send(packet)
            .await
            .map_err(|e| format!("Send failed: {e}"))
    }

    pub async fn send_mouse_delta(&self, dx: i32, dy: i32) {
        let seq = self.delta_seq.fetch_add(1, Ordering::Relaxed);
        let delta = UdpMouseDelta { dx, dy, seq };
        if let Ok(bytes) = serde_json::to_vec(&delta) {
            let _ = self.udp_socket.send_to(&bytes, self.target_udp_addr).await;
        }
    }
}
