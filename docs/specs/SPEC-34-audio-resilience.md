# SPEC-34: Phone Audio Resilience (Adaptive Audio Buffer & Bluetooth Audio Mode)

> **Status:** Draft / Proposed (2026-11-25)
> **Scope:** `scrcpy` audio argument building (`src-tauri/src/commands.rs`, `src-tauri/src/phone_hub/audio_only.rs`), transport detection (`src-tauri/src/device_identity.rs`), Windows Bluetooth audio sink module (Phase 2), per-mode & audio settings (`src/lib/modeSettings.ts`, `src/lib/types.ts`, `src/components/settings/`), and Phone Hub / Mirror status surfaces (`src/components/phone/OverviewView.tsx`, `src/components/EmbeddedMirrorWindow.tsx`).
> **Related:** SPEC-27 (`Phone Hub` audio-only), SPEC-28 (`modeDefaults`, single-home Settings IA), SPEC-30 (per-mode `Auto` + manual override UX pattern), `#93` / `d9b67b8` (in-place mirror session restart).
> **Plan:** `docs/superpowers/plans/2026-11-25-spec34-audio-resilience.md`

---

## 1. Problem (user report)

Phone audio forwarded by `scrcpy` over wireless ADB (`host:port`) stutters and glitches on weak or congested office Wi-Fi, while playing cleanly on a home network or over USB.

Root causes in the current codebase:
1. **`scrcpy` runs with its fixed 50 ms default audio buffer regardless of transport.** `build_scrcpy_args` (`src-tauri/src/commands.rs:353-417`), `build_scrcpy_camera_args` (`src-tauri/src/commands.rs:419-477`), and `build_audio_only_args` (`src-tauri/src/phone_hub/audio_only.rs:38-45`) never pass `--audio-buffer`, leaving `scrcpy 5.0.1` at its `50` ms default. On shared office Wi-Fi, normal 802.11 contention, Android Wi-Fi power-save DTIM intervals (~100 ms), and TCP retransmits easily exceed a 50 ms jitter budget and starve the audio decoder.
2. **No fallback audio transport when Wi-Fi jitter is extreme.** Some office networks drop or stall TCP bursts long enough that even a 200 ms ADB buffer cannot fully hide stalls, yet the user has no built-in way to route phone audio directly to Windows over Bluetooth A2DP without creating double audio.

---

## 2. Goals and non-goals

### Goals
- **G1 (Phase 1 — Adaptive audio buffer):** Add a per-mode **Audio buffer** setting (`Auto` / `50` / `100` / `200` ms) following SPEC-28 per-mode defaults. `Auto` chooses a low-latency buffer on USB (`50` ms) and a larger jitter-tolerant buffer on wireless (`150` ms), trading a modest amount of latency for glitch-free playback.
- **G2 (Phase 2 — Bluetooth audio mode):** Add a manual **"Phone audio via Bluetooth"** mode on Windows 10 2004+ (`10.0.19041.0`) using `Windows.Media.Audio.AudioPlaybackConnection` (requiring **no** Android companion app). Users pair the phone once in Windows Settings, pick it in AnyLeap, and stream audio directly to Windows while `scrcpy` runs with `--no-audio` to prevent double audio.
- **G3 (Clear trade-offs & USB guidance):** Surface clear latency expectations (±150–250 ms Bluetooth lag behind mirrored video) and explicitly recommend **USB** as the most stable, lowest-latency connection in user-facing help copy.
- **G4 (SPEC-28 compliance & clean error handling):** Follow the SPEC-28 single-home editor rule, positive sentence-case copy, zero third-party product names (`AGENTS.md` §1.3), and actionable recovery for every Windows/Bluetooth error state.
- **G5 (Phase 3 documented & deferred):** Record the engineering analysis explaining why automatic Wi-Fi → Bluetooth failover, dual-link audio bonding, and artificial Wi-Fi delay matching are not feasible or are rejected.

