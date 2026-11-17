# SPEC-29 Secure Content and Screen-Wake Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect secure windows and the keyguard on the phone, wake the phone screen when AnyLeap has it off and the user must see it, warn about Flex limits, and investigate the wake-kills-Flex report.

**Architecture:** A new `src-tauri/src/secure_watch.rs` holds a pure parser, a pure `decide()` state machine and an adaptive two-stage per-session poll task (cheap focus tick, full probe on focus change or every 10th tick, interval = clamp(max(1.5 s, 10 x EWMA), 1.5 s, 6 s), auto-disable after 5 ticks over 1 s). Session records gain screen and wake state. Notices go through `emit_continuity_notice`. Two per-mode settings: `secureDetection` and `wakeOnSecure`.

**Tech Stack:** Tauri 2, Rust, React 19 + TypeScript, Vitest, adb.

**Spec:** `docs/specs/SPEC-29-secure-content.md`.

## Global Constraints
- Never bypass or weaken `FLAG_SECURE`. No pixels of secure content are captured, stored or logged.
- No third-party product names in code, copy or docs (`AGENTS.md` §1.3).
- No automatic task moving between displays.
- Each phase ends green: `cargo clippy`, `cargo test`, `npx tsc --noEmit`, `npx vitest run`, and ships alone.
- SPEC-28 Phase 1 must be on the branch before Phase 2 (the setting lives in its Phone mirroring tabs).
- `wake_on_secure` and `secure_detection` are additive optional fields on `start_mirror` (serde default `true`); note it in the PR.
- Budget rule: detection may use about 10% of wall time and must not cost mirror FPS (at most 2% or 1 fps below detection off, spec §10). Never ship a fixed poll interval; the baseline timings (spec F9) come from one fast phone only.
- The slow-phone simulation must be restored after every run (kill tracked PIDs, verify, reboot if needed).

## Phase 1: detector

### Task 1.1: parser (TDD)
**Files:** create `src-tauri/src/secure_watch.rs`, `src-tauri/tests/fixtures/secure/*.txt`; modify `src-tauri/src/lib.rs` (mod).
- [ ] Record fixtures: Android 16 `dumpsys window` normal, secure (`fl=… SECURE …`), keyguard showing, Flex display focus.
- [ ] Failing tests for `parse_secure_probe` (valid, `D=-1`, malformed) and the fixture grep (`SECURE` only on the `fl=` line).
- [ ] Implement `SecureState`, `parse_secure_probe`, `SECURE_PROBE_SCRIPT` (spec §5.2). Run `cargo test secure_watch`; commit.

### Task 1.2: adaptive math (TDD, pure functions)
**Files:** `src-tauri/src/secure_watch.rs`.
- [ ] Failing tests for `next_interval(ewma)`: 94 ms gives 1.5 s, 400 ms gives 4 s, 700 ms gives 6 s, 0 gives 1.5 s; EWMA alpha 0.3 with the cold first probe ignored (899 ms sample not used).
- [ ] Failing tests for the auto-disable counter: N = 5 consecutive ticks with EWMA above 1 s; reset on a lower tick; notice once per session.
- [ ] Failing tests for `plan_tick(prev_focus_id, focus_id, tick_count, K = 10)`: same id is stage 1, changed id, K-th tick, first tick and restore tick are stage 2; unparsable focus is a failed tick.
- [ ] Failing tests for failure backoff (double per failure, cap 6 s, two good ticks return to the formula) and `should_poll(mode, screen_off, woke_by_us, minimized, secure_detection)`.
- [ ] Implement them as pure functions. Run `cargo test secure_watch`; commit.

### Task 1.3: poller task
**Files:** `src-tauri/src/secure_watch.rs`, `src-tauri/src/commands.rs`.
- [ ] Implement the two-stage task: stage 1 `dumpsys window | grep -m1 mCurrentFocus` every tick; stage 2 (`SECURE_PROBE_SCRIPT`) on focus change, every 10th tick, session start and restore. One probe at a time, 2 s timeout, `secure-state` on change only.
- [ ] Run only when needed: Mirror/HiFi while `screen_off` or `woke_by_us`; Flex while the window is visible; stop when minimized or hidden; immediate stage 2 on restore; skip camera sessions and `secure_detection == false`.
- [ ] Auto-disable: after 5 ticks over 1 s EWMA end the task, reset state, emit the one-time notice "This phone is too slow for secure-screen detection. Turned off for this session."
- [ ] Start and abort it in `spawn_session_inner` and on session removal; test that no task, timer or adb child remains after the session ends.
- [ ] Device: resolve Q1 (focus on the Flex display), Q4 (USB floor) and Q6 (lock screen changes `mCurrentFocus`); record in spec §13. Commit.

