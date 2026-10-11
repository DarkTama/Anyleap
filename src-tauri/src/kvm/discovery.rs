use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;

pub const DISCOVERY_PORT: u16 = 4179;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerBeacon {
    pub id: String,
    pub name: String,
    pub tcp_port: u16,
    pub udp_port: u16,
    pub screen_w: i32,
    pub screen_h: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredNode {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub tcp_port: u16,
    pub udp_port: u16,
    pub screen_w: i32,
    pub screen_h: i32,
    pub last_seen_secs: u64,
}

pub struct DiscoveryManager {
    peers: Arc<Mutex<HashMap<String, (PeerBeacon, String, Instant)>>>,
    running: Arc<std::sync::atomic::AtomicBool>,
}

impl DiscoveryManager {
    pub fn new() -> Self {
        Self {
            peers: Arc::new(Mutex::new(HashMap::new())),
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn start(&self, my_id: String, my_name: String, tcp_port: u16, udp_port: u16, screen_w: i32, screen_h: i32) {
        if self.running.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }

        let peers = Arc::clone(&self.peers);
        let running = Arc::clone(&self.running);

        // Broadcast transmitter task
        let bcast_id = my_id.clone();
        let bcast_name = my_name.clone();
        let bcast_running = Arc::clone(&running);
        tokio::spawn(async move {
            let socket = match UdpSocket::bind("0.0.0.0:0").await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[KVM-Discovery] Failed to bind tx socket: {e}");
                    return;
                }
            };
            let _ = socket.set_broadcast(true);
            let target: SocketAddr = format!("255.255.255.255:{DISCOVERY_PORT}").parse().unwrap();

            while bcast_running.load(std::sync::atomic::Ordering::Relaxed) {
                let beacon = PeerBeacon {
                    id: bcast_id.clone(),
                    name: bcast_name.clone(),
                    tcp_port,
                    udp_port,
                    screen_w,
                    screen_h,
                };
                if let Ok(bytes) = serde_json::to_vec(&beacon) {
                    let _ = socket.send_to(&bytes, target).await;
                }
                tokio::time::sleep(Duration::from_millis(2000)).await;
            }
        });

        // Broadcast receiver task
        let rx_my_id = my_id.clone();
        tokio::spawn(async move {
            let socket = match UdpSocket::bind(format!("0.0.0.0:{DISCOVERY_PORT}")).await {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("[KVM-Discovery] Failed to bind rx port {DISCOVERY_PORT}: {e}");
                    return;
                }
            };
            let mut buf = [0u8; 1024];

            while running.load(std::sync::atomic::Ordering::Relaxed) {
                if let Ok((len, addr)) = socket.recv_from(&mut buf).await {
                    if let Ok(beacon) = serde_json::from_slice::<PeerBeacon>(&buf[..len]) {
                        // Only ignore if the beacon ID matches OUR own unique ID
                        if beacon.id != rx_my_id {
                            let mut map = peers.lock().unwrap();
                            map.insert(
                                beacon.id.clone(),
                                (beacon, addr.ip().to_string(), Instant::now()),
                            );
                        }
                    }
                }
            }
        });
    }

    pub fn insert_manual_peer(&self, peer: PeerBeacon, ip: String) {
        let mut map = self.peers.lock().unwrap();
        map.insert(peer.id.clone(), (peer, ip, Instant::now()));
    }

    pub fn list_discovered(&self) -> Vec<DiscoveredNode> {
        let mut map = self.peers.lock().unwrap();
        let now = Instant::now();
        // Evict peers not heard from for 12 seconds
        map.retain(|_, (_, _, last_seen)| now.duration_since(*last_seen).as_secs() < 12);

        map.values()
            .map(|(beacon, ip, last_seen)| DiscoveredNode {
                id: beacon.id.clone(),
                name: beacon.name.clone(),
                ip: ip.clone(),
                tcp_port: beacon.tcp_port,
                udp_port: beacon.udp_port,
                screen_w: beacon.screen_w,
                screen_h: beacon.screen_h,
                last_seen_secs: now.duration_since(*last_seen).as_secs(),
            })
            .collect()
    }
}
