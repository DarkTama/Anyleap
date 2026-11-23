//! Flex virtual-display post-wake stall detection and in-place recovery.
//!
//! When a phone sleeps and wakes during a Flex (`--new-display`) session, the
//! scrcpy process often stays alive while its video stream stalls indefinitely
//! at `INFO: 0 fps`. On a static screen with no sleep/lock cycle, `0 fps` is
//! normal (scrcpy only encodes frames when display content changes). This module
//! pairs `--print-fps` output from the scrcpy child with filtered on-device
//! power/keyguard probes so stalled Flex sessions restart in place once per
//! sleep/lock -> awake+unlocked transition, never restarting on static screens
//! and applying exponential back-off across repeated wake stalls.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};
use tokio::sync::mpsc;

use crate::state::AppState;

/// Consecutive `0 fps` ticks while awake & unlocked after a sleep/lock cycle.
pub const POST_WAKE_STALL_SECS: u32 = 3;
/// Base cooldown (seconds) after an automatic in-place restart.
pub const BASE_BACKOFF_SECS: u64 = 10;
/// Maximum cooldown (seconds) between automatic restarts.
pub const MAX_BACKOFF_SECS: u64 = 120;
/// Consecutive non-zero FPS reports required to consider a restarted session healthy.
pub const HEALTHY_FRAMES_TO_RESET_BACKOFF: u32 = 3;
/// Poll cadence for phone power/keyguard state while a Flex session is active.
pub const SCREEN_POLL_INTERVAL: Duration = Duration::from_secs(3);
const POWER_PROBE_TIMEOUT: Duration = Duration::from_secs(2);

const POWER_PROBE_SCRIPT: &str = concat!(
    "dumpsys power | grep -E 'mWakefulness=|Display Power: state='; ",
    "echo __FL_WIN__; ",
    "dumpsys window policy 2>/dev/null | grep -E 'mDreamingLockscreen=|mShowingLockscreen=|isStatusBarKeyguard=|mKeyguardShowing=|KeyguardStateMonitor: mIsShowing=|KeyguardShowing='; ",
    "true",
);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PhonePowerState {
    /// Awake and unlocked (and physical panel on unless `--turn-screen-off` is active).
    Active,
    /// Phone is asleep/dozing (or physical panel off when `--turn-screen-off` is false).
    Asleep,
    /// Phone is awake on the lockscreen / keyguard.
    Locked,
    /// State could not be determined (ADB failure or unparseable output).
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StallDecision {
    KeepAlive,
    RestartInPlace,
}

#[derive(Debug, Clone, Copy)]
pub enum FpsWatchEvent {
    FpsCounterStarted,
    FpsSample(u32),
}

#[derive(Debug, Clone, Copy, Default)]
struct BackoffRecord {
    consecutive_restarts: u32,
    last_restart_ms: Option<u64>,
}

static BACKOFF_STATE: OnceLock<Mutex<HashMap<String, BackoffRecord>>> = OnceLock::new();

fn backoff_map() -> &'static Mutex<HashMap<String, BackoffRecord>> {
    BACKOFF_STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn get_backoff_for_serial(serial: &str) -> (u32, Option<u64>) {
    backoff_map()
        .lock()
        .ok()
        .and_then(|m| m.get(serial).copied())
        .map(|r| (r.consecutive_restarts, r.last_restart_ms))
        .unwrap_or((0, None))
}

pub fn record_restart_for_serial(serial: &str, at_ms: u64) -> u32 {
    if let Ok(mut map) = backoff_map().lock() {
        let entry = map.entry(serial.to_string()).or_default();
        entry.consecutive_restarts = entry.consecutive_restarts.saturating_add(1);
        entry.last_restart_ms = Some(at_ms);
        return entry.consecutive_restarts;
    }
    1
}

pub fn reset_backoff_for_serial(serial: &str) {
    if let Ok(mut map) = backoff_map().lock() {
        map.remove(serial);
    }
}

/// Exponential back-off in seconds after `consecutive_restarts` restarts:
/// `0 -> 0`, `1 -> 10`, `2 -> 20`, `3 -> 40`, `4 -> 80`, `>= 5 -> 120`.
pub fn restart_backoff_secs(consecutive_restarts: u32) -> u64 {
    if consecutive_restarts == 0 {
        return 0;
    }
    let shift = (consecutive_restarts - 1).min(10);
    BASE_BACKOFF_SECS
        .saturating_mul(1u64 << shift)
        .min(MAX_BACKOFF_SECS)
}

/// True when scrcpy's `--print-fps` counter has started emitting samples.
pub fn is_fps_counter_started(chunk: &str) -> bool {
    chunk.contains("FPS counter started")
}

/// Parse a single scrcpy `--print-fps` line such as `INFO: 0 fps` or
/// `INFO: 27 fps (+2 frames skipped)`.
pub fn parse_fps_line(line: &str) -> Option<u32> {
    let after_info = line.split("INFO:").nth(1)?.trim();
    let (num_str, rest) = after_info.split_once(" fps")?;
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) && !rest.starts_with('(') {
        return None;
    }
    num_str.trim().parse::<u32>().ok()
}

