# SPEC-29: Secure Content and Screen-Wake While Mirroring

> **Status:** Approved (2026-11-17)
> **Scope:** Rust session layer (`src-tauri/src/commands.rs`), a new `secure_watch` backend module, the Transfer Bar notice path (`src-tauri/src/kvm/commands.rs`, `src/components/desktop/TransferBarWindow.tsx`), the control bar screen-off toggle (`src/components/ControlBar.tsx`), and one per-mode setting (SPEC-28 Phone mirroring tabs).
> **Related:** SPEC-28 (per-mode settings), SPEC-25 (Continuity notices), GitHub issue #86.
> **Plan:** `docs/superpowers/plans/2026-11-17-spec29-secure-content.md`
> **Evidence:** device findings for issue #86 (TECNO CM7, Android 16, scrcpy 5.0.1, wireless adb).

## 1. Problem

1. **Secure screens are black in a mirror.** An app can mark a window as secure (`FLAG_SECURE`). Android blanks it in any capture, including scrcpy. In **Mirror mode** with the phone screen off (`--turn-screen-off`), the user sees black and cannot tell why. The fix is on the phone (look at it), but the phone panel is off.
2. **Secure content in Flex is worse.** A secure activity started on the Flex virtual display stays there, renders black, and still counts as seen. A messaging app's view-once media is consumed by opening it, so one tap in Flex destroys the message with nothing shown. Detection cannot prevent this: it happens at open.
3. **The keyguard lives on the phone panel only** (display 0). A locked phone cannot be unlocked from a Flex window, and with the screen off the user does not know the phone is waiting for a PIN or biometric.
4. **"Waking the phone kills Flex"** (issue #86 report). Not reproduced with adb key events (§9). The report involved the physical power button.

## 2. Goals

- G1. Detect "a secure window has focus" and "the keyguard is showing", per active display session, within a fixed duty-cycle budget that adapts to slow phones (§5.3).
- G1b. Never cost the mirror measurable frame rate (§10 acceptance criteria).
- G2. In Mirror mode with the screen off, turn the phone screen on when either appears, tell the user why, and restore screen-off afterwards only if AnyLeap turned it on.
- G3. In Flex mode, tell the user that secure content cannot be shown there; wake the phone screen when the keyguard shows.
- G4. Warn once, dismissibly, about view-once media in Flex.
- G5. Investigate the wake-kills-Flex report with a reproducible, user-assisted procedure.

## 3. Non-goals

- **Never bypass `FLAG_SECURE`.** No capture of secure layers, no `mDisableSecureWindows` or developer-option tricks, no root or privileged APIs, no screenshots of secure content. Black stays black.
- No automatic moving of tasks between displays (§7.3).
- No blocking or intercepting app launches in Flex.
- No inspection of window content, only the flag.
- No change to scrcpy itself.

## 4. Confirmed facts (device findings)

| # | Fact |
|---|---|
| F1 | A focused secure window has the word `SECURE` in the `fl=` line of its `mAttrs` in `dumpsys window windows`. A normal window has no such token. |
| F2 | The whole `dumpsys window` is about 86 KB; reading it is the cost of every probe (timings in F9). |
| F3 | `mCurrentFocus` is printed per display (`null` on unfocused displays). Any window can carry the flag, not only the focused one. |
| F4 | A secure activity started with `am start --display <vd>` is **not refused or moved**: it runs on the virtual display with `mDisplayId=<vd>` and `SECURE`. scrcpy logs nothing. Black is inferred from the flag, not observed (no pixels were inspected). |
| F5 | The keyguard shows on display 0 only. Readable keys: `isKeyguardShowing`, `mDreamingLockscreen`, `KeyguardController: mKeyguardShowing`. The per-display line `KeyguardShowing=… at display=N` (from `dumpsys activity activities`) tells which display. The Flex display and scrcpy survive a keyguard. |
| F6 | Killing scrcpy does not restore screen state. The phone can be left `Dozing`. |
| F7 | `--turn-screen-off` leaves the phone `mWakefulness=Awake` with the physical panel `OFF`. Key events (`KEYCODE_WAKEUP`, `KEYCODE_POWER`) turned the panel back on in 3 of 3 runs. |
| F8 | Lead: Android adds a `SleepToken "Display-off"` on the virtual display when the phone sleeps. It may pause Flex activities. Unproven (§9). |
| F9 | Baseline (TECNO CM7, 8 cores, mostly idle, wireless adb, 10 runs each): the full probe (focused window plus `SECURE` flag) takes about **130 ms** (121-180 ms), the first run cold **899 ms**. The focus-only check (`dumpsys window \| grep -m1 mCurrentFocus`) takes about **94 ms**. This is one fast phone and a **baseline, not a guarantee**: many phones are slower, so the detector measures itself (§5.3). |

## 5. Detector

### 5.1 Output

```rust
struct SecureState {
    secure_focused: bool,            // focused window has SECURE
    keyguard_showing: bool,          // keyguard is up (display 0)
    focused_display_id: Option<u32>, // display of the focused window, None if unknown
}
```

Emitted as the Tauri event `secure-state` (payload `{ serial, secureFocused, keyguardShowing, focusedDisplayId }`) **only on change**, never per poll.

### 5.2 adb commands (two stages)

**Stage 1, every tick (cheap focus check):**

```sh
adb -s <serial> shell 'dumpsys window | grep -m1 mCurrentFocus'
```

The window identity is the `Window{<id> ...}` token. About 94 ms on the baseline phone (F9). Stage 1 alone never changes `SecureState`.

**Stage 2 (full probe)** runs only when (a) the focused window identity differs from the previous tick, (b) every **K = 10th** tick as a safety refresh (catches a flag or keyguard change with no focus change), or (c) as the immediate probe at session start and on restore. One `adb -s <serial> shell` per stage-2 round, parsed on the device, one short output line, no files written on the phone:

```sh
adb -s <serial> shell 'w=$(dumpsys window); \
 f=$(echo "$w" | grep "mCurrentFocus=Window" | head -n1 | sed "s/.*Window{\([0-9a-f]*\) .*/\1/"); \
 s=0; [ -n "$f" ] && s=$(echo "$w" | grep -A6 "Window{$f " | grep -c " SECURE "); \
 k=0; echo "$w" | grep -q "isKeyguardShowing=true" && k=1; \
 d=$(echo "$w" | grep -A3 "Window{$f " | grep -m1 -o "mDisplayId=[0-9]*" | cut -d= -f2); \
 echo "S=$s K=$k D=${d:--1}"'
```

Stage 2 extends the validated single-call form (`mCurrentFocus` to window id, then `grep -c " SECURE "` in the window block) with the keyguard key and the display id. Capturing `dumpsys window` once into a variable avoids the `grep -m1` broken-pipe noise seen on device.

Host-side parsing, pure function `parse_secure_probe(&str) -> Option<SecureState>`:
- Expected form `S=<n> K=<0|1> D=<int>`; `secure_focused = S >= 1`. Anything else returns `None` and counts as a failed probe (§5.4); state unchanged.
- `D=-1` maps to `focused_display_id = None`.
- Only the focused window is checked. A secure window that is not focused (F3) is not reported in v1 (R2).
- Keyguard is always display 0 (F5); the global `isKeyguardShowing` is enough for the wake decision.
- `focusedDisplayId` decides which behaviour applies (§7). Whether the global `mCurrentFocus` follows the Flex display is open question Q1, verified in Phase 1.

The window-block depth (`-A6`) and the `fl=` position are version-sensitive. The parser is tested against recorded dumps (§10), and the probe fails closed (`None`) on a shape it does not understand.

### 5.3 Poller (adaptive)

**One task per display session** (Mirror, Flex, High fidelity; not Camera), keyed by serial, started in `spawn_session_inner` after the session is registered, aborted when the session is removed. One probe at a time per device, never overlapping, 2 s timeout.

**When it runs (only when needed):**

| Mode | Polls while | Why |
|---|---|---|
| Mirror, High fidelity | AnyLeap has the phone screen **off** (`screen_off`), or `woke_by_us` is set (needed for the restore, §6.2) | With the screen on the user sees the phone; nothing to wake or restore |
| Flex | the session window is visible and not minimized | The notice and keyguard wake need it |
| Any | never while the session window is minimized or hidden | The backend already knows `minimized` (`commands.rs:2070-2135`). On restore, run one stage-2 probe at once |

When the session ends, the task is aborted and no timer, interval or child process remains (tested, §10). Detection is also off when the per-mode setting `secureDetection` is off (§6.3).

**Duty-cycle budget (about 10% of wall time):**

- Time every tick (stage 1 or stage 2 round trip). Ignore the **first (cold) probe** of a session: it is not fed into the average (baseline cold run 899 ms, F9).
- `ewma = 0.3 * duration + 0.7 * ewma`, seeded with the second probe.
- `interval = clamp(max(1.5 s, 10 * ewma), 1.5 s, 6 s)`. On the baseline phone `10 * 94 ms = 0.94 s`, so the 1.5 s floor applies (about 6% duty). A phone with a 400 ms tick gets 4 s. A phone at 600 ms or more sits at the 6 s cap, where duty can exceed 10% (up to about 17% at 1 s); this is accepted because auto-disable follows.
- **Auto-disable:** if `ewma > 1 s` for **N = 5** consecutive ticks, detection is turned off for this session: the task ends, `secure-state` resets to clear, and a pending restore runs once if `woke_by_us`. Show one notice, once per session: **"This phone is too slow for secure-screen detection. Turned off for this session."** The setting itself is not changed; the next session starts the detector again and re-measures.
- **Failure backoff (kept):** a failed or timed-out probe doubles the current interval (cap 6 s). Three consecutive failures: keep the last state, hold at 6 s (§5.4). Two consecutive successful ticks return the interval to the formula above. A file transfer or recording on the same device slows ticks, so the budget reacts without special cases.

**Stage logic per tick** (pure function `plan_tick(prev_focus_id, focus_id, tick_count, K) -> Stage`): stage 2 if `focus_id != prev_focus_id`, or `tick_count % K == 0` (K = 10); otherwise stage 1 only. A stage-1 failure or an unparsable focus line is a failed tick and leaves the stored focus id unchanged.

**Keyguard timeliness:** when the phone locks, the focused window changes to the keyguard window, so stage 1 sees a new identity and triggers stage 2 on the same tick. Worst delay: one interval (1.5 s to 6 s) plus a stage-2 round (about 130 ms on the baseline). The K-th-tick refresh covers a keyguard that appears with no focus change (worst delay K intervals: 15 s at the floor). Whether the lock screen really changes `mCurrentFocus` on the target Android versions is verified on device in Phase 1 (Q6); if not, K drops to 3.

Budget summary: at most one `dumpsys window` per interval per device (1.5 s floor), one in K of them the larger stage-2 probe, and nothing when the screen is on (Mirror/HiFi), the window is minimized, or detection is off.

### 5.4 Failure

Three consecutive failed probes: keep the last state, go to the 6 s cap. A disconnect ends the session through the existing path. The poller never shows errors to the user; the only notice it can show is the one-time auto-disable notice (§5.3).

## 6. Mirror mode behaviour

### 6.1 Trigger and mechanism

Applies when detection is on (§6.3), the session mode is `mirror` or `hifi` **and** AnyLeap has the phone screen off (`screen_off` in the session record: `turn_screen_off` at launch or the ControlBar toggle).

When `secureFocused || keyguardShowing` becomes true and `screen_off` is true:
1. Turn the phone screen on.
2. Emit a Transfer Bar notice: **"Secure screen on the phone. Screen turned on so you can see it."** (secure) or **"Phone is locked. Screen turned on so you can unlock it."** (keyguard).
3. Set `screen_off = false`, `woke_by_us = Some(Secure | Keyguard)`.

**Wake mechanism: the scrcpy screen-power toggle (power on), not `KEYCODE_WAKEUP`.**

| | scrcpy power on (the path behind `restart_with_screen_off(off=false)`) | `KEYCODE_WAKEUP` |
|---|---|---|
| Reverses what we did | Yes. `--turn-screen-off` drives scrcpy's display power control; power on is the matching call | A different layer (F7: it works, but scrcpy's own state is untouched) |
| scrcpy internal state | Stays consistent, the next off toggle behaves | scrcpy still believes the screen is off; its next "off" can be a no-op and a reconnect can re-apply off |
| Side effects | None on input or lock state | Counts as user activity, can dismiss a dream, extends screen timeout, shows the keyguard if truly asleep |
| Works when the phone is truly asleep (`Dozing`, F6) | No: wakefulness unchanged | Yes |
| Platform | Existing Windows key-injection path; fallback is the restart in `restart_with_screen_off` | Pure adb, any OS |

