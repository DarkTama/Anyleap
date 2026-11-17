//! SPEC-29: secure-window and keyguard detection, Mirror wake, Flex notice.
//!
//! The module is split in a pure core (parser, interval math, `decide`) and a
//! generic poll loop (`run_poll`) that talks to the world through `PollEnv`.
//! The app implements `PollEnv` over Tauri state; tests and the device harness
//! implement it over mocks and plain adb. Nothing here reads pixels, and
//! nothing here tries to defeat the secure flag: it only reads window metadata.

use std::future::Future;
use std::time::Duration;

use serde::Serialize;

/// Stage 1 (cheap focus check). One `mCurrentFocus` line per display; display 0
/// prints `null` while a Flex display has focus, so every line is read.
pub const FOCUS_SCRIPT: &str = "dumpsys window | grep mCurrentFocus";

/// Stage 2 (full probe). One adb round, parsed on the device, one short line:
/// `S=<n> K=<0|1> D=<display or -1> A=<1|0|?> F=<focused window ids>`.
/// The `SECURE` token only counts on the `fl=` line of the focused window's own
/// block, so a title or package containing the word never matches.
pub const SECURE_PROBE_SCRIPT: &str = concat!(
    "w=$(dumpsys window); ",
    "ids=$(echo \"$w\" | grep \"mCurrentFocus=Window\" | sed \"s/.*Window{\\([0-9a-f]*\\) .*/\\1/\"); ",
    "s=0; fd=; sd=; ",
    "for f in $ids; do ",
    "b=$(echo \"$w\" | sed -n \"/^  Window #[0-9]* Window{$f /,/^  Window #[0-9]* Window{/p\"); ",
    "x=$(echo \"$b\" | grep -m1 -o \"mDisplayId=[0-9]*\" | cut -d= -f2); ",
    "[ -z \"$fd\" ] && fd=$x; ",
    "if echo \"$b\" | grep -qE \"^ +fl=(.* )?SECURE( |$)\"; then s=$((s+1)); [ $s -eq 1 ] && sd=$x; fi; ",
    "done; ",
    "d=${sd:-$fd}; ",
    "k=0; echo \"$w\" | grep -q \"isKeyguardShowing=true\" && k=1; ",
    "a=?; echo \"$w\" | grep -q \"mAwake=true\" && a=1; echo \"$w\" | grep -q \"mAwake=false\" && a=0; ",
    "echo \"S=$s K=$k D=${d:--1} A=$a F=$(echo $ids | tr ' ' ,)\""
);

/// Every K-th tick runs the full probe as a safety refresh.
pub const FULL_PROBE_EVERY: u64 = 10;
/// Auto-disable after this many consecutive ticks with EWMA above the slow limit.
pub const AUTO_DISABLE_TICKS: u32 = 5;

pub const NOTICE_SECURE_FLEX: &str =
    "Secure content can't be shown in Flex. Open it on your phone.";
pub const NOTICE_WOKE_SECURE: &str =
    "Secure screen on the phone. Screen turned on so you can see it.";
pub const NOTICE_WOKE_KEYGUARD: &str = "Phone is locked. Screen turned on so you can unlock it.";
pub const NOTICE_TOO_SLOW: &str =
    "This phone is too slow for secure-screen detection. Turned off for this session.";
pub const NOTICE_FLEX_VIEW_ONCE: &str =
    "Opening view-once media in Flex shows black and still marks it as seen.";

// ---------------------------------------------------------------------------
// Detector output
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SecureState {
    /// A focused window carries the secure flag.
    pub secure_focused: bool,
    /// The keyguard is up (display 0).
    pub keyguard_showing: bool,
    /// Display of the focused (secure, if any) window; None when unknown.
    pub focused_display_id: Option<u32>,
}

/// Payload of the `secure-state` event.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SecureStatePayload {
    pub serial: String,
    pub secure_focused: bool,
    pub keyguard_showing: bool,
    pub focused_display_id: Option<u32>,
}

impl SecureStatePayload {
    pub fn new(serial: &str, s: &SecureState) -> Self {
        Self {
            serial: serial.to_string(),
            secure_focused: s.secure_focused,
            keyguard_showing: s.keyguard_showing,
            focused_display_id: s.focused_display_id,
        }
    }
}

/// A parsed stage-2 line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    pub state: SecureState,
    /// `mAwake` of the phone policy: Some(false) when asleep, None when unknown.
    pub awake: Option<bool>,
    /// Focus identity (same format as `parse_focus_identity`).
    pub focus: String,
}

fn field<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    line.split_whitespace().find_map(|t| t.strip_prefix(key))
}

/// Parse the stage-2 output line. Anything unexpected is `None` (failed probe).
pub fn parse_probe(out: &str) -> Option<Probe> {
    let line = out
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("S=") && l.contains(" K="))?;
    let s: u32 = field(line, "S=")?.parse().ok()?;
    let k = match field(line, "K=")? {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    let d: i64 = field(line, "D=")?.parse().ok()?;
    let awake = match field(line, "A=") {
        Some("1") => Some(true),
        Some("0") => Some(false),
        _ => None,
    };
    let focus = field(line, "F=").unwrap_or("").to_string();
    Some(Probe {
        state: SecureState {
            secure_focused: s >= 1,
            keyguard_showing: k,
            focused_display_id: u32::try_from(d).ok(),
        },
        awake,
        focus,
    })
}

/// Spec §5.2 `parse_secure_probe`: the state half of `parse_probe`.
pub fn parse_secure_probe(out: &str) -> Option<SecureState> {
    parse_probe(out).map(|p| p.state)
}

/// Focus identity from stage-1 output: the `Window{<id> ...}` ids of every
/// `mCurrentFocus` line, joined by commas (empty when all displays print
/// `null`). `None` when no `mCurrentFocus` line exists (failed tick).
pub fn parse_focus_identity(out: &str) -> Option<String> {
    let mut seen = false;
    let mut ids: Vec<&str> = Vec::new();
    for line in out.lines().filter(|l| l.contains("mCurrentFocus=")) {
        seen = true;
        if let Some(rest) = line.split("Window{").nth(1) {
            if let Some(id) = rest.split_whitespace().next() {
                if !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit()) {
                    ids.push(id);
                }
            }
        }
    }
    seen.then(|| ids.join(","))
}

// ---------------------------------------------------------------------------
// Adaptive timing (pure)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    One,
    Two,
}

/// Stage logic per tick. `prev_focus == None` is the first tick: stage 2.
pub fn plan_tick(prev_focus: Option<&str>, focus: &str, tick_count: u64, k: u64) -> Stage {
    match prev_focus {
        None => Stage::Two,
        Some(p) if p != focus => Stage::Two,
        _ if k > 0 && tick_count.is_multiple_of(k) => Stage::Two,
        _ => Stage::One,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Tuning {
    pub floor: Duration,
    pub cap: Duration,
    /// EWMA above this counts toward auto-disable.
    pub slow: Duration,
    /// Idle re-check while the gate is closed.
    pub gate: Duration,
    /// Per-probe timeout.
    pub probe_timeout: Duration,
    /// Minimum time both flags must stay clear before the restore.
    pub restore_debounce: Duration,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            floor: Duration::from_millis(1500),
            cap: Duration::from_secs(6),
            slow: Duration::from_secs(1),
            gate: Duration::from_secs(1),
            probe_timeout: Duration::from_secs(2),
            restore_debounce: Duration::from_secs(3),
        }
    }
}

