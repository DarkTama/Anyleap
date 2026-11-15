use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

pub const VK_VOLUME_MUTE: u16 = 0xAD;
pub const VK_VOLUME_DOWN: u16 = 0xAE;
pub const VK_VOLUME_UP: u16 = 0xAF;
pub const VK_MEDIA_NEXT_TRACK: u16 = 0xB0;
pub const VK_MEDIA_PREV_TRACK: u16 = 0xB1;
pub const VK_MEDIA_STOP: u16 = 0xB2;
pub const VK_MEDIA_PLAY_PAUSE: u16 = 0xB3;

/// Recency window within which audible output captures media keys (10 seconds).
pub const AUDIO_ACTIVITY_WINDOW_MS: u64 = 10_000;

/// Peak meter threshold above background noise floor.
pub const AUDIO_ACTIVITY_THRESHOLD: f32 = 0.005;

static FOLLOW_AUDIO: AtomicBool = AtomicBool::new(true);
static AUDIO_STREAMING: AtomicBool = AtomicBool::new(false);
/// Monotonic millisecond timestamps (`0` = never observed).
static LOCAL_LAST_AUDIO_MS: AtomicU64 = AtomicU64::new(0);
static LINKED_LAST_AUDIO_MS: AtomicU64 = AtomicU64::new(0);
static MONOTONIC_EPOCH: OnceLock<Instant> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaKeyTarget {
    Local,
    Linked,
}

fn monotonic_ms() -> u64 {
    let epoch = MONOTONIC_EPOCH.get_or_init(Instant::now);
    (Instant::now().duration_since(*epoch).as_millis() as u64).saturating_add(1)
}

/// Returns true for Windows media and volume control virtual-key codes.
#[inline]
pub fn is_media_key(vk: u16) -> bool {
    matches!(
        vk,
        VK_VOLUME_MUTE
            | VK_VOLUME_DOWN
            | VK_VOLUME_UP
            | VK_MEDIA_NEXT_TRACK
            | VK_MEDIA_PREV_TRACK
            | VK_MEDIA_STOP
            | VK_MEDIA_PLAY_PAUSE
    )
}

#[inline]
pub fn is_volume_key(vk: u16) -> bool {
    matches!(vk, VK_VOLUME_MUTE | VK_VOLUME_DOWN | VK_VOLUME_UP)
}

/// Pure media-key routing decision parameterized by virtual-key code and
/// whether audio bridge playback is currently streaming:
/// - While bridging (`audio_streaming == true`), volume keys (`Mute/Down/Up`)
///   ALWAYS stay on the Main PC (`Local`) so they adjust bridged playback rather
///   than muting/unmuting the muted Linked PC endpoint.
/// - Transport keys (`Play/Pause`, `Next`, `Prev`, `Stop`) always follow
///   `decide_media_key_target`.
#[allow(clippy::too_many_arguments)] // Pure decision function takes explicit state snapshot fields.
pub fn decide_media_key_target_for_vk(
    vk: u16,
    audio_streaming: bool,
    follow_audio: bool,
    cursor_on_linked: bool,
    local_last_active_ms: Option<u64>,
    linked_last_active_ms: Option<u64>,
    now_ms: u64,
    window_ms: u64,
) -> MediaKeyTarget {
    if audio_streaming && is_volume_key(vk) {
        return MediaKeyTarget::Local;
    }
    decide_media_key_target(
        follow_audio,
        cursor_on_linked,
        local_last_active_ms,
        linked_last_active_ms,
        now_ms,
        window_ms,
    )
}

