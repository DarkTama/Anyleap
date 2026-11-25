# SPEC-34 Phone Audio Resilience Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate wireless `scrcpy` audio stutter on weak Wi-Fi via a per-mode adaptive audio buffer (Phase 1) and add an optional Windows Bluetooth A2DP sink mode with double-audio prevention (Phase 2), while documenting why automatic Wi-Fi ↔ Bluetooth failover stays deferred (Phase 3).

**Architecture:** Phase 1 adds `audioBuffer` (`"auto" | 50 | 100 | 200`) to `CoreSettings` and `ModeDefaultsV2` (`SPEC-28`), resolved in Rust via `is_wifi_serial` (`50` ms USB, `150` ms Wi-Fi) and passed as `--audio-buffer=<ms>` in `build_scrcpy_args` and `build_audio_only_args`. Phase 2 adds a Windows WinRT `bt_audio` module wrapping `Windows.Media.Audio.AudioPlaybackConnection`, enforcing `scrcpy --no-audio` + `#93` in-place session restarts whenever Bluetooth audio is active, with single-home settings in **Settings → Audio, media & power**.

**Tech Stack:** Tauri 2, Rust (`windows` crate 0.61 WinRT features on Windows; pure state machine cross-platform), React 19 + TypeScript + Tailwind (`SPEC-26` tokens), Vitest, Cargo test.

**Spec:** `docs/specs/SPEC-34-audio-resilience.md`.

## Global Constraints
- Follow `AGENTS.md` §1.3: zero third-party or competitor product names in code, comments, UI copy, tests, or docs.
- Follow `SPEC-28` §8.5 & §9.1: positive sentence-case copy, single settings home per control (`Phone mirroring` for `Audio buffer`; `Audio, media & power` for `Play phone audio via Bluetooth` and paired phone selection).
- Follow `SPEC-26` contrast rules: `--text-muted` only on `--ink-900` / `--ink-950`, never on `--ink-800`.
- Mention **USB** as the most stable option in user-facing help copy, and clearly warn about ±150–250 ms Bluetooth latency in Phase 2.
- Every phase ends green on all 5 `AGENTS.md` §2 gates (`npm test`, `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings`, `cargo check --manifest-path src-tauri/Cargo.toml`, `npm run build`) and ships alone.

---

## Phase 1: Adaptive Audio Buffer

### Task 1.1: Backend `AudioBufferSetting` & `resolve_audio_buffer_ms` (TDD)
**Files:** modify `src-tauri/src/commands.rs`, `src-tauri/src/phone_hub/audio_only.rs`.
- [x] Write failing Rust unit tests in `src-tauri/src/commands.rs`:
  - `resolve_audio_buffer_ms`: `Auto` + USB (`"SER123"`, `"emulator-5554"`) → `50`; `Auto` + wireless (`"192.168.1.24:36431"`, `"[fe80::1]:5555"`) → `150`; explicit `50` / `100` / `200` override transport.
  - `CoreSettings` JSON deserialization defaults missing or unknown `audioBuffer` to `Auto`.
  - `build_scrcpy_args` and `build_app_window_args` emit `--audio-buffer=50` for USB `Auto`, `--audio-buffer=150` for Wi-Fi `Auto`, `--audio-buffer=100` / `200` for explicit settings, and **omit** `--audio-buffer=*` when `no_audio = true`.
  - `degrade_args` strips any `--audio-buffer=*` before appending `--no-audio`.
- [x] Write failing unit test in `src-tauri/src/phone_hub/audio_only.rs`:
  - `build_audio_only_args` emits `--audio-buffer=50` on USB and `--audio-buffer=150` on `IP:port`.
- [x] Implement `AudioBufferSetting`, `resolve_audio_buffer_ms`, `CoreSettings.audio_buffer` (`#[serde(default)]`), and update `build_scrcpy_args`, `degrade_args`, and `build_audio_only_args` (plus existing `CoreSettings` test literals).
- [x] Run `cargo test --manifest-path src-tauri/Cargo.toml`; verify all tests pass.

