use super::protocol::{KvmPacket, UdpMouseDelta};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        State,
    },
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use tokio::net::UdpSocket;
use tokio::sync::{broadcast, watch};

#[derive(Clone)]
pub struct ServerContext {
    pub security_pin: Arc<Mutex<String>>,
    pub is_running: Arc<AtomicBool>,
    pub incoming_tx: broadcast::Sender<KvmPacket>,
    pub outgoing_tx: broadcast::Sender<KvmPacket>,
    pub mouse_delta_tx: broadcast::Sender<UdpMouseDelta>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PairRequest {
    pub computer_id: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PairResponse {
    pub success: bool,
    pub pin_required: bool,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfirmPinRequest {
    pub computer_id: String,
    pub pin: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConfirmPinResponse {
    pub success: bool,
    pub session_token: Option<String>,
    pub message: String,
}

pub struct KvmServerHandle {
    pub tcp_port: u16,
    pub udp_port: u16,
    pub security_pin: String,
    pub is_running: Arc<AtomicBool>,
    pub incoming_tx: broadcast::Sender<KvmPacket>,
    pub outgoing_tx: broadcast::Sender<KvmPacket>,
    pub mouse_delta_tx: broadcast::Sender<UdpMouseDelta>,
    pub shutdown_tx: watch::Sender<bool>,
}

pub async fn start_kvm_server(
    tcp_port: u16,
    udp_port: u16,
) -> Result<KvmServerHandle, String> {
    let (incoming_tx, _) = broadcast::channel(512);
    let (outgoing_tx, _) = broadcast::channel(512);
    let (mouse_delta_tx, _) = broadcast::channel(1024);
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    let pin_num = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_micros()
        % 900_000)
        + 100_000;
    let pin = format!("{:06}", pin_num);
    let security_pin = Arc::new(Mutex::new(pin.clone()));
    let is_running = Arc::new(AtomicBool::new(true));

    let ctx = ServerContext {
        security_pin: Arc::clone(&security_pin),
        is_running: Arc::clone(&is_running),
        incoming_tx: incoming_tx.clone(),
        outgoing_tx: outgoing_tx.clone(),
        mouse_delta_tx: mouse_delta_tx.clone(),
    };

    let router = Router::new()
        .route("/ws", get(ws_handler))
        .route("/pair", post(pair_handler))
        .route("/pair/confirm", post(confirm_handler))
        .with_state(ctx.clone());

    let tcp_addr: SocketAddr = format!("0.0.0.0:{tcp_port}")
        .parse()
        .map_err(|e| format!("Invalid TCP addr: {e}"))?;

    // Create reuseable TCP listener on Windows to prevent WSAEADDRINUSE 10048
    let socket = if tcp_addr.is_ipv4() {
        tokio::net::TcpSocket::new_v4().map_err(|e| format!("Socket error: {e}"))?
    } else {
        tokio::net::TcpSocket::new_v6().map_err(|e| format!("Socket error: {e}"))?
    };
    let _ = socket.set_reuseaddr(true);
    socket.bind(tcp_addr).map_err(|e| format!("Failed to bind TCP port {tcp_port}: {e}"))?;
    let tcp_listener = socket.listen(128).map_err(|e| format!("Listen error: {e}"))?;

    // Spawn Axum TCP/WSS Server with graceful shutdown
    tokio::spawn(async move {
        let server = axum::serve(tcp_listener, router).with_graceful_shutdown(async move {
            let _ = shutdown_rx.changed().await;
        });
        if let Err(e) = server.await {
            eprintln!("[KVM-Server] TCP server stopped: {e}");
        }
    });

    // Spawn UDP Mouse Delta Receiver on port 4178
    let udp_socket = UdpSocket::bind(format!("0.0.0.0:{udp_port}"))
        .await
        .map_err(|e| format!("Failed to bind UDP port {udp_port}: {e}"))?;
    let udp_delta_tx = mouse_delta_tx.clone();
    let udp_running = Arc::clone(&is_running);

    tokio::spawn(async move {
        let mut buf = [0u8; 256];
        while udp_running.load(Ordering::Relaxed) {
            if let Ok((len, _)) = udp_socket.recv_from(&mut buf).await {
                if let Ok(delta) = serde_json::from_slice::<UdpMouseDelta>(&buf[..len]) {
                    let _ = udp_delta_tx.send(delta);
                }
            }
        }
    });

    Ok(KvmServerHandle {
        tcp_port,
        udp_port,
        security_pin: pin,
        is_running,
        incoming_tx,
        outgoing_tx,
        mouse_delta_tx,
        shutdown_tx,
    })
}

async fn pair_handler(
    State(_ctx): State<ServerContext>,
    Json(_req): Json<PairRequest>,
) -> impl IntoResponse {
    Json(PairResponse {
        success: true,
        pin_required: true,
        message: "PIN verification required".to_string(),
    })
}

async fn confirm_handler(
    State(ctx): State<ServerContext>,
    Json(req): Json<ConfirmPinRequest>,
) -> impl IntoResponse {
    let pin = ctx.security_pin.lock().unwrap();
    if req.pin == *pin {
        let session_token = uuid::Uuid::new_v4().to_string();
        Json(ConfirmPinResponse {
            success: true,
            session_token: Some(session_token),
            message: "PIN verified successfully".to_string(),
        })
    } else {
        Json(ConfirmPinResponse {
            success: false,
            session_token: None,
            message: "Invalid security PIN".to_string(),
        })
    }
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(ctx): State<ServerContext>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws_socket(socket, ctx))
}

async fn handle_ws_socket(socket: WebSocket, ctx: ServerContext) {
    let (mut sender, mut receiver) = socket.split();
    let incoming_tx = ctx.incoming_tx.clone();
    let mut outgoing_rx = ctx.outgoing_tx.subscribe();

    // Outgoing forwarder: send internal KvmPackets -> remote WS client
    let send_task = tokio::spawn(async move {
        while let Ok(packet) = outgoing_rx.recv().await {
            if let Ok(text) = serde_json::to_string(&packet) {
                if sender.send(Message::Text(text.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // Incoming receiver: receive WS client packets -> broadcast internally
    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(text) = msg {
            if let Ok(packet) = serde_json::from_str::<KvmPacket>(&text) {
                let _ = incoming_tx.send(packet);
            }
        }
    }

    // Socket closed / crashed / abrupt disconnect: emit SessionDisconnect automatically!
    let _ = incoming_tx.send(KvmPacket::SessionDisconnect);
    send_task.abort();
}