/// Pure media-key routing decision:
/// - When `follow_audio` is true, route to whichever PC produced audible output
///   most recently within `window_ms`.
/// - Otherwise (or when neither PC had recent audio, or on an exact tie), route
///   to whichever PC currently holds the cursor.
pub fn decide_media_key_target(
    follow_audio: bool,
    cursor_on_linked: bool,
    local_last_active_ms: Option<u64>,
    linked_last_active_ms: Option<u64>,
    now_ms: u64,
    window_ms: u64,
) -> MediaKeyTarget {
    let cursor_target = if cursor_on_linked {
        MediaKeyTarget::Linked
    } else {
        MediaKeyTarget::Local
    };

    if !follow_audio {
        return cursor_target;
    }

    let local_recent = local_last_active_ms.filter(|&t| now_ms.saturating_sub(t) <= window_ms);
    let linked_recent = linked_last_active_ms.filter(|&t| now_ms.saturating_sub(t) <= window_ms);

    match (local_recent, linked_recent) {
        (Some(lt), Some(rt)) => {
            if rt > lt {
                MediaKeyTarget::Linked
            } else if lt > rt {
                MediaKeyTarget::Local
            } else {
                cursor_target
            }
        }
        (None, Some(_)) => MediaKeyTarget::Linked,
        (Some(_), None) => MediaKeyTarget::Local,
        (None, None) => cursor_target,
    }
}

pub fn set_follow_audio(enabled: bool) {
    FOLLOW_AUDIO.store(enabled, Ordering::Release);
}

pub fn set_audio_streaming(streaming: bool) {
    AUDIO_STREAMING.store(streaming, Ordering::Release);
}

pub fn follow_audio_enabled() -> bool {
    FOLLOW_AUDIO.load(Ordering::Acquire)
}

pub fn record_local_audio_peak(peak: f32) {
    if peak > AUDIO_ACTIVITY_THRESHOLD {
        LOCAL_LAST_AUDIO_MS.store(monotonic_ms(), Ordering::Release);
    }
}

pub fn record_linked_audio_activity(active: bool) {
    if active {
        LINKED_LAST_AUDIO_MS.store(monotonic_ms(), Ordering::Release);
    }
}

pub fn reset_audio_activity() {
    LOCAL_LAST_AUDIO_MS.store(0, Ordering::Release);
    LINKED_LAST_AUDIO_MS.store(0, Ordering::Release);
}

pub fn reset_linked_audio_activity() {
    LINKED_LAST_AUDIO_MS.store(0, Ordering::Release);
}

/// Lock-free routing lookup safe to call from `low_level_keyboard_proc`.
#[inline]
pub fn current_media_key_target(cursor_on_linked: bool) -> MediaKeyTarget {
    current_media_key_target_for_vk(0, cursor_on_linked)
}

/// Lock-free routing lookup with VK-specific volume-while-bridging override.
#[inline]
pub fn current_media_key_target_for_vk(vk: u16, cursor_on_linked: bool) -> MediaKeyTarget {
    let streaming = AUDIO_STREAMING.load(Ordering::Acquire);
    let follow = FOLLOW_AUDIO.load(Ordering::Acquire);
    let local_raw = LOCAL_LAST_AUDIO_MS.load(Ordering::Acquire);
    let linked_raw = LINKED_LAST_AUDIO_MS.load(Ordering::Acquire);
    let now = monotonic_ms();

    decide_media_key_target_for_vk(
        vk,
        streaming,
        follow,
        cursor_on_linked,
        (local_raw != 0).then_some(local_raw),
        (linked_raw != 0).then_some(linked_raw),
        now,
        AUDIO_ACTIVITY_WINDOW_MS,
    )
}