### Task 1.2: Frontend `modeDefaults` & `CoreSettings` Model (TDD)
**Files:** modify `src/lib/types.ts`, `src/lib/modeSettings.ts`, `src/lib/modeSettings.test.ts`, `src/lib/settingsStore.modeDefaults.test.ts`.
- [x] Write failing Vitest tests in `src/lib/modeSettings.test.ts`:
  - `defaultModeDefaults()` sets `audioBuffer: "auto"` on `mirror`, `flex`, `hifi`.
  - `normalizeModeDefaults` normalizes missing or invalid `audioBuffer` values to `"auto"`, and preserves `"auto" | 50 | 100 | 200`.
  - `resolveLaunchSettings` passes `audioBuffer` cleanly and never mutates other modes.
- [x] Add `AudioBufferSetting` (`"auto" | 50 | 100 | 200`) to `src/lib/types.ts` (`CoreSettings` + `DEFAULT_SETTINGS.audioBuffer = "auto"`) and `src/lib/modeSettings.ts` normalization.
- [x] Run `npx vitest run src/lib/modeSettings.test.ts src/lib/settingsStore.modeDefaults.test.ts`.

### Task 1.3: Settings UI in Phone Mirroring (`M-10` helper & `M-10b` Audio buffer) (TDD)
**Files:** modify `src/components/settings/registry.ts`, `src/components/settings/PhoneMirroringSection.tsx`, `src/components/settings/__tests__/PhoneMirroringSection.test.tsx`, `src/components/settings/__tests__/SettingsPanel.test.tsx`.
- [x] Update `M-10` helper in `registry.ts` and `PhoneMirroringSection.tsx` to mention USB as the most stable, lowest-latency option.
- [x] Add `M-10b` (`Audio buffer`) to `REGISTRY` (`section: "phone"`) with search keywords (`audio`, `buffer`, `stutter`, `lag`, `wifi`, `glitch`, `latency`, `usb`).
- [x] Render `SegmentedControl` (`Auto`, `50 ms`, `100 ms`, `200 ms`) in `DisplayModePanel` (`Audio & control` group):
  - Disable with `"Turn on Play phone audio on this PC to adjust the buffer."` when `s.noAudio` is `true`.
  - Show dynamic helper (`Auto uses 50 ms on USB and 150 ms on Wi-Fi…`, appending active transport info when a session of that mode is running).
  - Apply mid-session changes to running sessions of the same mode via the `#93` / `#108` / `#110` in-place restart path (`restart_with_audio_buffer`).
- [x] Add component + filter tests in `PhoneMirroringSection.test.tsx` and `SettingsPanel.test.tsx`.

