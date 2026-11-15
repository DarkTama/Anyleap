# SPEC-25b: Audio Bridge & Media Keys Follow Active Audio

> **Status:** Implemented / Shipped in v1.1.0

## Goal

Extend AnyLeap's Continuity experience (Windows-only, building on SPEC-19 QUIC and SPEC-23/24 Main PC / Linked PC links) with two audio capabilities:

1. **Audio Bridge (Linked PC → Main PC, default off):** Stream the Linked PC's desktop audio over the LAN QUIC link and blend it into the Main PC's default playback device with low latency and a per-stream volume slider.
2. **Media Keys Follow Active Audio (default on):** Route media and volume keys (`Play/Pause`, `Next`, `Previous`, `Stop`, `Volume Up`, `Volume Down`, `Volume Mute`) to whichever PC most recently produced audible output within the last 10 seconds, falling back to the PC where the cursor currently resides.

## Architecture & Audio Bridge

### Capture (Linked PC)
- Captures the Windows default render endpoint (`eRender`, `eConsole`) using **WASAPI loopback** in shared mode (`AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK`, with timer-driven polling fallback if a driver rejects `EVENTCALLBACK` on loopback).
- Converts the device mix format (16/24/32-bit PCM or 32-bit IEEE float, mono/stereo/multi-channel, arbitrary sample rate) to **48 kHz, 16-bit signed LE, stereo PCM** (no codec; ~1.5 Mbps active LAN bandwidth).
- When the endpoint reports `AUDCLNT_BUFFERFLAGS_SILENT`, flushes up to two trailing frames and pauses datagram emission until audio resumes.
- Slices PCM into **5 ms frames (240 stereo frames = 960 bytes)** and sends each over the active QUIC connection as an unreliable datagram.
- Starts/stops on `AudioStart` / `AudioStop` packets, peer disconnect, or server stop, and automatically follows Windows default render device changes (via periodic default endpoint ID check and `AUDCLNT_E_DEVICE_INVALIDATED` recovery).

### Playback & Jitter Buffer (Main PC)
- Receives audio datagrams on the QUIC client (`KvmClientHandle`) and feeds them into a sequence-ordered **~40 ms jitter buffer** (target depth: 8 × 5 ms frames).
- Handles out-of-order datagram arrival and `u32` sequence wraparound.
- **Packet loss concealment:** On a missing frame, repeats the last played frame once, then outputs silence (`0` PCM) for subsequent consecutive losses until frames resume.
- Scales PCM samples in software by `audioVolume` (`0..=100%`) and writes to the Windows default render endpoint in shared mode (`AUDCLNT_SHAREMODE_SHARED | AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM`), letting the Windows Audio Engine blend bridged audio with local Main PC audio.
- Auto-follows Windows default render device changes, starts/stops cleanly on settings toggle and link/unlink, and emits `kvm-audio-state { streaming, level }`.

### Linked PC Speaker Mute & Process Loopback (`muteLinkedWhileBridging`, default `true`)
- When `mute_local` is `true`, the Linked PC activates WASAPI **process loopback** (`ActivateAudioInterfaceAsync` with `VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK` and `PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE` targeting its own PID) and mutes the default render endpoint (`IAudioEndpointVolume::SetMute(true)`), recording prior mute state in a crash marker (`%LOCALAPPDATA%\AnyLeap\audio_mute_marker.json`).
- **Activation lifetime & safety:** The `VT_BLOB` activation `PROPVARIANT` is wrapped in `ManuallyDrop` so `PROPVARIANT::drop` (`PropVariantClear` -> `CoTaskMemFree`) never frees the Rust-owned `AUDIOCLIENT_ACTIVATION_PARAMS`, and both the activation params and the completion event `HANDLE` are reference-counted inside `ProcessLoopbackHandler` (`Arc`) so an activation timeout cannot leave dangling pointers or closed handles. Capture generation tracking (`capture_gen`) prevents an exiting capture thread from unmuting a newer capture session.
- **Restore rules:** AnyLeap restores (`SetMute(false)`) on `AudioStop`, peer disconnect/unlink, app exit (`RunEvent::Exit`), and next startup crash-marker recovery — **only** when AnyLeap applied the mute (`!prior_muted`). If `mute_local` is `false`, capture keeps the existing endpoint-loopback path without muting.
- **Runtime silence guard:** While muted on process loopback, AnyLeap compares other sessions' peak meters (`IAudioSessionManager2` excluding own PID) against captured PCM. If other sessions are active (`> 0.005`) while captured PCM stays digitally silent (`< 0.0005`) continuously for `~1 s`, AnyLeap unmutes, falls back to endpoint loopback, logs `AUDIO_MUTE_FALLBACK`, and reports `linkedMute: "unsupported"` for the rest of the session.

## Media Keys Follow Active Audio