### Non-goals
- Forking or modifying the bundled `scrcpy` binary or `scrcpy-server`.
- Building an Android companion app for audio capture, buffering, or transport bonding.
- Automatic or "seamless" Wi-Fi ↔ Bluetooth failover (documented as deferred in §9).
- Per-phone `audioBuffer` profiles in Phase 1 (`audioBuffer` is per mirroring mode, matching SPEC-28 §3 and SPEC-30 §2; Bluetooth paired device selection in Phase 2 is PC-scoped / matched to paired Windows devices).
- Changing `--audio-output-buffer` (SDL's local drain buffer, default `10` ms), `--audio-codec`, or `--audio-bit-rate`.
- Non-Windows Bluetooth A2DP sink implementation (`AudioPlaybackConnection` is Windows-only; macOS/Linux keep Phase 1 adaptive buffer and hide/disable the Windows-only Bluetooth sink controls).
- **No ADR:** Phase 1 adds an optional field to `CoreSettings`/`ModeDefaultsV2` with safe default `"auto"`, and Phase 2 wraps a self-contained Windows WinRT API behind a toggle. Both are localized and easily reversible without wire-protocol or encrypted-store changes (`AGENTS.md` §3).

---

## 3. Research Findings (Verified Before Design)

### 3.1 Current `scrcpy` audio configuration in AnyLeap
| Path | Behaviour today | Source |
|---|---|---|
| `CoreSettings` | Only carries `no_audio: bool`; no buffer, codec, bitrate, or source fields. | `src-tauri/src/commands.rs:119-139`, `src/lib/types.ts:63-81` |
| `build_scrcpy_args(serial, s)` | Checks `if s.no_audio { a.push("--no-audio".into()); }`. Passes no `--audio-codec`, `--audio-bit-rate`, `--audio-buffer`, `--audio-output-buffer`, or `--audio-source` (relying on `scrcpy` defaults). | `src-tauri/src/commands.rs:353-417` (`379-381`) |
| `build_scrcpy_camera_args(serial, s)` | Checks `if s.no_audio { a.push("--no-audio".into()); }`. Passes no audio buffer/codec/bitrate/source flags. | `src-tauri/src/commands.rs:419-477` (`467-469`) |
| `build_app_window_args` | Calls `build_scrcpy_args(serial, settings)` directly, inheriting its audio flags. | `src-tauri/src/commands.rs:4074-4096` (`4080`) |
| `build_audio_only_args` | Passes only `--serial`, `--no-video`, `--no-window`. Refuses to start if a mirror session without `--no-audio` / `--no-audio-playback` is active (`can_start_audio_only`); mirror/app launches stop active `audio_only` via `stop_for_serial`. | `src-tauri/src/phone_hub/audio_only.rs:28-61, 38-45`, `src-tauri/src/commands.rs:915-920, 4137-4142` |
| `degrade_args(args)` | One-shot retry on early (`< 3000` ms) encoder/connection failure (`stderr_suggests_encoder_failure`). Strips `--max-fps`, `--video-codec`, `--camera-size`, `--camera-fps`, and `--no-audio`, then forces `--video-codec=h264` and appends `--no-audio`. Preserves other flags. | `src-tauri/src/commands.rs:700-719` (`708, 717`), `1160-1187` |
| `#93` in-place restart | `restart_flex_session_in_place` and `start_recording`/`stop_recording` call `state.mark_restarting(&id)`, emit `session-restarting`, call `stop_child_gracefully`, remove old embedded HWND, and call `spawn_session_inner(..., destroy_mirror_window = false)`. `spawn_session_inner`'s exit listener consumes `take_restarting(&id)` and emits `session-replaced` without closing `EmbeddedMirrorWindow`, which re-embeds the new `SDL_app` child. | `src-tauri/src/commands.rs:930-945, 1048-1060, 4303-4457`; `src-tauri/src/state.rs:53-55, 74-87`; `src/components/EmbeddedMirrorWindow.tsx:102-130` |

### 3.2 Connection type detection (USB vs wireless)
- `is_wifi_serial(serial: &str) -> bool` (`src-tauri/src/device_identity.rs:106-108`) calls `split_wifi_serial(serial)` (`device_identity.rs:110-117`), returning `true` when `serial` splits on the last `:` into a non-empty host and a valid `u16` port (`IP:port`).
- `parse_adb_devices` (`device_identity.rs:210-217`) assigns `DeviceTransports { usb, wifi }` using `is_wifi_serial(&serial)`. When both transports are live for the same `hw_serial`, `merge_devices` sets `DeviceInfo.serial` to the USB serial (`device_identity.rs:20`, `CONTEXT.md:11`).
- `get_device_details` (`src-tauri/src/commands.rs:3308-3326`) reports `connection: "wifi" | "usb"` (`src/lib/types.ts:204`), matching `DeviceHeroCard.tsx:142-148` and `DeviceRack.tsx:286, 294`.
- **Conclusion:** Checking `is_wifi_serial(serial)` inside Rust arg-building deterministically identifies the exact transport (`USB` vs `Wi-Fi`) used by the `scrcpy --serial <serial>` subprocess.

### 3.3 `scrcpy 5.0.1` audio options (`--help`)
Verified directly from `src-tauri/binaries/scrcpy-x86_64-pc-windows-msvc.exe --help`:
- `--audio-buffer=ms`: "Configure the audio buffering delay (in milliseconds). Lower values decrease the latency, but increase the likelihood of buffer underrun (causing audio glitches). Default is 50."
- `--audio-output-buffer=ms`: "Configure the size of the SDL audio output buffer (in milliseconds). Do not change this setting unless you have a good reason. Default is 10."
- `--audio-codec=name`: `opus` (default), `aac`, `flac`, `raw`.
- `--audio-bit-rate=value`: default `128K` (`128000` bps).
- `--audio-source=source`: `output` (default; forwards whole audio output and disables playback on device), `playback` (captures audio playback; apps can opt out), `mic`, `mic-*`, `voice-call*`, `voice-performance`.
- `--no-audio` / `--no-audio-playback` / `--require-audio`.
- `-V, --verbosity=value`: `verbose`, `debug`, `info` (default), `warn`, `error`.

**Underrun signal investigation:** In `scrcpy` (`audio_player.c` / `audio_regulator.c`), default `-Vinfo` logging does **not** emit audio buffer underrun or stutter warnings; `audio_player.c` only logs `LOGW("Audio stream error: %s", SDL_GetError())` on SDL stream failure, while buffer regulator telemetry and underflow/silence insertion logs require `-Vverbose` or non-release debug builds (`SC_AUDIO_PLAYER_NDEBUG`). Therefore, Phase 1 **does not** attempt fragile stderr-scraping for automatic mid-session buffer escalation; buffer sizing stays static per connection type (`Auto`) or user override (`50` / `100` / `200` ms).

### 3.4 Windows `Windows.Media.Audio.AudioPlaybackConnection` (Microsoft Docs)
From official Microsoft Learn documentation (*AudioPlaybackConnection Class* and *Enable audio playback from remote Bluetooth-connected devices*):
- **Minimum Windows version:** Windows 10, version 2004 (OS build `10.0.19041.0`), `Windows.Foundation.UniversalApiContract` v10.0 (`655360`).
- **Pairing prerequisite:** The remote device (phone) must first be **paired** with the Windows PC through Windows Settings (Bluetooth & devices) with Bluetooth A2DP profile support before the OS exposes it to `AudioPlaybackConnection`.
- **Device discovery:** `AudioPlaybackConnection::GetDeviceSelector()` returns an Advanced Query Syntax (AQS) selector string passed to `Windows.Devices.Enumeration.DeviceInformation::CreateWatcher` (or `FindAllAsync`) to list paired remote Bluetooth audio-source devices.
- **Connection lifecycle:**
  1. `AudioPlaybackConnection::TryCreateFromId(device_id)` instantiates a connection, or returns `null` if the device does not support remote audio streaming.
  2. Subscribe to `StateChanged` (checks `State` ∈ `{ Closed = 0, Opened = 1 }`).
  3. Call `Start()` / `StartAsync()` to configure the system to accept incoming audio connections from that device (**does not** start audio playback yet).
  4. Call `Open()` / `OpenAsync()` to actively open the connection. Returns `AudioPlaybackConnectionOpenResult` with `Status` ∈ `{ Success (0), RequestTimedOut (1), DeniedBySystem (2), UnknownFailure (3) }` and `ExtendedError`.
  5. Call `Close()` / `Dispose()` to tear down the link.
- **Audio routing (no sample access):** Once opened, Windows' Bluetooth A2DP Sink stack plays the remote device's audio directly through the local Windows playback endpoint. The application has **no access to PCM samples**, `AudioGraph` input nodes, or jitter-buffer state on this stream.
- **Bluetooth adapter/radio detection:** `BluetoothAdapter::GetDefaultAsync()` (Windows 10 v1703, build `10.0.15063.0`+) and `Radio::GetRadiosAsync()` (Windows 10 build `10.0.10240.0`+) allow distinguishing missing hardware vs radio off.

### 3.5 Why running both scrcpy audio and Bluetooth audio causes double audio
When `AudioPlaybackConnection` is opened, Windows advertises a Bluetooth A2DP sink and Android routes media audio to that Bluetooth endpoint. Meanwhile, if `scrcpy` launches without `--no-audio`, its default `--audio-source=output` (or `playback`) captures audio on the device and streams a second copy over ADB to `scrcpy`'s SDL audio player on the PC. Because ADB/Opus (~50–150 ms) and Bluetooth A2DP (~150–250 ms) have different latencies, the user hears **double audio with a distracting echo** (or Android's `output` capture policy conflicts with Bluetooth routing).
**Invariant:** Whenever Bluetooth audio mode is active for a device, `scrcpy` **must** run with `--no-audio` (and any `phone_hub::audio_only` session for that serial must be stopped). Switching Bluetooth audio mode on or off while a mirror session is active must restart the mirror session in place (`#93` pattern) with updated `--no-audio` / `--audio-buffer` flags.