### Task 1.4: Phase 1 Verification & Docs Sync
- [x] Run all 5 `AGENTS.md` §2 gates (`npm test`, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo check`, `npm run build`).
- [ ] Manual check on **home Wi-Fi**, **office Wi-Fi**, and **USB** (`docs/MANUAL-TEST-CHECKLIST.md`).
- [x] Sync `README.md` (`## Features`), `CONTEXT.md`, and `SPEC-34` status (`AGENTS.md` §1.4).

---

## Phase 2: Phone Audio via Bluetooth (`AudioPlaybackConnection`)

### Task 2.1: Pure Bluetooth State Machine & Double-Audio Invariants (TDD)
**Files:** create `src-tauri/src/bt_audio.rs`; modify `src-tauri/src/lib.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/phone_hub/audio_only.rs`.
- [ ] Write failing cross-platform Rust unit tests in `bt_audio.rs`:
  - `BtAudioStatus` transitions (`Disabled` → `Connecting` → `Connected` → `Disconnected` / `Error`), `OpenResultStatus` (`Success`, `RequestTimedOut`, `DeniedBySystem`, `UnknownFailure`) error mapping, and `is_supported_windows_build(build)` (`>= 19041` true, `< 19041` false).
  - `should_force_no_audio(bt_active, user_no_audio)` returns `true` when `bt_active` is true.
  - `can_start_audio_only` rejects starting `audio_only` while Bluetooth audio is active.
- [ ] Implement the pure state machine, error enum (`BtAudioError`: `UnsupportedWindowsVersion`, `NoAdapter`, `RadioOff`, `NotPaired`, `UnsupportedDevice`, `TimedOut`, `DeniedBySystem`, `Unknown(u32)`), and unit tests.

### Task 2.2: Windows WinRT `AudioPlaybackConnection` Integration & In-Place Session Restart
**Files:** modify `src-tauri/Cargo.toml`, `src-tauri/src/bt_audio.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/state.rs`, `src-tauri/src/lib.rs`.
- [ ] Add required `windows = "0.61"` WinRT features (`Media_Audio`, `Devices_Enumeration`, `Devices_Bluetooth`, `Devices_Radios`, `Foundation`, `Foundation_Collections`) under `[target.'cfg(windows)'.dependencies]`.
- [ ] Implement `#[cfg(windows)]` backend in `bt_audio.rs`:
  - Check OS build `>= 19041`, `BluetoothAdapter::GetDefaultAsync()`, `Radio::GetRadiosAsync()`.
  - `DeviceInformation::FindAllAsync` + `DeviceWatcher` using `AudioPlaybackConnection::GetDeviceSelector()`.
  - `TryCreateFromId`, `StateChanged` listener, `StartAsync`, `OpenAsync`, `Close` on stop and `RunEvent::Exit`.
- [ ] Expose Tauri commands: `get_bt_audio_capabilities`, `list_bt_audio_devices`, `start_bt_audio`, `stop_bt_audio`.
- [ ] Wire double-audio prevention:
  - `start_mirror`, `start_camera_mirror`, `launch_app_window` force `--no-audio` when Bluetooth audio is active.
  - Switching Bluetooth audio on/off mid-session triggers an in-place restart of active mirror sessions for that serial using `#93` (`mark_restarting` + `session-restarting` + `stop_child_gracefully` + `spawn_session_inner(..., false)`), and stops `phone_hub::audio_only`.
- [ ] Run `cargo check`, `cargo clippy -- -D warnings`, and `cargo test`.

### Task 2.3: Frontend State, Settings UI (`A-01`, `A-02`), Overview & Mirror Indicators (TDD)
**Files:** modify `src/lib/tauri.ts`, `src/lib/settingsStore.ts`, `src/store/useAppStore.ts`, `src/components/settings/registry.ts`, `src/components/settings/AudioMediaPowerSection.tsx`, `src/components/phone/OverviewView.tsx`, `src/components/EmbeddedMirrorWindow.tsx`, plus tests.
- [ ] Persist `{ enabled: boolean, deviceId: string | null }` under `btAudio` in `settings.json` (`settingsStore.ts`).
- [ ] Add `A-01` (`Play phone audio via Bluetooth`) and `A-02` (`Paired phone (Bluetooth)`) to `REGISTRY` (`section: "audio"`).
- [ ] Render the **Phone audio via Bluetooth** group in `AudioMediaPowerSection.tsx`:
  - Explicit ±150–250 ms latency notice and USB stability note.
  - Paired device `<select>`, **Refresh** button, and **Open Windows Bluetooth settings** link (`ms-settings:bluetooth`).
  - Live `role="status"` / `role="alert"` banners for all error states in SPEC-34 §7 and mid-session disconnects with **Reconnect Bluetooth** / **Switch to Wi-Fi audio** actions.
- [ ] Update `OverviewView.tsx` (status chip + quick route indicator beside `Audio only`) and `EmbeddedMirrorWindow.tsx` (non-blocking banner when mirror audio is muted for Bluetooth).
- [ ] Write Vitest unit & component tests for all error states, accessibility labels, and double-audio toggle restarts (with mocked Tauri invoke).

### Task 2.4: Phase 2 Verification & Docs Sync
- [ ] Run all 5 `AGENTS.md` §2 gates (`npm test`, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo check`, `npm run build`).
- [ ] Manual check on Windows PC + paired Android phone (verify no double audio, ±150–250 ms lag warning accuracy, `#93` in-place restart on mode switch, error states).
- [ ] Sync `README.md`, `CONTEXT.md`, `docs/PLAN.md` / `docs/BACKLOG.md`, and mark `SPEC-34` Implemented (`AGENTS.md` §1.4).

---

## Phase 3: Automatic Failover (Deferred)
- **Not scheduled for implementation.** See `docs/specs/SPEC-34-audio-resilience.md` §9 for the technical rationale (separate pipelines, zero sample access on `AudioPlaybackConnection`, 1–2 s switch blackout + latency jump, impossibility of bonding without a custom Android app, and rejection of artificial Wi-Fi delay matching).
