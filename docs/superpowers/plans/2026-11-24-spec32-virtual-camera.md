# SPEC-32 Native Virtual Camera Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans (recommended) or execute phase-by-phase with verification gates. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose Android phones as a native Windows 11 virtual webcam (`"AnyLeap Camera"`) via Media Foundation (`MFCreateVirtualCamera`) and a direct `scrcpy-server` → MF hardware H.264 decode pipeline, keeping Windows 10 fallback and following SPEC-28 UI/settings rules.

**Architecture:** Promote the Rust build to a Cargo workspace (`src-tauri`, `crates/anyleap-vcam-ipc`, `crates/anyleap-vcam`). `anyleap.exe` runs a direct `scrcpy-server` socket client + Media Foundation H.264 decoder MFT outputting `NV12` frames into a triple-buffered cross-session shared memory ring with strict ACLs (`CurrentUser` + `LOCAL SERVICE`). `anyleap_vcam.dll` (a minimal `cdylib` COM `IMFMediaSourceEx` / `IMFMediaStream2` server registered in `HKLM` and loaded by `FrameServer`) serves frames and placeholder cards to video-call and browser apps.

**Tech Stack:** Rust (`windows` / `windows-core` 0.61, Tauri 2), Win32 Media Foundation & COM, NSIS (`src-tauri/nsis/hooks.nsh`), React 19 + TypeScript + Tailwind (SPEC-26 tokens), Vitest, Cargo test.

**Spec & ADR:** `docs/specs/SPEC-32-native-virtual-camera.md`, `docs/adr/0006-mf-virtual-camera.md`.

