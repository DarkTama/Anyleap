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
    #[serde(default)]
    pub monitors: Vec<MonitorDisplay>,
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
    /// First message of every session. Unpaired peers must supply the PIN;
    /// trusted peers (known certificate fingerprint) send an empty PIN.
    Pair {
        computer_id: String,
        name: String,
        pin: String,
        #[serde(default)]
        monitors: Vec<MonitorDisplay>,
    },
    PairAck {
        accepted: bool,
        computer_id: String,
        name: String,
        #[serde(default)]
        monitors: Vec<MonitorDisplay>,
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
        #[serde(default)]
        dwell_ms: u32,
        #[serde(default)]
        deadzone_px: i32,
    },
    CursorReturn {
        edge: String,
        relative_y: f32,
    },
    ClipboardText {
        text: String,
    },
    CursorPosition {
        norm_x: f32,
        norm_y: f32,
    },
    EmergencyRelease,
    ReleaseAll,
    SessionDisconnect,
    /// Internal server notification emitted when a single peer connection closes.
    PeerDisconnected {
        computer_id: String,
    },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct UdpMouseDelta {
    pub dx: i32,
    pub dy: i32,
    pub seq: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LinkedPeerInfo {
    pub computer_id: String,
    pub name: String,
    pub role: String, // "main" | "linked"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TrustedPeerInfo {
    pub computer_id: String,
    pub name: String,
    pub address: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct KvmStatus {
    pub is_host_running: bool,
    pub port: u16,
    pub security_pin: String,
    pub connected_peers: Vec<String>,
    pub is_input_trapped: bool,
    pub active_target_id: Option<String>,
    pub local_name: String,
    pub local_id: String,
    pub screen_width: i32,
    pub screen_height: i32,
    pub monitors: Vec<MonitorDisplay>,
    #[serde(default)]
    pub peers: Vec<ComputerNode>,
    /// Edge of this PC that leaps to the peer while this PC is the active controller.
    #[serde(default)]
    pub controller_edge: Option<String>,
    /// True while a remote controller currently drives this PC's cursor.
    #[serde(default)]
    pub receiver_entered: bool,
    /// True while this PC allows other PCs to control it (receiver running).
    #[serde(default)]
    pub allow_control: bool,
    /// Single active link and this PC's role in it; None when unlinked.
    #[serde(default)]
    pub linked_peer: Option<LinkedPeerInfo>,
}

/// Outbound traffic: control packets or a clipboard image (own uni stream).
#[derive(Debug, Clone)]
pub enum Outbound {
    Packet(KvmPacket),
    Image { format: String, data: Vec<u8> },
}

/// Bulk transfers completed on a unidirectional stream.
#[derive(Debug, Clone)]
pub enum TransferEvent {
    File { name: String, size: u64 },
    ClipboardImage { data: Vec<u8> },
}

/// Header at the start of every unidirectional stream (raw bytes follow).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamHeader {
    File { name: String, size: u64 },
    ClipboardImage { format: String },
}

pub const MAX_FRAME: usize = 16 * 1024 * 1024;

/// `u32` big-endian length prefix + JSON.
pub async fn write_frame<W, T>(w: &mut W, msg: &T) -> std::io::Result<()>
where
    W: tokio::io::AsyncWrite + Unpin,
    T: Serialize,
{
    use tokio::io::AsyncWriteExt;
    let body = serde_json::to_vec(msg).map_err(std::io::Error::other)?;
    if body.len() > MAX_FRAME {
        return Err(std::io::Error::other("frame too large"));
    }
    w.write_all(&(body.len() as u32).to_be_bytes()).await?;
    w.write_all(&body).await
}

/// Returns `Ok(None)` on clean EOF before a frame starts.
pub async fn read_frame<R, T>(r: &mut R) -> std::io::Result<Option<T>>
where
    R: tokio::io::AsyncRead + Unpin,
    T: serde::de::DeserializeOwned,
{
    read_frame_capped(r, MAX_FRAME).await
}

/// Like `read_frame` but rejects frames over `max` bytes before allocating.
pub async fn read_frame_capped<R, T>(r: &mut R, max: usize) -> std::io::Result<Option<T>>
where
    R: tokio::io::AsyncRead + Unpin,
    T: serde::de::DeserializeOwned,
{
    use tokio::io::AsyncReadExt;
    let mut len = [0u8; 4];
    match r.read_exact(&mut len).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes(len) as usize;
    if len > max {
        return Err(std::io::Error::other("frame too large"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    serde_json::from_slice(&body).map(Some).map_err(std::io::Error::other)
}

/// Mouse delta datagram: `dx: i32, dy: i32, seq: u32`, little-endian.
pub fn encode_delta(d: &UdpMouseDelta) -> [u8; 12] {
    let mut b = [0u8; 12];
    b[0..4].copy_from_slice(&d.dx.to_le_bytes());
    b[4..8].copy_from_slice(&d.dy.to_le_bytes());
    b[8..12].copy_from_slice(&d.seq.to_le_bytes());
    b
}

pub fn decode_delta(b: &[u8]) -> Option<UdpMouseDelta> {
    if b.len() != 12 {
        return None;
    }
    Some(UdpMouseDelta {
        dx: i32::from_le_bytes(b[0..4].try_into().ok()?),
        dy: i32::from_le_bytes(b[4..8].try_into().ok()?),
        seq: u32::from_le_bytes(b[8..12].try_into().ok()?),
    })
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
    }

    #[test]
    fn test_cursor_enter_defaults_and_roundtrip() {
        let legacy = r#"{"type":"CursorEnter","payload":{"edge":"left","relative_y":0.5}}"#;
        let decoded: KvmPacket = serde_json::from_str(legacy).expect("deserialize legacy");
        assert_eq!(
            decoded,
            KvmPacket::CursorEnter {
                edge: "left".into(),
                relative_y: 0.5,
                dwell_ms: 0,
                deadzone_px: 0,
            }
        );

        let pkt = KvmPacket::CursorEnter {
            edge: "right".into(),
            relative_y: 0.25,
            dwell_ms: 120,
            deadzone_px: 48,
        };
        let json = serde_json::to_string(&pkt).expect("serialize");
        let rt: KvmPacket = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(pkt, rt);
    }

    #[test]
    fn test_delta_datagram_roundtrip() {
        let delta = UdpMouseDelta { dx: 14, dy: -8, seq: 102 };
        let bytes = encode_delta(&delta);
        assert_eq!(bytes.len(), 12);
        assert_eq!(decode_delta(&bytes), Some(delta));
        assert_eq!(decode_delta(&bytes[..11]), None);
    }

    #[tokio::test]
    async fn test_frame_roundtrip() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let pkt = KvmPacket::ClipboardText { text: "héllo".into() };
        write_frame(&mut a, &pkt).await.unwrap();
        write_frame(&mut a, &StreamHeader::File { name: "x.txt".into(), size: 3 }).await.unwrap();
        drop(a);
        assert_eq!(read_frame::<_, KvmPacket>(&mut b).await.unwrap(), Some(pkt));
        assert_eq!(
            read_frame::<_, StreamHeader>(&mut b).await.unwrap(),
            Some(StreamHeader::File { name: "x.txt".into(), size: 3 })
        );
        assert!(read_frame::<_, KvmPacket>(&mut b).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_oversized_frame_rejected() {
        let (mut a, mut b) = tokio::io::duplex(64);
        tokio::io::AsyncWriteExt::write_all(&mut a, &u32::MAX.to_be_bytes()).await.unwrap();
        assert!(read_frame::<_, KvmPacket>(&mut b).await.is_err());
    }
}
