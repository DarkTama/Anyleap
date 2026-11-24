# SPEC-32: Native Virtual Camera via Windows Media Foundation

> **Status:** Proposed (2026-11-24). Supersedes the draft blueprint in [SPEC-15](SPEC-15-native-virtual-camera-mediasource.md).
> **Scope:** Windows 11 native virtual webcam (`MFCreateVirtualCamera` + out-of-process COM `IMFMediaSource` DLL), direct `scrcpy-server` video decode pipeline, Camera Studio UI/settings integration (`src-tauri/`, `src/components/`, `src/lib/modeSettings.ts`), and NSIS installer/updater hooks (`src-tauri/nsis/hooks.nsh`).
> **Related:** ADR-0006 (Media Foundation virtual camera architecture), SPEC-28 (Settings redesign & single-home rule), SPEC-26 (Ink & Gold tokens), SPEC-15 (superseded draft).
> **Plan:** `docs/superpowers/plans/2026-11-24-spec32-virtual-camera.md`

---

## 1. Problem

Today, AnyLeap's **Camera Studio** mode streams an Android phone's camera sensor into a desktop `scrcpy` window (`AnyLeap Camera — <serial>` / `AnyLeap Camera Studio — <serial>`), but video-call and browser apps cannot select AnyLeap as a webcam directly.

Users must install **an external virtual-camera app**, manually add a Window Capture source pointing at `AnyLeap Camera Studio`, keep that preview window unminimized and unoccluded, and select the external app's virtual camera in their video-call or browser app (`CameraLaunchDialog.tsx:141-153`, `ControlBar.tsx:397-435`). Window capture adds DWM compositor latency, clips or turns black when minimized, and breaks on DPI/resize changes.

---

## 2. Goals and Non-Goals

### Goals
- **G1. Native Windows 11 virtual camera ("AnyLeap Camera").** Expose a native software camera via Windows 11 Media Foundation (`MFCreateVirtualCamera`, build 22000+) selectable directly in video-call and browser apps and the Windows Camera app with no kernel driver and no DirectShow filter.
- **G2. Windows 10 fallback.** Keep today's `scrcpy` window + external virtual-camera app guide automatically on Windows 10 (pre-build 22000) and whenever native virtual-camera registration/activation is unavailable.
- **G3. Zero Android companion app.** Keep the phone side 100% ADB + bundled `scrcpy-server` (`--video-source=camera`).
- **G4. Resilient live UX.** Support resolution/fps selection, mirror/flip, a live "camera in use" indicator, and an immediate branded placeholder frame when the phone disconnects (never a frozen last frame or black stream).
- **G5. Incremental, independently shippable phases.** De-risk decoding and COM/FrameServer isolation in a Phase 0 spike before shipping test-pattern registration (Phase 1), live phone frames (Phase 2), and UX polish (Phase 3).

### Non-Goals
- **No kernel-mode driver (AVStream/UMDF/DMFT) and no DirectShow filter.**
- **No virtual microphone in this spec.** Phone audio continues to use the existing PC playback path or a future dedicated virtual-audio spec (to be authored separately).
- **No macOS or Linux virtual camera.** AnyLeap is Windows-first; non-Windows targets remain out of scope here.
- **No Android companion app and no `scrcpy` fork.** Upstream `scrcpy-server` (`5.0.1`) is used unmodified.
- **No per-phone virtual camera devices.** One virtual camera endpoint (`AnyLeap Camera`) serves the active Camera Studio session.

---

## 3. Research Findings

### 3.1 How Camera Studio Works Today

| Layer | Current Behaviour | Source |
|---|---|---|
| **Data model & args** | `CameraSettings` (`facing`, `cameraId`, `size`, `fps`, `highSpeed`, `torch`, `noAudio`, `embedded`, `orientation`, `videoBitRate`, `videoBuffer`) feeds `build_scrcpy_camera_args`, which emits `--serial`, `--video-source=camera`, `--camera-id` or `--camera-facing`, `--camera-size` (default `1280x720`), `--camera-fps`, `--capture-orientation`, `--video-bit-rate`, `--video-buffer`, `--camera-high-speed`, `--camera-torch`, `--no-audio`, optional embedded window flags, and `--window-title=AnyLeap Camera — <serial>`. | `src-tauri/src/commands.rs:39-61, 419-477` |
| **Backend IPC & process** | `list_device_cameras` runs `scrcpy --list-cameras` and parses options via `parse_list_cameras_output`. `start_camera_mirror` calls `spawn_session(..., "camera")` (`commands.rs:904-923, 930-1050`), spawning the `scrcpy` sidecar (`scrcpy-server` resolved via `resolve_server_path` at `:315-343`). | `src-tauri/src/commands.rs:65-71, 315-343, 1235-1264`; `src-tauri/src/lib.rs:68-70` |
| **In-session shortcuts** | `send_camera_shortcut` resolves the scrcpy HWND (`embed::get_embedded_scrcpy_hwnd` or `EnumWindows` by PID) and calls `focus_scrcpy_for_shortcut` + `keybd_event` (`Alt+T`, `Alt+Shift+T`, `Alt+Up/Down/Left/Right`) for torch, zoom, and rotation. | `src-tauri/src/commands.rs:1291-1449` |
| **Mode defaults (SPEC-28)** | `modeDefaults.camera` stores `{ playAudio, bitrate: "low" \| "balanced" \| "high", embedded }` (`2 / 8 / 16 Mbps`). `toCameraSettings` pre-fills launch settings; per-launch dialog choices are not persisted back. `CameraPanel` in Settings edits only those three fields. | `src/lib/modeSettings.ts:64-70, 80-84, 133, 149-159`; `src/components/settings/PhoneMirroringSection.tsx:515-547` |
| **Launch & guide UI** | `CameraLaunchDialog` (opened from `DeviceRow` or `StudioDeck`) queries `listDeviceCameras`, lets the user pick lens, resolution, FPS, orientation, profile, torch, and mic audio, and displays a 4-step guide instructing the user to capture `AnyLeap Camera Studio` inside an external virtual-camera app. | `src/components/CameraLaunchDialog.tsx:22-349`; `src/components/DeviceRow.tsx:250-282`; `src/components/deck/LaunchMatrix.tsx:39-45`; `src/components/deck/StudioDeck.tsx:558-586` |
| **Mirror window & bar** | When `embedded` is true, `App.tsx` opens `mirror-*` titled `AnyLeap Camera Studio — <serial>` and `EmbeddedMirrorWindow` renders `AnyLeap Camera — <serial>`. `ControlBar` shows `torch`, `zoomIn`, `zoomOut`, `cameraHelp` (modal repeating the window-capture guide), and `stopCamera`. | `src/App.tsx:527-535`; `src/components/EmbeddedMirrorWindow.tsx:281`; `src/components/ControlBar.tsx:397-435`; `src/lib/controlBarModel.ts:87, 124-133` |

