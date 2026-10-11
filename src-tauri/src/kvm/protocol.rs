use super::monitors::MonitorDisplay;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ComputerNode {
    pub id: String,
    pub name: String,
    pub is_local: bool,
    pub address: Option<String>,
    pub bounds: ScreenRect,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TopologyLink {
    pub from_id: String,
    pub from_edge: String, // "left", "right", "top", "bottom"
    pub to_id: String,
    pub to_edge: String,
    #[serde(default)]
    pub offset_x: Option<i32>,
    #[serde(default)]
    pub offset_y: Option<i32>,
    #[serde(default)]
    pub deadzone_px: Option<i32>,
    #[serde(default)]
    pub resistance_ms: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct TopologyConfig {
    pub computers: Vec<ComputerNode>,
    pub links: Vec<TopologyLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "payload")]
pub enum KvmPacket {
    Handshake {
        computer_id: String,
        name: String,
        pin: String,
    },
    HandshakeAck {
        accepted: bool,
        session_token: String,
        reason: Option<String>,
    },
    Heartbeat {
        timestamp: u64,
    },
    MouseButton {
        button: u8, // 1 = Left, 2 = Right, 3 = Middle
        is_down: bool,
    },
    MouseWheel {
        delta: i32,
    },
    KeyEvent {
        key_code: u16,
        scan_code: u16,
        is_down: bool,
        extended: bool,
    },
    CursorEnter {
        edge: String,
        relative_y: f32, // 0.0 .. 1.0 along the boundary
    },
    CursorReturn {
        edge: String,
        relative_y: f32,
    },
    ClipboardText {
        text: String,
    },
    ClipboardImage {
        format: String, // "png" or "jpeg"
        data: Vec<u8>,
    },
    FileChunk {
        file_id: String,
        name: String,
        size: u64,
        offset: u64,
        data: Vec<u8>,
        is_last: bool,
    },
    CursorPosition {
        norm_x: f32,
        norm_y: f32,
    },
    EmergencyRelease,
    SessionDisconnect,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct UdpMouseDelta {
    pub dx: i32,
    pub dy: i32,
    pub seq: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct KvmStatus {
    pub is_host_running: bool,
    pub tcp_port: u16,
    pub udp_port: u16,
    pub security_pin: String,
    pub connected_peers: Vec<String>,
    pub is_input_trapped: bool,
    pub active_target_id: Option<String>,
    pub local_name: String,
    pub local_id: String,
    pub screen_width: i32,
    pub screen_height: i32,
    pub monitors: Vec<MonitorDisplay>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kvm_packet_serialization() {
        let packet = KvmPacket::KeyEvent {
            key_code: 65,
            scan_code: 30,
            is_down: true,
            extended: false,
        };
        let json = serde_json::to_string(&packet).expect("serialize");
        let decoded: KvmPacket = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(packet, decoded);

        let img_pkt = KvmPacket::ClipboardImage {
            format: "png".to_string(),
            data: vec![137, 80, 78, 71],
        };
        let img_json = serde_json::to_string(&img_pkt).expect("serialize");
        let img_decoded: KvmPacket = serde_json::from_str(&img_json).expect("deserialize");
        assert_eq!(img_pkt, img_decoded);
    }

    #[test]
    fn test_udp_mouse_delta_serialization() {
        let delta = UdpMouseDelta { dx: 14, dy: -8, seq: 102 };
        let json = serde_json::to_string(&delta).expect("serialize");
        let decoded: UdpMouseDelta = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(delta, decoded);
    }
}
