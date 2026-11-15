use super::debug_log;
use super::media_keys;
use super::protocol::{encode_audio_datagram, AudioDatagram, KvmPacket, Outbound};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};
use tokio::sync::{broadcast, mpsc};

pub const AUDIO_SAMPLE_RATE: u32 = 48_000;
pub const AUDIO_CHANNELS: u16 = 2;
pub const AUDIO_FRAME_MS: u16 = 5;
/// 48,000 Hz * 5 ms = 240 stereo frames per datagram.
pub const AUDIO_FRAMES_PER_PACKET: usize = 240;
/// 240 frames * 2 channels * 2 bytes (i16 LE) = 960 bytes per datagram.
pub const AUDIO_PACKET_BYTES: usize = 960;
/// ~40 ms jitter buffer target = 8 * 5 ms frames.
pub const JITTER_TARGET_FRAMES: usize = 8;
pub const JITTER_MAX_FRAMES: usize = 64;
/// Digital silence threshold for runtime silence guard (16-bit PCM normalized).
pub const CAPTURE_SILENCE_THRESHOLD: f32 = 0.0005;
/// Continuous mismatch window (~1 s) before muting falls back to endpoint loopback.
pub const SILENCE_GUARD_TRIGGER_MS: u64 = 1000;

pub const LINKED_MUTE_UNMUTED: &str = "unmuted";
pub const LINKED_MUTE_MUTED: &str = "muted";
pub const LINKED_MUTE_UNSUPPORTED: &str = "unsupported";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AudioStatePayload {
    pub streaming: bool,
    pub level: f32,
    pub linked_mute: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MuteMarker {
    pub endpoint_id: String,
    pub prior_muted: bool,
    pub we_applied_mute: bool,
}

/// Pure decision for applying Linked PC speaker mute at bridge capture start.
/// Returns `(should_set_mute_true, we_applied_mute, reported_state)`.
pub fn decide_apply_mute(
    prior_muted: bool,
    mute_local: bool,
    process_loopback_ok: bool,
    session_unsupported: bool,
) -> (bool, bool, &'static str) {
    if !mute_local {
        return (false, false, LINKED_MUTE_UNMUTED);
    }
    if session_unsupported || !process_loopback_ok {
        return (false, false, LINKED_MUTE_UNSUPPORTED);
    }
    if prior_muted {
        return (false, false, LINKED_MUTE_MUTED);
    }
    (true, true, LINKED_MUTE_MUTED)
}

/// Pure decision for restoring Linked PC speaker mute on stop, disconnect, exit,
/// or crash-marker recovery on next startup. Restores ONLY when AnyLeap applied
/// the mute (`we_applied_mute && !prior_muted`).
pub fn decide_restore_mute(
    marker: Option<&MuteMarker>,
    current_endpoint_muted: Option<bool>,
) -> bool {
    let Some(m) = marker else {
        return false;
    };
    if !m.we_applied_mute || m.prior_muted {
        return false;
    }
    current_endpoint_muted != Some(false)
}

/// Tracks continuous mismatch where other Linked PC audio sessions are active
/// while process-loopback PCM remains digitally silent (e.g. hardware/exclusive path
/// bypassing virtual process loopback).
#[derive(Debug, Clone)]
pub struct SilenceGuardTracker {
    silent_since_ms: Option<u64>,
    threshold_ms: u64,
}

impl Default for SilenceGuardTracker {
    fn default() -> Self {
        Self::new(SILENCE_GUARD_TRIGGER_MS)
    }
}

impl SilenceGuardTracker {
    pub fn new(threshold_ms: u64) -> Self {
        Self {
            silent_since_ms: None,
            threshold_ms,
        }
    }

    pub fn reset(&mut self) {
        self.silent_since_ms = None;
    }

    /// Returns `true` once other-session peak > threshold while captured PCM peak stays
    /// below digital silence continuously for at least `threshold_ms`.
    pub fn step(&mut self, now_ms: u64, other_sessions_peak: f32, captured_pcm_peak: f32) -> bool {
        let other_active = other_sessions_peak > media_keys::AUDIO_ACTIVITY_THRESHOLD;
        let capture_silent = captured_pcm_peak < CAPTURE_SILENCE_THRESHOLD;
        if other_active && capture_silent {
            let start = *self.silent_since_ms.get_or_insert(now_ms);
            now_ms.saturating_sub(start) >= self.threshold_ms
        } else {
            self.silent_since_ms = None;
            false
        }
    }
}

pub fn audio_mute_marker_path() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("AnyLeap").join("audio_mute_marker.json")
}

pub fn write_mute_marker_to(path: &Path, marker: &MuteMarker) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let json = serde_json::to_vec(marker).map_err(|e| e.to_string())?;
    std::fs::write(path, json).map_err(|e| e.to_string())
}

pub fn read_mute_marker_from(path: &Path) -> Option<MuteMarker> {
    let raw = std::fs::read(path).ok()?;
    serde_json::from_slice(&raw).ok()
}

pub fn remove_mute_marker_at(path: &Path) {
    let _ = std::fs::remove_file(path);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MixFormatDesc {
    pub sample_rate: u32,
    pub channels: u16,
    pub bits_per_sample: u16,
    pub is_float: bool,
}

/// Converts device mix-format PCM buffers into 48 kHz 16-bit stereo LE PCM,
/// preserving interpolation phase across WASAPI packets.
#[derive(Debug, Clone)]
pub struct ResamplerState {
    pos: f64,
    prev_l: f32,
    prev_r: f32,
    has_prev: bool,
}

impl Default for ResamplerState {
    fn default() -> Self {
        Self::new()
    }
}

impl ResamplerState {
    pub fn new() -> Self {
        Self {
            pos: 0.0,
            prev_l: 0.0,
            prev_r: 0.0,
            has_prev: false,
        }
    }

    pub fn reset(&mut self) {
        self.pos = 0.0;
        self.prev_l = 0.0;
        self.prev_r = 0.0;
        self.has_prev = false;
    }

    pub fn convert_to_48k_stereo_i16(
        &mut self,
        input: &[u8],
        num_frames: usize,
        fmt: MixFormatDesc,
        out_pcm: &mut Vec<u8>,
    ) {
        if num_frames == 0 || fmt.channels == 0 || fmt.sample_rate == 0 {
            return;
        }
        let bytes_per_sample = (fmt.bits_per_sample / 8) as usize;
        let block_align = bytes_per_sample * fmt.channels as usize;
        if block_align == 0 || input.len() < num_frames * block_align {
            return;
        }

        if fmt.sample_rate == AUDIO_SAMPLE_RATE {
            out_pcm.reserve(num_frames * 4);
            for i in 0..num_frames {
                let (l, r) = read_stereo_sample(&input[i * block_align..], fmt, bytes_per_sample);
                write_i16_stereo(out_pcm, l, r);
            }
            return;
        }

        // Resample to 48 kHz using linear interpolation across packet boundaries.
        let step = fmt.sample_rate as f64 / AUDIO_SAMPLE_RATE as f64;
        if !self.has_prev {
            let (l0, r0) = read_stereo_sample(input, fmt, bytes_per_sample);
            self.prev_l = l0;
            self.prev_r = r0;
            self.has_prev = true;
            self.pos = 0.0;
        }

        let limit = (num_frames as f64) - 1.0;
        while self.pos < limit {
            let (l0, r0, l1, r1, t) = if self.pos < 0.0 {
                let (nl, nr) = read_stereo_sample(input, fmt, bytes_per_sample);
                (self.prev_l, self.prev_r, nl, nr, (self.pos + 1.0) as f32)
            } else {
                let i0 = self.pos.floor() as usize;
                let i1 = (i0 + 1).min(num_frames - 1);
                let (al, ar) =
                    read_stereo_sample(&input[i0 * block_align..], fmt, bytes_per_sample);
                let (bl, br) =
                    read_stereo_sample(&input[i1 * block_align..], fmt, bytes_per_sample);
                (al, ar, bl, br, (self.pos - i0 as f64) as f32)
            };
            let l = l0 + (l1 - l0) * t;
            let r = r0 + (r1 - r0) * t;
            write_i16_stereo(out_pcm, l, r);
            self.pos += step;
        }

        let last_off = (num_frames - 1) * block_align;
        let (ll, lr) = read_stereo_sample(&input[last_off..], fmt, bytes_per_sample);
        self.prev_l = ll;
        self.prev_r = lr;
        self.pos -= num_frames as f64;
    }
}

#[inline]
fn read_sample_f32(bytes: &[u8], fmt: MixFormatDesc) -> f32 {
    match (fmt.is_float, fmt.bits_per_sample) {
        (true, 32) => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]).clamp(-1.0, 1.0),
        (false, 16) => i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 32768.0,
        (false, 24) => {
            let raw = ((bytes[0] as i32) | ((bytes[1] as i32) << 8) | ((bytes[2] as i32) << 16))
                << 8
                >> 8;
            raw as f32 / 8_388_608.0
        }
        (false, 32) => {
            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32 / 2_147_483_648.0
        }
        _ => 0.0,
    }
}

#[inline]
fn read_stereo_sample(frame: &[u8], fmt: MixFormatDesc, bytes_per_sample: usize) -> (f32, f32) {
    let l = read_sample_f32(&frame[..bytes_per_sample], fmt);
    let r = if fmt.channels >= 2 {
        read_sample_f32(&frame[bytes_per_sample..2 * bytes_per_sample], fmt)
    } else {
        l
    };
    (l, r)
}