/// Extract all FPS samples from a stdout/stderr chunk in order.
pub fn parse_fps_samples(chunk: &str) -> Vec<u32> {
    chunk.lines().filter_map(parse_fps_line).collect()
}

/// Parse phone wakefulness and lockscreen state from filtered `dumpsys power` + `dumpsys window policy`.
pub fn parse_phone_power_state(
    power: &str,
    window: &str,
    turn_screen_off: bool,
) -> PhonePowerState {
    let mut wakefulness: Option<bool> = None;
    let mut display_on: Option<bool> = None;

    for raw in power.lines() {
        let line = raw.trim();
        if let Some(val) = line.strip_prefix("mWakefulness=") {
            let v = val.trim();
            if v.eq_ignore_ascii_case("Awake") {
                wakefulness = Some(true);
            } else if v.eq_ignore_ascii_case("Asleep") || v.eq_ignore_ascii_case("Dozing") {
                wakefulness = Some(false);
            }
        } else if line.contains("Display Power: state=OFF")
            || line.contains("Display Power: state=DOZE")
        {
            display_on = Some(false);
        } else if line.contains("Display Power: state=ON") {
            display_on = Some(true);
        }
    }

    if wakefulness == Some(false) {
        return PhonePowerState::Asleep;
    }

    let combined = format!("{power}\n{window}");
    let locked = combined.contains("mDreamingLockscreen=true")
        || combined.contains("mShowingLockscreen=true")
        || combined.contains("isStatusBarKeyguard=true")
        || combined.contains("mKeyguardShowing=true")
        || combined.contains("KeyguardStateMonitor: mIsShowing=true")
        || combined.contains("KeyguardShowing=true");

    if locked {
        return PhonePowerState::Locked;
    }

    match wakefulness {
        Some(true) => {
            if !turn_screen_off && display_on == Some(false) {
                PhonePowerState::Asleep
            } else {
                PhonePowerState::Active
            }
        }
        Some(false) => PhonePowerState::Asleep,
        None => match display_on {
            Some(false) if !turn_screen_off => PhonePowerState::Asleep,
            Some(true) => PhonePowerState::Active,
            _ => PhonePowerState::Unknown,
        },
    }
}

/// Pure stall detector state machine for a single Flex session.
#[derive(Debug, Clone)]
pub struct FlexStallDetector {
    pub fps_started: bool,
    pub power: PhonePowerState,
    pub saw_sleep_or_lock: bool,
    pub active_zero_fps: u32,
    pub consecutive_nonzero_fps: u32,
    pub consecutive_restarts: u32,
    pub last_restart_ms: Option<u64>,
}

impl FlexStallDetector {
    pub fn new(consecutive_restarts: u32, last_restart_ms: Option<u64>) -> Self {
        Self {
            fps_started: false,
            power: PhonePowerState::Unknown,
            saw_sleep_or_lock: false,
            active_zero_fps: 0,
            consecutive_nonzero_fps: 0,
            consecutive_restarts,
            last_restart_ms,
        }
    }

    pub fn on_fps_started(&mut self) {
        self.fps_started = true;
    }

    /// Update the phone power state and re-evaluate whether a stall restart should fire.
    pub fn on_power_state(&mut self, next: PhonePowerState, now_ms: u64) -> StallDecision {
        self.power = next;
        match next {
            PhonePowerState::Asleep | PhonePowerState::Locked => {
                self.saw_sleep_or_lock = true;
                self.active_zero_fps = 0;
                StallDecision::KeepAlive
            }
            PhonePowerState::Unknown => {
                self.active_zero_fps = 0;
                StallDecision::KeepAlive
            }
            PhonePowerState::Active => self.evaluate(now_ms),
        }
    }