/// `clamp(max(floor, 10 * ewma), floor, cap)`: about 10% duty on any phone.
pub fn next_interval_with(ewma_ms: f64, t: &Tuning) -> Duration {
    let want = Duration::from_secs_f64((10.0 * ewma_ms.max(0.0)) / 1000.0);
    want.max(t.floor).min(t.cap)
}

pub fn next_interval(ewma_ms: f64) -> Duration {
    next_interval_with(ewma_ms, &Tuning::default())
}

/// EWMA, backoff and auto-disable counter for one session.
#[derive(Debug, Clone)]
pub struct AdaptiveTimer {
    t: Tuning,
    ewma_ms: Option<f64>,
    seen_cold: bool,
    over_ticks: u32,
    ok_streak: u32,
    backoff: Option<Duration>,
}

impl AdaptiveTimer {
    pub fn new(t: Tuning) -> Self {
        Self {
            t,
            ewma_ms: None,
            seen_cold: false,
            over_ticks: 0,
            ok_streak: 0,
            backoff: None,
        }
    }

    pub fn ewma_ms(&self) -> Option<f64> {
        self.ewma_ms
    }

    pub fn interval(&self) -> Duration {
        self.backoff
            .unwrap_or_else(|| next_interval_with(self.ewma_ms.unwrap_or(0.0), &self.t))
    }

    /// Feed a successful tick. Returns true when detection must auto-disable.
    /// The first (cold) probe of a session is not fed to the EWMA.
    pub fn record_success(&mut self, took: Duration) -> bool {
        self.ok_streak += 1;
        if self.ok_streak >= 2 {
            self.backoff = None;
        }
        if !self.seen_cold {
            self.seen_cold = true;
            return false;
        }
        let ms = took.as_secs_f64() * 1000.0;
        let e = match self.ewma_ms {
            None => ms,
            Some(prev) => 0.3 * ms + 0.7 * prev,
        };
        self.ewma_ms = Some(e);
        if e > self.t.slow.as_secs_f64() * 1000.0 {
            self.over_ticks += 1;
        } else {
            self.over_ticks = 0;
        }
        self.over_ticks >= AUTO_DISABLE_TICKS
    }

    /// A failed or timed-out probe doubles the current interval (cap).
    pub fn record_failure(&mut self) {
        self.ok_streak = 0;
        self.backoff = Some((self.interval() * 2).min(self.t.cap));
    }
}

// ---------------------------------------------------------------------------
// Run-only-when-needed gate (pure)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PollKind {
    /// Mirror and High fidelity: the physical display is mirrored.
    Screen,
    /// Flex: apps run on a virtual display.
    Flex,
}

pub fn should_poll(
    kind: PollKind,
    screen_off: bool,
    woke_by_us: bool,
    minimized: bool,
    secure_detection: bool,
) -> bool {
    if !secure_detection || minimized {
        return false;
    }
    match kind {
        PollKind::Screen => screen_off || woke_by_us,
        PollKind::Flex => true,
    }
}

// ---------------------------------------------------------------------------
// Wake state machine (pure)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WakeReason {
    Secure,
    Keyguard,
}

/// Per-session screen and wake state (spec §6.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionWake {
    /// AnyLeap has the phone screen off (launch flag or ControlBar toggle).
    pub screen_off: bool,
    pub woke_by_us: Option<WakeReason>,
    pub wake_on_secure: bool,
    pub secure_detection: bool,
}

impl SessionWake {
    pub fn new(screen_off: bool, wake_on_secure: bool, secure_detection: bool) -> Self {
        Self {
            screen_off,
            woke_by_us: None,
            wake_on_secure,
            secure_detection,
        }
    }

    /// The user toggled screen-off in the ControlBar: user choice wins.
    pub fn user_toggled(&mut self, off: bool) {
        self.screen_off = off;
        self.woke_by_us = None;
    }

    /// The probe saw the phone asleep (physical power press): cancel the restore.
    pub fn note_awake(&mut self, awake: Option<bool>) {
        if awake == Some(false) {
            self.woke_by_us = None;
        }
    }

    /// Session end: reset.
    pub fn reset(&mut self) {
        self.woke_by_us = None;
    }

    pub fn apply(&mut self, a: &Action) {
        match a {
            Action::ScreenOn(r) => {
                self.screen_off = false;
                self.woke_by_us = Some(*r);
            }
            Action::ScreenOffRestore => {
                self.screen_off = true;
                self.woke_by_us = None;
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    NoticeSecureFlex,
    NoticeDetectionTooSlow,
    NoticeWokeSecure,
    NoticeWokeKeyguard,
    ScreenOn(WakeReason),
    ScreenOffRestore,
}

pub fn notice_text(a: &Action) -> Option<&'static str> {
    match a {
        Action::NoticeSecureFlex => Some(NOTICE_SECURE_FLEX),
        Action::NoticeDetectionTooSlow => Some(NOTICE_TOO_SLOW),
        Action::NoticeWokeSecure => Some(NOTICE_WOKE_SECURE),
        Action::NoticeWokeKeyguard => Some(NOTICE_WOKE_KEYGUARD),
        _ => None,
    }
}

/// What the poll loop knows about how long both flags have been clear.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClearInfo {
    /// Consecutive stage-2 probes with nothing that needs the screen.
    pub probes: u32,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, Copy)]
pub struct DecideCtx {
    pub kind: PollKind,
    /// Android display id of the Flex virtual display, when known.
    pub virtual_display: Option<u32>,
    pub clear: ClearInfo,
    pub restore_debounce: Duration,
}

/// Why the phone screen is needed right now, if it is.
pub fn wants_screen(kind: PollKind, s: &SecureState) -> Option<WakeReason> {
    if s.keyguard_showing {
        return Some(WakeReason::Keyguard);
    }
    // A secure window on display 0 is on the phone panel. In Flex a secure
    // window on the virtual display does not need the panel (it needs the user
    // to open it on the phone, see `secure_in_flex`).
    if kind == PollKind::Screen
        && s.secure_focused
        && matches!(s.focused_display_id, None | Some(0))
    {
        return Some(WakeReason::Secure);
    }
    None
}

/// A secure window is focused on the Flex virtual display.
pub fn secure_in_flex(s: &SecureState, virtual_display: Option<u32>) -> bool {
    s.secure_focused
        && match s.focused_display_id {
            Some(id) if id != 0 => virtual_display.is_none_or(|v| v == id),
            _ => false,
        }
}