---

## 4. Phase 1: Adaptive Audio Buffer

### 4.1 Settings model (`modeDefaults`)
Add `audioBuffer` to `CoreSettings` (`src/lib/types.ts` and `src-tauri/src/commands.rs`) so it lives per mode inside `ModeSettings` (`mirror`, `flex`, `hifi`) under `modeDefaults` (`SPEC-28` §5.2):

```ts
export type AudioBufferSetting = "auto" | 50 | 100 | 200;

export interface CoreSettings {
  // ...existing 17 fields...
  audioBuffer?: AudioBufferSetting; // default "auto"
}
```

- **Rust `CoreSettings` (`src-tauri/src/commands.rs`):**
  - Add `#[serde(default)] pub audio_buffer: AudioBufferSetting` where `AudioBufferSetting` deserializes `"auto"` (default) or `50 | 100 | 200` (with unknown/missing values safely falling back to `Auto`).
  - Existing `settings.json` profiles and Rust unit-test `CoreSettings` literals gain `Default::default()` (`Auto`), requiring zero breaking migration.
- **Camera mode (`CameraModeSettings`):** Keeps `scrcpy` default (`50` ms) in Phase 1; `CameraPanel` captures microphone audio rather than media playback and is unaffected by the `CoreSettings` display-mode tabs.
- **Headless `Audio only` (`phone_hub::audio_only`):** Uses `Auto` transport-based resolution (`50` ms USB / `150` ms Wi-Fi) by default, or respects the active mode's `audioBuffer` if passed/configured later, so headless wireless listening benefits from the 150 ms Wi-Fi buffer immediately.