#[inline]
fn write_i16_stereo(out: &mut Vec<u8>, l: f32, r: f32) {
    let li = (l.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    let ri = (r.clamp(-1.0, 1.0) * 32767.0).round() as i16;
    out.extend_from_slice(&li.to_le_bytes());
    out.extend_from_slice(&ri.to_le_bytes());
}

/// Scales 16-bit little-endian PCM in place by `volume_pct` (clamped to 0..=100).
pub fn apply_volume_pcm16(pcm: &mut [u8], volume_pct: u8) {
    let vol = volume_pct.min(100);
    if vol == 100 {
        return;
    }
    if vol == 0 {
        pcm.fill(0);
        return;
    }
    let v = vol as i32;
    for chunk in pcm.as_chunks_mut::<2>().0 {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as i32;
        let scaled = ((sample * v) / 100).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        let bytes = scaled.to_le_bytes();
        chunk[0] = bytes[0];
        chunk[1] = bytes[1];
    }
}

/// Peak absolute sample amplitude in `0.0..=1.0` for 16-bit little-endian PCM.
pub fn compute_pcm16_peak(pcm: &[u8]) -> f32 {
    let mut max_abs = 0i32;
    for chunk in pcm.as_chunks::<2>().0 {
        let s = i16::from_le_bytes([chunk[0], chunk[1]]) as i32;
        let abs = s.abs();
        if abs > max_abs {
            max_abs = abs;
        }
    }
    (max_abs as f32 / 32768.0).clamp(0.0, 1.0)
}

/// Sequence-ordered jitter buffer for fixed 5 ms (960-byte) PCM frames.
/// Target playout buffer is `target_frames` (~40 ms at 8 frames).
/// On packet loss: repeats the previous played frame once, then outputs silence.
#[derive(Debug)]
pub struct JitterBuffer {
    target_frames: usize,
    max_frames: usize,
    frames: BTreeMap<i64, Vec<u8>>,
    anchor_seq: Option<u32>,
    next_rel: i64,
    primed: bool,
    waited_pops: usize,
    last_frame: Option<Vec<u8>>,
    repeated_once: bool,
    consecutive_misses: usize,
}

impl Default for JitterBuffer {
    fn default() -> Self {
        Self::new(JITTER_TARGET_FRAMES, JITTER_MAX_FRAMES)
    }
}

impl JitterBuffer {
    pub fn new(target_frames: usize, max_frames: usize) -> Self {
        Self {
            target_frames: target_frames.max(1),
            max_frames: max_frames.max(target_frames.max(1) * 2),
            frames: BTreeMap::new(),
            anchor_seq: None,
            next_rel: 0,
            primed: false,
            waited_pops: 0,
            last_frame: None,
            repeated_once: false,
            consecutive_misses: 0,
        }
    }

    pub fn clear(&mut self) {
        self.frames.clear();
        self.anchor_seq = None;
        self.next_rel = 0;
        self.primed = false;
        self.waited_pops = 0;
        self.last_frame = None;
        self.repeated_once = false;
        self.consecutive_misses = 0;
    }

    pub fn push(&mut self, seq: u32, pcm: Vec<u8>) {
        if pcm.is_empty() {
            return;
        }
        let Some(anchor) = self.anchor_seq else {
            self.anchor_seq = Some(seq);
            self.frames.insert(0, pcm);
            return;
        };

        if !self.primed {
            let rel = seq.wrapping_sub(anchor) as i32 as i64;
            if rel.unsigned_abs() as usize > self.max_frames {
                self.clear();
                self.anchor_seq = Some(seq);
                self.frames.insert(0, pcm);
                return;
            }
            self.frames.insert(rel, pcm);
            while self.frames.len() > self.max_frames {
                self.frames.pop_first();
            }
            return;
        }

        let expected_seq = anchor.wrapping_add(self.next_rel as u32);
        let offset = seq.wrapping_sub(expected_seq) as i32 as i64;
        if offset < 0 {
            // Arrived too late (already played or concealed).
            return;
        }
        if offset >= self.max_frames as i64 {
            // Discontinuity / sequence jump: reset and re-prime at the new sequence.
            self.clear();
            self.anchor_seq = Some(seq);
            self.frames.insert(0, pcm);
            return;
        }
        let rel = self.next_rel + offset;
        self.frames.insert(rel, pcm);

        while self.frames.len() > self.max_frames {
            if let Some((&k, _)) = self.frames.first_key_value() {
                self.frames.remove(&k);
                if k >= self.next_rel {
                    self.next_rel = k + 1;
                }
            }
        }
    }

    pub fn pop(&mut self) -> Vec<u8> {
        if !self.primed {
            if self.frames.is_empty() {
                self.waited_pops = 0;
                return vec![0u8; AUDIO_PACKET_BYTES];
            }
            if self.frames.len() >= self.target_frames || self.waited_pops >= self.target_frames {
                self.primed = true;
                self.next_rel = *self.frames.keys().next().unwrap_or(&0);
            } else {
                self.waited_pops += 1;
                return vec![0u8; AUDIO_PACKET_BYTES];
            }
        }

        if let Some(frame) = self.frames.remove(&self.next_rel) {
            self.next_rel += 1;
            self.consecutive_misses = 0;
            self.repeated_once = false;
            self.last_frame = Some(frame.clone());
            return frame;
        }

        self.next_rel += 1;
        self.consecutive_misses += 1;

        if !self.repeated_once {
            if let Some(ref last) = self.last_frame {
                self.repeated_once = true;
                return last.clone();
            }
        }

        if self.consecutive_misses >= self.max_frames {
            self.clear();
        }
        vec![0u8; AUDIO_PACKET_BYTES]
    }
}

type AudioStateEmitter = Arc<dyn Fn(AudioStatePayload) + Send + Sync>;

struct AudioController {
    audio_bridge: AtomicBool,
    audio_volume: AtomicU8,
    mute_linked_while_bridging: AtomicBool,
    streaming: Arc<AtomicBool>,
    level_bits: Arc<AtomicU32>,
    sent_audio_start: AtomicBool,
    linked_mute_state: Mutex<String>,
    session_mute_unsupported: Arc<AtomicBool>,
    active_mute: Mutex<Option<MuteMarker>>,

    client_packet_tx: Mutex<Option<mpsc::Sender<KvmPacket>>>,
    linked_outgoing_tx: Mutex<Option<broadcast::Sender<Outbound>>>,
    emitter: Mutex<Option<AudioStateEmitter>>,
    jitter_buffer: Arc<Mutex<JitterBuffer>>,

    playback_stop: Mutex<Option<Arc<AtomicBool>>>,
    capture_stop: Mutex<Option<Arc<AtomicBool>>>,
    capture_gen: Arc<AtomicU64>,
    local_monitor_stop: Mutex<Option<Arc<AtomicBool>>>,
    linked_monitor_stop: Mutex<Option<Arc<AtomicBool>>>,
    linked_peer_count: Arc<AtomicUsize>,
}

impl AudioController {
    fn new() -> Self {
        Self {
            audio_bridge: AtomicBool::new(false),
            audio_volume: AtomicU8::new(100),
            mute_linked_while_bridging: AtomicBool::new(true),
            streaming: Arc::new(AtomicBool::new(false)),
            level_bits: Arc::new(AtomicU32::new(0.0f32.to_bits())),
            sent_audio_start: AtomicBool::new(false),
            linked_mute_state: Mutex::new(LINKED_MUTE_UNMUTED.to_string()),
            session_mute_unsupported: Arc::new(AtomicBool::new(false)),
            active_mute: Mutex::new(None),
            client_packet_tx: Mutex::new(None),
            linked_outgoing_tx: Mutex::new(None),
            emitter: Mutex::new(None),
            jitter_buffer: Arc::new(Mutex::new(JitterBuffer::default())),
            playback_stop: Mutex::new(None),
            capture_stop: Mutex::new(None),
            capture_gen: Arc::new(AtomicU64::new(0)),
            local_monitor_stop: Mutex::new(None),
            linked_monitor_stop: Mutex::new(None),
            linked_peer_count: Arc::new(AtomicUsize::new(0)),
        }
    }
}

static CONTROLLER: OnceLock<AudioController> = OnceLock::new();

fn controller() -> &'static AudioController {
    CONTROLLER.get_or_init(AudioController::new)
}

fn current_linked_mute() -> String {
    controller()
        .linked_mute_state
        .lock()
        .map(|g| g.clone())
        .unwrap_or_else(|_| LINKED_MUTE_UNMUTED.to_string())
}

fn set_linked_mute_state(state: &str) {
    if let Ok(mut g) = controller().linked_mute_state.lock() {
        *g = state.to_string();
    }
}

fn send_linked_mute_state(state: &str) {
    set_linked_mute_state(state);
    if let Ok(g) = controller().linked_outgoing_tx.lock() {
        if let Some(ref tx) = *g {
            let _ = tx.send(Outbound::Packet(KvmPacket::AudioMuteState {
                state: state.to_string(),
            }));
        }
    }
}

pub fn emit_audio_state(streaming: bool, level: f32) {
    let payload = AudioStatePayload {
        streaming,
        level,
        linked_mute: current_linked_mute(),
    };
    let cb = controller().emitter.lock().ok().and_then(|g| g.clone());
    if let Some(emit) = cb {
        emit(payload);
    }
}

fn set_app_handle(app: &AppHandle) {
    let app_clone = app.clone();
    let cb: Arc<dyn Fn(AudioStatePayload) + Send + Sync> = Arc::new(move |payload| {
        let _ = app_clone.emit("kvm-audio-state", payload);
    });
    if let Ok(mut g) = controller().emitter.lock() {
        *g = Some(cb);
    }
}

pub fn get_audio_state() -> AudioStatePayload {
    let c = controller();
    let streaming = c.streaming.load(Ordering::Acquire);
    let level = if streaming {
        f32::from_bits(c.level_bits.load(Ordering::Acquire))
    } else {
        0.0
    };
    AudioStatePayload {
        streaming,
        level,
        linked_mute: current_linked_mute(),
    }
}