    /// Feed a `--print-fps` sample. Returns `true` when enough healthy frames
    /// arrived to reset any prior restart back-off.
    pub fn on_fps_sample(&mut self, fps: u32, now_ms: u64) -> (StallDecision, bool) {
        self.fps_started = true;
        if fps > 0 {
            self.active_zero_fps = 0;
            self.saw_sleep_or_lock = false;
            self.consecutive_nonzero_fps = self.consecutive_nonzero_fps.saturating_add(1);
            let mut reset_backoff = false;
            if self.consecutive_nonzero_fps >= HEALTHY_FRAMES_TO_RESET_BACKOFF
                && (self.consecutive_restarts > 0 || self.last_restart_ms.is_some())
            {
                self.consecutive_restarts = 0;
                self.last_restart_ms = None;
                reset_backoff = true;
            }
            return (StallDecision::KeepAlive, reset_backoff);
        }

        self.consecutive_nonzero_fps = 0;
        if self.power == PhonePowerState::Active {
            self.active_zero_fps = self.active_zero_fps.saturating_add(1);
            (self.evaluate(now_ms), false)
        } else {
            self.active_zero_fps = 0;
            (StallDecision::KeepAlive, false)
        }
    }

    fn evaluate(&mut self, now_ms: u64) -> StallDecision {
        if !self.fps_started
            || self.power != PhonePowerState::Active
            || !self.saw_sleep_or_lock
            || self.active_zero_fps < POST_WAKE_STALL_SECS
        {
            return StallDecision::KeepAlive;
        }
        if let Some(prev_ms) = self.last_restart_ms {
            let cooldown_ms = restart_backoff_secs(self.consecutive_restarts).saturating_mul(1000);
            if now_ms.saturating_sub(prev_ms) < cooldown_ms {
                return StallDecision::KeepAlive;
            }
        }
        // Consume the wake cycle so at most one automatic restart fires per wake transition.
        self.saw_sleep_or_lock = false;
        self.active_zero_fps = 0;
        StallDecision::RestartInPlace
    }
}