Decision: **scrcpy power on first.** Fallback **`KEYCODE_WAKEUP`** when the phone is not `Awake` (Q2: probe adds `mWakefulness`) or the power-on path errors, before the restart fallback. Never `KEYCODE_POWER` (it toggles and could put the phone to sleep).

### 6.2 Restore rule (state machine)

Per session: `screen_off: bool`, `woke_by_us: Option<Reason>`.

| Event | State after | Action |
|---|---|---|
| Trigger, `screen_off`, setting on | `screen_off=false`, `woke_by_us=Some(r)` | wake, notice |
| Trigger, `screen_off` false | unchanged | nothing (user already sees it) |
| Both flags clear, `woke_by_us=Some(_)` | `woke_by_us=None`, `screen_off=true` | after a **3 s debounce** (clear for 2 consecutive stage-2 probes and at least 3 s), scrcpy power off, no notice |
| Both flags clear, `woke_by_us=None` | unchanged | nothing |
| User toggles screen-off in the ControlBar while `woke_by_us=Some` | `woke_by_us=None` | user choice wins, no auto restore |
| Physical power press or phone sleeps (wakefulness not Awake) while `woke_by_us=Some` | `woke_by_us=None` | cancel restore |
| Secure clears, keyguard still showing | `woke_by_us=Some(Keyguard)` | stay on |
| New trigger inside the debounce | unchanged | cancel pending restore |
| Session ends | reset | nothing (never touch a dead session) |