pub fn set_audio_settings(
    audio_bridge: bool,
    audio_volume: u8,
    media_keys_follow_audio: bool,
    mute_linked_while_bridging: bool,
    app_handle: &AppHandle,
) {
    let c = controller();
    set_app_handle(app_handle);

    let clamped_vol = audio_volume.min(100);
    c.audio_bridge.store(audio_bridge, Ordering::Release);
    c.audio_volume.store(clamped_vol, Ordering::Release);
    let prev_mute = c
        .mute_linked_while_bridging
        .swap(mute_linked_while_bridging, Ordering::AcqRel);
    media_keys::set_follow_audio(media_keys_follow_audio);

    let tx = c.client_packet_tx.lock().ok().and_then(|g| g.clone());
    if let Some(pkt_tx) = tx {
        if audio_bridge {
            ensure_main_playback_started();
            let was_started = c.sent_audio_start.swap(true, Ordering::AcqRel);
            if !was_started || prev_mute != mute_linked_while_bridging {
                let _ = pkt_tx.try_send(KvmPacket::AudioStart {
                    sample_rate: AUDIO_SAMPLE_RATE,
                    channels: AUDIO_CHANNELS,
                    frame_ms: AUDIO_FRAME_MS,
                    mute_local: mute_linked_while_bridging,
                });
            }
        } else {
            stop_main_playback();
            if c.sent_audio_start.swap(false, Ordering::AcqRel) {
                let _ = pkt_tx.try_send(KvmPacket::AudioStop);
            }
            set_linked_mute_state(LINKED_MUTE_UNMUTED);
            emit_audio_state(false, 0.0);
        }
    } else if !audio_bridge {
        stop_main_playback();
        c.sent_audio_start.store(false, Ordering::Release);
        set_linked_mute_state(LINKED_MUTE_UNMUTED);
        emit_audio_state(false, 0.0);
    }
}

pub fn on_main_connected(packet_tx: mpsc::Sender<KvmPacket>, app_handle: &AppHandle) {
    let c = controller();
    set_app_handle(app_handle);
    if let Ok(mut g) = c.client_packet_tx.lock() {
        *g = Some(packet_tx.clone());
    }
    c.sent_audio_start.store(false, Ordering::Release);
    set_linked_mute_state(LINKED_MUTE_UNMUTED);
    start_local_activity_monitor();

    if c.audio_bridge.load(Ordering::Acquire) {
        ensure_main_playback_started();
        if !c.sent_audio_start.swap(true, Ordering::AcqRel) {
            let _ = packet_tx.try_send(KvmPacket::AudioStart {
                sample_rate: AUDIO_SAMPLE_RATE,
                channels: AUDIO_CHANNELS,
                frame_ms: AUDIO_FRAME_MS,
                mute_local: c.mute_linked_while_bridging.load(Ordering::Acquire),
            });
        }
    }
}

pub fn on_main_disconnected() {
    let c = controller();
    if let Ok(mut g) = c.client_packet_tx.lock() {
        *g = None;
    }
    c.sent_audio_start.store(false, Ordering::Release);
    stop_main_playback();
    stop_local_activity_monitor();
    media_keys::reset_linked_audio_activity();
    set_linked_mute_state(LINKED_MUTE_UNMUTED);
    emit_audio_state(false, 0.0);
}

pub fn on_main_audio_mute_state(state: &str) {
    let normalized = match state {
        LINKED_MUTE_MUTED => LINKED_MUTE_MUTED,
        LINKED_MUTE_UNSUPPORTED => LINKED_MUTE_UNSUPPORTED,
        _ => LINKED_MUTE_UNMUTED,
    };
    set_linked_mute_state(normalized);
    let c = controller();
    let streaming = c.streaming.load(Ordering::Acquire);
    let level = if streaming {
        f32::from_bits(c.level_bits.load(Ordering::Acquire))
    } else {
        0.0
    };
    emit_audio_state(streaming, level);
}

pub fn on_main_audio_datagram(dg: AudioDatagram) {
    let c = controller();
    if !c.audio_bridge.load(Ordering::Acquire) {
        return;
    }
    if let Ok(mut jb) = c.jitter_buffer.lock() {
        jb.push(dg.seq, dg.pcm);
    }
}

pub fn on_linked_server_started(outgoing_tx: broadcast::Sender<Outbound>, app_handle: &AppHandle) {
    set_app_handle(app_handle);
    let c = controller();
    c.linked_peer_count.store(0, Ordering::Release);
    c.session_mute_unsupported.store(false, Ordering::Release);
    if let Ok(mut g) = c.linked_outgoing_tx.lock() {
        *g = Some(outgoing_tx.clone());
    }
    start_linked_activity_monitor(outgoing_tx);
}

pub fn on_linked_peer_connected() {
    controller()
        .linked_peer_count
        .fetch_add(1, Ordering::AcqRel);
}

pub fn on_linked_peer_disconnected(remaining_peers: usize) {
    let c = controller();
    c.linked_peer_count
        .store(remaining_peers, Ordering::Release);
    if remaining_peers == 0 {
        on_linked_audio_stop();
        c.session_mute_unsupported.store(false, Ordering::Release);
    }
}

pub fn on_linked_server_stopped() {
    let c = controller();
    c.linked_peer_count.store(0, Ordering::Release);
    stop_linked_capture("server_stop");
    c.session_mute_unsupported.store(false, Ordering::Release);
    if let Ok(mut g) = c.linked_outgoing_tx.lock() {
        *g = None;
    }
    stop_linked_activity_monitor();
    set_linked_mute_state(LINKED_MUTE_UNMUTED);
    emit_audio_state(false, 0.0);
}

pub fn on_linked_audio_start(
    sample_rate: u32,
    channels: u16,
    frame_ms: u16,
    mute_local: bool,
    audio_out_tx: broadcast::Sender<bytes::Bytes>,
    outgoing_tx: broadcast::Sender<Outbound>,
    app_handle: &AppHandle,
) {
    set_app_handle(app_handle);
    let _ = (sample_rate, channels, frame_ms);
    stop_linked_capture("restart");

    let c = controller();
    if let Ok(mut g) = c.linked_outgoing_tx.lock() {
        *g = Some(outgoing_tx);
    }
    let stop = Arc::new(AtomicBool::new(false));
    if let Ok(mut g) = c.capture_stop.lock() {
        *g = Some(Arc::clone(&stop));
    }
    let gen = c.capture_gen.fetch_add(1, Ordering::AcqRel).wrapping_add(1);
    c.streaming.store(true, Ordering::Release);
    emit_audio_state(true, 0.0);

    #[cfg(windows)]
    {
        let streaming = Arc::clone(&c.streaming);
        let level_bits = Arc::clone(&c.level_bits);
        let session_unsupported = Arc::clone(&c.session_mute_unsupported);
        let capture_gen = Arc::clone(&c.capture_gen);
        std::thread::spawn(move || {
            wasapi::run_loopback_capture(
                stop,
                mute_local,
                session_unsupported,
                audio_out_tx,
                streaming,
                level_bits,
                capture_gen,
                gen,
            );
        });
    }
    #[cfg(not(windows))]
    {
        let _ = (stop, mute_local, audio_out_tx);
    }
}

pub fn on_linked_audio_stop() {
    stop_linked_capture("audio_stop");
    if controller()
        .session_mute_unsupported
        .load(Ordering::Acquire)
    {
        send_linked_mute_state(LINKED_MUTE_UNSUPPORTED);
    } else {
        send_linked_mute_state(LINKED_MUTE_UNMUTED);
    }
    emit_audio_state(false, 0.0);
}

pub fn on_app_exit() {
    stop_linked_capture("app_exit");
    stop_main_playback();
}

pub fn restore_mute_from_crash_marker() {
    #[cfg(windows)]
    wasapi::restore_mute_from_crash_marker_win();
}

fn ensure_main_playback_started() {
    let c = controller();
    if let Ok(g) = c.playback_stop.lock() {
        if g.as_ref().is_some_and(|s| !s.load(Ordering::Relaxed)) {
            return;
        }
    }
    if let Ok(mut jb) = c.jitter_buffer.lock() {
        jb.clear();
    }
    let stop = Arc::new(AtomicBool::new(false));
    if let Ok(mut g) = c.playback_stop.lock() {
        *g = Some(Arc::clone(&stop));
    }
    c.streaming.store(true, Ordering::Release);
    media_keys::set_audio_streaming(true);
    emit_audio_state(true, 0.0);

    #[cfg(windows)]
    {
        let jb = Arc::clone(&c.jitter_buffer);
        let streaming = Arc::clone(&c.streaming);
        let level_bits = Arc::clone(&c.level_bits);
        std::thread::spawn(move || {
            wasapi::run_render_playback(stop, jb, streaming, level_bits);
        });
    }
    #[cfg(not(windows))]
    {
        let _ = stop;
    }
}

fn stop_main_playback() {
    let c = controller();
    if let Ok(mut g) = c.playback_stop.lock() {
        if let Some(stop) = g.take() {
            stop.store(true, Ordering::Release);
        }
    }
    if let Ok(mut jb) = c.jitter_buffer.lock() {
        jb.clear();
    }
    media_keys::set_audio_streaming(false);
    c.streaming.store(false, Ordering::Release);
    c.level_bits.store(0.0f32.to_bits(), Ordering::Release);
}

fn stop_linked_capture(reason: &str) {
    let c = controller();
    c.capture_gen.fetch_add(1, Ordering::AcqRel);
    if let Ok(mut g) = c.capture_stop.lock() {
        if let Some(stop) = g.take() {
            stop.store(true, Ordering::Release);
        }
    }
    #[cfg(windows)]
    wasapi::restore_active_mute(reason);
    #[cfg(not(windows))]
    let _ = reason;
    c.streaming.store(false, Ordering::Release);
    c.level_bits.store(0.0f32.to_bits(), Ordering::Release);
}

fn start_local_activity_monitor() {
    stop_local_activity_monitor();
    let c = controller();
    let stop = Arc::new(AtomicBool::new(false));
    if let Ok(mut g) = c.local_monitor_stop.lock() {
        *g = Some(Arc::clone(&stop));
    }
    #[cfg(windows)]
    std::thread::spawn(move || {
        wasapi::run_local_activity_monitor(stop);
    });
    #[cfg(not(windows))]
    let _ = stop;
}

