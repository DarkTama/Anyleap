use super::monitors::MonitorDisplay;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScreenRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LinkedSeam {
    pub linked_monitor_id: String,
    pub side: String, // "left" | "right" | "top" | "bottom" (side of the LINKED monitor)
    pub start: i32,   // overlap span start in Linked PC coordinates
    pub end: i32,     // overlap span end in Linked PC coordinates
    pub main_monitor_id: String,
    pub main_offset: i32, // remote_origin_axis - main_mon_axis so along_px = linked_coord + main_offset
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
    #[serde(default)]
    pub remote_origin: Option<Point>,
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
        #[serde(default)]
        target_monitor: Option<String>,
        #[serde(default)]
        along_px: Option<i32>,
    },
    CursorReturn {
        edge: String,
        relative_y: f32,
        #[serde(default)]
        target_monitor: Option<String>,
        #[serde(default)]
        along_px: Option<i32>,
    },
    Seams {
        #[serde(default)]
        seams: Vec<LinkedSeam>,
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
    // --- SPEC-25c ---
    SessionLock,
    SessionSleep,
    OpenUrl {
        url: String,
    },
    OpenFile {
        transfer_id: String,
    },
    TransferCancel {
        id: String,
    },
    // --- end SPEC-25c ---
    // --- SPEC-25b ---
    AudioStart {
        sample_rate: u32,
        channels: u16,
        frame_ms: u16,
        #[serde(default)]
        mute_local: bool,
    },
    AudioStop,
    AudioMuteState {
        state: String,
    },
    AudioActivity {
        active: bool,
    },
    /// Linked -> Main: receiver `SendInput` was rejected (UIPI, elevated foreground window).
    InputBlocked {
        foreground_process: String,
        /// `elevated_foreground` | `secure_desktop` | `sendinput_zero` (empty from older peers).
        #[serde(default)]
        reason: String,
    },
    MediaKey {
        vk: u16,
        is_down: bool,
    },
    // --- /SPEC-25b ---
    // --- SPEC-25a ---
    DragOffer {
        drag_id: String,
        files: Vec<FileMeta>,
        #[serde(default = "default_drag_button")]
        button: u8,
    },
    DragReady {
        drag_id: String,
    },
    DragCancel {
        drag_id: String,
    },
    DragResult {
        drag_id: String,
        outcome: DragOutcome,
        #[serde(default)]
        notice: Option<String>,
    },
    FileClipOffer {
        offer_id: String,
        files: Vec<FileMeta>,
    },
    ClipFetchRequest {
        offer_id: String,
    },
    CursorSchemeOffer {
        roles: Vec<String>,
    },
    // --- end SPEC-25a ---
}

fn default_drag_button() -> u8 {
    1
}

// --- SPEC-25a ---
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DragOutcome {
    Done,
    Failed,
    Cancelled,
}

impl DragOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileMeta {
    pub rel_path: String,
    pub size: u64,
    pub is_dir: bool,
}
// --- end SPEC-25a ---

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
    Image {
        format: String,
        data: Vec<u8>,
    },
    // --- SPEC-25c ---
    OpenFile {
        transfer_id: String,
        path: std::path::PathBuf,
    },
    // --- end SPEC-25c ---
    // --- SPEC-25a ---
    DragFile {
        drag_id: String,
        rel_path: String,
        path: std::path::PathBuf,
        size: u64,
    },
    ClipFile {
        offer_id: String,
        rel_path: String,
        path: std::path::PathBuf,
        size: u64,
    },
    CursorFile {
        role: String,
        name: String,
        data: Vec<u8>,
    },
    // --- end SPEC-25a ---
}

/// Bulk transfers completed on a unidirectional stream.
#[derive(Debug, Clone)]
pub enum TransferEvent {
    File {
        name: String,
        size: u64,
    },
    ClipboardImage {
        data: Vec<u8>,
    },
    // --- SPEC-25c ---
    OpenFile {
        transfer_id: String,
        name: String,
        size: u64,
        saved_path: std::path::PathBuf,
    },
    Progress {
        id: String,
        kind: String,
        name: String,
        pct: u8,
        state: String,
        size_bytes: u64,
    },
    Cancelled {
        id: String,
        kind: String,
        name: String,
        size_bytes: u64,
    },
    Failed {
        id: String,
        kind: String,
        name: String,
        reason: String,
    },
    // --- end SPEC-25c ---
    // --- SPEC-25a ---
    DragFile {
        drag_id: String,
        rel_path: String,
        path: std::path::PathBuf,
        size: u64,
    },
    ClipFile {
        offer_id: String,
        rel_path: String,
        path: std::path::PathBuf,
        size: u64,
    },
    CursorFile {
        role: String,
        name: String,
        data: Vec<u8>,
    },
    // --- end SPEC-25a ---
}

