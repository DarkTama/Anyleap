# SPEC-15: Driverless Native Virtual Camera (MediaSource COM Filter)

> **Status:** Planned (Phase 4) | Target Version: `v1.3.0`
> Technical Blueprint: Windows Media Foundation Virtual Camera Activation and Custom MediaSource Filter

## 1. Overview
Upgrade AnyLeap Camera Studio from requiring an OBS Virtual Camera workaround to supporting native Windows virtual webcam registration directly inside Windows Media Foundation (WMF).

Enables applications like Google Meet, Zoom, Microsoft Teams, and Discord to see "AnyLeap Virtual Camera" in their native settings dropdown without third-party software.

## 2. Technical Architecture

```
[ Android Camera / Remote Phone ]
                │  (H.264 / scrcpy Camera Stream)
                ▼
[ AnyLeap Desktop Host ]
                │  (Decodes to NV12 / Shared Memory Ring Buffer)
                ▼
[ Named Shared Memory: Local\AnyLeapCameraBuffer ]
                │
                ▼
[ AnyLeapMediaSource.dll (In-Proc COM Server) ]
                │  Implements:
                │  - IMFMediaSource
                │  - IMFMediaStream
                │  - IMFGetService
                ▼
[ Windows Media Foundation Subsystem ]
                │  MFCreateVirtualCamera
                ▼
[ Video Conferencing Apps (Meet / Zoom / Teams / Discord) ]
```

## 3. Deep Implementation Details

### 3.1 COM MediaSource Implementation (`src-tauri/native/AnyLeapMediaSource/`)
- Implements COM interfaces:
  - `IMFMediaSource`: Manages stream descriptors, presentation clocks, start/stop commands (`IMFMediaSource::Start`, `IMFMediaSource::Stop`).
  - `IMFMediaStream`: Supplies media samples via `IMFMediaStream::RequestSample` when downstream apps pull frames.
  - `IMFQualityAdvise`: Handles frame rate throttling and quality adjustments.
- **Frame Transport**:
  - Uses Windows Named Shared Memory (`CreateFileMappingW` / `MapViewOfFile`) + Win32 Mutex/Event (`CreateEventW`) for ultra-low latency frame passing from the main Tauri app process to the in-process MediaSource COM DLL loaded by conferencing applications.
  - Zero network overhead, zero IPC socket serialization.

### 3.2 Registration & Virtual Camera Activation
- **Windows 11 / Windows 10 (21H2+) API**:
  - Calls `MFCreateVirtualCamera(MFVirtualCameraType_SoftwareCameraByMediaSource, MFVirtualCameraLifetime_Session, MFVirtualCameraAccess_CurrentUser, L"AnyLeap Virtual Camera", clsid_string, NULL, &virtual_camera)`.
  - Calls `IMFVirtualCamera::Start()` on camera start.
  - Calls `IMFVirtualCamera::Stop()` and releases interface when camera session ends.
- **User-Level Isolation**:
  - Registers under `MFVirtualCameraAccess_CurrentUser`, avoiding administrative UAC prompts during standard runtime use.

### 3.3 Graceful Fallback
- If Windows Media Foundation virtual camera activation fails (e.g. older Windows 10 versions lacking `MFCreateVirtualCamera` or COM activation blocked by group policy):
  - AnyLeap automatically switches to the existing Window Capture mode and displays the OBS Virtual Camera setup instructions.