fn stop_local_activity_monitor() {
    if let Ok(mut g) = controller().local_monitor_stop.lock() {
        if let Some(stop) = g.take() {
            stop.store(true, Ordering::Release);
        }
    }
}

fn start_linked_activity_monitor(outgoing_tx: broadcast::Sender<Outbound>) {
    stop_linked_activity_monitor();
    let c = controller();
    let stop = Arc::new(AtomicBool::new(false));
    if let Ok(mut g) = c.linked_monitor_stop.lock() {
        *g = Some(Arc::clone(&stop));
    }
    #[cfg(windows)]
    {
        let peer_count = Arc::clone(&c.linked_peer_count);
        std::thread::spawn(move || {
            wasapi::run_linked_activity_monitor(stop, peer_count, outgoing_tx);
        });
    }
    #[cfg(not(windows))]
    let _ = (stop, outgoing_tx);
}

fn stop_linked_activity_monitor() {
    if let Ok(mut g) = controller().linked_monitor_stop.lock() {
        if let Some(stop) = g.take() {
            stop.store(true, Ordering::Release);
        }
    }
}

#[cfg(windows)]
mod wasapi {
    use super::*;
    use std::mem::ManuallyDrop;
    use windows::core::{implement, Interface, Ref, HRESULT, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation};
    use windows::Win32::Media::Audio::*;
    use windows::Win32::Media::KernelStreaming::WAVE_FORMAT_EXTENSIBLE;
    use windows::Win32::Media::Multimedia::KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
    use windows::Win32::System::Com::StructuredStorage::{
        PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0,
    };
    use windows::Win32::System::Com::*;
    use windows::Win32::System::Threading::{
        CreateEventW, GetCurrentProcessId, SetEvent, WaitForSingleObject,
    };
    use windows::Win32::System::Variant::VT_BLOB;

    const AUDCLNT_BUFFERFLAGS_SILENT_FLAG: u32 = 0x2;
    const DEVICE_POLL_INTERVAL: Duration = Duration::from_millis(500);
    const STATE_EMIT_INTERVAL: Duration = Duration::from_millis(100);

    struct ComGuard(bool);
    impl ComGuard {
        unsafe fn init() -> Self {
            let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
            Self(hr.is_ok())
        }
    }
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    struct HandleGuard(HANDLE);
    unsafe impl Send for HandleGuard {}
    unsafe impl Sync for HandleGuard {}
    impl Drop for HandleGuard {
        fn drop(&mut self) {
            if !self.0.is_invalid() {
                unsafe {
                    let _ = CloseHandle(self.0);
                }
            }
        }
    }

