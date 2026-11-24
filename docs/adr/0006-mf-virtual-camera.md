# ADR 0006: Native Windows 11 Virtual Camera via Media Foundation

## Status
Proposed (accompanies SPEC-32; supersedes draft SPEC-15)

## Context
AnyLeap's Camera Studio currently spawns `scrcpy --video-source=camera` (`src-tauri/src/commands.rs:419-477`) to render the phone's camera sensor into a desktop `SDL_app` window. Video-call and browser apps cannot consume this feed natively; users must open an external virtual-camera app and window-capture `AnyLeap Camera Studio` (`src/components/CameraLaunchDialog.tsx:141-153`).

Bundled `scrcpy 5.0.1` has no Windows virtual-camera sink (`--v4l2-sink` is Linux-only). Exposing a native virtual webcam on Windows introduces a new OS subsystem and a machine-wide COM registration (`HKLM\SOFTWARE\Classes\CLSID\...`) loaded into the Windows Camera Frame Server service (`FrameServer`), which is hard to reverse once installed across user machines and directly impacts auto-updates (`src-tauri/nsis/hooks.nsh`).

## Decision
1. **Use Windows 11 Media Foundation Virtual Camera (`MFCreateVirtualCamera`, build 22000+)** with `MFVirtualCameraAccess_CurrentUser` and `MFVirtualCameraLifetime_Session`.
   - Windows 10 (and systems where registration is declined/blocked) keeps the existing `scrcpy` window + external virtual-camera app flow as a graceful fallback.
   - No kernel driver (AVStream/UMDF) and no DirectShow filter are built.
2. **Isolate the COM Media Source into a separate Cargo workspace `cdylib` crate (`crates/anyleap-vcam` → `anyleap_vcam.dll`)** implementing `IMFMediaSourceEx` and `IMFMediaStream2` (`MFVideoFormat_NV12`), sharing a small `crates/anyleap-vcam-ipc` protocol crate with `src-tauri`.
   - Register the COM CLSID under `HKLM\SOFTWARE\Classes\CLSID\{<CLSID>}\InprocServer32` (`ThreadingModel = "Both"`) pointing to a `LOCAL SERVICE`-readable path, because `FrameServer` runs in Session 0 under `NT AUTHORITY\LOCAL SERVICE` and cannot load `HKCU` COM servers or user-profile-only paths.
   - Keep `anyleap.exe` per-user for seamless auto-updates; invoke an elevated helper only when registering/updating/unregistering `anyleap_vcam.dll`, and handle loaded-DLL replacement via stop-first + rename-swap (`#111` hook pattern).
3. **Decode raw frames in AnyLeap via a direct `scrcpy-server` Rust socket client + Windows Media Foundation hardware H.264 MFT decoder** (Option b), transferring `NV12` frames to `anyleap_vcam.dll` over an ACL-hardened cross-session shared frame ring buffer + Named Pipe/event.
   - Virtual microphone is deferred to a future spec.

## Alternatives considered
- **DirectShow source filter (`regsvr32` legacy COM filter):** Deprecated by modern Windows 11 FrameServer/UWP/browser capture pipelines, fails in many sandboxed/packaged apps, and offers no `MFCreateVirtualCamera` lifecycle management.
- **Kernel/UMDF virtual camera driver (AVStream / IddCx-style driver package):** Requires WHQL/EV kernel code signing, driver store deployment, and high maintenance risk for a user-mode phone bridge.
- **`scrcpy --record` to a named pipe + bundled FFmpeg DLLs (`avcodec-63.dll`):** Adds a separate `scrcpy.exe` process, MKV mux/demux buffering latency, and FFI coupling to FFmpeg DLL majors that rotate across `fetch-binaries.ps1` bumps.
- **Window Capture (`Windows.Graphics.Capture`) of `SDL_app`:** Breaks when the preview window is minimized/occluded and incurs DWM + BGRA→NV12 overhead.
- **Compiling the COM Media Source inside `src-tauri`'s `cdylib`:** Would load Tauri/WebView2/Tokio/Quinn dependencies into Session 0 `FrameServer` (`svchost.exe`).

## Consequences
### Positive
- Native `"AnyLeap Camera"` support in Windows Camera and video-call/browser apps on Windows 11 with zero external virtual-camera app and zero kernel driver signing.
- Headless capture works even when AnyLeap is minimized to the tray, with immediate phone-disconnect placeholder frames and an accurate "camera in use" indicator.
- Direct `scrcpy-server` socket + MF hardware decode minimizes latency and avoids `avcodec` ABI drift.

### Negative / Risks
- `FrameServer` requires `HKLM` CLSID registration and `LOCAL SERVICE` file/IPC ACLs, necessitating a one-time UAC elevation step during setup or first enablement.
- Loaded `anyleap_vcam.dll` inside `FrameServer` complicates in-place updates and uninstalls unless `hooks.nsh` follows strict stop-and-swap / versioned-DLL hygiene.
- Implementing the `scrcpy-server 5.0.1` wire framing in Rust couples that client to the pinned `scrcpy-server` version in `scripts/fetch-binaries.ps1`.