Restore never runs if the screen was off for another reason, and never wakes twice.

### 6.3 Settings

Two per-mode keys in `ModeSettings` (SPEC-28 Phone mirroring tabs), group "Phone while mirroring":

**1. Detection master switch, Mirror, High fidelity and Flex (not Camera):**
- Label: **Detect secure screens**
- Helper: "Checks the phone now and then for a lock screen or a protected screen. Uses a little adb time. Turn off if the phone is slow."
- Key: `secureDetection: boolean`, **default on in every mode**. High fidelity does not default off: the budget (§5.3) is mode independent, its heavy cost is the video path rather than the probe, and auto-disable protects slow phones. The slow-phone acceptance run (§10) covers High fidelity; if it fails there, that mode's default flips to off in a follow-up.
- Off means: no poller, no wake, no Flex notice, no Flex keyguard wake (the user accepts blind black screens and an unseen lock). The one-time Flex warning (§7.4) still shows because it needs no detection.

**2. Wake switch, Mirror and High fidelity only:**
- Label: **Turn the phone screen on for secure screens**
- Helper: "When the phone shows a lock screen or a protected screen (for example a private browser tab), AnyLeap turns the phone screen on so you can see it, then turns it off again."
- Key: `wakeOnSecure: boolean`, **default on**. Disabled, with the reason "Only used when the phone screen is turned off", while "Turn phone screen off" is off; disabled with "Needs Detect secure screens" while `secureDetection` is off.