### 4.2 Buffer resolution & justification (`resolve_audio_buffer_ms`)

```rust
pub fn resolve_audio_buffer_ms(serial: &str, setting: AudioBufferSetting) -> u32 {
    match setting {
        AudioBufferSetting::Ms(ms) if matches!(ms, 50 | 100 | 200) => ms,
        AudioBufferSetting::Auto | _ => {
            if crate::device_identity::is_wifi_serial(serial) {
                150
            } else {
                50
            }
        }
    }
}
```

| Connection | `Auto` value | Justification |
|---|---|---|
| **USB** (`!is_wifi_serial`) | **`50` ms** | USB bulk transfers have sub-millisecond jitter and negligible packet loss. Matches `scrcpy`'s `50` ms default for snappy A/V sync in games and video. |
| **Wireless** (`is_wifi_serial`) | **`150` ms** | 3× the default buffer. Absorbs typical Android Wi-Fi power-save DTIM wake intervals (~100 ms) and moderate office Wi-Fi TCP retransmit spikes while keeping audio ~50–100 ms faster than Bluetooth A2DP. |
| **Manual overrides** | **`50` / `100` / `200` ms** | `50` ms for low-latency wired/clean Wi-Fi gaming; `100` ms for good home Wi-Fi with small safety margin; `200` ms maximum cushion for congested office Wi-Fi. |