/// Inject a received media key on the Linked PC (called from async packet loop, never a hook).
pub fn inject_media_key(vk: u16, is_down: bool) {
    if is_media_key(vk) {
        let _ = super::injector::inject_key(vk, 0, is_down, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_all_media_and_volume_vks() {
        for vk in [
            VK_VOLUME_MUTE,
            VK_VOLUME_DOWN,
            VK_VOLUME_UP,
            VK_MEDIA_NEXT_TRACK,
            VK_MEDIA_PREV_TRACK,
            VK_MEDIA_STOP,
            VK_MEDIA_PLAY_PAUSE,
        ] {
            assert!(is_media_key(vk), "expected 0x{vk:X} to be a media key");
        }
        assert!(!is_media_key(0x41)); // 'A'
        assert!(!is_media_key(0x1B)); // Esc
        assert!(!is_media_key(0x91)); // ScrollLock
    }

    #[test]
    fn follow_audio_disabled_routes_by_cursor() {
        assert_eq!(
            decide_media_key_target(false, false, Some(100), Some(9_900), 10_000, 10_000),
            MediaKeyTarget::Local
        );
        assert_eq!(
            decide_media_key_target(false, true, Some(9_900), Some(100), 10_000, 10_000),
            MediaKeyTarget::Linked
        );
    }

    #[test]
    fn follow_audio_prefers_most_recent_active_pc_within_window() {
        let w = AUDIO_ACTIVITY_WINDOW_MS;
        // Linked active more recently, even though cursor is on Main PC
        assert_eq!(
            decide_media_key_target(true, false, Some(20_000), Some(24_000), 25_000, w),
            MediaKeyTarget::Linked
        );
        // Main active more recently, even though cursor is on Linked PC
        assert_eq!(
            decide_media_key_target(true, true, Some(24_500), Some(22_000), 25_000, w),
            MediaKeyTarget::Local
        );
        // Only Linked active
        assert_eq!(
            decide_media_key_target(true, false, None, Some(24_000), 25_000, w),
            MediaKeyTarget::Linked
        );
        // Only Local active
        assert_eq!(
            decide_media_key_target(true, true, Some(24_000), None, 25_000, w),
            MediaKeyTarget::Local
        );
    }

    #[test]
    fn expired_activity_falls_back_to_cursor_position() {
        let w = AUDIO_ACTIVITY_WINDOW_MS;
        // Linked activity was 10,001 ms ago -> expired
        assert_eq!(
            decide_media_key_target(true, false, None, Some(10_000), 20_001, w),
            MediaKeyTarget::Local
        );
        assert_eq!(
            decide_media_key_target(true, true, Some(5_000), Some(10_000), 20_001, w),
            MediaKeyTarget::Linked
        );
        // Exactly at 10,000 ms boundary -> still active
        assert_eq!(
            decide_media_key_target(true, false, None, Some(10_000), 20_000, w),
            MediaKeyTarget::Linked
        );
    }

    #[test]
    fn volume_keys_always_stay_local_while_audio_bridge_is_streaming() {
        let w = AUDIO_ACTIVITY_WINDOW_MS;
        for vk in [VK_VOLUME_MUTE, VK_VOLUME_DOWN, VK_VOLUME_UP] {
            // Even when Linked was recently active and cursor is on Linked, volume stays Local while bridging
            assert_eq!(
                decide_media_key_target_for_vk(vk, true, true, true, None, Some(24_000), 25_000, w),
                MediaKeyTarget::Local
            );
            // Even with follow_audio = false and cursor on Linked, volume stays Local while bridging
            assert_eq!(
                decide_media_key_target_for_vk(vk, true, false, true, None, None, 25_000, w),
                MediaKeyTarget::Local
            );
            // When NOT bridging, volume follows normal active-audio / cursor rules
            assert_eq!(
                decide_media_key_target_for_vk(
                    vk,
                    false,
                    true,
                    false,
                    None,
                    Some(24_000),
                    25_000,
                    w
                ),
                MediaKeyTarget::Linked
            );
        }
        // Transport keys still follow active audio / cursor while bridging
        for vk in [
            VK_MEDIA_PLAY_PAUSE,
            VK_MEDIA_NEXT_TRACK,
            VK_MEDIA_PREV_TRACK,
            VK_MEDIA_STOP,
        ] {
            assert_eq!(
                decide_media_key_target_for_vk(
                    vk,
                    true,
                    true,
                    false,
                    None,
                    Some(24_000),
                    25_000,
                    w
                ),
                MediaKeyTarget::Linked
            );
            assert_eq!(
                decide_media_key_target_for_vk(
                    vk,
                    true,
                    true,
                    false,
                    Some(24_500),
                    Some(20_000),
                    25_000,
                    w
                ),
                MediaKeyTarget::Local
            );
        }
    }
}