Neither key is part of `build_scrcpy_args`. The frontend passes them at session start as optional `wake_on_secure` and `secure_detection` fields (serde default `true`, so older callers work); the backend stores them on the session record. This is a small additive change to the SPEC-28 "no Rust IPC change" rule and is called out in the plan.

Flex: with detection on, the keyguard wake (§7.2) is always on and not a separate setting; the user cannot see the keyguard otherwise.

## 7. Flex mode behaviour

Apps run on the virtual display; the phone panel is usually off or ignored.

### 7.1 Secure window on the Flex display

When `secureFocused` is true and `focusedDisplayId` is the session's virtual display id (not 0):
- Transfer Bar notice: **"Secure content can't be shown in Flex. Open it on your phone."**
- Shown once per rising edge; re-armed when the flag clears. Not repeated per poll.
- No wake: the activity lives on the virtual display, nothing to see on the phone. The notice says what to do.

A secure window focused on display 0 while Flex runs is visible on the phone panel; no notice.

### 7.2 Keyguard

`keyguardShowing` becomes true in Flex: turn the phone screen on with the §6.1 mechanism (when AnyLeap has it off) and show **"Phone is locked. Screen turned on so you can unlock it."** Restore follows §6.2 (reason `Keyguard`). Always on.

### 7.3 No automatic task moving