**Argument emission in `build_scrcpy_args` (`src-tauri/src/commands.rs`):**
- When `!s.no_audio`, always emit `format!("--audio-buffer={}", resolve_audio_buffer_ms(serial, s.audio_buffer))` (explicitly emitting `--audio-buffer=50` on USB just as `--render-fit=letterbox` is always emitted explicitly in `commands.rs:408-410`).
- When `s.no_audio` is `true` (muted or Bluetooth audio mode active), emit `--no-audio` and omit `--audio-buffer=...`.
- Never alter `--audio-output-buffer` (remains `scrcpy`'s `10` ms default).
- In `degrade_args` (`commands.rs:700-719`), strip any `--audio-buffer=*` alongside `--no-audio` before appending `--no-audio`, keeping degraded retry args clean.
- `build_app_window_args` (`commands.rs:4074-4096`) calls `build_scrcpy_args`, so per-app windows automatically inherit the resolved `--audio-buffer`!

### 4.3 Mid-session changes & `#93` in-place restart
- Changing `Audio buffer` in **Settings → Phone mirroring** saves `modeDefaults[mode]` immediately (affecting the next launch).
- If a mirror session of that mode is currently running (and audio is enabled), AnyLeap offers a one-click **"Apply & restart mirror"** inline action (or restarts in place using the `#93` `mark_restarting` + `session-restarting` + `session-replaced` path so the embedded mirror window stays open). Re-launching on next session works automatically without extra prompts.

---

## 5. Phase 2: Phone Audio via Bluetooth (`AudioPlaybackConnection`)

### 5.1 Architecture (`src-tauri/src/bt_audio.rs`)
Windows-only implementation (`#[cfg(windows)]`) using the `windows` crate WinRT bindings (`Windows::Media::Audio::AudioPlaybackConnection`, `Windows::Devices::Enumeration::DeviceInformation`, `Windows::Devices::Bluetooth::BluetoothAdapter`, `Windows::Devices::Radios::Radio`), paired with a pure, platform-independent state machine (`BtAudioState` / `BtConnectionStatus`) so transition logic is unit-tested on all CI targets.

**Capabilities & Prerequisites:**
- Requires Windows 10 version 2004 (`10.0.19041.0`) or later (`os_supports_audio_playback_connection()` checks Windows build `>= 19041`).
- Requires the phone to be paired in Windows Settings once (Bluetooth A2DP source). No Android app is installed or required.

**Discovery & Device Matching:**
- Backend command `list_bt_audio_devices()` queries `DeviceInformation::FindAllAsyncAqsFilter(&AudioPlaybackConnection::GetDeviceSelector()?)` and subscribes a `DeviceWatcher` to emit `bt-audio-devices-changed` events when paired devices appear/disappear.
- Each returned `BtAudioDevice` contains `{ id: String, name: String, isDefaultMatch: bool }`. When the user has an active Android device (`ro.product.model` or nickname), the UI highlights a matching Bluetooth device name if found, while letting the user pick any paired phone explicitly.

**Connection Lifecycle (`BtAudioManager` in `AppState`):**
1. **Enable (`start_bt_audio(device_id, active_serial?)`):**
   - Verify OS build `>= 19041`, Bluetooth adapter exists (`BluetoothAdapter::GetDefaultAsync()`), and Bluetooth radio is `On` (`Radio::GetRadiosAsync()`).
   - Call `AudioPlaybackConnection::TryCreateFromId(&device_id)` (returns `Err` with "not supported" message if `null`).
   - Register `StateChanged` handler (emits `bt-audio-state-changed` to frontend; if state drops to `Closed` unexpectedly while mode is active, transitions status to `Disconnected` with recovery notice).
   - Call `conn.StartAsync().await` then `conn.OpenAsync().await`.
   - Inspect `AudioPlaybackConnectionOpenResult::Status()`:
     - `Success` → store active `AudioPlaybackConnection`, mark `bt_audio_enabled = true`.
     - `RequestTimedOut` / `DeniedBySystem` / `UnknownFailure` → map to explicit error state (§7), drop connection, **do not** silence scrcpy audio on failure.
   - **Double-audio prevention:** Once `OpenAsync` succeeds, if `active_serial` is provided (or any mirror/audio-only session matches):
     - Stop `phone_hub::audio_only::stop_for_serial(app, serial)`.
     - If an active mirror session for `serial` does not have `--no-audio`, restart it in place (`#93` pattern: `mark_restarting` → `session-restarting` → `stop_child_gracefully` → `spawn_session_inner(..., destroy_mirror_window = false)`) with `--no-audio` injected and `--audio-buffer=*` stripped.
2. **Mirror launch while Bluetooth audio is active (`start_mirror`, `launch_app_window`, `phone_audio_start`):**
   - `start_mirror` / `launch_app_window` check `BtAudioManager`: if Bluetooth audio is currently `Opened` (or enabled for this session), force `--no-audio` in the spawned `scrcpy` args (overriding `noAudio: false` without mutating the user's saved `modeDefaults`).
   - `phone_audio_start` (`audio_only`) refuses to start while Bluetooth audio is active (or prompts to switch back to Wi-Fi audio), preventing double audio.
3. **Disable (`stop_bt_audio(active_serial?)`) & App Exit:**
   - Call `conn.Close()` and drop the `AudioPlaybackConnection` so Windows stops acting as an A2DP sink and phone audio returns to the phone's speakers/headset.
   - If a mirror session is currently running with `--no-audio` solely due to Bluetooth mode (and the mode's `noAudio` setting is `false`), restart it in place without `--no-audio` (restoring the resolved `--audio-buffer=<ms>`).
   - App exit (`RunEvent::Exit`) closes any open `AudioPlaybackConnection` cleanly.

### 5.2 Latency & Media Trade-off Notice
Bluetooth A2DP adds roughly **±150–250 ms** of wireless audio latency (on top of codec/OS buffering), meaning audio will noticeably lag behind mirrored video. The UI must clearly state that Bluetooth mode is **best for music, podcasts, and voice**, and **not recommended for video or games** where lip-sync or timing matters.

---

## 6. Settings Placement & UX Copy (SPEC-28 Single Home)

Following SPEC-28 §8.5 and §9.1: every persistent setting has **one editor**; other surfaces either act as a live session control or show read-only status with a link.

### 6.1 Phase 1 — Settings → Phone mirroring (`PhoneMirroringSection.tsx`)
Inside the **`Audio & control`** group (per mode: `Mirror`, `Flex`, `High fidelity`), right below `M-10` (**Play phone audio on this PC**):

| ID | Control | Label | Helper / Copy | Disabled state |
|---|---|---|---|---|
| **M-10** (existing, helper updated) | Switch | `Play phone audio on this PC` | `Off: sound stays on the phone. For the most stable, lowest-latency audio and video, use USB.` | — |
| **M-10b** (new) | Segmented control (`Auto` / `50 ms` / `100 ms` / `200 ms`) | `Audio buffer` | When `Auto`: `Auto uses 50 ms on USB and 150 ms on Wi-Fi. Higher values reduce stutter on busy Wi-Fi with a little more delay.` (When a session of this mode is running on USB/Wi-Fi, appends `Currently 150 ms on Wi-Fi.` or `Currently 50 ms on USB.`)<br>When manual (`50` / `100` / `200`): `Fixed buffer for this mode. Raise to 200 ms if Wi-Fi audio stutters, or use USB for the most stable audio.` | Disabled with `"Turn on Play phone audio on this PC to adjust the buffer."` when `noAudio` is `true` (or Bluetooth audio is active). |

Registry entry (`src/components/settings/registry.ts`):
- `e("M-10b", "phone", "Audio buffer", "Auto uses 50 ms on USB and 150 ms on Wi-Fi. Higher values reduce stutter on busy Wi-Fi with a little more delay.", ["audio", "buffer", "stutter", "lag", "wifi", "glitch", "latency", "usb"])`

### 6.2 Phase 2 — Settings → Audio, media & power (`AudioMediaPowerSection.tsx`)
Add a **"Phone audio via Bluetooth"** group in **Settings → Audio, media & power** (the single persistent home for PC audio routing alongside `T-06..T-09`):

| ID | Control | Label | Helper / Copy |
|---|---|---|---|
| **A-01** | Switch (`role="switch"`) | `Play phone audio via Bluetooth` | `Routes phone sound through Windows Bluetooth instead of Wi-Fi. Expect ±150–250 ms delay behind mirrored video—best for music and podcasts, not video or games. USB remains the most stable option.` |
| **A-02** | Select + Refresh button + Windows link | `Paired phone (Bluetooth)` | Helper: `Pair your phone once in Windows Bluetooth settings, then choose it here.` Action link: `Open Windows Bluetooth settings` (`ms-settings:bluetooth`). |
| — | Inline status / warning banner (`role="status"` / `role="alert"`) | Live connection state | Shows `Connected via Bluetooth (mirror audio muted to prevent echo)`, `Connecting…`, or exact error state (§7). |

### 6.3 Phone Overview (`OverviewView.tsx`) & Mirror Window (`EmbeddedMirrorWindow.tsx`)
- **Phone → Overview:** Beside the **Audio only** quick action, show a compact audio-route indicator when Bluetooth audio is active (`Bluetooth audio active · Change in Settings`) or allow a quick **Use Bluetooth audio** / **Switch back to Wi-Fi audio** action only if a paired Bluetooth device is already configured in Settings (otherwise links to Settings → Audio, media & power).
- **Mirror window:** When Bluetooth audio is active during a mirror session, show a subtle non-blocking status pill/notice on mode switch (`"Bluetooth audio active (±150–250 ms lag). Mirror audio is muted to prevent double audio."`).

---

## 7. Error States (Phase 2)

| Condition | Detection | User-facing message & recovery |
|---|---|---|
| **Windows too old** | Windows build `< 19041` (pre-2004) | `"Phone audio via Bluetooth requires Windows 10 version 2004 or newer. Use USB for the most stable audio, or raise Audio buffer to 200 ms in Phone mirroring settings."` (Switch disabled.) |
| **No Bluetooth adapter** | `BluetoothAdapter::GetDefaultAsync()` returns `None` and no Bluetooth radio in `Radio::GetRadiosAsync()` | `"No Bluetooth adapter found on this PC. Connect via USB or raise Audio buffer to 200 ms over Wi-Fi."` (Switch disabled.) |
| **Bluetooth radio off** | Bluetooth `RadioState` is `Off` or `Disabled` | `"Bluetooth is turned off on this PC. Turn it on in Windows Settings, then click Refresh."` + button `Open Windows Bluetooth settings`. |
| **No paired audio-source phone** | `FindAllAsync(GetDeviceSelector())` returns empty | `"No paired phone audio devices found. Pair your phone once in Windows Bluetooth settings, then click Refresh."` + `Refresh` & `Open Windows Bluetooth settings`. |
| **Unsupported device (`TryCreateFromId == null`)** | `TryCreateFromId` returns `null` | `"This paired device does not support Bluetooth audio streaming to Windows. Remove and re-pair the phone in Windows Bluetooth settings."` |
| **`OpenAsync` timed out** | `Status::RequestTimedOut` | `"Could not reach the phone over Bluetooth. Make sure Bluetooth is on, the phone is nearby, and it isn't playing to another headset, then try again."` + `Retry`. |
| **`OpenAsync` denied by system** | `Status::DeniedBySystem` | `"Windows blocked the Bluetooth audio connection. Close any other app using Bluetooth audio from this phone and retry."` |
| **`OpenAsync` unknown failure** | `Status::UnknownFailure` | `"Bluetooth audio connection failed (error 0x{code:08X}). Retry, or switch to USB / 200 ms Wi-Fi audio."` |
| **Dropped mid-session** | `StateChanged` → `Closed` while mode enabled | Non-blocking `role="alert"` banner: `"Bluetooth audio disconnected. Phone sound may play on the phone speaker."` with buttons **Reconnect Bluetooth** and **Switch to Wi-Fi audio**. |
| **Non-Windows OS** | `#[cfg(not(windows))]` | Bluetooth audio controls hidden/omitted; `Audio buffer` (`Auto / 50 / 100 / 200 ms`) works identically on all platforms. |

---

## 8. Accessibility

Following SPEC-26 and SPEC-28 §10:
- **Labelling:** `Audio buffer` uses `SegmentedControl` with visible `<label>` / `aria-labelledby` and `aria-describedby` pointing to the dynamic helper text (`50 ms on USB / 150 ms on Wi-Fi`). `Play phone audio via Bluetooth` uses `SwitchRow` (`role="switch"`, `aria-checked`, Space/Enter toggle). `Paired phone (Bluetooth)` `<select>` binds an explicit `<label htmlFor>`.
- **Live announcements:** Bluetooth connection state transitions and errors render inside `role="status"` (`aria-live="polite"`) for non-fatal status and `role="alert"` for failures/disconnects.
- **Keyboard & touch targets:** All segmented buttons, switches, Refresh, Retry, and Windows Settings links maintain `>= 32 px` height (`min-h-8`) and use SPEC-26 gold 2 px `:focus-visible` outlines.
- **Contrast:** Muted helper and latency-warning text render only on `--ink-900` or `--ink-950` (`>= 5.15:1`), never on `--ink-800` (where `--text` `#C9CBD6` is used). Active segments use `--ink-950` on `--gold-400` (`9.30:1`).

---

## 9. Phase 3 (Deferred, Documented Only): Automatic Wi-Fi → Bluetooth Failover

Automatic failover (monitoring Wi-Fi audio quality and automatically switching the active stream to Bluetooth when Wi-Fi degrades, then switching back when Wi-Fi recovers) is **explicitly deferred and not scheduled for implementation**.

### 9.1 Why automatic failover cannot be seamless
1. **Two completely separate OS/media pipelines:**
   - **Wi-Fi path:** Android `AudioRecord` / `AudioPlaybackCapture` → MediaCodec Opus encoder → `scrcpy-server` → ADB/TCP forward socket → `scrcpy` FFmpeg decoder → `sc_audio_regulator` → SDL audio output.
   - **Bluetooth path:** Android `AudioPolicy` → Bluetooth A2DP encoder (SBC/AAC/aptX) → Bluetooth RF → Windows Bluetooth stack (`AudioPlaybackConnection`) → Windows audio mixer/endpoint.
2. **Zero sample access on the Bluetooth path:** `Windows.Media.Audio.AudioPlaybackConnection` plays the A2DP stream directly inside the Windows audio subsystem. AnyLeap does not receive PCM samples, packet timestamps, or buffer-fill telemetry from `AudioPlaybackConnection`, so it cannot pre-buffer, time-align, or cross-fade between the ADB and Bluetooth streams.
3. **Unavoidable 1–2 second blackout on switch:** Opening an `AudioPlaybackConnection` (`StartAsync` + `OpenAsync`), completing Bluetooth A2DP link negotiation, letting Android reroute active playback streams to A2DP, and restarting `scrcpy` with `--no-audio` causes a 1–2 s audio gap and an audible pop/skip.
4. **Abrupt latency jump:** Mid-playback switching jumps audio latency from ~50–150 ms (scrcpy) to ~150–250 ms (Bluetooth A2DP), producing an obvious time skip and breaking A/V lip-sync mid-video.

### 9.2 Why combining or bonding both links for one stream is impossible
Packet-level bonding / striping (sending redundant or split audio frames over Wi-Fi + Bluetooth and reconstructing one continuous stream on the PC) requires a shared framing protocol, shared codec, shared sequence numbers, and a unified jitter buffer/decoder in userspace. Because AnyLeap cannot read raw A2DP packets or PCM frames from `AudioPlaybackConnection`—and Android routes media output to a single sink unless running a custom capture app—bonding ADB and Bluetooth A2DP into one stream is not possible without a custom Android audio app (explicitly out of scope).

### 9.3 Alternative considered and rejected: delaying the Wi-Fi path to match Bluetooth
- **Alternative:** Force `--audio-buffer=200` (or buffer video + audio by ~200 ms) on all Wi-Fi sessions so ADB audio latency roughly matches Bluetooth A2DP (~200 ms), avoiding a latency jump on failover.
- **Why rejected:**
  1. It permanently degrades **every** normal home/office Wi-Fi mirroring session with ~200 ms of sluggish audio (and video if A/V synced) just to prepare for a rare failover.
  2. It still fails to make switching seamless: the 1–2 s `OpenAsync` / A2DP negotiation blackout and Android stream-reroute pause still occur, and independent clocks/decoders still prevent phase-aligned splicing without PCM access.

---

## 10. Test Plan

### 10.1 Unit tests (automated, no phone/adb required)
**Rust (`cargo test --manifest-path src-tauri/Cargo.toml`):**
- `resolve_audio_buffer_ms`:
  - `Auto` + USB serial (`"ABC123XYZ"`, `"emulator-5554"`) → `50` ms.
  - `Auto` + wireless serial (`"192.168.1.24:36431"`, `"[fe80::1]:5555"`) → `150` ms.
  - Explicit `Ms(50)`, `Ms(100)`, `Ms(200)` override both USB and Wi-Fi serials.
  - `CoreSettings` JSON deserialization defaults missing `audioBuffer` to `Auto`.
- `build_scrcpy_args` & `build_app_window_args`:
  - `no_audio = false` emits `--audio-buffer=50` on USB and `--audio-buffer=150` on `IP:port` when `Auto`.
  - `no_audio = false` with explicit `100` / `200` emits `--audio-buffer=100` / `--audio-buffer=200`.
  - `no_audio = true` (or Bluetooth audio active) emits `--no-audio` and **omits** `--audio-buffer=*`.
- `degrade_args`: strips any `--audio-buffer=*` and emits `--no-audio`.
- `build_audio_only_args`: emits `--audio-buffer=50` on USB and `--audio-buffer=150` on `IP:port`.
- (Phase 2) Pure `BtAudioState` transition tests: disconnected → connecting → connected → dropped → reconnecting; `OpenResultStatus` error mapping; Windows build `< 19041` rejection; double-audio guard refusing `audio_only` while BT audio is open.

**Frontend (Vitest `npm test`):**
- `src/lib/modeSettings.test.ts`: `audioBuffer` defaults to `"auto"` across `mirror`, `flex`, `hifi`; `normalizeModeDefaults` fills missing/invalid `audioBuffer` to `"auto"`; `resolveLaunchSettings` passes `audioBuffer` without mutating other modes.
- `src/components/settings/__tests__/PhoneMirroringSection.test.tsx`: renders `Audio buffer` (`Auto / 50 ms / 100 ms / 200 ms`), disables when `Play phone audio on this PC` is off, edits only the active mode tab, mentions USB stability in helper text.
- `src/components/settings/__tests__/SettingsPanel.test.tsx` & filter tests: registry entries `M-10b`, `A-01`, `A-02` appear under search keywords (`"buffer"`, `"stutter"`, `"bluetooth"`, `"usb"`).
- (Phase 2) `AudioMediaPowerSection` tests with mocked Tauri commands: renders Bluetooth latency warning, paired device list, error banners (no adapter, Bluetooth off, not paired, old Windows, `OpenAsync` timeout/denied), and triggers in-place session restart on toggle.

### 10.2 Manual verification checklist (`docs/MANUAL-TEST-CHECKLIST.md`)
1. **Home Wi-Fi (clean network):** Mirror a video/music clip with `Auto` (`150` ms) and `50` ms; verify zero stutter and acceptable A/V sync.
2. **Office Wi-Fi (weak/congested):** Reproduce stutter at `50` ms, switch to `Auto` (`150` ms) and `200` ms, verify stutter disappears or drops sharply.
3. **USB baseline:** Connect via USB with `Auto`; inspect spawned args / verify `50` ms low-latency audio and crisp lip-sync.
4. **Bluetooth audio mode (Phase 2):**
   - Pair phone once in Windows Settings → select in AnyLeap → turn on `Play phone audio via Bluetooth`.
   - Verify audio plays through PC speakers with zero echo/double audio (`scrcpy` running with `--no-audio`).
   - Toggle Bluetooth audio on/off mid-mirror session: confirm `#93` in-place restart keeps the embedded mirror window open without losing geometry.
   - Verify expected ±150–250 ms lag warning matches reality and turning Bluetooth off on the phone triggers the non-blocking disconnect banner.
   - Verify error copy when Bluetooth is off on the PC or phone is unpaired.

---

## 11. Phasing

| Phase | Scope | Ships independently? |
|---|---|---|
| **Phase 1: Adaptive audio buffer** | `audioBuffer` (`Auto / 50 / 100 / 200 ms`) in `modeDefaults`, `CoreSettings`, `build_scrcpy_args`, `build_audio_only_args`, `PhoneMirroringSection`, unit tests + office/home Wi-Fi check. | Yes — no WinRT dependencies. |
| **Phase 2: Phone audio via Bluetooth** | `bt_audio.rs` (`AudioPlaybackConnection`), double-audio `--no-audio` guard + `#93` in-place restart, `AudioMediaPowerSection` controls, error states, Overview/Mirror status, unit + manual BT tests. | Yes — builds cleanly on Phase 1. |
| **Phase 3: Automatic failover** | **Deferred** (documented in §9; no implementation planned). | N/A |

---

## 12. Risks & Open Questions

### Risks
- **R1 (`scrcpy` server/USB-to-Wi-Fi handoff):** If a user connects via USB and switches to Wi-Fi later, `Auto` only updates on the next session launch/restart (no dynamic mid-session USB→Wi-Fi buffer hot-swap without restarting `scrcpy`). Accepted: restarting on transport drop already creates a new session.
- **R2 (Android Bluetooth A2DP volume & codec differences):** Some phones default Bluetooth media volume low or connect HFP (call profile, mono low-quality) alongside A2DP if a voice call is active. Mitigation: document HFP call behaviour and surface Windows' Bluetooth state cleanly.
- **R3 (WinRT feature bloat / compile time in `windows` crate):** Adding `Media_Audio`, `Devices_Enumeration`, `Devices_Bluetooth`, `Devices_Radios` features to `windows = "0.61"` in `src-tauri/Cargo.toml` may require `Foundation` / `Foundation_Collections` WinRT async traits (`IAsyncOperation`). Verify exact `windows` crate feature names and `cargo check` time during Phase 2 Task 2.1.

### Open Questions
- **Q1 (Phase 2):** Should `Play phone audio via Bluetooth` automatically attempt `OpenAsync` on `start_mirror` when the setting is on and the paired phone is in range, or only connect when the user toggles it on? *(Recommended: auto-open on `start_mirror` when enabled and selected device is present, falling back to a clear error banner rather than silently playing double audio.)*
- **Q2 (Phase 2):** On Android devices paired to Windows that also connect as a Bluetooth audio *headset* in the reverse direction (PC → Phone), does `AudioPlaybackConnection::GetDeviceSelector()` filter strictly to A2DP Source devices? *(Verify on real Windows 11 + Android hardware in Phase 2 Task 2.1 before finalizing auto-match heuristics.)*