**How frames reach external apps today:** They do not leave `scrcpy.exe` through any media sink. `scrcpy.exe` decodes video internally and draws it to its `SDL_app` HWND (optionally reparented by `src-tauri/src/embed.rs`), and an external virtual-camera app captures that window pixels-out-of-DWM via Window Capture.

---

### 3.2 Getting Raw Decoded Frames on Windows

Inspecting the bundled `scrcpy 5.0.1` binary (`src-tauri/binaries/scrcpy-x86_64-pc-windows-msvc.exe --help`, pinned in `scripts/fetch-binaries.ps1:17`) confirms:
- `--v4l2-sink=/dev/videoN` and `--v4l2-buffer=ms` explicitly document: *"This feature is only available on Linux."* There is no Windows virtual-camera sink in `scrcpy 5.0.1`.
- `scrcpy` does bundle `scrcpy-server` as a Tauri resource (`src-tauri/tauri.conf.json:44`, `scrcpy-res/scrcpy-server`) and FFmpeg DLLs (`avcodec-63.dll`, `avformat-63.dll`, `avutil-61.dll`, `swresample-7.dll`) alongside `SDL3.dll` in `binaries/dll/` (`scripts/fetch-binaries.ps1:108-113`, `src-tauri/tauri.conf.json:45`).

#### Options Comparison

| Criterion | (a) `scrcpy --record` to pipe/file + FFmpeg DLLs (`avcodec-63.dll`) | (b) Direct `scrcpy-server` Rust client + Media Foundation MFT decoder | (c1) Window Capture (`Windows.Graphics.Capture`) of `SDL_app` | (c2) Forking `scrcpy` to add a Windows sink |
|---|---|---|---|---|
| **Extra process / hops** | Spawns `scrcpy.exe` (`--no-window --record=\\.\pipe\... --record-format=mkv`) which muxes MKV into a pipe; AnyLeap demuxes with `avformat` and decodes with `avcodec`. | **None.** AnyLeap pushes & launches `scrcpy-server` over `adb forward`, reads raw H.264 packets directly over TCP, and decodes in-process via MF MFT. | Spawns `scrcpy.exe` with a visible window; captures DWM surface via WGC/DXGI and converts BGRA → NV12. | Requires maintaining custom C build + replacing upstream signed `scrcpy` zip. |
| **Latency** | Higher (+10–30 ms muxer/pipe buffering; MKV interleaving delays when audio is enabled; MP4 is unstreamable over non-seekable pipes). | **Lowest (~8–20 ms decode).** Zero container overhead; direct Annex-B packet → MFT → `NV12`. | Medium (+1–2 vsyncs DWM compositor + GPU/CPU color conversion). | Low, but high maintenance cost. |
| **Headless / minimized** | Yes (`--no-window`). | **Yes** (native headless by default; optional in-app preview renders from the same decoded buffer). | **No.** Minimizing or hiding `SDL_app` stalls/blanks WGC capture. | Yes. |
| **ABI & maintenance** | Dynamic FFI to `avcodec-63.dll` / `avformat-63.dll` breaks whenever `fetch-binaries.ps1` bumps FFmpeg major versions; no clean control channel when `scrcpy.exe` owns it. | Pins to `scrcpy-server 5.0.1` wire framing (already pinned in `fetch-binaries.ps1`) and uses stable Win32 Media Foundation COM APIs (`windows` 0.61). Also unlocks direct control-socket camera commands (no `keybd_event` focus hack). | Fragile to DPI scaling, window chrome, and occlusion. | Violates upstream-binary policy (`scripts/fetch-binaries.ps1` SHA-256 verification). |
| **Hardware decode & format** | CPU decode by default unless D3D11VA FFmpeg hwcontext is wired manually; outputs YUV420P requiring NV12 conversion. | **Hardware-accelerated** via Windows inbox H.264/HEVC MFT (`MFT_CATEGORY_VIDEO_DECODER`), outputting **`MFVideoFormat_NV12`** natively. | GPU BGRA → CPU/shader NV12 conversion required. | Manual. |

#### Recommendation: Option (b) — Direct `scrcpy-server` Client + Media Foundation Hardware Decode

Option (b) eliminates container mux/demux overhead, avoids fragile `avcodec-63.dll` / `avformat-63.dll` FFI version churn, runs completely headless without a visible `SDL_app` window, outputs `NV12` directly from Windows hardware decoders, detects phone/USB disconnect immediately on TCP EOF, and opens the door to replacing `keybd_event` shortcut injection (`commands.rs:1327-1449`) with native `scrcpy-server` control messages.

*(Note: `avcodec-63.dll` remains a documented fallback only if a future codec/device bitstream fails in the inbox MF MFT during Phase 0/2 testing.)*

#### What the Phase 0 Spike Must Prove
1. **Laptop decode viability:** Running `scrcpy-server 5.0.1` (`video_source=camera`, `tunnel_forward=true`, `send_frame_meta=true`, `video_codec=h264`, `audio=false`) via `adb forward` into a standalone Rust prototype parses the initial device/codec header and 12-byte frame headers, handles `SC_PACKET_FLAG_CONFIG` (SPS/PPS) and `MF_E_TRANSFORM_STREAM_CHANGE`, and decodes live `NV12` frames via Media Foundation `IMFTransform` within ≤ 25 ms decode latency and < 15% CPU on a laptop.
2. **Virtual camera visibility:** Registering a minimal Rust `cdylib` COM `IMFMediaSource` DLL (`anyleap_vcam.dll`) and activating `MFCreateVirtualCamera` makes `"AnyLeap Camera"` appear in the **Windows Camera** app and display a synthetic `NV12` test pattern across the Session 1 ↔ Session 0 boundary.

---

### 3.3 Windows Media Foundation Virtual Camera Architecture

Per Microsoft Learn (`MFCreateVirtualCamera`, `IMFVirtualCamera`, *Writing a Custom Media Source*) and the official `microsoft/Windows-Camera` (`Samples/VirtualCamera`, `SimpleMediaSource`) reference samples:

1. **Process & Session Isolation (`FrameServer`)**
   - When an app opens `"AnyLeap Camera"`, Windows routes activation through the **Windows Camera Frame Server** service (`FrameServer`, hosted in `svchost.exe` running as **`NT AUTHORITY\LOCAL SERVICE`** in **Session 0**).
   - `FrameServer` calls `CoCreateInstance` on the `sourceId` CLSID passed to `MFCreateVirtualCamera`, loading `anyleap_vcam.dll` **in-process inside `FrameServer`** (Session 0), never inside `anyleap.exe` (interactive Session 1+).
   - Therefore, decoded `NV12` frames must cross the process and session boundary from `anyleap.exe` to `anyleap_vcam.dll` via **cross-session IPC** (a file-backed shared memory ring/triple buffer + Win32 Named Pipe / cross-session event, §4.2). Note: non-elevated Session 1+ processes lack `SeCreateGlobalPrivilege`, so creating `Global\*` kernel objects directly fails with `ERROR_ACCESS_DENIED` unless backed by an accessible file mapping or Named Pipe (§8.1).

2. **Required COM Interfaces**
   - Class factory: `IClassFactory` (`DllGetClassObject`, `DllCanUnloadNow`).
   - Media source: `IMFMediaSourceEx` (inherits `IMFMediaSource`, `IMFMediaEventGenerator`), `IMFGetService`, and `IKsControl` (minimal stub returning `E_NOTIMPL` / standard camera topology attributes where FrameServer probes).
   - Media stream: `IMFMediaStream2` (inherits `IMFMediaStream`, `IMFMediaEventGenerator`) delivering `IMFSample` buffers via `RequestSample`.
   - Stream/presentation descriptors: `IMFPresentationDescriptor`, `IMFStreamDescriptor`, `IMFMediaTypeHandler`, `IMFMediaType`.