AnyLeap does not move the secure task to display 0 (for example with a display-move command). Reasons:
1. View-once media is consumed on open (F4). Moving after detection cannot undo that; the moved task shows black or the already-consumed state.
2. Moving restarts or recreates the activity, which can reset a private session, drop typed input, or lose the opened state.
3. It silently pulls the user's Flex workflow onto the phone panel and can hide other tasks.
4. It needs a task id parsed from `dumpsys`, a bigger fragile surface than the flag.

### 7.4 One-time warning

On the **first Flex session start**, show a dismissible Transfer Bar notice: **"Opening view-once media in Flex shows black and still marks it as seen."** One action: "Got it". Persisted as `flexSecureWarningDismissed: boolean` in `settings.json` (next to `modeDefaultsNotice`), per user not per device. It returns only if the flag is cleared.

## 8. Backend and IPC

| Item | Detail |
|---|---|
| New module | `src-tauri/src/secure_watch.rs`: `parse_secure_probe`, `SecureState`, pure `decide(prev, next, &SessionWake) -> Vec<Action>`, and the poll task, `plan_tick`, `next_interval`, auto-disable counter |
| `Action` | `NoticeSecureFlex`, `NoticeDetectionTooSlow`, `NoticeWokeSecure`, `NoticeWokeKeyguard`, `ScreenOn(reason)`, `ScreenOffRestore` |
| Session record | gains `screen_off: bool`, `woke_by_us: Option<WakeReason>`, `wake_on_secure: bool`, `secure_detection: bool`, `virtual_display_id: Option<u32>` (from the scrcpy log line `New display: … (id=N)`) |
| Notice path | `emit_continuity_notice(app, text)` as is (`kvm/commands.rs:368`), event `kvm-continuity-notice`, shown by `TransferBarWindow.tsx`. Whether the Transfer Bar window can open without a KVM link is Q3 |
| Screen state sync | The ControlBar toggle and the poller write the same `screen_off`; the backend emits `screen-power` so the frontend `setDeviceToggle(serial, "screenOff", …)` follows |
| Event | `secure-state` (change only) |

## 9. Wake and Flex investigation (task)