## Global Constraints
- Follow `AGENTS.md` §1.3: zero third-party or competitor product names in code, comments, UI copy, tests, or docs.
- Follow `SPEC-28` §8.5 & §9.1: single editor in **Settings → Phone mirroring → Camera**; positive sentence-case labels; SPEC-26 contrast rules (`--text-muted` only on `--ink-900` / `--ink-950`, never `--ink-800`).
- Follow `AGENTS.md` §2: every phase ends green on all 5 gates (`npm test`, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo check`, `npm run build`) and ships independently.
- Windows 10 (build < 22000) and unregistered environments must fall back cleanly to today's `scrcpy` Camera Studio window flow.

---

## Phase 0: Technical Spike (De-Risk Decode & FrameServer Boundary)

### Task 0.1: Spike `scrcpy-server` raw stream + MF H.264 MFT decode
**Files:** scratch/spike crate `crates/spike-vcam-decode/` (excluded from release installer).
- [ ] Push bundled `scrcpy-server 5.0.1` via `adb push` and launch `app_process` (`video_source=camera`, `tunnel_forward=true`, `send_frame_meta=true`, `video_codec=h264`, `audio=false`).
- [ ] Parse initial dummy byte, 64-byte device name, 12-byte codec/resolution header, and 12-byte packet headers (`pts_flags` + `packet_size`).
- [ ] Feed `SC_PACKET_FLAG_CONFIG` (SPS/PPS) and video packets into `CLSID_CMSH264DecoderMFT` (`IMFTransform`) outputting `MFVideoFormat_NV12`; handle `MF_E_TRANSFORM_STREAM_CHANGE`.
- [ ] Measure and record decode latency (ms/frame), CPU usage, and frame cadence on target laptop; verify target ≤ 15 ms decode and < 15% CPU at `720p30`.

### Task 0.2: Spike `anyleap_vcam.dll` + `MFCreateVirtualCamera` + Cross-Session IPC
**Files:** scratch/spike crate `crates/spike-vcam-source/`.
- [ ] Implement minimal Rust `cdylib` COM DLL (`DllGetClassObject`, `DllCanUnloadNow`, `IMFMediaSourceEx`, `IMFMediaStream2`) generating an `NV12` test pattern.
- [ ] Register CLSID under `HKLM\SOFTWARE\Classes\CLSID\{...}\InprocServer32` (`ThreadingModel = "Both"`) in a `LOCAL SERVICE`-readable folder.
- [ ] Activate via `MFCreateVirtualCamera(MFVirtualCameraType_SoftwareCameraSource, MFVirtualCameraLifetime_Session, MFVirtualCameraAccess_CurrentUser, ...)` and confirm `"AnyLeap Camera"` renders in the **Windows Camera** app.
- [ ] Validate cross-session frame transfer between non-elevated `anyleap.exe` (Session 1+) and `FrameServer` (`LOCAL SERVICE`, Session 0) using file-backed `CreateFileMappingW` + Named Pipe/event with explicit DACLs (`ERROR_ACCESS_DENIED`-free).
- [ ] Document spike findings in `docs/specs/SPEC-32-native-virtual-camera.md` (or linked spike note) before Phase 1; discard/archive scratch spike binaries cleanly.

---

## Phase 1: Static Test-Pattern Virtual Camera & Installer Lifecycle

### Task 1.1: Cargo Workspace & `anyleap-vcam-ipc` Crate (TDD)
**Files:** create `Cargo.toml` (workspace root), `crates/anyleap-vcam-ipc/Cargo.toml`, `crates/anyleap-vcam-ipc/src/lib.rs`, `crates/anyleap-vcam-ipc/src/nv12.rs`.
- [ ] Write failing unit tests in `crates/anyleap-vcam-ipc`:
  - `SharedFrameHeader::validate` rejects bad magic/version, `width > 3840`, `height > 2160`, `stride < width`, and `slot >= 3`.
  - Seqlock writer/reader round-trip across threads verifies zero torn frames.
  - Branded `"AnyLeap Camera"` `NV12` test-card & disconnect-placeholder generators produce valid non-black luma/chroma planes at `1280x720` and `1920x1080`.
- [ ] Implement `SharedFrameHeader`, `SharedFrameWriter`, `SharedFrameReader`, explicit Win32 SDDL/ACL helper (`CurrentUser` + `LOCAL SERVICE`), and `NV12` pattern generators.
- [ ] Run `cargo test -p anyleap-vcam-ipc`; verify `cargo check --manifest-path src-tauri/Cargo.toml` still works.

### Task 1.2: `crates/anyleap-vcam` COM Media Source DLL
**Files:** create `crates/anyleap-vcam/Cargo.toml`, `crates/anyleap-vcam/src/lib.rs`, `crates/anyleap-vcam/src/media_source.rs`, `crates/anyleap-vcam/src/media_stream.rs`, `crates/anyleap-vcam/src/com.rs`.
- [ ] Implement `DllGetClassObject`, `DllCanUnloadNow`, `DllRegisterServer`, `DllUnregisterServer` (`HKLM\SOFTWARE\Classes\CLSID\{<CLSID>}`).
- [ ] Implement `IMFMediaSourceEx`, `IMFGetService`, `IKsControl` stub, and `IMFMediaStream2` advertising `MFVideoFormat_NV12` (`1280x720@30`, `1920x1080@30`, `1280x720@60`, `640x480@30`).
- [ ] Update `consumer_count` in shared state on `Start`/`Stop`/`Shutdown`; serve `SharedFrameReader` frames or the branded test card when no writer is active.
- [ ] Verify DLL compiles cleanly (`cargo build -p anyleap-vcam --release`, `cargo clippy -- -D warnings`).

### Task 1.3: Backend Lifecycle, Registration Helper, & NSIS Hooks (`#111` Pattern)
**Files:** create `src-tauri/src/vcam/mod.rs`, `src-tauri/src/vcam/win11.rs`; modify `src-tauri/src/lib.rs`, `src-tauri/tauri.conf.json`, `src-tauri/nsis/hooks.nsh`.
- [ ] Add OS build check (`>= 22000`), COM registration status check (`HKLM` CLSID probe), and `MFCreateVirtualCamera` start/stop/remove wrapper (`MFVirtualCameraLifetime_Session`, `MFVirtualCameraAccess_CurrentUser`).
- [ ] Bundle `anyleap_vcam.dll` in Tauri resources and implement elevated registration helper (copies DLL to `LOCAL SERVICE`-readable `%ProgramData%\AnyLeap\vcam\` with explicit ACLs + `HKLM` registration, using versioned/rename-swap replacement if `FrameServer` holds the prior DLL).
- [ ] Update `src-tauri/nsis/hooks.nsh` following `#111`: stop `anyleap.exe` first in `NSIS_HOOK_PREINSTALL` / `PREUNINSTALL`, handle locked `anyleap_vcam.dll` swap cleanly on update, and clean up `HKLM` CLSID + `%ProgramData%\AnyLeap\vcam\` in `NSIS_HOOK_POSTUNINSTALL`.
- [ ] Run all 5 `AGENTS.md` §2 gates; test Phase 1 static virtual camera in Windows Camera app manually.

---

## Phase 2: Live Phone Camera Frames

### Task 2.1: Direct `scrcpy-server` Camera Socket Client (TDD)
**Files:** create `src-tauri/src/vcam/scrcpy_stream.rs`; modify `src-tauri/src/vcam/mod.rs`.
- [ ] Write failing unit tests for `scrcpy-server 5.0.1` header parsing (dummy byte, 64-byte device name, 12-byte codec/width/height) and 12-byte frame packet parsing (`SC_PACKET_FLAG_CONFIG`, `SC_PACKET_FLAG_KEYFRAME`, PTS, payload slice, clean EOF detection).
- [ ] Implement ADB setup (`adb push scrcpy-server`, `adb forward tcp:<port> localabstract:scrcpy_<scid>`, `app_process` with `--video-source=camera` equivalent server args, `video_codec=h264`) and control socket writer (`torch`, `zoom`, `rotate`, replacing `keybd_event` focus hacks when using the native pipeline).
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml vcam::scrcpy_stream`.

### Task 2.2: Media Foundation Hardware H.264 Decoder & Pipeline Wiring
**Files:** create `src-tauri/src/vcam/mf_decoder.rs`, `src-tauri/src/vcam/pipeline.rs`; modify `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`.
- [ ] Implement `MfH264Decoder` (`CLSID_CMSH264DecoderMFT` / `MFTEnumEx`) with SPS/PPS buffering, `MF_E_TRANSFORM_STREAM_CHANGE` renegotiation, and `NV12` plane extraction.
- [ ] Wire `start_camera_mirror` so that on Windows 11 with native virtual camera enabled & registered it launches the `vcam::pipeline` (plus optional in-app preview / phone mic audio), while on Windows 10 or when disabled/unregistered it preserves the exact `build_scrcpy_camera_args` + `spawn_session` fallback unchanged.
- [ ] Wire `send_camera_shortcut` to forward `torch` / `zoom` / `rotate` over the `scrcpy-server` control socket when a native virtual-camera session is active.
- [ ] Run all 5 `AGENTS.md` §2 gates; manually verify live phone camera streaming in Windows Camera and video-call/browser apps.

---

## Phase 3: Camera Studio UX, Controls, & Disconnect Resilience

### Task 3.1: Phone Disconnect Placeholder & Horizontal Mirror Transform (TDD)
**Files:** modify `crates/anyleap-vcam-ipc/src/nv12.rs`, `src-tauri/src/vcam/pipeline.rs`, `crates/anyleap-vcam/src/media_stream.rs`.
- [ ] Write failing unit tests for horizontal mirror (`NV12` Y-plane row reversal + UV-pair reversal, odd-width guard) and disconnect placeholder state transition (`Streaming` → `Placeholder` immediately on socket EOF).
- [ ] Implement `mirror_horizontal` in `pipeline.rs` (runtime toggleable via Tauri command `set_vcam_mirror`) and automatic reconnect loop emitting `"AnyLeap Camera — Phone disconnected / Reconnecting..."` placeholder frames without ever freezing or outputting black frames.
- [ ] Expose `get_vcam_status` / `vcam-status-changed` event reporting `{ active, inUse, consumerCount, state, registered, supported }`.

### Task 3.2: Settings & Launch UI (`SPEC-28` Single-Home Compliance) (TDD)
**Files:** modify `src/lib/modeSettings.ts`, `src/lib/modeSettings.test.ts`, `src/components/settings/registry.ts`, `src/components/settings/PhoneMirroringSection.tsx`, `src/components/settings/__tests__/SettingsPanel.test.tsx`, `src/lib/settingsFilter.test.ts`, `src/components/CameraLaunchDialog.tsx`, `src/lib/controlBarModel.ts`, `src/lib/controlBarModel.test.ts`, `src/components/ControlBar.tsx`, `src/components/DeviceRow.tsx`, `src/components/deck/StudioDeck.tsx`.
- [ ] Extend `CameraModeSettings` with `virtualCameraEnabled: boolean` (default `true`) and `mirrorHorizontal: boolean` (default `false`); update normalization and unit tests.
- [ ] Add `M-C1` and `M-C2` to `REGISTRY` and `CameraPanel` (`Settings → Phone mirroring → Camera`), including Windows 10 disabled explanation, `"Set up virtual camera"` action when unregistered, and SPEC-28 contrast/a11y attributes.
- [ ] Update `CameraLaunchDialog.tsx` to replace the external-app guide with the native readiness card + Settings link on Windows 11 (and keep neutral fallback copy on Windows 10, zero third-party app names).
- [ ] Add `mirrorHorizontal` toggle (`aria-pressed`) and `"Camera in use"` `aria-live="polite"` indicator in `ControlBar.tsx`, `DeviceRow.tsx`, and `StudioDeck.tsx`.
- [ ] Verify `noThirdPartyNames.test.ts` passes cleanly.

### Task 3.3: Documentation Sync & Final Verification (`AGENTS.md` §1.4 & §2)
**Files:** modify `README.md`, `CONTEXT.md`, `docs/PLAN.md`, `docs/BACKLOG.md`, `docs/specs/SPEC-32-native-virtual-camera.md`, `docs/MANUAL-TEST-CHECKLIST.md`.
- [ ] Update `README.md` Features, `CONTEXT.md` status, `docs/PLAN.md` / `docs/BACKLOG.md`, `SPEC-32` status header, and manual checklist items (§10.2 of SPEC-32).
- [ ] Run all 5 `AGENTS.md` §2 gates (`npm test`, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo check`, `npm run build`).