### Activity Detection
- **Linked PC:** Polls `IAudioMeterInformation::GetPeakValue()` on the default render endpoint while a peer is connected and sends `KvmPacket::AudioActivity { active: bool }` when peak crosses the noise-floor threshold (`0.005`), heartbeating `active: true` every 2 seconds while audio remains active.
- **Main PC:** Polls its own default render endpoint peak while linked. When `audioBridge` is active, excludes AnyLeap's own process ID via `IAudioSessionManager2` / `IAudioSessionControl2::GetProcessId()` so bridged audio playing locally does not falsely mark the Main PC as the active audio source.

### Routing Decision & Keyboard Hook Path
- Implemented strictly in the `WH_KEYBOARD_LL` keyboard hook path (`hooks.rs`) using lock-free atomic timestamps and non-blocking `mpsc::try_send` (no blocking calls, allocations, or `SendInput` inside the hook callback).
- Supported virtual keys: `VK_VOLUME_MUTE` (`0xAD`), `VK_VOLUME_DOWN` (`0xAE`), `VK_VOLUME_UP` (`0xAF`), `VK_MEDIA_NEXT_TRACK` (`0xB0`), `VK_MEDIA_PREV_TRACK` (`0xB1`), `VK_MEDIA_STOP` (`0xB2`), `VK_MEDIA_PLAY_PAUSE` (`0xB3`).
- Pure routing rule (`decide_media_key_target_for_vk`):
  - **Volume keys while bridging (`audio_streaming == true`):** Volume Up, Volume Down, and Volume Mute always stay on the Main PC (`Local`) so volume keys never adjust or unmute the muted Linked PC endpoint.
  - Otherwise, if `mediaKeysFollowAudio` is `false`: route to the PC where the cursor is (`Linked` when trapped, `Local` otherwise).
  - If `mediaKeysFollowAudio` is `true`: compare the most recent local and linked audible timestamps within a 10-second window (`10_000 ms`). Route to the more recently active PC; if neither was active within 10 seconds (or on a tie), route to the PC where the cursor is.
- When routed to `Linked`, the hook swallows the key locally and sends `KvmPacket::MediaKey { vk, is_down }`; the Linked PC async packet handler injects it via `SendInput`. When routed to `Local`, the hook passes the key through (`CallNextHookEx`) even if the cursor is currently on the Linked PC.

## Protocol & Tauri Contract

### `KvmPacket` Additions
- `AudioStart { sample_rate: u32, channels: u16, frame_ms: u16, mute_local: bool }`
- `AudioStop`
- `AudioMuteState { state: String }` (`"muted" | "unmuted" | "unsupported"`)
- `AudioActivity { active: bool }`
- `MediaKey { vk: u16, is_down: bool }`

### QUIC Datagrams
- Mouse deltas remain **exactly 12 bytes**.
- Audio datagram wire layout: `[0x41 ('A'), seq: u32 LE, frame_len: u16 LE, pcm bytes...]`. Total length is never 12 bytes; `decode_audio_datagram` rejects any 12-byte datagram so mouse deltas and audio datagrams never collide.

### Tauri Commands & Events
- `kvm_set_audio_settings(audioBridge: bool, audioVolume: u8, mediaKeysFollowAudio: bool, muteLinkedWhileBridging?: bool)`
- `kvm_get_audio_state() -> { streaming: bool, level: f32, linkedMute: "muted" | "unmuted" | "unsupported" }`
- Event `kvm-audio-state`: `{ streaming: bool, level: f32, linkedMute: "muted" | "unmuted" | "unsupported" }`

## Verification
- Pure unit tests: audio datagram codec (never 12 bytes, rejects 12-byte mouse deltas starting with `0x41`), jitter buffer (40 ms priming, reordering, loss → repeat once then silence, `u32` wraparound), PCM volume scaling and 44.1→48 kHz resampling, media-key routing decision (including volume keys forced `Local` while bridging), mute apply/restore decision & crash marker roundtrip, silence-guard 1-second continuous mismatch tracker, and write-through debug log flushing (`debug_log::flush` / immediate `write_line` flush / panic hook).
- Loopback QUIC integration test (`loopback_audio_start_and_audio_datagrams`): verifies `AudioStart` (`mute_local`), `AudioMuteState`, 8 × 5 ms (960-byte) audio datagrams through `JitterBuffer`, `AudioActivity`, `MediaKey`, and `AudioStop`.
- Windows-only ignored hardware tests (`repro_process_loopback_capture_with_mute_local`, `verify_crash_marker_restores_endpoint_mute_on_startup`): verify 10 s live process-loopback capture while muted without crashing, clean mute restore on stop, and startup crash-marker unmute recovery.
- Quality gates: `cargo test`, `cargo check --manifest-path src-tauri/Cargo.toml`, `npm test`, `npm run build`.