Goal: find out whether waking the phone stops or pauses Flex (issue #86), and why.

**Not reproduced:** 3 runs (`KEYCODE_WAKEUP`, `KEYCODE_POWER`, `KEYCODE_WAKEUP`) with `--turn-screen-off` and Flex: scrcpy lived, the virtual display stayed, no scrcpy log line at wake. Frame production was not measured. The user's report used the **physical power button**.

Procedure (user-assisted, one device):
1. Start Flex with the screen off; open a visibly animating app on the virtual display.
2. Measure frames: `scrcpy -v` debug logs, or `--record=<temp>` and sample file growth each second.
3. The user presses the physical power button once (screen on), waits 10 s, presses again (off, locked), waits 10 s.
4. At each step capture `dumpsys power` (`mWakefulness`, display power), `dumpsys display` (`type=VIRTUAL`, state), SleepToken lines in `dumpsys window` and `dumpsys activity activities`, and the resumed activity on the Flex display.
5. Repeat with adb `KEYCODE_WAKEUP` / `KEYCODE_SLEEP` in the same setup for comparison.

Hypothesis to confirm or drop (F8): sleep adds a `SleepToken "Display-off"` for the virtual display, Flex activities pause and stop drawing, and a wake removes the token for display 0 only.

Possible mitigations, **open questions, not commitments**:
- M1. scrcpy options that change virtual display power behaviour (check the scrcpy 5.0.1 docs).
- M2. Re-apply display power after a wake seen by the poller while a Flex session runs.
- M3. Check whether `stayAwake` removes the token for Flex.
- M4. If paused activities are the cause, resume the display's top task.

Output: findings recorded in §13 (Q5), and a follow-up SPEC if a fix is needed. This spec ships no product change for the wake report.

## 10. Test plan

**Unit (Rust, `cargo test`):**
- `parse_secure_probe`: `S=1 K=0 D=0`, `S=0 K=1 D=-1`, `S=1 K=0 D=34`; malformed (`""`, `S=x`, missing field) returns `None`.
- Flag parsing against recorded `dumpsys window` fixtures from Android 16: normal window, secure window, keyguard showing, Flex focus. `SECURE` appearing outside the `fl=` line (a title, a package name) must not match.
- `decide()` state machine: wake on rising secure or keyguard with screen off; no wake when the screen is on; no wake when the setting is off; restore only if `woke_by_us`; restore debounced and cancelled by a new trigger; user toggle clears `woke_by_us`; secure and keyguard overlapping keep the screen on until both clear; Flex notice only on the rising edge on the virtual display; Flex keyguard wake ignores the setting; session end resets.
- Interval math (pure `next_interval`): EWMA 94 ms gives 1.5 s; 200 ms gives 2 s; 400 ms gives 4 s; 700 ms gives 6 s (cap); 0 ms gives 1.5 s (floor). The first (cold) probe is not fed to the EWMA (an 899 ms first sample leaves the EWMA unset). EWMA update (alpha 0.3) on a known sequence.
- Failure backoff: doubles per failed probe, caps at 6 s, back to the formula after two successful ticks.
- Auto-disable (pure counter): EWMA above 1 s for 5 consecutive ticks disables; 4 ticks then one under 1 s resets the count; exactly 1.0 s does not count; the notice is emitted once; a new session starts with a clean counter.
- Two-stage trigger (`plan_tick`): same focus id gives stage 1; changed id gives stage 2; `tick_count % 10 == 0` gives stage 2; an unparsable focus line is a failed tick and keeps the old id; the first tick and the restore tick give stage 2.
- Run-only-when-needed gate (pure `should_poll(mode, screen_off, woke_by_us, minimized, secure_detection)`): Mirror and HiFi true only with screen off or `woke_by_us`; Flex true when visible; false when minimized or detection off.
- Teardown: ending a session leaves no task or timer (the abort handle is finished).
- TS (`modeSettings.test.ts`): `secureDetection` default on in Mirror, High fidelity and Flex; `wakeOnSecure` default on; both kept by the SPEC-28 migration; `wakeOnSecure` disabled state follows `turnScreenOff` and `secureDetection`.

**Device (manual, TECNO CM7 or any Android 14+), a browser's private tab as the secure window:**
1. Mirror, screen off, open a private tab on the phone: panel turns on, notice shows. Close the tab: about 3 s later the screen is off again.
2. Same with the setting off: no wake, no notice.
3. Mirror, the user turns the screen on from the ControlBar during a private tab, closes the tab: the screen stays on.
4. Flex, start a private tab on the virtual display: the Flex notice shows once; no task moves; no wake.
5. Lock the phone with the power key during Flex: keyguard wake and notice; unlock; screen-off restored.
6. Minimize the session window: polling stops (check with an adb call counter); restoring resumes with one immediate probe. With the Mirror screen on and nothing woken by AnyLeap, the counter stays at zero. Ending the session leaves no adb child process or timer.
6a. Lock the phone with the power key while Mirror screen-off is active: the keyguard is detected on the next interval (focus identity change triggers stage 2); record the time from lock to wake.
7. First Flex start shows the one-time warning; dismissed, it does not return.
8. No screenshot or recording of secure content in any step.

**Acceptance: no measurable FPS cost.** Measure delivered mirror frame rate with detection **on** versus **off** (`secureDetection`) in the same mode, same content (a looping animation or video on the phone), same link:
- Source: `scrcpy --print-fps` or AnyLeap's own frame counter, whichever the build exposes; use one source for both arms.
- Method: 60 s per run, discard the first 5 s, 3 runs per arm alternating off/on/off/on/off/on, compare the mean of per-run means. For Mirror/HiFi the screen is off so the poller is active.
- **Pass: mean FPS with detection on is at most 2% below detection off (or at most 1 fps, whichever is larger), and the lowest one-second FPS is not more than 5 fps below off's lowest.**
- Record the adaptive state too: final interval, final EWMA, whether auto-disable fired.
- Run on **(a) the TECNO CM7** (baseline phone) and **(b) a simulated slow phone** (below). On (b) pass means either the FPS threshold holds with detection on, **or** auto-disable fires within N = 5 ticks of the EWMA passing 1 s, shows the notice once, and FPS is within the threshold afterwards. A pass on (a) alone is not enough.

**Simulating a slow phone (no root, reversible):** load the CPU with shell busy loops that we start and then kill.
1. Pre-check: record `adb shell getprop ro.product.model`, `adb shell nproc`, and battery level and temperature (`adb shell dumpsys battery`). Do not start below 30% battery or above 38 C; keep the phone on USB power or a charger.
2. Start N hog loops (N = core count, 8 on the CM7; N = cores / 2 for a milder case). Per hog: `adb shell "sh -c 'while :; do :; done' >/dev/null 2>&1 & echo \$!"`, appending each printed PID to `hogs.txt` on the PC.
3. Validate the simulation: `time adb shell "dumpsys window | grep -m1 mCurrentFocus"` must now be clearly slower than the 94 ms baseline (target above 300 ms; add hogs if not). If it is not slower, the run is invalid.
4. Run the FPS test. Cap the hog session at 5 minutes; abort if battery temperature reaches 42 C.
5. **Restore, always (also on failure or Ctrl-C; the script uses a `trap`):** `adb shell kill <pid>` for each PID in `hogs.txt`, then verify with `adb shell "ps -A | grep -c 'while [:]'"` returning 0 and `adb shell top -n 1 -b | head -n 15` showing normal load. If a hog survives, `adb reboot` (a reboot always undoes it). Confirm temperature and the focus-only probe time are back to baseline (about 94 ms), then delete `hogs.txt`.
The hogs change no settings, files or apps.

## 11. Phasing

1. **Phase 1: detector.** Parser, two-stage probe, adaptive poller (EWMA interval, auto-disable, run-only-when-needed gate, failure backoff), `secure-state` event, unit tests, fixtures, FPS acceptance on both phones. Resolves Q1, Q4 and Q6. Ships alone, no UI.
2. **Phase 2: Mirror wake.** Session record fields, `decide()`, scrcpy power on/off with `KEYCODE_WAKEUP` fallback, Transfer Bar notice, restore rule, `wakeOnSecure` in the SPEC-28 tabs. Requires SPEC-28 Phase 1.
3. **Phase 3: Flex.** Flex notice, keyguard wake, one-time warning.
4. **Phase 4: wake investigation (§9),** in parallel with 2 and 3.

## 12. Risks

| ID | Risk | Mitigation |
|---|---|---|
| R1 | `dumpsys` format differs on other Android versions or OEMs. | Fail closed (`None`), fixtures per version, trust only `SECURE` on the `fl=` line. |
| R2 | A secure window that is not focused is missed. | Accepted in v1. Later option: scan all windows on visible displays. |
| R3 | Polling adds adb load, mirror jank or slow-phone cost. | 10% duty budget (interval = max(1.5 s, 10 x EWMA), cap 6 s), cheap focus-only ticks, full probe only on focus change or every 10th tick, runs only when the screen is off (Mirror/HiFi) or visible (Flex), auto-disable after 5 ticks over 1 s, `secureDetection` setting, FPS acceptance on the baseline and a simulated slow phone. |
| R9 | The slow-phone simulation stresses the real device. | Battery and temperature limits, 5 minute cap, PID-tracked kill with `trap`, verified restore, reboot as final undo. |
| R4 | Auto-wake surprises users (screen lights up). | Notice with the reason, per-mode setting, restore after clear. Default on because unexplained black is the worse surprise. |
| R5 | Wrong restore (screen off while the user looks at it). | Restore only if `woke_by_us`, debounce, cancel on any user action (toggle, power key). |
| R6 | scrcpy power-on is Windows key injection; the fallback restarts scrcpy. | `KEYCODE_WAKEUP` before the restart fallback; test the non-Windows path later. |
| R7 | Privacy of reading window metadata. | Only the flag and focus ids on the paired device; nothing stored or sent; no pixels touched. |
| R8 | View-once media is still consumed in Flex. | Not preventable (§1). The one-time warning (§7.4) and the notice are the mitigation. |

## 13. Open questions

- Q1. Does the global `mCurrentFocus` follow the Flex display when an app there has focus, or must the probe read focus per display (`mFocusedApp`, per-display sections)?
- Q2. Add `mWakefulness` to the probe to choose between scrcpy power on and `KEYCODE_WAKEUP`? (One more grep, no extra round.)
- Q3. Can the Transfer Bar window show a notice with no KVM link active, or does it need a creation path outside `kvm`?
- Q4. Does the 1.5 s floor suit USB as well as wireless, or should it be 1 s on USB? (Measure stage-1 cost on USB; the formula already adapts above the floor.)
- Q5. Wake investigation findings (§9): fill in after the user-assisted run.
- Q6. Does the lock screen always change `mCurrentFocus` (so stage 1 sees it) on Android 14-16 and other OEMs? If not, lower K for the keyguard check.