    struct CoTaskMemGuard<T>(*mut T);
    impl<T> Drop for CoTaskMemGuard<T> {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    CoTaskMemFree(Some(self.0 as *const _));
                }
            }
        }
    }

    unsafe fn pwstr_to_string_and_free(pwstr: PWSTR) -> String {
        if pwstr.0.is_null() {
            return String::new();
        }
        let mut len = 0usize;
        while *pwstr.0.add(len) != 0 {
            len += 1;
        }
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(pwstr.0, len));
        CoTaskMemFree(Some(pwstr.0 as *const _));
        s
    }

    unsafe fn default_render_device(
        enumerator: &IMMDeviceEnumerator,
    ) -> windows::core::Result<(IMMDevice, String)> {
        let dev = enumerator.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let id = dev
            .GetId()
            .map(|p| pwstr_to_string_and_free(p))
            .unwrap_or_default();
        Ok((dev, id))
    }

    unsafe fn default_render_id(enumerator: &IMMDeviceEnumerator) -> Option<String> {
        let dev = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let id = dev.GetId().ok().map(|p| pwstr_to_string_and_free(p))?;
        Some(id)
    }

    unsafe fn parse_mix_format(wfx: *const WAVEFORMATEX) -> MixFormatDesc {
        if wfx.is_null() {
            return MixFormatDesc {
                sample_rate: AUDIO_SAMPLE_RATE,
                channels: AUDIO_CHANNELS,
                bits_per_sample: 32,
                is_float: true,
            };
        }
        let base = &*wfx;
        let ext_extra = std::mem::size_of::<WAVEFORMATEXTENSIBLE>()
            .saturating_sub(std::mem::size_of::<WAVEFORMATEX>());
        let is_float = if base.wFormatTag as u32 == WAVE_FORMAT_EXTENSIBLE
            && (base.cbSize as usize) >= ext_extra
        {
            let ext = &*(wfx as *const WAVEFORMATEXTENSIBLE);
            let sub = ext.SubFormat;
            sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
        } else {
            base.wFormatTag == 3 // WAVE_FORMAT_IEEE_FLOAT
        };
        MixFormatDesc {
            sample_rate: base.nSamplesPerSec,
            channels: base.nChannels,
            bits_per_sample: base.wBitsPerSample,
            is_float,
        }
    }

    #[implement(IActivateAudioInterfaceCompletionHandler)]
    struct ProcessLoopbackHandler {
        event: Arc<HandleGuard>,
        _params: Arc<AUDIOCLIENT_ACTIVATION_PARAMS>,
    }

    impl IActivateAudioInterfaceCompletionHandler_Impl for ProcessLoopbackHandler_Impl {
        fn ActivateCompleted(
            &self,
            _activateoperation: Ref<'_, IActivateAudioInterfaceAsyncOperation>,
        ) -> windows::core::Result<()> {
            unsafe {
                let _ = SetEvent(self.event.0);
            }
            Ok(())
        }
    }

    unsafe fn activate_process_loopback_client(
        target_pid: u32,
    ) -> windows::core::Result<IAudioClient> {
        let params = Arc::new(AUDIOCLIENT_ACTIVATION_PARAMS {
            ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
            Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 {
                ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS {
                    TargetProcessId: target_pid,
                    ProcessLoopbackMode: PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE,
                },
            },
        });
        let params_ptr = Arc::as_ptr(&params) as *mut AUDIOCLIENT_ACTIVATION_PARAMS as *mut u8;
        // PROPVARIANT::drop invokes PropVariantClear, which calls CoTaskMemFree on VT_BLOB.
        // Wrap in ManuallyDrop because the activation blob is Rust-owned (not CoTaskMemAlloc'd).
        let prop = ManuallyDrop::new(PROPVARIANT {
            Anonymous: PROPVARIANT_0 {
                Anonymous: ManuallyDrop::new(PROPVARIANT_0_0 {
                    vt: VT_BLOB,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: PROPVARIANT_0_0_0 {
                        blob: BLOB {
                            cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32,
                            pBlobData: params_ptr,
                        },
                    },
                }),
            },
        });

        let ev = Arc::new(HandleGuard(CreateEventW(None, true, false, None)?));
        let handler: IActivateAudioInterfaceCompletionHandler = ProcessLoopbackHandler {
            event: Arc::clone(&ev),
            _params: Arc::clone(&params),
        }
        .into();
        let operation = ActivateAudioInterfaceAsync(
            VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK,
            &IAudioClient::IID,
            Some(&*prop),
            &handler,
        )?;
        let wait = WaitForSingleObject(ev.0, 3000);
        if wait != WAIT_OBJECT_0 {
            return Err(windows::core::Error::from_hresult(windows::core::HRESULT(
                -1,
            )));
        }
        let mut activate_hr = HRESULT(0);
        let mut activated = None;
        operation.GetActivateResult(&mut activate_hr, &mut activated)?;
        activate_hr.ok()?;
        let punk = activated
            .ok_or_else(|| windows::core::Error::from_hresult(windows::core::HRESULT(-1)))?;
        punk.cast::<IAudioClient>()
    }

    unsafe fn apply_endpoint_mute(enumerator: &IMMDeviceEnumerator) -> &'static str {
        let Ok((device, device_id)) = default_render_device(enumerator) else {
            return LINKED_MUTE_UNSUPPORTED;
        };
        let Ok(ep_vol): windows::core::Result<IAudioEndpointVolume> =
            device.Activate(CLSCTX_ALL, None)
        else {
            return LINKED_MUTE_UNSUPPORTED;
        };
        let prior_muted = ep_vol.GetMute().map(|b| b.as_bool()).unwrap_or(false);
        let (should_mute, we_applied, state) = decide_apply_mute(prior_muted, true, true, false);
        let marker = MuteMarker {
            endpoint_id: device_id,
            prior_muted,
            we_applied_mute: we_applied,
        };
        if we_applied {
            let _ = write_mute_marker_to(&audio_mute_marker_path(), &marker);
        }
        if let Ok(mut g) = controller().active_mute.lock() {
            *g = Some(marker);
        }
        if should_mute {
            if let Err(e) = ep_vol.SetMute(true, std::ptr::null()) {
                debug_log::note(|| format!("AUDIO_MUTE_APPLY err={e}"));
                remove_mute_marker_at(&audio_mute_marker_path());
                if let Ok(mut g) = controller().active_mute.lock() {
                    *g = None;
                }
                return LINKED_MUTE_UNSUPPORTED;
            }
        }
        debug_log::note(|| {
            format!(
                "AUDIO_MUTE_APPLY prior_muted={prior_muted} we_muted={we_applied} state={state}"
            )
        });
        state
    }

    pub fn restore_active_mute(reason: &str) {
        let marker = controller()
            .active_mute
            .lock()
            .ok()
            .and_then(|mut g| g.take());
        let marker_path = audio_mute_marker_path();
        let disk_marker = read_mute_marker_from(&marker_path);
        let effective = marker.as_ref().or(disk_marker.as_ref());
        let Some(m) = effective else {
            remove_mute_marker_at(&marker_path);
            return;
        };
        unsafe {
            let _com = ComGuard::init();
            if let Ok(enumerator) =
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
            {
                let wide_id: Vec<u16> = m.endpoint_id.encode_utf16().chain(Some(0)).collect();
                let dev = enumerator
                    .GetDevice(PCWSTR(wide_id.as_ptr()))
                    .or_else(|_| enumerator.GetDefaultAudioEndpoint(eRender, eConsole));
                if let Ok(device) = dev {
                    if let Ok(ep_vol) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                        let cur = ep_vol.GetMute().ok().map(|b| b.as_bool());
                        if decide_restore_mute(Some(m), cur) {
                            let _ = ep_vol.SetMute(false, std::ptr::null());
                            debug_log::note(|| format!("AUDIO_MUTE_RESTORE source={reason}"));
                        }
                    }
                }
            }
        }
        remove_mute_marker_at(&marker_path);
    }

    pub fn restore_mute_from_crash_marker_win() {
        let marker_path = audio_mute_marker_path();
        let Some(m) = read_mute_marker_from(&marker_path) else {
            return;
        };
        unsafe {
            let _com = ComGuard::init();
            if let Ok(enumerator) =
                CoCreateInstance::<_, IMMDeviceEnumerator>(&MMDeviceEnumerator, None, CLSCTX_ALL)
            {
                let wide_id: Vec<u16> = m.endpoint_id.encode_utf16().chain(Some(0)).collect();
                let dev = enumerator
                    .GetDevice(PCWSTR(wide_id.as_ptr()))
                    .or_else(|_| enumerator.GetDefaultAudioEndpoint(eRender, eConsole));
                if let Ok(device) = dev {
                    if let Ok(ep_vol) = device.Activate::<IAudioEndpointVolume>(CLSCTX_ALL, None) {
                        let cur = ep_vol.GetMute().ok().map(|b| b.as_bool());
                        if decide_restore_mute(Some(&m), cur) {
                            let _ = ep_vol.SetMute(false, std::ptr::null());
                            debug_log::note(|| {
                                "AUDIO_MUTE_RESTORE source=crash_marker".to_string()
                            });
                        }
                    }
                }
            }
        }
        remove_mute_marker_at(&marker_path);
    }

    #[allow(clippy::too_many_arguments)] // Capture worker takes explicit shared atomics and channels.
    pub fn run_loopback_capture(
        stop: Arc<AtomicBool>,
        mute_local: bool,
        session_unsupported: Arc<AtomicBool>,
        audio_out_tx: broadcast::Sender<bytes::Bytes>,
        streaming: Arc<AtomicBool>,
        level_bits: Arc<AtomicU32>,
        capture_gen: Arc<AtomicU64>,
        my_gen: u64,
    ) {
        unsafe {
            let _com = ComGuard::init();
            let Ok(enumerator): windows::core::Result<IMMDeviceEnumerator> =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
            else {
                streaming.store(false, Ordering::Release);
                emit_audio_state(false, 0.0);
                return;
            };

            let our_pid = GetCurrentProcessId();
            let mut seq: u32 = 0;
            let mut resampler = ResamplerState::new();
            let mut pcm_accum: Vec<u8> = Vec::with_capacity(AUDIO_PACKET_BYTES * 4);
            let epoch = Instant::now();

            while !stop.load(Ordering::Acquire) {
                let want_process_loopback =
                    mute_local && !session_unsupported.load(Ordering::Acquire);
                let mut using_process_loopback = false;
                let mut device_id = String::new();
                let mut mix_desc = MixFormatDesc {
                    sample_rate: AUDIO_SAMPLE_RATE,
                    channels: AUDIO_CHANNELS,
                    bits_per_sample: 32,
                    is_float: true,
                };
                let mut event_driven = false;
                let mut event_guard: Option<HandleGuard> = None;

                let audio_client: Option<IAudioClient> = if want_process_loopback {
                    debug_log::note(|| {
                        format!("AUDIO_CAPTURE_ATTEMPT mode=process_loopback pid={our_pid}")
                    });
                    match activate_process_loopback_client(our_pid) {
                        Ok(client) => {
                            let wfx_float = WAVEFORMATEX {
                                wFormatTag: 3, // WAVE_FORMAT_IEEE_FLOAT
                                nChannels: AUDIO_CHANNELS,
                                nSamplesPerSec: AUDIO_SAMPLE_RATE,
                                nAvgBytesPerSec: AUDIO_SAMPLE_RATE * (AUDIO_CHANNELS as u32) * 4,
                                nBlockAlign: AUDIO_CHANNELS * 4,
                                wBitsPerSample: 32,
                                cbSize: 0,
                            };
                            let flags = AUDCLNT_STREAMFLAGS_LOOPBACK
                                | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                                | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY;
                            if client
                                .Initialize(
                                    AUDCLNT_SHAREMODE_SHARED,
                                    flags,
                                    200_000,
                                    0,
                                    &wfx_float,
                                    None,
                                )
                                .is_ok()
                            {
                                using_process_loopback = true;
                                Some(client)
                            } else {
                                let wfx_i16 = WAVEFORMATEX {
                                    wFormatTag: WAVE_FORMAT_PCM as u16,
                                    nChannels: AUDIO_CHANNELS,
                                    nSamplesPerSec: AUDIO_SAMPLE_RATE,
                                    nAvgBytesPerSec: AUDIO_SAMPLE_RATE
                                        * (AUDIO_CHANNELS as u32)
                                        * 2,
                                    nBlockAlign: AUDIO_CHANNELS * 2,
                                    wBitsPerSample: 16,
                                    cbSize: 0,
                                };
                                if client
                                    .Initialize(
                                        AUDCLNT_SHAREMODE_SHARED,
                                        flags,
                                        200_000,
                                        0,
                                        &wfx_i16,
                                        None,
                                    )
                                    .is_ok()
                                {
                                    using_process_loopback = true;
                                    mix_desc.bits_per_sample = 16;
                                    mix_desc.is_float = false;
                                    Some(client)
                                } else {
                                    None
                                }
                            }
                        }
                        Err(e) => {
                            debug_log::note(|| {
                                format!("AUDIO_CAPTURE_MODE mode=endpoint_loopback_fallback reason=activate_err err={e}")
                            });
                            None
                        }
                    }
                } else {
                    None
                };

                let audio_client = match audio_client {
                    Some(c) => c,
                    None => {
                        if want_process_loopback {
                            session_unsupported.store(true, Ordering::Release);
                            send_linked_mute_state(LINKED_MUTE_UNSUPPORTED);
                        } else if mute_local && session_unsupported.load(Ordering::Acquire) {
                            send_linked_mute_state(LINKED_MUTE_UNSUPPORTED);
                        } else {
                            send_linked_mute_state(LINKED_MUTE_UNMUTED);
                        }
                        debug_log::note(|| {
                            format!(
                                "AUDIO_CAPTURE_MODE mode=endpoint_loopback mute_local={mute_local}"
                            )
                        });
                        emit_audio_state(true, 0.0);

                        let Ok((device, id)) = default_render_device(&enumerator) else {
                            std::thread::sleep(Duration::from_millis(250));
                            continue;
                        };
                        device_id = id;
                        let Ok(client): windows::core::Result<IAudioClient> =
                            device.Activate(CLSCTX_ALL, None)
                        else {
                            std::thread::sleep(Duration::from_millis(250));
                            continue;
                        };
                        let Ok(mix_ptr) = client.GetMixFormat() else {
                            std::thread::sleep(Duration::from_millis(250));
                            continue;
                        };
                        let _mix_guard = CoTaskMemGuard(mix_ptr);
                        mix_desc = parse_mix_format(mix_ptr);
                        let init_event = client.Initialize(
                            AUDCLNT_SHAREMODE_SHARED,
                            AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK,
                            200_000,
                            0,
                            mix_ptr,
                            None,
                        );
                        if init_event.is_ok() {
                            if let Ok(ev) = CreateEventW(None, false, false, None) {
                                if client.SetEventHandle(ev).is_ok() {
                                    event_driven = true;
                                    event_guard = Some(HandleGuard(ev));
                                } else {
                                    let _ = CloseHandle(ev);
                                }
                            }
                        } else {
                            let init_poll = client.Initialize(
                                AUDCLNT_SHAREMODE_SHARED,
                                AUDCLNT_STREAMFLAGS_LOOPBACK,
                                200_000,
                                0,
                                mix_ptr,
                                None,
                            );
                            if init_poll.is_err() {
                                std::thread::sleep(Duration::from_millis(250));
                                continue;
                            }
                        }
                        client
                    }
                };

                let Ok(capture_client): windows::core::Result<IAudioCaptureClient> =
                    audio_client.GetService()
                else {
                    if using_process_loopback {
                        session_unsupported.store(true, Ordering::Release);
                    }
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                };
                if audio_client.Start().is_err() {
                    if using_process_loopback {
                        session_unsupported.store(true, Ordering::Release);
                    }
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                }

                if using_process_loopback {
                    debug_log::note(|| {
                        format!("AUDIO_CAPTURE_MODE mode=process_loopback pid={our_pid}")
                    });
                    let mute_state = apply_endpoint_mute(&enumerator);
                    send_linked_mute_state(mute_state);
                    if mute_state == LINKED_MUTE_UNSUPPORTED {
                        session_unsupported.store(true, Ordering::Release);
                    }
                    emit_audio_state(true, 0.0);
                }

                resampler.reset();
                pcm_accum.clear();
                let mut last_device_check = Instant::now();
                let mut last_state_emit = Instant::now();
                let mut last_silence_check = Instant::now();
                let mut silence_guard = SilenceGuardTracker::default();
                let mut silence_guard_pcm_peak = 0.0f32;
                let mut trailing_silent_frames = 0u8;
                let mut peak_window = 0.0f32;

                while !stop.load(Ordering::Acquire) {
                    if event_driven {
                        if let Some(ref ev) = event_guard {
                            let _ = WaitForSingleObject(ev.0, 10);
                        }
                    } else {
                        std::thread::sleep(Duration::from_millis(5));
                    }

                    if !using_process_loopback
                        && last_device_check.elapsed() >= DEVICE_POLL_INTERVAL
                    {
                        last_device_check = Instant::now();
                        if let Some(cur_id) = default_render_id(&enumerator) {
                            if cur_id != device_id {
                                break;
                            }
                        }
                    }

                    let mut invalidated = false;
                    loop {
                        let pkt_size = match capture_client.GetNextPacketSize() {
                            Ok(n) => n,
                            Err(e) => {
                                if e.code() == AUDCLNT_E_DEVICE_INVALIDATED {
                                    invalidated = true;
                                }
                                break;
                            }
                        };
                        if pkt_size == 0 {
                            break;
                        }

                        let mut data_ptr = std::ptr::null_mut();
                        let mut num_frames = 0u32;
                        let mut flags = 0u32;
                        if let Err(e) = capture_client.GetBuffer(
                            &mut data_ptr,
                            &mut num_frames,
                            &mut flags,
                            None,
                            None,
                        ) {
                            if e.code() == AUDCLNT_E_DEVICE_INVALIDATED {
                                invalidated = true;
                            }
                            break;
                        }

                        if num_frames > 0 {
                            let is_silent = (flags & AUDCLNT_BUFFERFLAGS_SILENT_FLAG) != 0
                                || data_ptr.is_null();
                            if is_silent {
                                if trailing_silent_frames < 2 && !pcm_accum.is_empty() {
                                    let pad =
                                        AUDIO_PACKET_BYTES - (pcm_accum.len() % AUDIO_PACKET_BYTES);
                                    if pad < AUDIO_PACKET_BYTES {
                                        pcm_accum.resize(pcm_accum.len() + pad, 0);
                                    }
                                    trailing_silent_frames += 1;
                                } else {
                                    pcm_accum.clear();
                                    resampler.reset();
                                }
                            } else {
                                trailing_silent_frames = 0;
                                let bytes_per_sample = (mix_desc.bits_per_sample / 8) as usize;
                                let block_align =
                                    bytes_per_sample.saturating_mul(mix_desc.channels as usize);
                                if let Some(byte_len) = (num_frames as usize)
                                    .checked_mul(block_align)
                                    .filter(|&n| n > 0 && !data_ptr.is_null())
                                {
                                    let slice = std::slice::from_raw_parts(data_ptr, byte_len);
                                    resampler.convert_to_48k_stereo_i16(
                                        slice,
                                        num_frames as usize,
                                        mix_desc,
                                        &mut pcm_accum,
                                    );
                                }
                            }

                            while pcm_accum.len() >= AUDIO_PACKET_BYTES {
                                let frame: Vec<u8> =
                                    pcm_accum.drain(..AUDIO_PACKET_BYTES).collect();
                                let pk = compute_pcm16_peak(&frame);
                                if pk > peak_window {
                                    peak_window = pk;
                                }
                                if pk > silence_guard_pcm_peak {
                                    silence_guard_pcm_peak = pk;
                                }
                                if let Some(dg) = encode_audio_datagram(seq, &frame) {
                                    let _ = audio_out_tx.send(bytes::Bytes::from(dg));
                                    seq = seq.wrapping_add(1);
                                }
                            }
                        }

                        if let Err(e) = capture_client.ReleaseBuffer(num_frames) {
                            if e.code() == AUDCLNT_E_DEVICE_INVALIDATED {
                                invalidated = true;
                            }
                            break;
                        }
                    }

                    if using_process_loopback
                        && last_silence_check.elapsed() >= Duration::from_millis(200)
                    {
                        last_silence_check = Instant::now();
                        let other_peak = query_render_peak(&enumerator, true, our_pid);
                        let now_ms = epoch.elapsed().as_millis() as u64;
                        let triggered =
                            silence_guard.step(now_ms, other_peak, silence_guard_pcm_peak);
                        silence_guard_pcm_peak = 0.0;
                        if triggered {
                            debug_log::note(|| {
                                "AUDIO_MUTE_FALLBACK reason=runtime_silence_guard_triggered"
                                    .to_string()
                            });
                            restore_active_mute("silence_guard");
                            session_unsupported.store(true, Ordering::Release);
                            send_linked_mute_state(LINKED_MUTE_UNSUPPORTED);
                            emit_audio_state(true, peak_window);
                            break;
                        }
                    }

                    if last_state_emit.elapsed() >= STATE_EMIT_INTERVAL {
                        last_state_emit = Instant::now();
                        level_bits.store(peak_window.to_bits(), Ordering::Release);
                        emit_audio_state(true, peak_window);
                        peak_window = 0.0;
                    }

                    if invalidated {
                        break;
                    }
                }

                let _ = audio_client.Stop();
            }

            if capture_gen.load(Ordering::Acquire) == my_gen {
                restore_active_mute("capture_loop_exit");
                streaming.store(false, Ordering::Release);
                level_bits.store(0.0f32.to_bits(), Ordering::Release);
                emit_audio_state(false, 0.0);
            }
        }
    }

    pub fn run_render_playback(
        stop: Arc<AtomicBool>,
        jitter_buf: Arc<Mutex<JitterBuffer>>,
        streaming: Arc<AtomicBool>,
        level_bits: Arc<AtomicU32>,
    ) {
        unsafe {
            let _com = ComGuard::init();
            let Ok(enumerator): windows::core::Result<IMMDeviceEnumerator> =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
            else {
                streaming.store(false, Ordering::Release);
                emit_audio_state(false, 0.0);
                return;
            };

            let target_fmt = WAVEFORMATEX {
                wFormatTag: WAVE_FORMAT_PCM as u16,
                nChannels: AUDIO_CHANNELS,
                nSamplesPerSec: AUDIO_SAMPLE_RATE,
                nAvgBytesPerSec: AUDIO_SAMPLE_RATE * (AUDIO_CHANNELS as u32) * 2,
                nBlockAlign: AUDIO_CHANNELS * 2,
                wBitsPerSample: 16,
                cbSize: 0,
            };

            while !stop.load(Ordering::Acquire) {
                let Ok((device, device_id)) = default_render_device(&enumerator) else {
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                };
                let Ok(audio_client): windows::core::Result<IAudioClient> =
                    device.Activate(CLSCTX_ALL, None)
                else {
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                };

                let init_res = audio_client.Initialize(
                    AUDCLNT_SHAREMODE_SHARED,
                    AUDCLNT_STREAMFLAGS_EVENTCALLBACK
                        | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM
                        | AUDCLNT_STREAMFLAGS_SRC_DEFAULT_QUALITY,
                    200_000, // 20 ms shared-mode buffer
                    0,
                    &target_fmt,
                    None,
                );
                if init_res.is_err() {
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                }

                let Ok(ev) = CreateEventW(None, false, false, None) else {
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                };
                let ev_guard = HandleGuard(ev);
                if audio_client.SetEventHandle(ev_guard.0).is_err() {
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                }

                let Ok(buffer_frames) = audio_client.GetBufferSize() else {
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                };
                let Ok(render_client): windows::core::Result<IAudioRenderClient> =
                    audio_client.GetService()
                else {
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                };

                if audio_client.Start().is_err() {
                    std::thread::sleep(Duration::from_millis(250));
                    continue;
                }

                let mut pending_pcm: VecDeque<u8> = VecDeque::with_capacity(AUDIO_PACKET_BYTES * 4);
                let mut last_device_check = Instant::now();
                let mut last_state_emit = Instant::now();
                let mut peak_window = 0.0f32;

                while !stop.load(Ordering::Acquire) {
                    let wait = WaitForSingleObject(ev_guard.0, 20);
                    if stop.load(Ordering::Acquire) {
                        break;
                    }

                    if last_device_check.elapsed() >= DEVICE_POLL_INTERVAL {
                        last_device_check = Instant::now();
                        if let Some(cur_id) = default_render_id(&enumerator) {
                            if cur_id != device_id {
                                break;
                            }
                        }
                    }

                    if wait != WAIT_OBJECT_0 {
                        if last_state_emit.elapsed() >= STATE_EMIT_INTERVAL {
                            last_state_emit = Instant::now();
                            level_bits.store(peak_window.to_bits(), Ordering::Release);
                            emit_audio_state(true, peak_window);
                            peak_window = 0.0;
                        }
                        continue;
                    }

                    let padding = match audio_client.GetCurrentPadding() {
                        Ok(p) => p,
                        Err(e) => {
                            if e.code() == AUDCLNT_E_DEVICE_INVALIDATED {
                                break;
                            }
                            continue;
                        }
                    };
                    let available = buffer_frames.saturating_sub(padding);
                    if available == 0 {
                        continue;
                    }

                    let needed_bytes = available as usize * 4;
                    let vol = controller().audio_volume.load(Ordering::Acquire);
                    while pending_pcm.len() < needed_bytes {
                        let mut frame = if let Ok(mut jb) = jitter_buf.lock() {
                            jb.pop()
                        } else {
                            vec![0u8; AUDIO_PACKET_BYTES]
                        };
                        apply_volume_pcm16(&mut frame, vol);
                        let pk = compute_pcm16_peak(&frame);
                        if pk > peak_window {
                            peak_window = pk;
                        }
                        pending_pcm.extend(frame);
                    }

                    match render_client.GetBuffer(available) {
                        Ok(dst_ptr) if !dst_ptr.is_null() => {
                            let dst = std::slice::from_raw_parts_mut(dst_ptr, needed_bytes);
                            for b in dst.iter_mut() {
                                *b = pending_pcm.pop_front().unwrap_or(0);
                            }
                            if let Err(e) = render_client.ReleaseBuffer(available, 0) {
                                if e.code() == AUDCLNT_E_DEVICE_INVALIDATED {
                                    break;
                                }
                            }
                        }
                        Err(e) if e.code() == AUDCLNT_E_DEVICE_INVALIDATED => break,
                        _ => {}
                    }

                    if last_state_emit.elapsed() >= STATE_EMIT_INTERVAL {
                        last_state_emit = Instant::now();
                        level_bits.store(peak_window.to_bits(), Ordering::Release);
                        emit_audio_state(true, peak_window);
                        peak_window = 0.0;
                    }
                }

                let _ = audio_client.Stop();
            }

            streaming.store(false, Ordering::Release);
            level_bits.store(0.0f32.to_bits(), Ordering::Release);
            emit_audio_state(false, 0.0);
        }
    }

    /// Queries peak output level on the default render device, excluding our own PID
    /// when `exclude_own_pid` is true (so bridged playback on Main PC isn't counted
    /// as local Main PC audio).
    unsafe fn query_render_peak(
        enumerator: &IMMDeviceEnumerator,
        exclude_own_pid: bool,
        our_pid: u32,
    ) -> f32 {
        let Ok(dev) = enumerator.GetDefaultAudioEndpoint(eRender, eConsole) else {
            return 0.0;
        };
        if exclude_own_pid {
            if let Ok(mgr) = dev.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) {
                if let Ok(sessions) = mgr.GetSessionEnumerator() {
                    if let Ok(count) = sessions.GetCount() {
                        let mut max_peak = 0.0f32;
                        for i in 0..count {
                            if let Ok(ctrl) = sessions.GetSession(i) {
                                let pid_ok = ctrl
                                    .cast::<IAudioSessionControl2>()
                                    .and_then(|c2| c2.GetProcessId())
                                    .map(|pid| pid != our_pid)
                                    .unwrap_or(true);
                                if pid_ok {
                                    if let Ok(meter) = ctrl.cast::<IAudioMeterInformation>() {
                                        if let Ok(p) = meter.GetPeakValue() {
                                            if p > max_peak {
                                                max_peak = p;
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        return max_peak;
                    }
                }
            }
        }
        dev.Activate::<IAudioMeterInformation>(CLSCTX_ALL, None)
            .and_then(|m| m.GetPeakValue())
            .unwrap_or(0.0)
    }

    pub fn run_local_activity_monitor(stop: Arc<AtomicBool>) {
        unsafe {
            let _com = ComGuard::init();
            let Ok(enumerator): windows::core::Result<IMMDeviceEnumerator> =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
            else {
                return;
            };
            let our_pid = GetCurrentProcessId();
            while !stop.load(Ordering::Acquire) {
                let exclude_self = controller().audio_bridge.load(Ordering::Acquire);
                let peak = query_render_peak(&enumerator, exclude_self, our_pid);
                media_keys::record_local_audio_peak(peak);
                std::thread::sleep(Duration::from_millis(150));
            }
        }
    }

    pub fn run_linked_activity_monitor(
        stop: Arc<AtomicBool>,
        peer_count: Arc<AtomicUsize>,
        outgoing_tx: broadcast::Sender<Outbound>,
    ) {
        unsafe {
            let _com = ComGuard::init();
            let Ok(enumerator): windows::core::Result<IMMDeviceEnumerator> =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
            else {
                return;
            };
            let mut was_active = false;
            let mut last_sent_active = Instant::now() - Duration::from_secs(10);

            while !stop.load(Ordering::Acquire) {
                if peer_count.load(Ordering::Acquire) > 0 {
                    let peak = query_render_peak(&enumerator, false, 0);
                    let active = peak > media_keys::AUDIO_ACTIVITY_THRESHOLD;
                    if active {
                        if !was_active || last_sent_active.elapsed() >= Duration::from_secs(2) {
                            let _ = outgoing_tx
                                .send(Outbound::Packet(KvmPacket::AudioActivity { active: true }));
                            last_sent_active = Instant::now();
                        }
                        was_active = true;
                    } else if was_active {
                        was_active = false;
                        let _ = outgoing_tx
                            .send(Outbound::Packet(KvmPacket::AudioActivity { active: false }));
                    }
                } else {
                    was_active = false;
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_frame(tag: u8) -> Vec<u8> {
        vec![tag; AUDIO_PACKET_BYTES]
    }

    #[test]
    fn jitter_buffer_primes_at_40ms_and_reorders_out_of_order_frames() {
        let mut jb = JitterBuffer::new(JITTER_TARGET_FRAMES, JITTER_MAX_FRAMES);

        // Push 8 frames (40 ms) out of order: 0, 2, 1, 5, 3, 4, 7, 6
        for seq in [0u32, 2, 1, 5, 3, 4, 7, 6] {
            jb.push(seq, test_frame((seq + 1) as u8));
        }

        for expected in 1u8..=8 {
            let out = jb.pop();
            assert_eq!(out.len(), AUDIO_PACKET_BYTES);
            assert_eq!(out[0], expected);
        }
    }

    #[test]
    fn jitter_buffer_waits_for_target_delay_before_priming() {
        let mut jb = JitterBuffer::new(4, 16);
        jb.push(0, test_frame(11));
        jb.push(1, test_frame(22));
        // Fewer than 4 frames and fewer than 4 pop ticks -> outputs silence while priming
        for _ in 0..4 {
            assert!(jb.pop().iter().all(|&b| b == 0));
        }
        // After 4 pop ticks (~target delay), playout begins with seq 0 then seq 1
        assert_eq!(jb.pop()[0], 11);
        assert_eq!(jb.pop()[0], 22);
    }

    #[test]
    fn jitter_buffer_loss_repeats_last_frame_once_then_silence() {
        let mut jb = JitterBuffer::new(2, 16);
        jb.push(0, test_frame(77));
        jb.push(1, test_frame(88));
        // seq 2 and 3 are lost; seq 4 arrives
        jb.push(4, test_frame(99));

        assert_eq!(jb.pop()[0], 77); // seq 0
        assert_eq!(jb.pop()[0], 88); // seq 1
                                     // seq 2 missing -> repeat last frame (88) ONCE
        assert_eq!(jb.pop()[0], 88);
        // seq 3 missing -> silence
        assert!(jb.pop().iter().all(|&b| b == 0));
        // seq 4 arrives and plays normally
        assert_eq!(jb.pop()[0], 99);
    }

    #[test]
    fn jitter_buffer_handles_u32_wraparound() {
        let mut jb = JitterBuffer::new(4, 16);
        let seqs = [u32::MAX - 1, u32::MAX, 0, 1];
        // Deliver slightly out of order across wraparound boundary
        jb.push(seqs[0], test_frame(1));
        jb.push(seqs[2], test_frame(3));
        jb.push(seqs[1], test_frame(2));
        jb.push(seqs[3], test_frame(4));

        for expected in 1u8..=4 {
            assert_eq!(jb.pop()[0], expected);
        }
    }

    #[test]
    fn apply_volume_scales_and_mutes_pcm16() {
        let orig = 10_000i16.to_le_bytes();
        let mut buf = vec![orig[0], orig[1], orig[0], orig[1]];

        let mut half = buf.clone();
        apply_volume_pcm16(&mut half, 50);
        assert_eq!(i16::from_le_bytes([half[0], half[1]]), 5_000);

        let mut full = buf.clone();
        apply_volume_pcm16(&mut full, 100);
        assert_eq!(full, buf);

        apply_volume_pcm16(&mut buf, 0);
        assert!(buf.iter().all(|&b| b == 0));
    }

    #[test]
    fn resampler_converts_float_44100_to_48000_stereo_i16() {
        let mut rs = ResamplerState::new();
        // 441 mono float frames (10 ms at 44.1 kHz) -> ~480 stereo i16 frames (10 ms at 48 kHz)
        let mut input = Vec::with_capacity(441 * 4);
        for i in 0..441 {
            let v = (i as f32 / 441.0).sin() * 0.5;
            input.extend_from_slice(&v.to_le_bytes());
        }
        let mut out = Vec::new();
        rs.convert_to_48k_stereo_i16(
            &input,
            441,
            MixFormatDesc {
                sample_rate: 44_100,
                channels: 1,
                bits_per_sample: 32,
                is_float: true,
            },
            &mut out,
        );
        // Should be close to 480 frames * 4 bytes = 1920 bytes
        assert!(out.len() >= 1880 && out.len() <= 1924, "len={}", out.len());
        assert_eq!(out.len() % 4, 0);
    }

    #[test]
    fn mute_apply_and_restore_decisions_respect_prior_user_mute_and_crash_marker() {
        // Normal mute_local=true when endpoint was unmuted
        assert_eq!(
            decide_apply_mute(false, true, true, false),
            (true, true, LINKED_MUTE_MUTED)
        );
        // Endpoint already muted by user -> do NOT mark we_applied_mute
        assert_eq!(
            decide_apply_mute(true, true, true, false),
            (false, false, LINKED_MUTE_MUTED)
        );
        // mute_local=false -> do not mute, stay unmuted
        assert_eq!(
            decide_apply_mute(false, false, true, false),
            (false, false, LINKED_MUTE_UNMUTED)
        );
        // Process loopback unavailable or session unsupported -> unsupported, do not mute
        assert_eq!(
            decide_apply_mute(false, true, false, false),
            (false, false, LINKED_MUTE_UNSUPPORTED)
        );
        assert_eq!(
            decide_apply_mute(false, true, true, true),
            (false, false, LINKED_MUTE_UNSUPPORTED)
        );

        let we_muted = MuteMarker {
            endpoint_id: "ep-1".into(),
            prior_muted: false,
            we_applied_mute: true,
        };
        assert!(decide_restore_mute(Some(&we_muted), Some(true)));
        assert!(!decide_restore_mute(Some(&we_muted), Some(false)));

        let user_muted = MuteMarker {
            endpoint_id: "ep-1".into(),
            prior_muted: true,
            we_applied_mute: false,
        };
        assert!(!decide_restore_mute(Some(&user_muted), Some(true)));
        assert!(!decide_restore_mute(None, Some(true)));

        // Crash marker roundtrip on temp path
        let dir = std::env::temp_dir().join(format!("anyleap-mute-marker-{}", std::process::id()));
        let marker_path = dir.join("audio_mute_marker.json");
        let _ = std::fs::remove_dir_all(&dir);
        write_mute_marker_to(&marker_path, &we_muted).unwrap();
        let loaded = read_mute_marker_from(&marker_path).unwrap();
        assert_eq!(loaded, we_muted);
        assert!(decide_restore_mute(Some(&loaded), Some(true)));
        remove_mute_marker_at(&marker_path);
        assert!(read_mute_marker_from(&marker_path).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn silence_guard_triggers_only_after_continuous_1s_mismatch() {
        let mut tracker = SilenceGuardTracker::new(1000);
        // Both active -> no trigger
        assert!(!tracker.step(0, 0.1, 0.05));
        // Other active, capture silent at t=100ms -> starts timer
        assert!(!tracker.step(100, 0.2, 0.0));
        // Still silent at t=900ms (800ms elapsed) -> not yet
        assert!(!tracker.step(900, 0.2, 0.0001));
        // Capture sees real audio at t=950ms -> resets timer
        assert!(!tracker.step(950, 0.2, 0.02));
        // Goes silent again at t=1000ms -> restarts from 1000ms
        assert!(!tracker.step(1000, 0.2, 0.0));
        assert!(!tracker.step(1999, 0.2, 0.0));
        // Reaches 1000ms continuous mismatch at t=2000ms -> triggers!
        assert!(tracker.step(2000, 0.2, 0.0));
    }

    #[cfg(windows)]
    fn encode_ps_utf16le_base64(script: &str) -> String {
        const TBL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut bytes = Vec::with_capacity(script.len() * 2);
        for u in script.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0] as usize;
            let b1 = *chunk.get(1).unwrap_or(&0) as usize;
            let b2 = *chunk.get(2).unwrap_or(&0) as usize;
            let n = (b0 << 16) | (b1 << 8) | b2;
            out.push(TBL[(n >> 18) & 63] as char);
            out.push(TBL[(n >> 12) & 63] as char);
            out.push(if chunk.len() > 1 {
                TBL[(n >> 6) & 63] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                TBL[n & 63] as char
            } else {
                '='
            });
        }
        out
    }

    #[cfg(windows)]
    fn query_default_endpoint_muted() -> Option<(String, bool)> {
        use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
        use windows::Win32::Media::Audio::{
            eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
        };
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
            COINIT_MULTITHREADED,
        };
        unsafe {
            let com_ok = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let res = (|| -> Option<(String, bool)> {
                let enumerator: IMMDeviceEnumerator =
                    CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
                let dev = enumerator.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
                let id_pwstr = dev.GetId().ok()?;
                let mut len = 0usize;
                while *id_pwstr.0.add(len) != 0 {
                    len += 1;
                }
                let id = String::from_utf16_lossy(std::slice::from_raw_parts(id_pwstr.0, len));
                CoTaskMemFree(Some(id_pwstr.0 as *const _));
                let ep_vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).ok()?;
                let muted = ep_vol.GetMute().ok()?.as_bool();
                Some((id, muted))
            })();
            if com_ok {
                CoUninitialize();
            }
            res
        }
    }

    #[cfg(windows)]
    #[ignore]
    #[test]
    fn repro_process_loopback_capture_with_mute_local() {
        use std::io::Write;
        // Generate a 1-second 440 Hz stereo 16-bit 48 kHz PCM WAV in temp dir.
        let wav_path = std::env::temp_dir().join("anyleap-repro-tone.wav");
        {
            let sample_rate = 48_000u32;
            let channels = 2u16;
            let bits = 16u16;
            let num_samples = sample_rate as usize; // 1 second
            let data_bytes = (num_samples * channels as usize * (bits as usize / 8)) as u32;
            let mut f = std::fs::File::create(&wav_path).unwrap();
            f.write_all(b"RIFF").unwrap();
            f.write_all(&(36 + data_bytes).to_le_bytes()).unwrap();
            f.write_all(b"WAVEfmt ").unwrap();
            f.write_all(&16u32.to_le_bytes()).unwrap();
            f.write_all(&1u16.to_le_bytes()).unwrap(); // PCM
            f.write_all(&channels.to_le_bytes()).unwrap();
            f.write_all(&sample_rate.to_le_bytes()).unwrap();
            f.write_all(&(sample_rate * channels as u32 * 2).to_le_bytes())
                .unwrap();
            f.write_all(&(channels * 2).to_le_bytes()).unwrap();
            f.write_all(&bits.to_le_bytes()).unwrap();
            f.write_all(b"data").unwrap();
            f.write_all(&data_bytes.to_le_bytes()).unwrap();
            for i in 0..num_samples {
                let s = ((i as f64 * 440.0 * 2.0 * std::f64::consts::PI / sample_rate as f64).sin()
                    * 12000.0) as i16;
                f.write_all(&s.to_le_bytes()).unwrap();
                f.write_all(&s.to_le_bytes()).unwrap();
            }
        }

        // Launch playback via explorer's Shell.Application so it runs in the interactive
        // desktop session outside our process tree (not excluded by process loopback).
        let wav_escaped = wav_path.to_string_lossy().replace('\'', "''");
        let player_script = format!(
            "$p = New-Object Media.SoundPlayer '{wav_escaped}'; $p.PlayLooping(); Start-Sleep 12"
        );
        let encoded = encode_ps_utf16le_base64(&player_script);
        let launch_cmd = format!(
            "(New-Object -ComObject Shell.Application).ShellExecute('powershell.exe', '-NoProfile -WindowStyle Hidden -EncodedCommand {encoded}', '', 'open', 0)"
        );
        let spawn_out = std::process::Command::new("powershell")
            .args(["-NoProfile", "-Command", &launch_cmd])
            .output();
        eprintln!(
            "repro: ShellExecute spawn status={:?}",
            spawn_out.as_ref().map(|o| o.status)
        );
        std::thread::sleep(Duration::from_millis(500));

        let stop = Arc::new(AtomicBool::new(false));
        let session_unsupported = Arc::new(AtomicBool::new(false));
        let (audio_out_tx, mut audio_out_rx) = broadcast::channel::<bytes::Bytes>(256);
        let streaming = Arc::new(AtomicBool::new(true));
        let level_bits = Arc::new(AtomicU32::new(0));
        let capture_gen = Arc::new(AtomicU64::new(1));

        let stop_c = Arc::clone(&stop);
        let unsupported_c = Arc::clone(&session_unsupported);
        let streaming_c = Arc::clone(&streaming);
        let level_c = Arc::clone(&level_bits);
        let gen_c = Arc::clone(&capture_gen);

        let handle = std::thread::spawn(move || {
            wasapi::run_loopback_capture(
                stop_c,
                true,
                unsupported_c,
                audio_out_tx,
                streaming_c,
                level_c,
                gen_c,
                1,
            );
        });

        let start = Instant::now();
        let mut total_packets = 0usize;
        let mut non_silent_packets = 0usize;
        let mut observed_muted = false;
        while start.elapsed() < Duration::from_secs(10) {
            if !observed_muted && start.elapsed() >= Duration::from_secs(1) {
                observed_muted = query_default_endpoint_muted().is_some_and(|(_, m)| m);
            }
            match audio_out_rx.try_recv() {
                Ok(dg) => {
                    total_packets += 1;
                    if dg.len() > 7 {
                        let pk = compute_pcm16_peak(&dg[7..]);
                        if pk > CAPTURE_SILENCE_THRESHOLD {
                            non_silent_packets += 1;
                        }
                    }
                }
                Err(broadcast::error::TryRecvError::Empty) => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                Err(broadcast::error::TryRecvError::Lagged(n)) => {
                    total_packets += n as usize;
                }
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }

        stop.store(true, Ordering::Release);
        let _ = handle.join();
        wasapi::restore_active_mute("repro_test_end");
        let _ = std::fs::remove_file(&wav_path);

        let muted_after = query_default_endpoint_muted().map(|(_, m)| m);
        let unsupported = session_unsupported.load(Ordering::Acquire);
        eprintln!(
            "repro: finished 10s total_packets={total_packets} non_silent_packets={non_silent_packets} observed_muted={observed_muted} muted_after={muted_after:?} unsupported={unsupported}"
        );
        assert!(total_packets > 0, "expected captured audio packets");
        assert!(
            non_silent_packets > 0 || unsupported,
            "expected either non-silent PCM while muted or clean fallback to unsupported"
        );
        assert_eq!(
            muted_after,
            Some(false),
            "endpoint mute must be restored after stop"
        );
    }

    #[cfg(windows)]
    #[ignore]
    #[test]
    fn verify_crash_marker_restores_endpoint_mute_on_startup() {
        use windows::Win32::Media::Audio::Endpoints::IAudioEndpointVolume;
        use windows::Win32::Media::Audio::{
            eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator,
        };
        use windows::Win32::System::Com::{
            CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_ALL, COINIT_MULTITHREADED,
        };

        let (ep_id, prior_muted) = query_default_endpoint_muted().expect("default render endpoint");
        assert!(
            !prior_muted,
            "precondition: default render endpoint must start unmuted"
        );

        unsafe {
            let com_ok = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).unwrap();
            let dev = enumerator
                .GetDefaultAudioEndpoint(eRender, eConsole)
                .unwrap();
            let ep_vol: IAudioEndpointVolume = dev.Activate(CLSCTX_ALL, None).unwrap();
            ep_vol.SetMute(true, std::ptr::null()).unwrap();
            if com_ok {
                CoUninitialize();
            }
        }

        let marker = MuteMarker {
            endpoint_id: ep_id,
            prior_muted: false,
            we_applied_mute: true,
        };
        let marker_path = audio_mute_marker_path();
        write_mute_marker_to(&marker_path, &marker).unwrap();

        restore_mute_from_crash_marker();

        let after = query_default_endpoint_muted().map(|(_, m)| m);
        assert_eq!(
            after,
            Some(false),
            "startup crash-marker recovery must unmute endpoint"
        );
        assert!(
            !marker_path.exists(),
            "crash marker file must be removed after recovery"
        );
    }
}