### Task 1.4: FPS acceptance (device)
**Files:** a test script under `scripts/` (no product code).
- [ ] Script: 60 s runs, detection on versus off, 3 runs per arm, FPS from `scrcpy --print-fps` or the AnyLeap frame counter; pass at most 2% (or 1 fps) drop and no worse than 5 fps in the lowest second.
- [ ] Run on the TECNO CM7.
- [ ] Run on a simulated slow phone: CPU-hog busy loops over adb (spec §10), PIDs saved, validated slower probe (above 300 ms), 5 minute and temperature caps, `trap` kill, verification, reboot if a hog survives. Pass: threshold holds, or auto-disable fires within 5 ticks and FPS recovers.
- [ ] Confirm the phone is restored (no hogs, baseline probe time). Record numbers in spec §13. Commit.

## Phase 2: Mirror wake

### Task 2.1: state machine (TDD)
**Files:** `src-tauri/src/secure_watch.rs`.
- [ ] Failing tests for every row of spec §6.2 and §7 (rising edge, 3 s debounce, user toggle, session end, overlap, setting off).
- [ ] Implement `decide(prev, next, &SessionWake) -> Vec<Action>` as a pure function, no I/O. Commit.

### Task 2.2: session record and actions
**Files:** `src-tauri/src/commands.rs`, `src/components/ControlBar.tsx`, `src/store/useAppStore.ts`.
- [ ] Add `screen_off`, `woke_by_us`, `wake_on_secure`, `secure_detection` to the session record; launch and `restart_with_screen_off` keep `screen_off` current.
- [ ] Implement `ScreenOn` (scrcpy power on; `KEYCODE_WAKEUP` fallback when not Awake or on error; never `KEYCODE_POWER`) and `ScreenOffRestore`.
- [ ] Emit `screen-power` so `setDeviceToggle(serial, "screenOff", …)` follows the backend.
- [ ] Notices via `emit_continuity_notice`; resolve Q2 and Q3. Commit.

### Task 2.3: `secureDetection` and `wakeOnSecure` settings
**Files:** `src/lib/modeSettings.ts` (+ test), `src/components/settings/**`, launch paths.
- [ ] Failing tests: `secureDetection` default on in Mirror, High fidelity and Flex; `wakeOnSecure` default on; both kept by the SPEC-28 migration; `wakeOnSecure` disabled state follows `turnScreenOff` and `secureDetection`.
- [ ] Add both fields; Switch "Detect secure screens" on the Mirror, High fidelity and Flex tabs and "Turn the phone screen on for secure screens" on Mirror and High fidelity, under "Phone while mirroring" (labels and helpers from spec §6.3); pass `secure_detection` and `wake_on_secure` to `start_mirror`. Commit.
- [ ] Device test steps 1-3, 6 and 6a of spec §10.

## Phase 3: Flex

### Task 3.1: Flex notice and keyguard wake
**Files:** `src-tauri/src/secure_watch.rs`, `src-tauri/src/commands.rs`.
- [ ] Parse `virtual_display_id` from the scrcpy log line `New display: … (id=N)` (test with the real line).
- [ ] `decide()` Flex arms: notice on the rising edge on the virtual display only; keyguard wake always on. Commit.

### Task 3.2: one-time warning
**Files:** `src/lib/settingsStore.ts`, Flex launch path, Transfer Bar.
- [ ] Persist `flexSecureWarningDismissed`; show the notice with a "Got it" action on first Flex start only. Test the flag logic. Commit.
- [ ] Device test steps 4, 5, 7, 8 of spec §10.

## Phase 4: wake/Flex investigation (parallel)

### Task 4.1: user-assisted reproduction
- [ ] Follow spec §9 with a physical power press; capture `dumpsys power`, `dumpsys display`, SleepToken lines and frame growth.
- [ ] Write the result in spec §13 (Q5). Decide on mitigations M1-M4 (open questions); if a fix is needed, open a new SPEC. No product code in this phase.