async fn query_phone_power_state(
    app: &AppHandle,
    serial: &str,
    turn_screen_off: bool,
) -> PhonePowerState {
    // adb_cmd uses tauri_plugin_shell::process::Command, which sets CREATE_NO_WINDOW on Windows.
    let Ok(cmd) = crate::commands::adb_cmd(app).await else {
        return PhonePowerState::Unknown;
    };
    let Ok(Ok(out)) = tokio::time::timeout(
        POWER_PROBE_TIMEOUT,
        cmd.args(["-s", serial, "shell", POWER_PROBE_SCRIPT])
            .output(),
    )
    .await
    else {
        return PhonePowerState::Unknown;
    };
    if !out.status.success() && out.stdout.is_empty() {
        return PhonePowerState::Unknown;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    let (power, window) = stdout.split_once("__FL_WIN__").unwrap_or((&stdout, ""));
    parse_phone_power_state(power, window, turn_screen_off)
}

fn session_still_active(app: &AppHandle, session_id: &str) -> bool {
    app.try_state::<AppState>()
        .and_then(|st| st.sessions.lock().ok().map(|m| m.contains_key(session_id)))
        .unwrap_or(false)
}

/// Spawn a background stall monitor for an active Flex display session.
/// Returns a channel sender for feeding `--print-fps` events from scrcpy output.
pub fn spawn_flex_watcher(
    app: AppHandle,
    session_id: String,
    serial: String,
    turn_screen_off: bool,
) -> mpsc::UnboundedSender<FpsWatchEvent> {
    let (tx, mut rx) = mpsc::unbounded_channel::<FpsWatchEvent>();

    tauri::async_runtime::spawn(async move {
        let (restarts, last_ms) = get_backoff_for_serial(&serial);
        let mut detector = FlexStallDetector::new(restarts, last_ms);
        let mut poll_tick = tokio::time::interval(SCREEN_POLL_INTERVAL);
        poll_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                maybe_ev = rx.recv() => {
                    let Some(ev) = maybe_ev else {
                        break;
                    };
                    let ts = now_ms();
                    let decision = match ev {
                        FpsWatchEvent::FpsCounterStarted => {
                            detector.on_fps_started();
                            StallDecision::KeepAlive
                        }
                        FpsWatchEvent::FpsSample(fps) => {
                            let (dec, reset) = detector.on_fps_sample(fps, ts);
                            if reset {
                                reset_backoff_for_serial(&serial);
                            }
                            dec
                        }
                    };
                    if decision == StallDecision::RestartInPlace {
                        if !session_still_active(&app, &session_id) {
                            break;
                        }
                        record_restart_for_serial(&serial, ts);
                        let _ = crate::commands::restart_flex_session_in_place(&app, &session_id).await;
                        break;
                    }
                }
                _ = poll_tick.tick() => {
                    if !session_still_active(&app, &session_id) {
                        break;
                    }
                    let power = query_phone_power_state(&app, &serial, turn_screen_off).await;
                    if !session_still_active(&app, &session_id) {
                        break;
                    }
                    let ts = now_ms();
                    if detector.on_power_state(power, ts) == StallDecision::RestartInPlace {
                        record_restart_for_serial(&serial, ts);
                        let _ = crate::commands::restart_flex_session_in_place(&app, &session_id).await;
                        break;
                    }
                }
            }
        }
    });

    tx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scrcpy_print_fps_lines() {
        assert_eq!(parse_fps_line("INFO: 0 fps"), Some(0));
        assert_eq!(parse_fps_line("INFO: 20 fps\r\n"), Some(20));
        assert_eq!(parse_fps_line("INFO: 27 fps (+2 frames skipped)"), Some(27));
        assert_eq!(parse_fps_line("INFO: FPS counter started"), None);
        assert_eq!(
            parse_fps_line("[server] INFO: New display: 1280x960/160 (id=66)"),
            None
        );
        assert_eq!(parse_fps_line("DEBUG: User requested to quit"), None);
        assert_eq!(parse_fps_line("INFO: 12 fpsx"), None);
    }

    #[test]
    fn parses_multiline_fps_chunks() {
        let chunk = "INFO: FPS counter started\nINFO: Texture (D3D11VA): 1280x960\nINFO: 16 fps (+3 frames skipped)\nINFO: 0 fps\n";
        assert!(is_fps_counter_started(chunk));
        assert_eq!(parse_fps_samples(chunk), vec![16, 0]);
    }

    #[test]
    fn probe_script_is_filtered_and_polls_every_3s() {
        assert_eq!(SCREEN_POLL_INTERVAL, Duration::from_secs(3));
        assert!(POWER_PROBE_SCRIPT
            .contains("dumpsys power | grep -E 'mWakefulness=|Display Power: state='"));
        assert!(POWER_PROBE_SCRIPT.contains("dumpsys window policy 2>/dev/null | grep -E"));
        assert!(!POWER_PROBE_SCRIPT.contains("|| dumpsys window"));
    }

    #[test]
    fn parses_phone_power_state_variants() {
        let awake_on = "  mWakefulness=Awake\n  Display Power: state=ON\n";
        let unlocked_policy = "    mShowingLockscreen=false\n    isStatusBarKeyguard=false\n";
        assert_eq!(
            parse_phone_power_state(awake_on, unlocked_policy, false),
            PhonePowerState::Active
        );
        assert_eq!(
            parse_phone_power_state(awake_on, "", false),
            PhonePowerState::Active
        );

        let asleep = "  mWakefulness=Asleep\n  Display Power: state=OFF\n";
        assert_eq!(
            parse_phone_power_state(asleep, "", false),
            PhonePowerState::Asleep
        );
        assert_eq!(
            parse_phone_power_state(asleep, "", true),
            PhonePowerState::Asleep
        );

        let dozing = "  mWakefulness=Dozing\n  Display Power: state=DOZE\n";
        assert_eq!(
            parse_phone_power_state(dozing, "", false),
            PhonePowerState::Asleep
        );

        let locked = "  mWakefulness=Awake\n  Display Power: state=ON\n";
        assert_eq!(
            parse_phone_power_state(locked, "    isStatusBarKeyguard=true\n", false),
            PhonePowerState::Locked
        );
        assert_eq!(
            parse_phone_power_state(locked, "    mShowingLockscreen=true\n", false),
            PhonePowerState::Locked
        );
        assert_eq!(
            parse_phone_power_state(locked, "    KeyguardStateMonitor: mIsShowing=true\n", false),
            PhonePowerState::Locked
        );

        // --turn-screen-off keeps mWakefulness=Awake with Display Power: state=OFF while unlocked
        let screen_off_flag = "  mWakefulness=Awake\n  Display Power: state=OFF\n";
        assert_eq!(
            parse_phone_power_state(screen_off_flag, unlocked_policy, true),
            PhonePowerState::Active
        );
        assert_eq!(
            parse_phone_power_state(screen_off_flag, unlocked_policy, false),
            PhonePowerState::Asleep
        );

        assert_eq!(
            parse_phone_power_state("", "", false),
            PhonePowerState::Unknown
        );
        assert_eq!(
            parse_phone_power_state("garbage", "", false),
            PhonePowerState::Unknown
        );
    }

    #[test]
    fn backoff_scales_exponentially_and_caps() {
        assert_eq!(restart_backoff_secs(0), 0);
        assert_eq!(restart_backoff_secs(1), 10);
        assert_eq!(restart_backoff_secs(2), 20);
        assert_eq!(restart_backoff_secs(3), 40);
        assert_eq!(restart_backoff_secs(4), 80);
        assert_eq!(restart_backoff_secs(5), 120);
        assert_eq!(restart_backoff_secs(9), 120);
    }

    #[test]
    fn never_restarts_while_asleep_locked_or_unknown() {
        let mut d = FlexStallDetector::new(0, None);
        d.on_fps_started();

        // Unknown power state: never restart.
        for t in 0..30 {
            assert_eq!(d.on_fps_sample(0, t * 1000).0, StallDecision::KeepAlive);
        }

        // Asleep: never restart.
        assert_eq!(
            d.on_power_state(PhonePowerState::Asleep, 31_000),
            StallDecision::KeepAlive
        );
        for t in 32..80 {
            assert_eq!(d.on_fps_sample(0, t * 1000).0, StallDecision::KeepAlive);
        }

        // Locked: never restart.
        assert_eq!(
            d.on_power_state(PhonePowerState::Locked, 81_000),
            StallDecision::KeepAlive
        );
        for t in 82..120 {
            assert_eq!(d.on_fps_sample(0, t * 1000).0, StallDecision::KeepAlive);
        }
    }

    #[test]
    fn restarts_after_sleep_then_wake_if_zero_fps_persists() {
        let mut d = FlexStallDetector::new(0, None);
        d.on_fps_started();
        d.on_power_state(PhonePowerState::Active, 1_000);
        assert_eq!(d.on_fps_sample(20, 2_000).0, StallDecision::KeepAlive);

        // Phone sleeps and locks.
        assert_eq!(
            d.on_power_state(PhonePowerState::Asleep, 3_000),
            StallDecision::KeepAlive
        );
        assert_eq!(d.on_fps_sample(0, 4_000).0, StallDecision::KeepAlive);

        // User wakes and unlocks phone.
        assert_eq!(
            d.on_power_state(PhonePowerState::Active, 5_000),
            StallDecision::KeepAlive
        );
        // 1s, 2s at 0 fps -> wait for POST_WAKE_STALL_SECS (3).
        assert_eq!(d.on_fps_sample(0, 6_000).0, StallDecision::KeepAlive);
        assert_eq!(d.on_fps_sample(0, 7_000).0, StallDecision::KeepAlive);
        // 3rd consecutive 0 fps while Active after sleep -> restart!
        assert_eq!(d.on_fps_sample(0, 8_000).0, StallDecision::RestartInPlace);
    }

    #[test]
    fn self_recovery_after_wake_cancels_restart() {
        let mut d = FlexStallDetector::new(0, None);
        d.on_fps_started();
        d.on_power_state(PhonePowerState::Asleep, 1_000);
        d.on_power_state(PhonePowerState::Active, 2_000);
        assert_eq!(d.on_fps_sample(0, 3_000).0, StallDecision::KeepAlive);
        assert_eq!(d.on_fps_sample(0, 4_000).0, StallDecision::KeepAlive);
        // Burst of frames arrives on wake (like diag3/diag4): resets detector.
        assert_eq!(d.on_fps_sample(30, 5_000).0, StallDecision::KeepAlive);
        assert!(!d.saw_sleep_or_lock);
        assert_eq!(d.active_zero_fps, 0);
        // Subsequent short static pauses do NOT trigger post-wake restart.
        assert_eq!(d.on_fps_sample(0, 6_000).0, StallDecision::KeepAlive);
        assert_eq!(d.on_fps_sample(0, 7_000).0, StallDecision::KeepAlive);
        assert_eq!(d.on_fps_sample(0, 8_000).0, StallDecision::KeepAlive);
    }

    #[test]
    fn restarts_at_most_once_per_wake_transition() {
        let mut d = FlexStallDetector::new(0, None);
        d.on_fps_started();
        d.on_power_state(PhonePowerState::Asleep, 1_000);
        d.on_power_state(PhonePowerState::Active, 2_000);

        assert_eq!(d.on_fps_sample(0, 3_000).0, StallDecision::KeepAlive);
        assert_eq!(d.on_fps_sample(0, 4_000).0, StallDecision::KeepAlive);
        assert_eq!(d.on_fps_sample(0, 5_000).0, StallDecision::RestartInPlace);

        // Same wake transition must never trigger a second restart even if 0 fps persists.
        for s in 6u64..=600 {
            let ts = s * 1000;
            if s % 3 == 0 {
                assert_eq!(
                    d.on_power_state(PhonePowerState::Active, ts),
                    StallDecision::KeepAlive
                );
            }
            assert_eq!(d.on_fps_sample(0, ts).0, StallDecision::KeepAlive);
        }

        // A new sleep -> wake cycle arms one new restart.
        d.on_power_state(PhonePowerState::Locked, 601_000);
        d.on_power_state(PhonePowerState::Active, 602_000);
        assert_eq!(d.on_fps_sample(0, 603_000).0, StallDecision::KeepAlive);
        assert_eq!(d.on_fps_sample(0, 604_000).0, StallDecision::KeepAlive);
        assert_eq!(d.on_fps_sample(0, 605_000).0, StallDecision::RestartInPlace);
    }

    #[test]
    fn static_screen_for_ten_minutes_without_sleep_never_restarts() {
        let mut d = FlexStallDetector::new(0, None);
        d.on_fps_started();
        d.on_power_state(PhonePowerState::Active, 1_000);
        assert_eq!(d.on_fps_sample(24, 2_000).0, StallDecision::KeepAlive);

        // 10 minutes (600s) of 0 fps on a static page with Active power polls (and brief Unknown glitches): never restart.
        for s in 3u64..=603 {
            let ts = s * 1000;
            if s == 120 {
                assert_eq!(
                    d.on_power_state(PhonePowerState::Unknown, ts),
                    StallDecision::KeepAlive
                );
            } else if s % 3 == 0 {
                assert_eq!(
                    d.on_power_state(PhonePowerState::Active, ts),
                    StallDecision::KeepAlive
                );
            }
            assert_eq!(d.on_fps_sample(0, ts).0, StallDecision::KeepAlive);
        }
    }

    #[test]
    fn post_wake_restart_respects_backoff_and_resets_on_healthy_frames() {
        // New session inherits backoff (1 restart at t=21_000 -> 10s cooldown until 31_000).
        let mut d2 = FlexStallDetector::new(1, Some(21_000));
        d2.on_fps_started();
        d2.on_power_state(PhonePowerState::Asleep, 22_000);
        d2.on_power_state(PhonePowerState::Active, 23_000);
        assert_eq!(d2.on_fps_sample(0, 24_000).0, StallDecision::KeepAlive);
        assert_eq!(d2.on_fps_sample(0, 25_000).0, StallDecision::KeepAlive);
        // Threshold (3) reached at t=26_000, but 10s cooldown holds until 31_000!
        assert_eq!(d2.on_fps_sample(0, 26_000).0, StallDecision::KeepAlive);
        assert_eq!(d2.on_fps_sample(0, 30_999).0, StallDecision::KeepAlive);
        assert_eq!(d2.on_fps_sample(0, 31_000).0, StallDecision::RestartInPlace);
        // Consumed after firing once.
        assert_eq!(d2.on_fps_sample(0, 32_000).0, StallDecision::KeepAlive);

        // Healthy frames reset backoff after HEALTHY_FRAMES_TO_RESET_BACKOFF samples.
        let mut d3 = FlexStallDetector::new(2, Some(40_000));
        d3.on_fps_started();
        assert_eq!(
            d3.on_fps_sample(24, 41_000),
            (StallDecision::KeepAlive, false)
        );
        assert_eq!(
            d3.on_fps_sample(24, 42_000),
            (StallDecision::KeepAlive, false)
        );
        assert_eq!(
            d3.on_fps_sample(24, 43_000),
            (StallDecision::KeepAlive, true)
        );
        assert_eq!(d3.consecutive_restarts, 0);
        assert_eq!(d3.last_restart_ms, None);
    }
}
