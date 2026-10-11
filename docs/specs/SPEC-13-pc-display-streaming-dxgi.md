# SPEC-13: PC Display Streaming & Second Screen (DXGI + Media Foundation)

> **Status:** Planned (Phase 2) | Target Version: `v1.1.0`
> Technical Blueprint: Native Windows Desktop Duplication and Hardware MFT Pipeline

## 1. Overview
Extend AnyLeap's workstation engine to capture PC monitors at 60 FPS with minimal latency (< 20ms) using GPU hardware acceleration, and stream video to paired remote devices (tablets, phones, secondary PCs) for display mirroring and monitor extension.

## 2. Technical Pipeline

```
[ DXGI Desktop Duplication ]
       │  (VRAM D3D11 Texture2D)
       ▼
[ Media Foundation Transform (MFT) ]
       │  (Hardware NVENC / QuickSync / AMF H.264/HEVC)
       ▼
[ Annex-B NAL Packetizer ]
       │  (Segmented Video Frames)
       ▼
[ Axum WebSocket /video_streamer/windows ]
       │  (Binary Socket Stream)
       ▼
[ Remote Client Decoder / Canvas Player ]
```

## 3. Deep Implementation Details

### 3.1 DXGI Frame Acquisition (`src-tauri/src/display/dxgi_capture.rs`)
- **Factory & Device**:
  - Initializes `D3D11CreateDevice` with `D3D11_CREATE_DEVICE_BGRA_SUPPORT`.
  - Queries `IDXGIDevice` -> `IDXGIAdapter` -> `IDXGIOutput` -> `IDXGIOutput1`.
- **Duplication Loop**:
  - Calls `IDXGIOutput1::DuplicateOutput` to obtain `IDXGIOutputDuplication`.
  - Invokes `AcquireNextFrame(timeout_ms, &mut frame_info, &mut desktop_resource)`.
  - If `DXGI_ERROR_ACCESS_LOST` (screen resolution change or UAC secure desktop switch), releases interface and re-initializes duplication loop.
  - Copies surface to staging texture or passes directly as hardware D3D11 input to encoder.
  - Calls `ReleaseFrame` immediately after capture to prevent desktop rendering stalls.

### 3.2 Hardware Video Encoder (`src-tauri/src/display/mf_encoder.rs`)
- **MFT Initialization**:
  - Initializes COM with `MFStartup(MF_VERSION, MFSTARTUP_NOSOCKET)`.
  - Enumerates hardware encoders via `MFTEnumEx(MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_FLAG_HARDWARE)`.
  - Configures input type: `MFVideoFormat_NV12` or `MFVideoFormat_ARGB32`.
  - Configures output type: `MFVideoFormat_H264` (Baseline/High Profile) or `MFVideoFormat_HEVC`.
- **Low Latency Codec Parameters**:
  - `CODECAPI_AVLowLatencyMode` = `VARIANT_TRUE`.
  - `CODECAPI_AVEncCommonRateControlMode` = `eAVEncCommonRateControlMode_CBR`.
  - `CODECAPI_AVEncCommonQualityVsSpeed` = `100` (Fastest encoding).
  - `CODECAPI_AVEncAdaptiveGOP` = `0` (Fixed GOP length, 60 frames max between keyframes).
- **IDR Recovery Request**:
  - When receiver reports frame drop / decode failure, triggers `CODECAPI_AVEncVideoForceKeyFrame` to emit instant I-frame.

### 3.3 Streaming Protocol
- Emits video packets to `/video_streamer/windows`:
  - Packet Header: `[1 byte frame_type (0=Key, 1=Delta)] [8 byte timestamp_us] [4 byte payload_len] [payload bytes]`.

### 3.4 Second Screen Virtual Display Driver (IddCx)
- Installs optional Indirect Display Driver (IddCx v1.4+) to instantiate headless virtual monitors for "Extended Desktop" mode.
- If uninstalled, system gracefully operates in "Mirror Desktop" mode using existing physical outputs.