/// Pure decision for one stage-2 result. No I/O; the caller performs the
/// actions and applies `SessionWake::apply` for each action that succeeded.
pub fn decide(
    prev: &SecureState,
    next: &SecureState,
    wake: &SessionWake,
    ctx: &DecideCtx,
) -> Vec<Action> {
    let mut out = Vec::new();
    if !wake.secure_detection {
        return out;
    }
    if ctx.kind == PollKind::Flex
        && secure_in_flex(next, ctx.virtual_display)
        && !secure_in_flex(prev, ctx.virtual_display)
    {
        out.push(Action::NoticeSecureFlex);
    }
    let was = wants_screen(ctx.kind, prev);
    match wants_screen(ctx.kind, next) {
        Some(reason) => {
            // Rising edge only: a user who turns the screen off again on a
            // secure page is not woken in a loop.
            if was.is_none() && wake.screen_off {
                // The Flex keyguard wake is always on; the secure wake follows the setting.
                let allowed = reason == WakeReason::Keyguard && ctx.kind == PollKind::Flex
                    || wake.wake_on_secure;
                if allowed {
                    out.push(Action::ScreenOn(reason));
                    out.push(match reason {
                        WakeReason::Secure => Action::NoticeWokeSecure,
                        WakeReason::Keyguard => Action::NoticeWokeKeyguard,
                    });
                }
            }
        }
        None => {
            if wake.woke_by_us.is_some()
                && ctx.clear.probes >= 2
                && ctx.clear.elapsed >= ctx.restore_debounce
            {
                out.push(Action::ScreenOffRestore);
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Poll loop
// ---------------------------------------------------------------------------

/// What the loop can see of its session on each tick.
#[derive(Debug, Clone)]
pub struct SessionView {
    pub wake: SessionWake,
    /// The session window is minimized or hidden.
    pub hidden: bool,
    pub virtual_display: Option<u32>,
}

/// The world the poll loop talks to.
pub trait PollEnv: Send + Sync {
    /// `None` when the session no longer exists (the loop ends).
    fn view(&self) -> Option<SessionView>;
    /// Mutate the session's wake state under the owner's lock.
    fn mutate(&self, f: &mut dyn FnMut(&mut SessionWake));
    /// `adb -s <serial> shell <script>`; stdout on success.
    fn adb_shell(&self, script: &str) -> impl Future<Output = Result<String, String>> + Send;
    /// Turn the phone screen on (`off == false`) or off through scrcpy's screen
    /// power control, falling back to a wake key. Never the power key.
    fn screen_power(
        &self,
        off: bool,
        awake: Option<bool>,
    ) -> impl Future<Output = Result<(), String>> + Send;
    fn emit_state(&self, s: &SecureState);
    fn emit_notice(&self, text: &str);
    fn emit_screen_power(&self, off: bool);
    fn log(&self, _msg: &str) {}
    /// Per-tick adaptive state (EWMA in ms, next interval); for the FPS acceptance harness.
    fn tick_stats(&self, _ewma_ms: Option<f64>, _interval: Duration) {}
}

/// Run one session's detector until the session ends or auto-disables.
pub async fn run_poll<E: PollEnv>(env: &E, kind: PollKind, tuning: Tuning) {
    let mut timer = AdaptiveTimer::new(tuning);
    let mut state = SecureState::default();
    let mut prev_focus: Option<String> = None;
    let mut tick: u64 = 0;
    let mut polling = false;
    let mut clear_probes: u32 = 0;
    let mut clear_since: Option<tokio::time::Instant> = None;

    loop {
        let Some(view) = env.view() else { return };
        let open = should_poll(
            kind,
            view.wake.screen_off,
            view.wake.woke_by_us.is_some(),
            view.hidden,
            view.wake.secure_detection,
        );
        if !open {
            polling = false;
            tokio::time::sleep(tuning.gate).await;
            continue;
        }
        // First tick, resume after hidden, and while we hold the screen on
        // (the restore needs fresh stage-2 probes): full probe at once.
        let force_full = !polling || view.wake.woke_by_us.is_some();
        polling = true;
        tick += 1;

        let t0 = tokio::time::Instant::now();
        let mut probe: Option<Probe> = None;
        let mut failed = false;

        let mut stage = Stage::Two;
        if !force_full {
            match adb_timed(env, FOCUS_SCRIPT, tuning.probe_timeout).await {
                Ok(out) => match parse_focus_identity(&out) {
                    Some(f) => {
                        stage = plan_tick(prev_focus.as_deref(), &f, tick, FULL_PROBE_EVERY);
                        prev_focus = Some(f);
                    }
                    None => failed = true,
                },
                Err(_) => failed = true,
            }
        }
        if !failed && stage == Stage::Two {
            match adb_timed(env, SECURE_PROBE_SCRIPT, tuning.probe_timeout)
                .await
                .ok()
                .and_then(|o| parse_probe(&o))
            {
                Some(p) => {
                    prev_focus = Some(p.focus.clone());
                    probe = Some(p);
                }
                None => failed = true,
            }
        }

        if failed {
            timer.record_failure();
        } else if timer.record_success(t0.elapsed()) {
            env.log("secure_watch: phone too slow, detection off for this session");
            auto_disable(env, &mut state).await;
            return;
        }

        if let Some(p) = probe {
            handle_probe(
                env,
                kind,
                &tuning,
                &mut state,
                &mut clear_probes,
                &mut clear_since,
                p,
            )
            .await;
        }
        env.tick_stats(timer.ewma_ms(), timer.interval());
        tokio::time::sleep(timer.interval()).await;
    }
}

async fn adb_timed<E: PollEnv>(env: &E, script: &str, limit: Duration) -> Result<String, String> {
    match tokio::time::timeout(limit, env.adb_shell(script)).await {
        Ok(r) => r,
        Err(_) => Err("probe timed out".into()),
    }
}

async fn auto_disable<E: PollEnv>(env: &E, state: &mut SecureState) {
    if *state != SecureState::default() {
        *state = SecureState::default();
        env.emit_state(state);
    }
    env.emit_notice(NOTICE_TOO_SLOW);
    let held = env.view().is_some_and(|v| v.wake.woke_by_us.is_some());
    if held && env.screen_power(true, None).await.is_ok() {
        env.mutate(&mut |w| w.apply(&Action::ScreenOffRestore));
        env.emit_screen_power(true);
    }
}

async fn handle_probe<E: PollEnv>(
    env: &E,
    kind: PollKind,
    tuning: &Tuning,
    state: &mut SecureState,
    clear_probes: &mut u32,
    clear_since: &mut Option<tokio::time::Instant>,
    p: Probe,
) {
    let prev = *state;
    if p.state != prev {
        *state = p.state;
        env.emit_state(state);
    }
    if wants_screen(kind, &p.state).is_some() {
        *clear_probes = 0;
        *clear_since = None;
    } else {
        *clear_probes += 1;
        clear_since.get_or_insert_with(tokio::time::Instant::now);
    }
    env.mutate(&mut |w| w.note_awake(p.awake));
    let Some(view) = env.view() else { return };
    let ctx = DecideCtx {
        kind,
        virtual_display: view.virtual_display,
        clear: ClearInfo {
            probes: *clear_probes,
            elapsed: clear_since.map(|t| t.elapsed()).unwrap_or_default(),
        },
        restore_debounce: tuning.restore_debounce,
    };
    for a in decide(&prev, &p.state, &view.wake, &ctx) {
        match a {
            Action::ScreenOn(_) | Action::ScreenOffRestore => {
                let off = a == Action::ScreenOffRestore;
                match env.screen_power(off, p.awake).await {
                    Ok(()) => {
                        env.mutate(&mut |w| w.apply(&a));
                        env.emit_screen_power(off);
                    }
                    Err(e) => env.log(&format!("secure_watch: screen power failed: {e}")),
                }
            }
            other => {
                if let Some(t) = notice_text(&other) {
                    env.emit_notice(t);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Session handle
// ---------------------------------------------------------------------------

/// Owns the poll task of one session; dropping it aborts the task, so every
/// session removal path tears the detector down.
pub struct WatchHandle(pub tauri::async_runtime::JoinHandle<()>);

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;

    // -- parser ------------------------------------------------------------

    #[test]
    fn parses_valid_lines() {
        let a = parse_secure_probe("S=1 K=0 D=0").unwrap();
        assert!(a.secure_focused && !a.keyguard_showing);
        assert_eq!(a.focused_display_id, Some(0));
        let b = parse_secure_probe("S=0 K=1 D=-1").unwrap();
        assert!(!b.secure_focused && b.keyguard_showing);
        assert_eq!(b.focused_display_id, None);
        let c = parse_secure_probe("S=1 K=0 D=34\n").unwrap();
        assert_eq!(c.focused_display_id, Some(34));
        let p = parse_probe("S=0 K=0 D=0 A=0 F=ab12,cd34").unwrap();
        assert_eq!(p.awake, Some(false));
        assert_eq!(p.focus, "ab12,cd34");
        assert_eq!(parse_probe("S=0 K=0 D=0 A=?").unwrap().awake, None);
    }

    #[test]
    fn rejects_malformed_lines() {
        for bad in [
            "",
            "S=x K=0 D=0",
            "S=1 K=0",
            "S=1 K=2 D=0",
            "garbage",
            "S=1 D=0",
        ] {
            assert!(parse_secure_probe(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn parses_focus_identity() {
        let one = "  mCurrentFocus=Window{86bc632 u0 pkg/Act}\n";
        assert_eq!(parse_focus_identity(one).as_deref(), Some("86bc632"));
        let flex = "  mCurrentFocus=null\n  mCurrentFocus=Window{5c5096c u0 pkg/Act}\n";
        assert_eq!(parse_focus_identity(flex).as_deref(), Some("5c5096c"));
        assert_eq!(
            parse_focus_identity("  mCurrentFocus=null\n").as_deref(),
            Some("")
        );
        assert_eq!(parse_focus_identity(""), None);
        assert_eq!(parse_focus_identity("error: closed"), None);
    }

    // The probe script is shell; run its logic over recorded dumps to prove the
    // contract (skipped when no POSIX shell with the usual tools is present).
    fn run_probe_on_fixture(name: &str) -> Option<String> {
        let path = format!(
            "{}/tests/fixtures/secure/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        let sh = std::process::Command::new("sh")
            .arg("-c")
            .arg(format!(
                "dumpsys() {{ cat '{path}'; }}; {SECURE_PROBE_SCRIPT}"
            ))
            .output()
            .ok()?;
        sh.status
            .success()
            .then(|| String::from_utf8_lossy(&sh.stdout).to_string())
    }

    #[test]
    fn probe_script_over_recorded_dumps() {
        let Some(normal) = run_probe_on_fixture("normal.txt") else {
            return;
        };
        let st = |o: &str| parse_secure_probe(o).unwrap();
        assert_eq!(
            st(&normal),
            SecureState {
                secure_focused: false,
                keyguard_showing: false,
                focused_display_id: Some(0)
            }
        );
        let secure = st(&run_probe_on_fixture("secure.txt").unwrap());
        assert!(secure.secure_focused && !secure.keyguard_showing);
        let kg = st(&run_probe_on_fixture("keyguard.txt").unwrap());
        assert!(kg.keyguard_showing, "keyguard flag");
        assert!(
            !kg.secure_focused,
            "unfocused secure window is not reported"
        );
        let flex = st(&run_probe_on_fixture("flex.txt").unwrap());
        assert!(flex.secure_focused);
        assert_eq!(flex.focused_display_id, Some(29));
        // SECURE in a title, package name or pfl= line must not match.
        let word = st(&run_probe_on_fixture("secure_word_elsewhere.txt").unwrap());
        assert!(!word.secure_focused);
    }

    // -- interval math -----------------------------------------------------

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    #[test]
    fn interval_formula() {
        assert_eq!(next_interval(94.0), ms(1500));
        assert_eq!(next_interval(200.0), ms(2000));
        assert_eq!(next_interval(400.0), ms(4000));
        assert_eq!(next_interval(700.0), ms(6000));
        assert_eq!(next_interval(0.0), ms(1500));
    }

    #[test]
    fn cold_probe_is_not_fed_and_ewma_updates() {
        let mut t = AdaptiveTimer::new(Tuning::default());
        t.record_success(ms(899));
        assert_eq!(t.ewma_ms(), None);
        assert_eq!(t.interval(), ms(1500));
        t.record_success(ms(100));
        assert_eq!(t.ewma_ms(), Some(100.0));
        t.record_success(ms(200));
        let e = t.ewma_ms().unwrap();
        assert!((e - 130.0).abs() < 1e-9, "{e}");
        t.record_success(ms(400));
        let e = t.ewma_ms().unwrap();
        assert!((e - 211.0).abs() < 1e-9, "{e}");
    }

    #[test]
    fn failure_backoff_doubles_caps_and_recovers() {
        let mut t = AdaptiveTimer::new(Tuning::default());
        t.record_success(ms(100));
        t.record_success(ms(100));
        assert_eq!(t.interval(), ms(1500));
        t.record_failure();
        assert_eq!(t.interval(), ms(3000));
        t.record_failure();
        assert_eq!(t.interval(), ms(6000));
        t.record_failure();
        assert_eq!(t.interval(), ms(6000));
        t.record_success(ms(100));
        assert_eq!(t.interval(), ms(6000), "one good tick is not enough");
        t.record_success(ms(100));
        assert_eq!(t.interval(), ms(1500));
    }

    #[test]
    fn auto_disable_counter() {
        let slow = |t: &mut AdaptiveTimer, n: u64| t.record_success(ms(n));
        let mut t = AdaptiveTimer::new(Tuning::default());
        slow(&mut t, 50); // cold
        for _ in 0..4 {
            assert!(!slow(&mut t, 2000));
        }
        assert!(slow(&mut t, 2000), "5th consecutive tick over 1 s disables");

        // 4 over, then under: resets.
        let mut t = AdaptiveTimer::new(Tuning::default());
        slow(&mut t, 50);
        for _ in 0..4 {
            assert!(!slow(&mut t, 5000));
        }
        // EWMA needs to drop below 1 s: feed fast ticks until it does.
        let mut dropped = false;
        for _ in 0..30 {
            slow(&mut t, 1);
            if t.ewma_ms().unwrap() <= 1000.0 {
                dropped = true;
                break;
            }
        }
        assert!(dropped);
        assert!(!slow(&mut t, 2000), "counter restarted");

        // Exactly 1.0 s does not count.
        let mut t = AdaptiveTimer::new(Tuning::default());
        slow(&mut t, 50);
        for _ in 0..10 {
            assert!(!slow(&mut t, 1000));
        }
    }

    // -- stage plan and gate -----------------------------------------------

    #[test]
    fn plan_tick_rules() {
        assert_eq!(plan_tick(Some("a"), "a", 3, 10), Stage::One);
        assert_eq!(plan_tick(Some("a"), "b", 3, 10), Stage::Two);
        assert_eq!(plan_tick(Some("a"), "a", 10, 10), Stage::Two);
        assert_eq!(plan_tick(Some("a"), "a", 20, 10), Stage::Two);
        assert_eq!(plan_tick(None, "a", 1, 10), Stage::Two);
    }

    #[test]
    fn gate_rules() {
        use PollKind::*;
        assert!(should_poll(Screen, true, false, false, true));
        assert!(should_poll(Screen, false, true, false, true));
        assert!(!should_poll(Screen, false, false, false, true));
        assert!(should_poll(Flex, false, false, false, true));
        assert!(!should_poll(Flex, true, false, true, true), "minimized");
        assert!(
            !should_poll(Screen, true, true, false, false),
            "detection off"
        );
    }

    // -- decide ------------------------------------------------------------

    const SEC0: SecureState = SecureState {
        secure_focused: true,
        keyguard_showing: false,
        focused_display_id: Some(0),
    };
    const KG: SecureState = SecureState {
        secure_focused: false,
        keyguard_showing: true,
        focused_display_id: None,
    };
    const CLEAR: SecureState = SecureState {
        secure_focused: false,
        keyguard_showing: false,
        focused_display_id: Some(0),
    };

    fn ctx(kind: PollKind, vd: Option<u32>, probes: u32, secs: u64) -> DecideCtx {
        DecideCtx {
            kind,
            virtual_display: vd,
            clear: ClearInfo {
                probes,
                elapsed: Duration::from_secs(secs),
            },
            restore_debounce: Duration::from_secs(3),
        }
    }

    fn off_wake() -> SessionWake {
        SessionWake::new(true, true, true)
    }

    #[test]
    fn wakes_on_rising_secure_with_screen_off() {
        let acts = decide(
            &CLEAR,
            &SEC0,
            &off_wake(),
            &ctx(PollKind::Screen, None, 0, 0),
        );
        assert_eq!(
            acts,
            vec![
                Action::ScreenOn(WakeReason::Secure),
                Action::NoticeWokeSecure
            ]
        );
        let acts = decide(&CLEAR, &KG, &off_wake(), &ctx(PollKind::Screen, None, 0, 0));
        assert_eq!(
            acts,
            vec![
                Action::ScreenOn(WakeReason::Keyguard),
                Action::NoticeWokeKeyguard
            ]
        );
    }

    #[test]
    fn no_wake_when_screen_on_or_setting_off_or_detection_off() {
        let c = ctx(PollKind::Screen, None, 0, 0);
        let mut w = off_wake();
        w.screen_off = false;
        assert!(decide(&CLEAR, &SEC0, &w, &c).is_empty());
        let mut w = off_wake();
        w.wake_on_secure = false;
        assert!(decide(&CLEAR, &SEC0, &w, &c).is_empty());
        let mut w = off_wake();
        w.secure_detection = false;
        assert!(decide(&CLEAR, &SEC0, &w, &c).is_empty());
    }

    #[test]
    fn no_wake_loop_after_user_turns_screen_off_on_secure_page() {
        let c = ctx(PollKind::Screen, None, 0, 0);
        let mut w = off_wake();
        w.apply(&Action::ScreenOn(WakeReason::Secure));
        w.user_toggled(true);
        assert_eq!(w.woke_by_us, None);
        assert!(decide(&SEC0, &SEC0, &w, &c).is_empty(), "no rising edge");
    }

    #[test]
    fn restore_only_if_woke_by_us_and_debounced() {
        let mut w = off_wake();
        w.apply(&Action::ScreenOn(WakeReason::Secure));
        assert!(!w.screen_off);
        // clear but debounce not met
        assert!(decide(&SEC0, &CLEAR, &w, &ctx(PollKind::Screen, None, 1, 5)).is_empty());
        assert!(decide(&SEC0, &CLEAR, &w, &ctx(PollKind::Screen, None, 2, 2)).is_empty());
        let acts = decide(&CLEAR, &CLEAR, &w, &ctx(PollKind::Screen, None, 2, 3));
        assert_eq!(acts, vec![Action::ScreenOffRestore]);
        w.apply(&acts[0]);
        assert!(w.screen_off && w.woke_by_us.is_none());
        // screen was off for another reason: never restored
        let mut other = off_wake();
        other.screen_off = false;
        assert!(decide(&CLEAR, &CLEAR, &other, &ctx(PollKind::Screen, None, 9, 99)).is_empty());
    }

    #[test]
    fn overlap_keeps_screen_on_until_both_clear() {
        let mut w = off_wake();
        w.apply(&Action::ScreenOn(WakeReason::Keyguard));
        // secure clears, keyguard still showing: no restore
        assert!(decide(&SEC0, &KG, &w, &ctx(PollKind::Screen, None, 0, 0)).is_empty());
        // keyguard clears while secure still on: no restore
        assert!(decide(&KG, &SEC0, &w, &ctx(PollKind::Screen, None, 0, 0)).is_empty());
    }

    #[test]
    fn user_toggle_and_phone_sleep_cancel_restore() {
        let mut w = off_wake();
        w.apply(&Action::ScreenOn(WakeReason::Secure));
        w.user_toggled(false);
        assert_eq!(w.woke_by_us, None);
        assert!(decide(&CLEAR, &CLEAR, &w, &ctx(PollKind::Screen, None, 9, 99)).is_empty());

        let mut w = off_wake();
        w.apply(&Action::ScreenOn(WakeReason::Secure));
        w.note_awake(Some(true));
        assert!(w.woke_by_us.is_some());
        w.note_awake(Some(false));
        assert_eq!(w.woke_by_us, None);
    }

    #[test]
    fn session_end_resets() {
        let mut w = off_wake();
        w.apply(&Action::ScreenOn(WakeReason::Secure));
        w.reset();
        assert_eq!(w.woke_by_us, None);
    }

    const FLEX_SEC: SecureState = SecureState {
        secure_focused: true,
        keyguard_showing: false,
        focused_display_id: Some(29),
    };

    #[test]
    fn flex_notice_on_rising_edge_on_virtual_display_only() {
        let c = ctx(PollKind::Flex, Some(29), 0, 0);
        let w = off_wake();
        assert_eq!(
            decide(&CLEAR, &FLEX_SEC, &w, &c),
            vec![Action::NoticeSecureFlex]
        );
        assert!(
            decide(&FLEX_SEC, &FLEX_SEC, &w, &c).is_empty(),
            "not repeated per poll"
        );
        // cleared, then again: re-armed
        assert_eq!(
            decide(&CLEAR, &FLEX_SEC, &w, &c),
            vec![Action::NoticeSecureFlex]
        );
        // secure window on display 0 while Flex runs: no notice, no wake
        assert!(decide(&CLEAR, &SEC0, &w, &c).is_empty());
        // another virtual display id: no notice
        let other = SecureState {
            focused_display_id: Some(31),
            ..FLEX_SEC
        };
        assert!(decide(&CLEAR, &other, &w, &c).is_empty());
        // virtual display unknown: any non-zero display counts
        let c2 = ctx(PollKind::Flex, None, 0, 0);
        assert_eq!(
            decide(&CLEAR, &other, &w, &c2),
            vec![Action::NoticeSecureFlex]
        );
    }

    #[test]
    fn flex_keyguard_wake_ignores_wake_setting_but_not_detection() {
        let c = ctx(PollKind::Flex, Some(29), 0, 0);
        let mut w = off_wake();
        w.wake_on_secure = false;
        assert_eq!(
            decide(&CLEAR, &KG, &w, &c),
            vec![
                Action::ScreenOn(WakeReason::Keyguard),
                Action::NoticeWokeKeyguard
            ]
        );
        w.secure_detection = false;
        assert!(decide(&CLEAR, &KG, &w, &c).is_empty());
        // screen already on: nothing
        let mut w = off_wake();
        w.screen_off = false;
        assert!(decide(&CLEAR, &KG, &w, &c).is_empty());
    }

    #[test]
    fn flex_restores_after_keyguard_clears() {
        let mut w = off_wake();
        w.apply(&Action::ScreenOn(WakeReason::Keyguard));
        let acts = decide(&KG, &CLEAR, &w, &ctx(PollKind::Flex, Some(29), 2, 3));
        assert_eq!(acts, vec![Action::ScreenOffRestore]);
    }

    // -- poll loop with a mock environment ----------------------------------

    struct Mock {
        wake: Mutex<SessionWake>,
        alive: Mutex<bool>,
        hidden: Mutex<bool>,
        replies: Mutex<VecDeque<(Duration, Result<String, String>)>>,
        last_reply: Mutex<(Duration, Result<String, String>)>,
        scripts: Mutex<Vec<&'static str>>,
        focus_delay: Mutex<Duration>,
        notices: Mutex<Vec<String>>,
        power: Mutex<Vec<bool>>,
        states: Mutex<Vec<SecureState>>,
    }

    impl Mock {
        fn new(wake: SessionWake) -> Self {
            Self {
                wake: Mutex::new(wake),
                alive: Mutex::new(true),
                hidden: Mutex::new(false),
                replies: Mutex::new(VecDeque::new()),
                last_reply: Mutex::new((Duration::ZERO, Ok("S=0 K=0 D=0 A=1 F=aa".into()))),
                scripts: Mutex::new(Vec::new()),
                focus_delay: Mutex::new(Duration::ZERO),
                notices: Mutex::new(Vec::new()),
                power: Mutex::new(Vec::new()),
                states: Mutex::new(Vec::new()),
            }
        }
    }

    impl PollEnv for Mock {
        fn view(&self) -> Option<SessionView> {
            (*self.alive.lock().unwrap()).then(|| SessionView {
                wake: self.wake.lock().unwrap().clone(),
                hidden: *self.hidden.lock().unwrap(),
                virtual_display: None,
            })
        }
        fn mutate(&self, f: &mut dyn FnMut(&mut SessionWake)) {
            f(&mut self.wake.lock().unwrap());
        }
        async fn adb_shell(&self, script: &str) -> Result<String, String> {
            self.scripts
                .lock()
                .unwrap()
                .push(if script == FOCUS_SCRIPT {
                    "focus"
                } else {
                    "probe"
                });
            let (d, r) = {
                let mut q = self.replies.lock().unwrap();
                match q.pop_front() {
                    Some(x) => {
                        *self.last_reply.lock().unwrap() = x.clone();
                        x
                    }
                    None => {
                        let last = self.last_reply.lock().unwrap().clone();
                        // Stage-1 default reply when the queue is empty.
                        if script == FOCUS_SCRIPT {
                            (
                                *self.focus_delay.lock().unwrap(),
                                Ok("  mCurrentFocus=Window{aa u0 p/A}".into()),
                            )
                        } else {
                            last
                        }
                    }
                }
            };
            tokio::time::sleep(d).await;
            r
        }
        async fn screen_power(&self, off: bool, _awake: Option<bool>) -> Result<(), String> {
            self.power.lock().unwrap().push(off);
            Ok(())
        }
        fn emit_state(&self, s: &SecureState) {
            self.states.lock().unwrap().push(*s);
        }
        fn emit_notice(&self, text: &str) {
            self.notices.lock().unwrap().push(text.to_string());
        }
        fn emit_screen_power(&self, _off: bool) {}
    }

    fn fast() -> Tuning {
        Tuning {
            floor: ms(5),
            cap: ms(20),
            slow: ms(30),
            gate: ms(5),
            probe_timeout: ms(200),
            restore_debounce: ms(20),
        }
    }

    #[tokio::test]
    async fn loop_wakes_then_restores_and_emits_state_on_change_only() {
        let env = Mock::new(SessionWake::new(true, true, true));
        {
            let mut q = env.replies.lock().unwrap();
            // cold probe clear, then secure appears (focus change), then clear.
            q.push_back((ms(1), Ok("S=0 K=0 D=0 A=1 F=aa".into())));
            q.push_back((ms(1), Ok("  mCurrentFocus=Window{bb u0 p/A}".into())));
            q.push_back((ms(1), Ok("S=1 K=0 D=0 A=1 F=bb".into())));
            for _ in 0..40 {
                q.push_back((ms(1), Ok("S=0 K=0 D=0 A=1 F=aa".into())));
            }
        }
        let env = std::sync::Arc::new(env);
        let e2 = env.clone();
        let h = tokio::spawn(async move { run_poll(&*e2, PollKind::Screen, fast()).await });
        tokio::time::sleep(ms(400)).await;
        *env.alive.lock().unwrap() = false;
        h.await.unwrap();
        let notices = env.notices.lock().unwrap().clone();
        assert!(
            notices.contains(&NOTICE_WOKE_SECURE.to_string()),
            "{notices:?}"
        );
        let power = env.power.lock().unwrap().clone();
        assert_eq!(power, vec![false, true], "wake, then restore");
        // first probe (display known), secure on, secure off
        assert_eq!(env.states.lock().unwrap().len(), 3);
        assert!(env.wake.lock().unwrap().screen_off);
    }

    #[tokio::test]
    async fn loop_does_nothing_while_gate_closed_and_ends_with_session() {
        // Screen on, nothing woken: Mirror never probes.
        let env = std::sync::Arc::new(Mock::new(SessionWake::new(false, true, true)));
        let e2 = env.clone();
        let h = tokio::spawn(async move { run_poll(&*e2, PollKind::Screen, fast()).await });
        tokio::time::sleep(ms(60)).await;
        assert!(env.scripts.lock().unwrap().is_empty(), "no adb call");
        *env.alive.lock().unwrap() = false;
        tokio::time::timeout(ms(500), h).await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn loop_hidden_window_stops_and_resume_probes_at_once() {
        let env = std::sync::Arc::new(Mock::new(SessionWake::new(true, true, true)));
        *env.hidden.lock().unwrap() = true;
        let e2 = env.clone();
        let h = tokio::spawn(async move { run_poll(&*e2, PollKind::Screen, fast()).await });
        tokio::time::sleep(ms(40)).await;
        assert!(env.scripts.lock().unwrap().is_empty());
        *env.hidden.lock().unwrap() = false;
        tokio::time::sleep(ms(30)).await;
        assert_eq!(env.scripts.lock().unwrap().first().copied(), Some("probe"));
        *env.alive.lock().unwrap() = false;
        h.await.unwrap();
    }

    #[tokio::test]
    async fn loop_auto_disables_on_slow_phone_and_notifies_once() {
        let env = std::sync::Arc::new(Mock::new(SessionWake::new(true, true, true)));
        *env.last_reply.lock().unwrap() = (ms(60), Ok("S=0 K=0 D=0 A=1 F=aa".into()));
        *env.focus_delay.lock().unwrap() = ms(60);
        let e2 = env.clone();
        let h = tokio::spawn(async move { run_poll(&*e2, PollKind::Screen, fast()).await });
        tokio::time::timeout(ms(3000), h).await.unwrap().unwrap();
        let n = env.notices.lock().unwrap().clone();
        assert_eq!(n, vec![NOTICE_TOO_SLOW.to_string()]);
        // cold + 5 slow ticks, never more.
        assert!(env.scripts.lock().unwrap().len() <= 8);
    }

    #[tokio::test]
    async fn failures_back_off_without_state_change() {
        let env = std::sync::Arc::new(Mock::new(SessionWake::new(true, true, true)));
        *env.last_reply.lock().unwrap() = (ms(0), Err("closed".into()));
        {
            let mut q = env.replies.lock().unwrap();
            for _ in 0..3 {
                q.push_back((ms(0), Err("closed".into())));
            }
        }
        let e2 = env.clone();
        let h = tokio::spawn(async move { run_poll(&*e2, PollKind::Screen, fast()).await });
        tokio::time::sleep(ms(100)).await;
        *env.alive.lock().unwrap() = false;
        h.await.unwrap();
        assert!(env.states.lock().unwrap().is_empty());
        assert!(env.notices.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn watch_handle_drop_aborts_task() {
        let handle = tauri::async_runtime::spawn(async {
            loop {
                tokio::time::sleep(ms(5)).await;
            }
        });
        let probe = handle.inner().abort_handle();
        let wh = WatchHandle(handle);
        assert!(!probe.is_finished());
        drop(wh);
        tokio::time::sleep(ms(30)).await;
        assert!(probe.is_finished(), "task gone after drop");
    }
}

// ---------------------------------------------------------------------------
// scrcpy screen-power injection (Windows)
// ---------------------------------------------------------------------------

/// Drive scrcpy's screen-power shortcut for the session window: power off
/// (`off == true`) or power on. Returns false when no scrcpy window was found.
/// This reverses exactly what `--turn-screen-off` did, so scrcpy's own state
/// stays consistent (spec §6.1). Not available off Windows (returns false).
pub fn inject_screen_power(serial: &str, pid: u32, off: bool) -> bool {
    #[cfg(windows)]
    {
        inject_screen_power_win(serial, pid, off)
    }
    #[cfg(not(windows))]
    {
        let _ = (serial, pid, off);
        false
    }
}

#[cfg(windows)]
fn inject_screen_power_win(serial: &str, pid: u32, off: bool) -> bool {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, SetForegroundWindow,
        WM_SYSKEYDOWN, WM_SYSKEYUP,
    };
    extern "system" {
        fn AttachThreadInput(id_attach: u32, id_attach_to: u32, f_attach: i32) -> i32;
        fn keybd_event(b_vk: u8, b_scan: u8, dw_flags: u32, dw_extra_info: usize);
        fn GetCurrentThreadId() -> u32;
        fn SetFocus(hWnd: HWND) -> HWND;
    }
    struct EnumData {
        target_pid: u32,
        hwnd: Option<HWND>,
    }
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        if IsWindowVisible(hwnd).as_bool() {
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let data = &mut *(lparam.0 as *mut EnumData);
            if pid == data.target_pid {
                data.hwnd = Some(hwnd);
                return BOOL(0);
            }
        }
        BOOL(1)
    }

    let mut data = EnumData {
        target_pid: pid,
        hwnd: None,
    };
    let _ = unsafe { EnumWindows(Some(enum_proc), LPARAM(&mut data as *mut EnumData as isize)) };
    let hwnd = crate::embed::get_embedded_scrcpy_hwnd(serial)
        .map(|h| HWND(h as *mut _))
        .or(data.hwnd);
    let Some(hwnd) = hwnd else { return false };
    unsafe {
        let target_thread = GetWindowThreadProcessId(hwnd, None);
        let current_thread = GetCurrentThreadId();
        let attached = AttachThreadInput(current_thread, target_thread, 1);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(hwnd);
        std::thread::sleep(Duration::from_millis(50));

        const VK_MENU: u8 = 0x12;
        const VK_SHIFT: u8 = 0x10;
        const VK_O: u8 = 0x4F;
        const KEYEVENTF_KEYUP: u32 = 0x0002;

        keybd_event(VK_MENU, 0, 0, 0);
        if !off {
            keybd_event(VK_SHIFT, 0, 0, 0);
        }
        keybd_event(VK_O, 0, 0, 0);
        keybd_event(VK_O, 0, KEYEVENTF_KEYUP, 0);
        if !off {
            keybd_event(VK_SHIFT, 0, KEYEVENTF_KEYUP, 0);
        }
        keybd_event(VK_MENU, 0, KEYEVENTF_KEYUP, 0);
        if attached != 0 {
            let _ = AttachThreadInput(current_thread, target_thread, 0);
        }
        // Also post the key message directly as a fallback for the SDL window.
        // Only for "off": the plain Alt+O message would undo a power-on.
        if off {
            let _ = PostMessageW(Some(hwnd), WM_SYSKEYDOWN, WPARAM(0x4F), LPARAM(1 << 29));
            let _ = PostMessageW(Some(hwnd), WM_SYSKEYUP, WPARAM(0x4F), LPARAM(1 << 29));
        }
    }
    true
}

// ---------------------------------------------------------------------------
// App wiring (Tauri state)
// ---------------------------------------------------------------------------

/// Per-launch options the frontend passes with `start_mirror` (serde default on).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WakeOpts {
    pub wake_on_secure: bool,
    pub secure_detection: bool,
}

impl Default for WakeOpts {
    fn default() -> Self {
        Self {
            wake_on_secure: true,
            secure_detection: true,
        }
    }
}

/// Payload of the `screen-power` event: the frontend follows the backend.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScreenPowerPayload {
    pub serial: String,
    pub off: bool,
}

use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;

/// True when scrcpy was launched on a virtual display (Flex).
pub fn args_are_flex(args: &[String]) -> bool {
    args.iter()
        .any(|a| a == "--flex-display" || a == "--new-display" || a.starts_with("--new-display="))
}

pub fn args_screen_off(args: &[String]) -> bool {
    args.iter().any(|a| a == "--turn-screen-off")
}

/// Poll environment over a registered session.
pub struct AppEnv {
    pub app: AppHandle,
    pub session_id: String,
    pub serial: String,
}

impl AppEnv {
    fn with_session<R>(&self, f: impl FnOnce(&mut crate::state::Session) -> R) -> Option<R> {
        let st = self.app.try_state::<AppState>()?;
        let mut map = st.sessions.lock().ok()?;
        map.get_mut(&self.session_id).map(f)
    }
}

impl PollEnv for AppEnv {
    fn view(&self) -> Option<SessionView> {
        let (wake, pid, vd) = self.with_session(|s| (s.wake.clone(), s.pid, s.display_id))?;
        Some(SessionView {
            wake,
            hidden: window_hidden(&self.serial, pid),
            virtual_display: vd,
        })
    }

    fn mutate(&self, f: &mut dyn FnMut(&mut SessionWake)) {
        let _ = self.with_session(|s| f(&mut s.wake));
    }

    async fn adb_shell(&self, script: &str) -> Result<String, String> {
        let out = crate::commands::adb_cmd(&self.app)
            .await?
            .args(["-s", &self.serial, "shell", script])
            .output()
            .await
            .map_err(|e| e.to_string())?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
        }
        Ok(String::from_utf8_lossy(&out.stdout).to_string())
    }

    async fn screen_power(&self, off: bool, awake: Option<bool>) -> Result<(), String> {
        let pid = self
            .with_session(|s| s.pid)
            .ok_or_else(|| "session ended".to_string())?;
        // Power on while the phone is truly asleep needs a wake key: scrcpy's
        // toggle only reverses its own display-off.
        let asleep = awake == Some(false);
        if off || !asleep {
            if inject_screen_power(&self.serial, pid, off) {
                return Ok(());
            }
            if off {
                return Err("scrcpy window not found".into());
            }
        }
        // Never KEYCODE_POWER: it toggles and could put the phone to sleep.
        let out = crate::commands::adb_cmd(&self.app)
            .await?
            .args([
                "-s",
                &self.serial,
                "shell",
                "input",
                "keyevent",
                "KEYCODE_WAKEUP",
            ])
            .output()
            .await
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
        }
    }

    fn emit_state(&self, s: &SecureState) {
        let _ = self
            .app
            .emit("secure-state", SecureStatePayload::new(&self.serial, s));
    }

    fn emit_notice(&self, text: &str) {
        crate::kvm::commands::emit_continuity_notice(&self.app, text);
    }

    fn emit_screen_power(&self, off: bool) {
        let _ = self.app.emit(
            "screen-power",
            ScreenPowerPayload {
                serial: self.serial.clone(),
                off,
            },
        );
    }

    fn log(&self, msg: &str) {
        crate::kvm::debug_log::note(|| msg.to_string());
    }
}

/// Start the detector for a freshly registered session. Camera sessions and
/// sessions with detection off get no task.
pub fn start_watch(
    app: &AppHandle,
    session_id: &str,
    serial: &str,
    args: &[String],
    mode: &str,
    opts: WakeOpts,
) -> Option<WatchHandle> {
    if mode == "camera" || !opts.secure_detection {
        return None;
    }
    let kind = if args_are_flex(args) {
        PollKind::Flex
    } else {
        PollKind::Screen
    };
    let env = AppEnv {
        app: app.clone(),
        session_id: session_id.to_string(),
        serial: serial.to_string(),
    };
    Some(WatchHandle(tauri::async_runtime::spawn(async move {
        run_poll(&env, kind, Tuning::default()).await;
    })))
}

/// Minimized or hidden session window: the detector pauses (spec §5.3).
/// Unknown window (not found) counts as visible.
pub fn window_hidden(serial: &str, pid: u32) -> bool {
    #[cfg(windows)]
    {
        window_hidden_win(serial, pid)
    }
    #[cfg(not(windows))]
    {
        let _ = (serial, pid);
        false
    }
}

#[cfg(windows)]
fn window_hidden_win(serial: &str, pid: u32) -> bool {
    use windows::core::BOOL;
    use windows::Win32::Foundation::{HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetAncestor, GetWindowThreadProcessId, IsIconic, IsWindowVisible, GA_ROOT,
    };
    struct EnumData {
        target_pid: u32,
        hwnd: Option<HWND>,
    }
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let data = &mut *(lparam.0 as *mut EnumData);
        if pid == data.target_pid && IsWindowVisible(hwnd).as_bool() {
            data.hwnd = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }
    if let Some(h) = crate::embed::get_embedded_scrcpy_hwnd(serial) {
        let hwnd = HWND(h as *mut _);
        let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
        let top = if root.0.is_null() { hwnd } else { root };
        return unsafe { IsIconic(top) }.as_bool() || !unsafe { IsWindowVisible(top) }.as_bool();
    }
    let mut data = EnumData {
        target_pid: pid,
        hwnd: None,
    };
    let _ = unsafe { EnumWindows(Some(enum_proc), LPARAM(&mut data as *mut EnumData as isize)) };
    match data.hwnd {
        // A window exists and is visible: minimized windows stay "visible" to
        // IsWindowVisible, so check the iconic state too.
        Some(h) => unsafe { IsIconic(h) }.as_bool(),
        // Either hidden or not created yet: the second case is startup, the
        // first is a hidden window. Treat as hidden only when scrcpy has any
        // window (none found at all means still starting: poll).
        None => false,
    }
}
