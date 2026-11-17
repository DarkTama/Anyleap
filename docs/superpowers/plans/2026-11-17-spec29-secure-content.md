# SPEC-29 Secure Content and Screen-Wake Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Detect secure windows and the keyguard on the phone, wake the phone screen when AnyLeap has it off and the user must see it, warn about Flex limits, and investigate the wake-kills-Flex report.

**Architecture:** A new `src-tauri/src/secure_watch.rs` holds a pure parser, a pure `decide()` state machine and a per-session poll task. Session records gain screen and wake state. Notices go through `emit_continuity_notice`. One new per-mode setting, `wakeOnSecure`.

**Tech Stack:** Tauri 2, Rust, React 19 + TypeScript, Vitest, adb.

**Spec:** `docs/specs/SPEC-29-secure-content.md`.

## Global Constraints
- Never bypass or weaken `FLAG_SECURE`. No pixels of secure content are captured, stored or logged.
- No third-party product names in code, copy or docs (`AGENTS.md` §1.3).
- No automatic task moving between displays.
- Each phase ends green: `cargo clippy`, `cargo test`, `npx tsc --noEmit`, `npx vitest run`, and ships alone.
- SPEC-28 Phase 1 must be on the branch before Phase 2 (the setting lives in its Phone mirroring tabs).
- `wake_on_secure` is an additive optional field on `start_mirror` (serde default `true`); note it in the PR.

## Phase 1: detector

### Task 1.1: parser (TDD)
**Files:** create `src-tauri/src/secure_watch.rs`, `src-tauri/tests/fixtures/secure/*.txt`; modify `src-tauri/src/lib.rs` (mod).
- [ ] Record fixtures: Android 16 `dumpsys window` normal, secure (`fl=… SECURE …`), keyguard showing, Flex display focus.
- [ ] Failing tests for `parse_secure_probe` (valid, `D=-1`, malformed) and the fixture grep (`SECURE` only on the `fl=` line).
- [ ] Implement `SecureState`, `parse_secure_probe`, `SECURE_PROBE_SCRIPT` (spec §5.2). Run `cargo test secure_watch`; commit.

### Task 1.2: poller and backoff (TDD)
**Files:** `src-tauri/src/secure_watch.rs`, `src-tauri/src/commands.rs`.
- [ ] Failing tests for `next_interval(prev, round_ms, ok)`: doubles over 0.6 s or on failure, cap 6 s, reset after two rounds under 0.4 s.
- [ ] Implement the task: one round at a time, 2 s timeout, 1.5 s base, paused while the session window is minimized or hidden, immediate probe on restore, `secure-state` emitted on change only.
- [ ] Start and abort it in `spawn_session_inner` and on session removal. Skip camera sessions.
- [ ] Device: resolve Q1 (focus on the Flex display) and Q4; record in spec §13. Commit.

## Phase 2: Mirror wake

### Task 2.1: state machine (TDD)
**Files:** `src-tauri/src/secure_watch.rs`.
- [ ] Failing tests for every row of spec §6.2 and §7 (rising edge, 3 s debounce, user toggle, session end, overlap, setting off).
- [ ] Implement `decide(prev, next, &SessionWake) -> Vec<Action>` as a pure function, no I/O. Commit.

### Task 2.2: session record and actions
**Files:** `src-tauri/src/commands.rs`, `src/components/ControlBar.tsx`, `src/store/useAppStore.ts`.
- [ ] Add `screen_off`, `woke_by_us`, `wake_on_secure` to the session record; launch and `restart_with_screen_off` keep `screen_off` current.
- [ ] Implement `ScreenOn` (scrcpy power on; `KEYCODE_WAKEUP` fallback when not Awake or on error; never `KEYCODE_POWER`) and `ScreenOffRestore`.
- [ ] Emit `screen-power` so `setDeviceToggle(serial, "screenOff", …)` follows the backend.
- [ ] Notices via `emit_continuity_notice`; resolve Q2 and Q3. Commit.

### Task 2.3: `wakeOnSecure` setting
**Files:** `src/lib/modeSettings.ts` (+ test), `src/components/settings/**`, launch paths.
- [ ] Failing tests: default on in every mode, kept by the SPEC-28 migration, disabled state follows `turnScreenOff`.
- [ ] Add the field and the Switch under "Phone while mirroring" on the Mirror and High fidelity tabs (label and helper from spec §6.3); pass `wake_on_secure` to `start_mirror`. Commit.
- [ ] Device test steps 1-3 and 6 of spec §10.

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