3. **Registration, Admin Rights, and Access Scope**
   - **`MFVirtualCameraAccess_CurrentUser` vs `_AllUsers`:** `MFVirtualCameraAccess_CurrentUser` scopes enumeration/activation of the virtual camera instance to the current interactive Windows user (no UAC needed for `MFCreateVirtualCamera` itself), whereas `_AllUsers` requires administrator rights on every activation call. AnyLeap uses **`MFVirtualCameraAccess_CurrentUser`**.
   - **COM `CLSID` Registration (`HKLM` vs `HKCU`):** Even when `MFCreateVirtualCamera` uses `MFVirtualCameraAccess_CurrentUser`, `FrameServer` runs under `NT AUTHORITY\LOCAL SERVICE` in Session 0 and **cannot** resolve `HKCU\Software\Classes\CLSID` of the interactive user. The COM DLL (`InprocServer32`, `ThreadingModel = "Both"`) **must** be registered under **`HKLM\SOFTWARE\Classes\CLSID\{<CLSID>}`** and stored in a path readable/executable by `LOCAL SERVICE` and `ALL APPLICATION PACKAGES` (e.g. `%ProgramFiles%\AnyLeap\` or `%ProgramData%\AnyLeap\vcam\`). Writing `HKLM` requires one-time administrator elevation at install/enable time (or an elevated registration helper, §9).

4. **Lifetime (`_Session` vs `_System`) and AnyLeap Exit Behaviour**
   - **`MFVirtualCameraLifetime_Session`:** The virtual camera device node exists only while AnyLeap holds the live `IMFVirtualCamera` handle (or until `Shutdown`/`Stop` is called). If `anyleap.exe` crashes or exits, Windows tears down the virtual camera endpoint automatically so video-call apps do not list a permanently dead camera.
   - **`MFVirtualCameraLifetime_System`:** Persists across reboots and app restarts even when AnyLeap is closed; requires explicit `IMFVirtualCamera::Remove()` on uninstall.
   - **Decision:** Default to **`MFVirtualCameraLifetime_Session`** in Phase 1/2 (camera appears while Camera Studio or `"Keep virtual camera ready"` is active, and cleanly disappears if AnyLeap exits). If Phase 3 UX exposes an always-available idle placeholder ("AnyLeap Camera — Connect phone to start"), `anyleap.exe` in the tray keeps a session-lifetime `IMFVirtualCamera` alive while running (or optionally upgrades to `_System` only if explicitly justified later). Never leave a silent black device behind when AnyLeap is not installed/running.

5. **Supported Formats, Resolutions, and Frame Rates**
   - **Pixel format:** **`MFVideoFormat_NV12`** as the primary media type (zero-copy/native output of Windows hardware H.264 MFTs and universally accepted by `FrameServer` and all video-call and browser apps without extra color conversion), with **`MFVideoFormat_YUY2`** optionally exposed if needed by legacy consumers (Phase 0 verifies whether `NV12` alone suffices for Windows Camera + browser WebRTC).
   - **Resolutions & FPS advertised by `IMFStreamDescriptor`:** Fixed discrete media types matching Camera Studio's launcher presets (`1280x720`, `1920x1080`, `640x480`, plus portrait equivalents `720x1280` / `1080x1920` or letterboxed/scaled into the active stream type so orientation changes never break downstream apps that lock stream geometry at open time) at **30 fps** and **60 fps** (`MFVideoInterlace_Progressive`).

---

### 3.4 Rust Feasibility & Workspace Structure

- **`windows` crate (`0.61`, already pinned in `src-tauri/Cargo.toml:48-49`)** includes `Win32_Media_MediaFoundation` with bindings for `MFCreateVirtualCamera`, `IMFVirtualCamera`, `IMFMediaSource`, `IMFMediaSourceEx`, `IMFMediaStream`, `IMFMediaStream2`, `IMFActivate`, `IMFSample`, `IMFMediaBuffer`, `IMFMediaType`, `IMFStreamDescriptor`, `IMFPresentationDescriptor`, `IMFMediaEventGenerator`, `IMFTransform`, `MFTEnumEx`, and `windows_core::implement` for authoring COM servers in safe/idiomatic Rust.
- **Separate `cdylib` crate in a Cargo workspace:** `anyleap_vcam.dll` **must** be its own tiny `cdylib` crate (e.g., `crates/anyleap-vcam`) rather than sharing `src-tauri/Cargo.toml`:
  - `src-tauri` links Tauri, WebView2, `quinn`, `tokio`, `image`, etc. (`anyleap_lib` is already a Tauri `cdylib` for mobile entry points, `src-tauri/Cargo.toml:14-15`). Loading Tauri's runtime dependencies into `svchost.exe` (`FrameServer`) would bloat Session 0 memory, pull unnecessary DLLs (`AdbWinApi.dll`, etc.), and risk `DllMain` loader-lock deadlocks.
  - A standalone `crates/anyleap-vcam` crate depends **only** on `windows` + `windows-core` (and a tiny shared-header/protocol `crates/anyleap-vcam-ipc` `rlib` shared with `src-tauri`), keeping `anyleap_vcam.dll` small (~200–400 KB), dependency-free, and safe to load inside `FrameServer`.
  - Promote root `Cargo.toml` to a Cargo workspace (`members = ["src-tauri", "crates/anyleap-vcam", "crates/anyleap-vcam-ipc"]`) while keeping `src-tauri/Cargo.toml` manifest-path commands (`AGENTS.md` §2) working seamlessly.

---

## 4. Architecture

### 4.1 End-to-End Data Flow

```text
+------------------+       ADB TCP forward       +----------------------------------------+
|  Android Phone   | --------------------------> |  anyleap.exe (User Session 1+)         |
|  scrcpy-server   |  H.264 Annex-B + 12B hdr    |  1. ScrcpyCameraClient (socket)        |
|  (--video-source |                             |  2. MF H.264 MFT Decoder -> NV12       |
|   =camera)       | <-------------------------- |  3. Transform (mirror/flip/scale)      |
+------------------+   control socket (torch,    |  4. Placeholder generator (on EOF)     |
                       zoom, rotate)             |  5. IMFVirtualCamera lifecycle ctrl    |
                                                 +-------------------+--------------------+
                                                                     |
                                         File-backed Shared Frame    |  + Named Pipe /
                                         Ring Buffer (3x NV12 slots) |    Frame Event
                                                                     v
+------------------+       MF Frame Server       +----------------------------------------+
| Video-Call &     | <-------------------------- |  svchost.exe (FrameServer, Session 0)  |
| Browser Apps /   |   NV12 IMFSample stream     |  anyleap_vcam.dll (COM InprocServer32) |
| Windows Camera   |                             |  - IMFMediaSourceEx / IMFMediaStream2  |
+------------------+                             |  - Reads latest complete slot + paces  |
                                                 |    or emits placeholder on stall/EOF   |
                                                 +----------------------------------------+
```

### 4.2 Shared Frame Buffer & Synchronization

- **Triple-buffered lock-free / sequence-counter ring (`SharedFrameBuffer`):**
  - Header (`64` bytes, cache-line aligned): `magic` (`u32`), `version` (`u32`), `writer_pid` (`u32`), `state` (`u32`: `Stopped | Streaming | Placeholder | Reconnecting`), `width` (`u32`), `height` (`u32`), `stride` (`u32`), `fps_num` (`u32`), `fps_den` (`u32`), `active_slot` (`AtomicU32`), `frame_seq` (`AtomicU64`), `timestamp_100ns` (`AtomicI64`), `consumer_count` (`AtomicU32` — incremented by `IMFMediaSource::Start`, decremented by `Stop`/`Shutdown`, driving the **"camera in use"** indicator).
  - Payload: 3 contiguous `NV12` frame slots sized for max negotiated resolution (e.g., `1920x1080` NV12 = `3,110,400` bytes/slot → ~9.3 MB total, or dynamically sized on pipe handshake up to 1080p/4K cap).
- **Writer protocol (`anyleap.exe`):** Writes into `(active_slot + 1) % 3` (never the slot currently marked `active_slot` or pinned by reader), issues `Release` store on `active_slot` and `frame_seq`, and signals the frame-ready notification (or sends a 1-byte tick over the Named Pipe).
- **Reader protocol (`anyleap_vcam.dll`):** Checks `frame_seq` before and after `memcpy` into an `IMFMediaBuffer` (seqlock-style consistency check, retrying if the writer wraps). If no new frame arrives within the frame interval (e.g. phone paused or stalled), repeats the latest frame or renders the disconnect placeholder after a short grace period (`1500 ms`), ensuring `RequestSample` never stalls downstream apps.

---

## 5. Phased Delivery Plan

Each phase is independently shippable and leaves `main` green on all 5 `AGENTS.md` §2 gates.

### Phase 0: Technical Spike (Time-Boxed Verification, No Production Wiring)
- **Deliverables (spike branch / prototype binary & test DLL):**
  1. Standalone Rust CLI (`spike-decode`) that pushes `scrcpy-server 5.0.1` (`--video-source=camera`), reads the raw H.264 stream over `adb forward`, decodes via Windows Media Foundation `IMFTransform` (`CLSID_CMSH264DecoderMFT` / `MFTEnumEx`) to `NV12`, and logs decode latency, CPU%, and `MF_E_TRANSFORM_STREAM_CHANGE` handling.
  2. Minimal `cdylib` COM DLL (`anyleap_vcam.dll`) implementing `IMFMediaSourceEx` + `IMFMediaStream2` generating a static/moving color-bar `NV12` pattern, registered manually via elevated `regsvr32`, activated via `MFCreateVirtualCamera`, and verified in the **Windows Camera** app.
  3. Cross-session IPC test verifying non-elevated `anyleap.exe` (Session 1+) can share frames with `anyleap_vcam.dll` inside `FrameServer` (`LOCAL SERVICE`, Session 0) without `SeCreateGlobalPrivilege`.
- **Exit Gate:** Both proof points demonstrated on Windows 11 22000+; spike findings documented before Phase 1 starts.

### Phase 1: Static Test-Pattern Virtual Camera + Install / Uninstall
- **Scope:**
  - Promote workspace with `crates/anyleap-vcam-ipc` and `crates/anyleap-vcam` (`anyleap_vcam.dll`).
  - DLL renders a clean branded `"AnyLeap Camera"` `NV12` test card (`1280x720` @ 30 fps) when no live stream is attached.
  - Windows 11 version detection (`RtlGetVersion` / build >= `22000`) and `MFCreateVirtualCamera(MFVirtualCameraType_SoftwareCameraSource, MFVirtualCameraLifetime_Session, MFVirtualCameraAccess_CurrentUser, "AnyLeap Camera", clsid, ...)`.
  - Elevated registration helper + NSIS `hooks.nsh` install/update/uninstall integration (§9).
  - Tauri commands to register/check/start/stop the virtual camera healthily, surfaced behind a feature/diagnostic toggle or Settings status check.
- **Shippable Value:** Users on Windows 11 can install/uninstall cleanly and verify `"AnyLeap Camera"` enumerates and displays the branded test card in Windows Camera and video-call/browser apps.

### Phase 2: Live Phone Camera Frames
- **Scope:**
  - Implement `ScrcpyCameraClient` in `src-tauri/src/vcam/scrcpy_stream.rs` (pushes bundled `scrcpy-server`, opens `adb forward` video + control sockets, parses device/codec header and 12-byte frame packets).
  - Implement Media Foundation hardware H.264 decoder (`src-tauri/src/vcam/mf_decoder.rs`) outputting `NV12` into the cross-session `SharedFrameBuffer`.
  - Handle Windows 10 fallback seamlessly: on build < 22000 or when virtual-camera COM registration is missing/declined, launch the legacy `scrcpy.exe` Camera Studio window + external virtual-camera app guide unchanged.
  - Handle orientation (`0/90/180/270`) and aspect fit into the active virtual camera stream geometry (letterbox/pillarbox or dynamic renegotiation via `MEStreamFormatChanged` when stopped/restarted).
  - Preserve audio behaviour (`playAudio` in `CameraModeSettings`) and replace/supplement `send_camera_shortcut` with native `scrcpy-server` control messages when running via direct client.
- **Shippable Value:** Starting Camera Studio on Windows 11 streams live phone camera video directly into `"AnyLeap Camera"` with no external virtual-camera app required.

### Phase 3: Camera Studio UX, Controls, and Disconnect Resilience
- **Scope:**
  - **Camera Studio virtual-camera toggle & mode status** in `Settings → Phone mirroring → Camera` and read-only indicator in `CameraLaunchDialog` (§6).
  - **Resolution & FPS controls** synced cleanly with `modeDefaults.camera` / `CameraLaunchDialog` (1080p/720p/480p, 30/60 fps).
  - **Mirror / Flip horizontal** (and vertical/rotation) applied in AnyLeap's `NV12` frame staging pass (single-pass UV-interleaved horizontal flip) so self-view/whiteboard framing works for any app.
  - **"Camera in use" indicator:** `anyleap_vcam.dll` updates `consumer_count` in shared memory on `IMFMediaSource::Start` / `Stop` / `Shutdown`; AnyLeap polls/receives notification and shows a live `"Camera in use by an app"` badge in Studio Deck, Device Row, and the Camera bar.
  - **Phone disconnect resilience:** On ADB/TCP EOF or server crash, `anyleap.exe` immediately switches `state = Placeholder` and writes an `"AnyLeap Camera — Phone disconnected / Reconnecting..."` `NV12` card at 15–30 fps (never freezing on the last video frame and never emitting solid black frames). Auto-reconnects seamlessly when the phone returns over USB/Wi-Fi.
- **Shippable Value:** Complete, resilient daily-driver virtual webcam UX.

---

## 6. Settings & UI Placement (SPEC-28 Single-Home Rule)

Following `SPEC-28 §9.1` (one editor per setting; other places show read-only status + link) and `SPEC-28 §8.5` (positive, sentence-case, zero internal jargon, zero third-party product names):

### 6.1 Single Editor: `Settings → Phone mirroring → Camera` (`CameraPanel`)
Extend `CameraModeSettings` (`src/lib/modeSettings.ts:66-70`) and `REGISTRY` (`src/components/settings/registry.ts`) with:

| ID | Label | Helper Text | Control | Default |
|---|---|---|---|---|
| `M-C1` | Use as a Windows virtual camera | `Makes "AnyLeap Camera" available directly in video-call and browser apps (Windows 11).` | Switch (`virtualCameraEnabled`) | `true` (on Win 11; disabled with Windows 10 explanation on Win 10) |
| `M-C2` | Mirror camera image horizontally | `Flips the virtual camera video left-to-right.` | Switch (`mirrorHorizontal`) | `false` |
| Existing | Use the phone microphone on this PC / Bitrate / Show phone in one AnyLeap window | (Unchanged, SPEC-28) | Existing | Existing |

- Note on **Resolution & FPS**: Per `SPEC-28` (`Commands` table at `SPEC-28 §6`) and `CameraLaunchDialog.tsx:36-37`, resolution (`720p` / `1080p` / `480p`) and frame rate (`30` / `60` fps) remain per-launch choices in `CameraLaunchDialog` (or, if persisted defaults are added in Phase 3, they live **exclusively** in `CameraPanel` with `CameraLaunchDialog` pre-filling from them without writing back, preserving SPEC-28's single-home invariant).
- If `M-C1` is enabled on Windows 11 but the COM DLL is not yet registered in `HKLM`, `CameraPanel` shows an inline status banner with a **"Set up virtual camera"** button (invoking the elevated registration helper once with UAC) and never silently fails.

### 6.2 Launch Dialog, Deck, and Control Bar
- **`CameraLaunchDialog.tsx`:**
  - When Windows 11 native virtual camera is active (`virtualCameraEnabled` + registered), replaces the external-app 4-step guide (`CameraLaunchDialog.tsx:141-153`) with a concise readiness card: *"AnyLeap Camera will appear directly in your video-call and browser apps."* + a read-only link *"Virtual camera settings"* opening **Settings → Phone mirroring → Camera** (SPEC-28 §9.1).
  - On Windows 10 (or when native virtual camera is off/unavailable), keeps the existing fallback guide phrased neutrally (*"Open your external virtual-camera app..."*, removing third-party app names from `CameraLaunchDialog.tsx:145-152`).
- **`ControlBar.tsx` / `controlBarModel.ts`:**
  - Adds **Flip horizontal** (`mirrorHorizontal` toggle, `aria-pressed`) in the `camera` group alongside `torch`, `zoomIn`, `zoomOut`, `cameraHelp`, and `stopCamera`.
  - Displays a non-intrusive **"In use"** status indicator (`aria-live="polite"`) whenever `consumer_count > 0`.

### 6.3 Accessibility (SPEC-28 §10 & SPEC-26)
- All new switches use `role="switch"` with `aria-checked`, visible `<label>` (`htmlFor`), and `aria-describedby` helper text.
- Helper text (`--text-muted`, `#8B8FA3`) is placed strictly on `--ink-900` or `--ink-950` (never `--ink-800`, where contrast drops to `4.24:1` vs `4.5:1` minimum).
- The **"Camera in use"** indicator pairs color (`--status-success` / `--gold-400`) with explicit text/icon and `aria-live="polite"` so state never relies on color alone.
- `ControlBar` camera buttons use `40 x 40 px` hit targets with `aria-label`, `aria-pressed`, and focus-visible tooltips.

---

## 7. Performance Budget

Target hardware baseline: mid-range Windows 11 laptop (e.g., 4-core CPU with integrated GPU), `1280x720 @ 30 fps` default (stretch target: `1920x1080 @ 60 fps`):

| Metric | Budget (`720p30`) | Budget (`1080p60`) | Enforcement / Notes |
|---|---|---|---|
| **Glass-to-glass decode-to-FrameServer latency** | **≤ 45 ms** (decode ≤ 15 ms, copy/flip ≤ 3 ms, USB ADB + encode ~20–25 ms) | **≤ 55 ms** | Zero container muxing; direct MFT hardware decode; triple-buffer lock-free write. |
| **Total AnyLeap + FrameServer CPU** | **≤ 8%** of one logical core | **≤ 15%** of one logical core | Hardware H.264 MFT (`NV12` output); zero pixel-by-pixel scalar Rust loops when mirror/flip is off (`memcpy` per plane); SIMD/row-chunked swap when mirror is on. |
| **Additional RAM** | **≤ 40 MB** | **≤ 80 MB** | Triple-buffer `NV12` (1.38 MB/slot at 720p = 4.1 MB; 3.11 MB/slot at 1080p = 9.3 MB) + MFT surfaces. |
| **Idle CPU (no consumer / placeholder)** | **< 0.5%** | **< 0.5%** | When `consumer_count == 0`, skip `NV12` staging writes if preview is also off; placeholder renders on a low-cadence timer (15 fps static blit). |

---

## 8. Security & Isolation

1. **Shared Memory & Pipe ACLs (Session 1+ ↔ Session 0)**
   - Because `FrameServer` runs as `NT AUTHORITY\LOCAL SERVICE` (Session 0) while `anyleap.exe` runs as the interactive user (Session 1+), the shared transport **must** use an explicit `SECURITY_DESCRIPTOR` (SDDL / `EXPLICIT_ACCESS_W`) that grants:
     - **Current user's Logon SID (or User SID):** `GENERIC_READ | GENERIC_WRITE`.
     - **`NT AUTHORITY\LOCAL SERVICE` (`S-1-5-19`):** `FILE_MAP_READ` (read-only on shared frame memory!) + pipe/event read/wait access. `FrameServer` **never** needs write access to pixel buffers; only `consumer_count` (or a small separate `LOCAL SERVICE`-writable/pipe-reported telemetry channel) moves back to `anyleap.exe`.
     - **`ALL APPLICATION PACKAGES` (`S-1-15-2-1`):** Granted `RX` on `anyleap_vcam.dll` (and read on shared resources **only if** testing shows a packaged/AppContainer consumer loads the media source out-of-FrameServer; otherwise keep AppContainer denied on the IPC object so only `FrameServer` + current user touch it).
     - **Deny/omit `Everyone` and `Users` write access** so another local user on a shared PC cannot inject fake frames or squat on the named object (`FILE_FLAG_FIRST_PIPE_INSTANCE` + per-user-SID + random per-session token passed via `IMFVirtualCamera::AddProperty` / `IMFAttributes`).
2. **COM DLL Surface (`anyleap_vcam.dll` in `FrameServer`)**
   - Installed into a root/Admin-protected directory (`%ProgramFiles%\AnyLeap\vcam\` or `%ProgramData%\AnyLeap\vcam\` with ACL: `SYSTEM`/`Administrators` full control, `LOCAL SERVICE` + `Users` + `ALL APPLICATION PACKAGES` read & execute only — never writable by non-admin users, preventing DLL-hijack privilege escalation into `FrameServer`).
   - `anyleap_vcam.dll` strictly validates all shared-memory header fields (`width <= 3840`, `height <= 2160`, `stride >= width`, `active_slot < 3`, bounds-checked slice offsets) before reading pixels so a corrupt/malicious writer cannot trigger an out-of-bounds read inside `FrameServer`.
   - Zero `unsafe` unvalidated pointer arithmetic; fuzz/unit-test header parser in `anyleap-vcam-ipc`.
3. **Privacy**
   - Phone camera frames are never written to disk (`%LOCALAPPDATA%` / logs) unless the user explicitly triggers recording/screenshot.
   - `FrameServer` respects Windows Camera Privacy settings (`E_ACCESSDENIED` handled gracefully with an actionable message directing the user to *Windows Settings → Privacy & security → Camera*).

---

## 9. Installer & Updater Impact (`#111` Hook Pattern)

AnyLeap uses Tauri's NSIS installer with custom hooks in `src-tauri/nsis/hooks.nsh:3-43` (`src-tauri/tauri.conf.json:36`). Commit `11ec44f` (`#111`) established the rule that `NSIS_HOOK_PREINSTALL` and `NSIS_HOOK_PREUNINSTALL` must stop `anyleap.exe` **first** (without `/T`, because the updater may have launched the installer), then terminate `adb.exe` and `scrcpy.exe`, before overwriting or deleting bundled binaries/DLLs.

With `anyleap_vcam.dll`, two new constraints apply:

1. **Privilege Boundary (`HKLM` + `LOCAL SERVICE`-readable path)**
   - Today's NSIS bundle installs `anyleap.exe` per-user (`%LOCALAPPDATA%\AnyLeap`) without UAC.
   - `anyleap_vcam.dll` requires `HKLM\SOFTWARE\Classes\CLSID\{...}` and a `LOCAL SERVICE`-readable path (`%ProgramFiles%\AnyLeap\vcam\anyleap_vcam.dll` or `%ProgramData%\AnyLeap\vcam\`).
   - **Recommended split:** Keep `anyleap.exe` per-user for seamless background updates, and ship a tiny signed/bundled registration helper (or `regsvr32` invocation) that runs **only** on explicit user opt-in ("Set up virtual camera" UAC prompt) or when an updated `anyleap_vcam.dll` version differs from the registered version, plus a best-effort cleanup in `NSIS_HOOK_POSTUNINSTALL` (invoking elevated unregister only if the `HKLM` CLSID key exists).
2. **`FrameServer` DLL Locking During Update / Uninstall**
   - Unlike `adb.exe` / `scrcpy.exe`, AnyLeap **must not** `taskkill` `svchost.exe` (`FrameServer`).
   - Stopping `anyleap.exe` first in `NSIS_HOOK_PREINSTALL` / `PREUNINSTALL` tears down any active `MFVirtualCameraLifetime_Session` camera, allowing `FrameServer` to release `anyleap_vcam.dll`.
   - If `FrameServer` (or a third-party app) still holds `anyleap_vcam.dll` mapped during an update:
     - **Side-by-side / rename-swap pattern:** Install the new DLL with a versioned filename (or rename the locked `anyleap_vcam.dll` → `anyleap_vcam.old` — Windows permits renaming a loaded DLL — write the new `anyleap_vcam.dll`, update `InprocServer32` if needed, and schedule `/REBOOTOK` / next-launch cleanup of `*.old`). Because `anyleap-vcam-ipc` uses a versioned wire header, an updated `anyleap.exe` remains clean even across a pending unload.

---

## 10. Test Plan

### 10.1 Automated Tests (`cargo test` + `npm test`)
- **`crates/anyleap-vcam-ipc` (`cargo test`):**
  - Header validation: rejects out-of-range `width`/`height`, misaligned/short `stride`, invalid `slot >= 3`, bad magic/version.
  - Triple-buffer seqlock read/write under simulated concurrent threads + horizontal mirror `NV12` transform (verifying Y-plane byte reversal and UV-plane 2-byte pair reversal).
  - Placeholder `NV12` frame generator dimensions and non-zero/non-black luma bounds.
- **`src-tauri` (`cargo test`):**
  - `scrcpy-server` 5.0.1 wire header & 12-byte packet framing parser (`SC_PACKET_FLAG_CONFIG`, `SC_PACKET_FLAG_KEYFRAME`, PTS extraction, EOF disconnect detection).
  - Windows version gate (`is_mf_vcam_supported`) and fallback selection logic (Win10 → external-app path; Win11 + unregistered → guided setup or fallback).
  - `CameraSettings` / `build_scrcpy_camera_args` tests remain green for Windows 10 fallback.
- **Frontend (`npm test`):**
  - `modeSettings.test.ts`: `CameraModeSettings` normalization/migration with `virtualCameraEnabled` and `mirrorHorizontal` defaults.
  - `SettingsPanel.test.tsx` & `settingsFilter.test.ts`: `M-C1`, `M-C2` appear only on Camera tab, obey SPEC-28 single-home & contrast rules, and filter by `"virtual camera"`, `"mirror"`, `"flip"`.
  - `CameraLaunchDialog.test.tsx`: displays native readiness card on Win11-registered vs external-app fallback card on Win10; `"Virtual camera settings"` link navigates to Settings → Phone mirroring → Camera.
  - `ControlBar.test.tsx` & `controlBarModel.test.ts`: `flip` toggle `aria-pressed` and `"In use"` live indicator.
  - `noThirdPartyNames.test.ts`: passes across `src/**`, `docs/specs/SPEC-32*`, `docs/adr/0006*`.

### 10.2 Human / Hardware Verification Checklist (Cannot Be Automated in CI)
1. **Windows 11 Build 22000+ Live Device Test:** Connect Android 12+ phone via USB and Wi-Fi, launch Camera Studio, open **Windows Camera** app, select `"AnyLeap Camera"`, verify crisp 720p30 / 1080p60 video and latency feel.
2. **Video-Call & Browser App Matrix:** Verify `"AnyLeap Camera"` enumerates and streams in at least two desktop video-call apps and two browser WebRTC camera pickers, including toggling horizontal mirror and resolution/FPS.
3. **In-Use & Disconnect Resilience:** Open `"AnyLeap Camera"` in Windows Camera → confirm AnyLeap UI displays **"Camera in use"**; unplug USB / turn off Wi-Fi → confirm Windows Camera immediately shows AnyLeap's placeholder frame (not frozen, not black), and resumes live video on reconnect.
4. **Windows 10 Fallback Verification:** Verify on a Windows 10 host (or simulated pre-22000 build) that Camera Studio falls back cleanly to the existing `scrcpy` window flow with zero errors.
5. **Installer / Auto-Updater Upgrade Test:** Install vN, start & stop `"AnyLeap Camera"`, run the vN+1 NSIS updater while `FrameServer` has recently loaded `anyleap_vcam.dll`, verify clean file replacement without reboot errors, then uninstall and verify `HKLM\SOFTWARE\Classes\CLSID\{...}` and DLL files are removed.

---

## 11. Open Questions

1. **Optimal cross-session transport (Phase 0 check):** *Partially answered by Task 0.2 spike (§11.1).* File-backed unnamed `CreateFileMappingW` in `%ProgramData%` avoids `SeCreateGlobalPrivilege` (`ERROR_ACCESS_DENIED (5)` on `Global\` sections) and `CreateEventW("Global\\...")` succeeded non-elevated in Session 1; cross-session `FrameServer` (`LOCAL SERVICE`, Session 0) `OpenEventW` / `CreateFileW` access still requires elevated `register.ps1` verification.
2. **H.265 / HEVC fallback:** Many Windows 11 laptops ship only the inbox H.264 decoder MFT unless the HEVC Video Extension is installed. Should Camera Studio force `video_codec=h264` for the native virtual-camera path (recommended for universal reliability) or probe HEVC MFT availability first?
3. **Preview window vs headless:** When `virtualCameraEnabled` is on, should Camera Studio default to headless (zero `SDL_app` window; preview rendered inside AnyLeap's Camera dialog/tab only when open) to save GPU/CPU, or keep an optional embedded preview window?
4. **Installer elevation UX:** Confirm whether UAC registration should happen once inside the NSIS installer (requiring elevation on install) or lazily on first Camera Studio / Settings opt-in via an elevated helper binary (keeping silent per-user `anyleap.exe` auto-updates intact).

### 11.1 Phase 0 Spike Findings (Task 0.2 — Virtual Camera Source, `crates/spike-vcam-source`)

Tested on **Windows 11 Build 26100** (`10.0.26100.0`, non-elevated interactive user in Session 1 without `SeCreateGlobalPrivilege`). **Status: Partial** — the in-process COM source, file-backed SHM ring, DACLs, and `MFCreateVirtualCamera` call were verified; live `FrameServer` (Session 0) activation, `MFEnumDeviceSources` visibility, and Windows Camera rendering remain blocked until `crates/spike-vcam-source/register.ps1` is run once from an elevated prompt (the orchestration session ended before admin registration ran).

- **What worked:**
  - Standalone Rust `cdylib` COM server (`anyleap_vcam_spike.dll`, 246,272 bytes in release, isolated `[workspace]` in `crates/spike-vcam-source/` with `windows` 0.61.3) implementing `DllGetClassObject`, `DllCanUnloadNow`, `IClassFactory`, `IMFMediaSourceEx` / `IMFMediaSource` / `IMFMediaEventGenerator`, `IMFGetService`, `IKsControl`, and `IMFMediaStream2` / `IMFMediaStream` for `1280x720 @ 30 fps` `MFVideoFormat_NV12` (`1,382,400` bytes/frame).
  - Non-elevated `vcam-host` (`--local-probe`) loaded `anyleap_vcam_spike.dll` via `LoadLibraryW` + `DllGetClassObject`, activated `IMFMediaSource`, fed 1280x720 NV12 moving color bars + 6-digit frame counter through the triple-buffered `SharedFrameHeader` seqlock (`C:\ProgramData\AnyLeapSpike\frames.shm`), and read **6/6** valid `IMFSample` buffers via `MFCreateSourceReaderFromMediaSource` (3 unit tests in `ipc.rs` also passed: SID validation, pattern motion, seqlock round-trip + `active_slot >= 3` bounds rejection).
  - `MFCreateVirtualCamera(MFVirtualCameraType_SoftwareCameraSource, MFVirtualCameraLifetime_Session, MFVirtualCameraAccess_CurrentUser, "AnyLeap Camera (spike)", "{A7E4B91C-5D2A-4F8E-9C31-7B2E4D1F8A01}", Some(&[KSCATEGORY_VIDEO_CAMERA]))` succeeded non-elevated in returning an `IMFVirtualCamera` handle.
- **Exact registration requirements:**
  - `IMFVirtualCamera::Start()` failed with `REGDB_E_CLASSNOTREG` (`0x80040154`, "Class not registered") when the CLSID was absent from `HKLM`, confirming `FrameServer` requires `HKLM\SOFTWARE\Classes\CLSID\{<CLSID>}\InprocServer32` (`(Default)` = DLL path, `ThreadingModel = "Both"`).
  - Default `C:\ProgramData` ACLs (`BUILTIN\Users:(OI)(CI)(RX)`, `NT AUTHORITY\SYSTEM:(F)`, `BUILTIN\Administrators:(F)`) do **not** include an explicit `NT AUTHORITY\LOCAL SERVICE` (`S-1-5-19`) or `ALL APPLICATION PACKAGES` (`S-1-15-2-1`) ACE on newly created subfolders/copied DLLs; `register.ps1` therefore explicitly grants `*S-1-5-19:(OI)(CI)(RX)` and `*S-1-15-2-1:(OI)(CI)(RX)` on `C:\ProgramData\AnyLeapSpike`.
  - Cleanup script `crates/spike-vcam-source/unregister.ps1` removes only `HKLM\SOFTWARE\Classes\CLSID\{A7E4B91C-5D2A-4F8E-9C31-7B2E4D1F8A01}`; nothing was written to `HKLM` during the spike.
- **DACL used (SDDL via `ConvertStringSecurityDescriptorToSecurityDescriptorW`):**
  - Backing file (`C:\ProgramData\AnyLeapSpike\frames.shm`) & unnamed `CreateFileMappingW` section:
    `D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;<USER_SID>)(A;;FR;;;S-1-5-19)` — grants `LOCAL SERVICE` read-only (`FILE_GENERIC_READ`) and full access to `SYSTEM`, `Administrators`, and the current user SID.
  - Global notification event (`Global\AnyLeapSpikeFrameEvent`):
    `D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;S-1-5-19)(A;;0x001F0003;;;<USER_SID>)` — grants `LOCAL SERVICE` full access and the current user `EVENT_ALL_ACCESS` (`0x001F0003`), avoiding `Everyone`/`Users`.
  - Diagnostic DLL log file (`C:\ProgramData\AnyLeapSpike\vcam_dll.log`): pre-created by `vcam-host` with `(A;;GA;;;S-1-5-19)` so `FrameServer` can append without needing directory write access.
- **FrameServer process and session observations:**
  - `FrameServer` service on the test machine (`10.0.26100.0`) is `Running` (`StartType: Manual`).
  - Live Session 0 (`LOCAL SERVICE`) loading of `anyleap_vcam_spike.dll`, `MFEnumDeviceSources` enumeration of `"AnyLeap Camera (spike)"`, and cross-session `LOCAL SERVICE` access to `frames.shm` / `Global\AnyLeapSpikeFrameEvent` remain **to be verified** after running `register.ps1` as Administrator and executing `vcam-host.exe`.
- **Latency observed (in-process `--local-probe`, Session 1):**
  - `ShmWriter::write_frame` → `Global\AnyLeapSpikeFrameEvent` → `ShmReader::read_latest` memcpy latency measured **`279–1676 µs` (`0.28–1.68 ms`)** for 1,382,400-byte `1280x720` NV12 frames (`ReadSample` end-to-end cadence `1.06–46.54 ms`, well within SPEC-32's `≤ 3 ms` staging budget). Cross-session `FrameServer` IPC latency still needs measurement after `register.ps1`.
- **Pitfalls hit:**
  1. **`Global\` section vs `Global\` event privilege asymmetry:** Without `SeCreateGlobalPrivilege` in non-elevated Session 1, `CreateFileMappingW(INVALID_HANDLE_VALUE, ..., "Global\\...")` failed with `ERROR_ACCESS_DENIED (5)` as expected, whereas `CreateEventW(..., "Global\\...")` **succeeded**. Using an unnamed file-backed `CreateFileMappingW(hFile, ..., NULL)` on `C:\ProgramData\AnyLeapSpike\frames.shm` avoids `SeCreateGlobalPrivilege` cleanly.
  2. **COM `#[implement]` interface casting & thread safety in `windows` 0.61.3:** `IMFMediaEventQueue` and `IUnknown` tokens wrap `NonNull<c_void>` (`!Send`/`!Sync`), requiring an explicit `SendCom` wrapper for the stream worker thread, and `IMFGetService::GetService` requires `ComObjectInterface::<IKsControl>::as_interface_ref(self)` rather than `self.to_interface()`.
  3. **Pre-registration vs runtime failure boundary:** `MFCreateVirtualCamera` itself succeeds without `HKLM` registration, deferring `REGDB_E_CLASSNOTREG (0x80040154)` until `IMFVirtualCamera::Start()`; Phase 1 production code must probe `HKLM\SOFTWARE\Classes\CLSID\{...}` up front so Settings can surface `"Set up virtual camera"` before attempting `Start()`.