/// Header at the start of every unidirectional stream (raw bytes follow).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StreamHeader {
    File {
        name: String,
        size: u64,
    },
    ClipboardImage {
        format: String,
    },
    // --- SPEC-25c ---
    OpenFile {
        transfer_id: String,
        name: String,
        size: u64,
    },
    // --- end SPEC-25c ---
    // --- SPEC-25a ---
    DragFile {
        drag_id: String,
        rel_path: String,
        size: u64,
    },
    ClipFile {
        offer_id: String,
        rel_path: String,
        size: u64,
    },
    CursorFile {
        role: String,
        name: String,
        size: u64,
    },
    // --- end SPEC-25a ---
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
    let Some(body) = read_frame_body(r, max).await? else {
        return Ok(None);
    };
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(std::io::Error::other)
}

async fn read_frame_body<R>(r: &mut R, max: usize) -> std::io::Result<Option<Vec<u8>>>
where
    R: tokio::io::AsyncRead + Unpin,
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
    Ok(Some(body))
}

/// Like `read_frame` but a well-framed body that fails to deserialize (for example a
/// packet variant from a newer peer) is logged and skipped instead of ending the session.
pub async fn read_frame_skip_unknown<R, T>(r: &mut R) -> std::io::Result<Option<T>>
where
    R: tokio::io::AsyncRead + Unpin,
    T: serde::de::DeserializeOwned,
{
    loop {
        let Some(body) = read_frame_body(r, MAX_FRAME).await? else {
            return Ok(None);
        };
        match serde_json::from_slice(&body) {
            Ok(v) => return Ok(Some(v)),
            Err(e) => {
                super::debug_log::note(|| format!("PACKET_UNKNOWN_SKIPPED err={e}"));
                eprintln!("[KVM] skipping unrecognized packet: {e}");
            }
        }
    }
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

// --- SPEC-25b ---
pub const AUDIO_DATAGRAM_TAG: u8 = 0x41;
pub const AUDIO_DATAGRAM_HEADER_LEN: usize = 7;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDatagram {
    pub seq: u32,
    pub pcm: Vec<u8>,
}

/// Encode a PCM audio frame into a QUIC datagram:
/// `[0x41 ('A'), seq: u32 LE, frame_len: u16 LE, pcm bytes...]`.
/// Returns `None` when `pcm` is empty, exceeds `u16::MAX`, or would produce a
/// 12-byte datagram (reserved exclusively for `UdpMouseDelta`).
pub fn encode_audio_datagram(seq: u32, pcm: &[u8]) -> Option<Vec<u8>> {
    if pcm.is_empty() || pcm.len() > u16::MAX as usize {
        return None;
    }
    let total_len = AUDIO_DATAGRAM_HEADER_LEN + pcm.len();
    if total_len == 12 {
        return None;
    }
    let mut buf = Vec::with_capacity(total_len);
    buf.push(AUDIO_DATAGRAM_TAG);
    buf.extend_from_slice(&seq.to_le_bytes());
    buf.extend_from_slice(&(pcm.len() as u16).to_le_bytes());
    buf.extend_from_slice(pcm);
    Some(buf)
}

/// Decode an audio datagram. Returns `None` for 12-byte mouse deltas or malformed frames.
pub fn decode_audio_datagram(b: &[u8]) -> Option<AudioDatagram> {
    if b.len() == 12 || b.len() <= AUDIO_DATAGRAM_HEADER_LEN || b[0] != AUDIO_DATAGRAM_TAG {
        return None;
    }
    let seq = u32::from_le_bytes(b[1..5].try_into().ok()?);
    let frame_len = u16::from_le_bytes(b[5..7].try_into().ok()?) as usize;
    if frame_len == 0 || b.len() != AUDIO_DATAGRAM_HEADER_LEN + frame_len {
        return None;
    }
    Some(AudioDatagram {
        seq,
        pcm: b[AUDIO_DATAGRAM_HEADER_LEN..].to_vec(),
    })
}
// --- /SPEC-25b ---

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
                target_monitor: None,
                along_px: None,
            }
        );

        let pkt = KvmPacket::CursorEnter {
            edge: "right".into(),
            relative_y: 0.25,
            dwell_ms: 120,
            deadzone_px: 48,
            target_monitor: Some(r"\\.\DISPLAY2".into()),
            along_px: Some(540),
        };
        let json = serde_json::to_string(&pkt).expect("serialize");
        let rt: KvmPacket = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(pkt, rt);
    }

    #[test]
    fn test_cursor_return_seams_and_topology_roundtrip() {
        let legacy_ret = r#"{"type":"CursorReturn","payload":{"edge":"left","relative_y":0.5}}"#;
        let decoded_ret: KvmPacket = serde_json::from_str(legacy_ret).expect("legacy return");
        assert_eq!(
            decoded_ret,
            KvmPacket::CursorReturn {
                edge: "left".into(),
                relative_y: 0.5,
                target_monitor: None,
                along_px: None,
            }
        );

        let seams_pkt = KvmPacket::Seams {
            seams: vec![LinkedSeam {
                linked_monitor_id: r"\\.\DISPLAY1".into(),
                side: "top".into(),
                start: 0,
                end: 1920,
                main_monitor_id: r"\\.\DISPLAY2".into(),
                main_offset: -1920,
            }],
        };
        let s_json = serde_json::to_string(&seams_pkt).expect("serialize seams");
        assert_eq!(
            serde_json::from_str::<KvmPacket>(&s_json).unwrap(),
            seams_pkt
        );

        let legacy_topo = r#"{"computers":[],"links":[]}"#;
        let topo: TopologyConfig = serde_json::from_str(legacy_topo).expect("legacy topo");
        assert_eq!(topo.remote_origin, None);

        let topo_with_origin = TopologyConfig {
            computers: vec![],
            links: vec![],
            remote_origin: Some(Point { x: 1920, y: -420 }),
        };
        let t_json = serde_json::to_string(&topo_with_origin).expect("serialize topo");
        assert_eq!(
            serde_json::from_str::<TopologyConfig>(&t_json).unwrap(),
            topo_with_origin
        );
    }

    #[test]
    fn test_delta_datagram_roundtrip() {
        let delta = UdpMouseDelta {
            dx: 14,
            dy: -8,
            seq: 102,
        };
        let bytes = encode_delta(&delta);
        assert_eq!(bytes.len(), 12);
        assert_eq!(decode_delta(&bytes), Some(delta));
        assert_eq!(decode_delta(&bytes[..11]), None);
    }

    #[tokio::test]
    async fn test_frame_roundtrip() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        let pkt = KvmPacket::ClipboardText {
            text: "héllo".into(),
        };
        write_frame(&mut a, &pkt).await.unwrap();
        write_frame(
            &mut a,
            &StreamHeader::File {
                name: "x.txt".into(),
                size: 3,
            },
        )
        .await
        .unwrap();
        drop(a);
        assert_eq!(read_frame::<_, KvmPacket>(&mut b).await.unwrap(), Some(pkt));
        assert_eq!(
            read_frame::<_, StreamHeader>(&mut b).await.unwrap(),
            Some(StreamHeader::File {
                name: "x.txt".into(),
                size: 3
            })
        );
        assert!(read_frame::<_, KvmPacket>(&mut b).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_oversized_frame_rejected() {
        let (mut a, mut b) = tokio::io::duplex(64);
        tokio::io::AsyncWriteExt::write_all(&mut a, &u32::MAX.to_be_bytes())
            .await
            .unwrap();
        assert!(read_frame::<_, KvmPacket>(&mut b).await.is_err());
    }

    // --- SPEC-25b ---
    #[test]
    fn test_audio_and_media_packet_roundtrip() {
        let packets = vec![
            KvmPacket::AudioStart {
                sample_rate: 48_000,
                channels: 2,
                frame_ms: 5,
                mute_local: true,
            },
            KvmPacket::AudioStop,
            KvmPacket::AudioMuteState {
                state: "muted".to_string(),
            },
            KvmPacket::AudioActivity { active: true },
            KvmPacket::MediaKey {
                vk: 0xB3,
                is_down: true,
            },
        ];
        for pkt in packets {
            let json = serde_json::to_string(&pkt).expect("serialize");
            let decoded: KvmPacket = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(pkt, decoded);
        }
    }

    #[test]
    fn test_audio_datagram_codec_and_never_12_bytes() {
        let pcm = vec![0x5Au8; 960];
        let encoded = encode_audio_datagram(42, &pcm).expect("encode 960 bytes");
        assert_eq!(encoded.len(), 967);
        assert_ne!(encoded.len(), 12);
        assert_eq!(encoded[0], AUDIO_DATAGRAM_TAG);
        let decoded = decode_audio_datagram(&encoded).expect("decode 960 bytes");
        assert_eq!(decoded.seq, 42);
        assert_eq!(decoded.pcm, pcm);

        // 5-byte PCM would produce 7 + 5 = 12 bytes -> must be rejected
        assert_eq!(encode_audio_datagram(1, &[1, 2, 3, 4, 5]), None);
        assert_eq!(encode_audio_datagram(1, &[]), None);

        // A 12-byte mouse delta whose first byte happens to be 0x41 and len field == 5
        // must NEVER decode as an audio datagram.
        let mut fake_delta = [0u8; 12];
        fake_delta[0] = AUDIO_DATAGRAM_TAG;
        fake_delta[5..7].copy_from_slice(&5u16.to_le_bytes());
        assert_eq!(decode_audio_datagram(&fake_delta), None);
        assert!(decode_delta(&fake_delta).is_some());

        // Truncated / mismatched length must fail
        assert_eq!(decode_audio_datagram(&encoded[..encoded.len() - 1]), None);
        let mut bad_tag = encoded.clone();
        bad_tag[0] = 0x42;
        assert_eq!(decode_audio_datagram(&bad_tag), None);
    }
    // --- /SPEC-25b ---

    // --- SPEC-25a ---
    #[test]
    fn test_spec25a_packets_and_stream_headers_roundtrip() {
        let packets = vec![
            KvmPacket::DragOffer {
                drag_id: "d-1".into(),
                files: vec![
                    FileMeta {
                        rel_path: "folder".into(),
                        size: 0,
                        is_dir: true,
                    },
                    FileMeta {
                        rel_path: "folder/a.txt".into(),
                        size: 42,
                        is_dir: false,
                    },
                ],
                button: 1,
            },
            KvmPacket::DragReady {
                drag_id: "d-1".into(),
            },
            KvmPacket::DragCancel {
                drag_id: "d-1".into(),
            },
            KvmPacket::DragResult {
                drag_id: "d-1".into(),
                outcome: DragOutcome::Failed,
                notice: Some("Drop was not accepted by the target app".into()),
            },
            KvmPacket::FileClipOffer {
                offer_id: "c-1".into(),
                files: vec![FileMeta {
                    rel_path: "pic.png".into(),
                    size: 1024,
                    is_dir: false,
                }],
            },
            KvmPacket::ClipFetchRequest {
                offer_id: "c-1".into(),
            },
            KvmPacket::CursorSchemeOffer {
                roles: vec!["Arrow".into(), "Hand".into()],
            },
        ];
        for p in packets {
            let json = serde_json::to_string(&p).unwrap();
            let rt: KvmPacket = serde_json::from_str(&json).unwrap();
            assert_eq!(p, rt);
        }

        let headers = vec![
            StreamHeader::DragFile {
                drag_id: "d-1".into(),
                rel_path: "folder/a.txt".into(),
                size: 42,
            },
            StreamHeader::ClipFile {
                offer_id: "c-1".into(),
                rel_path: "pic.png".into(),
                size: 1024,
            },
            StreamHeader::CursorFile {
                role: "Arrow".into(),
                name: "arrow.cur".into(),
                size: 512,
            },
        ];
        for h in headers {
            let json = serde_json::to_string(&h).unwrap();
            let rt: StreamHeader = serde_json::from_str(&json).unwrap();
            assert_eq!(h, rt);
        }
    }
    // --- end SPEC-25a ---

    #[test]
    fn input_blocked_round_trips() {
        let p = KvmPacket::InputBlocked {
            foreground_process: "taskmgr.exe".into(),
            reason: "elevated_foreground".into(),
        };
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<KvmPacket>(&json).unwrap(), p);
    }

    #[test]
    fn input_blocked_without_reason_deserializes() {
        let json = r#"{"type":"InputBlocked","payload":{"foreground_process":"taskmgr.exe"}}"#;
        let p = serde_json::from_str::<KvmPacket>(json).unwrap();
        assert_eq!(
            p,
            KvmPacket::InputBlocked {
                foreground_process: "taskmgr.exe".into(),
                reason: String::new(),
            }
        );
    }

    #[tokio::test]
    async fn unknown_packet_variant_is_skipped() {
        use tokio::io::AsyncWriteExt;
        let (mut a, mut b) = tokio::io::duplex(4096);
        let unknown = br#"{"type":"FutureThing","payload":{"x":1}}"#;
        a.write_all(&(unknown.len() as u32).to_be_bytes())
            .await
            .unwrap();
        a.write_all(unknown).await.unwrap();
        write_frame(&mut a, &KvmPacket::SessionDisconnect)
            .await
            .unwrap();
        drop(a);
        let first = read_frame_skip_unknown::<_, KvmPacket>(&mut b)
            .await
            .unwrap();
        assert_eq!(first, Some(KvmPacket::SessionDisconnect));
        let end = read_frame_skip_unknown::<_, KvmPacket>(&mut b)
            .await
            .unwrap();
        assert_eq!(end, None);
    }
}
