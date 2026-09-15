# Design Specification: Android Camera as PC Stream & Studio Controls (Sub-project 1)

## Status
Approved

## Context & Problem
Issue #1 requests using Android devices as high-fidelity cameras on PC.
Currently, AnyLeap only mirrors device screens. Users need the ability to directly capture raw camera sensor feeds via scrcpy's native camera mode (`--video-source=camera`), configure camera parameters (front/back lens, resolution, fps, torch), and operate camera controls during the active session.

## Scope
This specification covers Sub-project 1:
- Scrcpy native camera source capture (`--video-source=camera`).
- Camera listing and discovery (`--list-cameras`).
- User interface for camera configuration and launch (`CameraLaunchDialog`).
- In-session camera controls in `ControlBar` (torch toggle, zoom, flip lens).
- Tracking session mode (`display` vs `camera`).

*(Sub-project 2: Virtual Camera DirectShow/v4l2 bridging will be specified and implemented in a subsequent phase).*

## Architecture & Data Flow

### 1. Data Model (`src/lib/types.ts` & `src-tauri/src/commands.rs`)

```typescript
export interface CameraDeviceOption {
  id: string;
  facing: "back" | "front" | "external" | "unknown";
  resolution: string; // e.g. "4096x3072"
  fps: number[];      // e.g. [10, 15, 20, 24, 30]
  zoomRange?: [number, number]; // e.g. [1, 15]
}

export interface CameraSettings {
  facing: "back" | "front" | "external";
  cameraId?: string;
  size?: string;       // e.g. "1920x1080", "1280x720", or "" (sensor native)
  fps?: number;        // e.g. 30, 60, 0 (default)
  highSpeed?: boolean; // --camera-high-speed
  torch?: boolean;     // --camera-torch
  noAudio?: boolean;   // default false (mic on)
}

export type SessionMode = "display" | "camera";

export interface SessionInfo {
  id: string;
  serial: string;
  pid: number;
  started_at: number;
  mode: SessionMode;
}
```

### 2. Backend Commands (`src-tauri/src/commands.rs`)

- `list_device_cameras(app, serial: String) -> Result<Vec<CameraDeviceOption>, String>`:
  Executes `scrcpy --serial <serial> --list-cameras` using the bundled `scrcpy` and `adb` binaries. Parses output matching patterns like:
  `--camera-id=0    (back, 4096x3072, fps={10, 15, 20, 24, 30}, zoom-range=[1, 15])`

- `start_camera_mirror(app, serial: String, settings: CameraSettings) -> Result<SessionInfo, String>`:
  Builds arguments:
  - `--serial <serial>`
  - `--video-source=camera`
  - `--camera-facing=<facing>` (or `--camera-id=<id>` if specified)
  - Optional `--camera-size=<size>`
  - Optional `--camera-fps=<fps>`
  - Optional `--camera-high-speed`
  - Optional `--camera-torch`
  - Optional `--no-audio` (when unchecked)
  - Window title: `AnyLeap Camera — <serial>`
  Registers session with `mode: "camera"`.

- `send_camera_shortcut(app, serial: String, action: CameraAction) -> Result<(), String>`:
  Windows input injection sending shortcuts to the window titled `AnyLeap Camera — <serial>`:
  - `TorchOn`: `Alt + T`
  - `TorchOff`: `Alt + Shift + T`
  - `ZoomIn`: `Alt + Up`
  - `ZoomOut`: `Alt + Down`

### 3. Frontend UI Components

1. **`src/components/CameraLaunchDialog.tsx`**:
   - Modal launched from `DeviceRow`.
   - On open, invokes `list_device_cameras(serial)`. If fails or empty, provides standard fallbacks: Back camera and Front camera.
   - Presets for resolution: 1080p (1920x1080), 720p (1280x720), or Sensor Native.
   - FPS selection: 30 fps, 60 fps (if supported) or auto.
   - Checkboxes: Flash / Torch initially on, Enable Microphone audio (default checked).
   - "Start Camera" button invoking `startCameraMirror`.

2. **`src/components/DeviceRow.tsx`**:
   - Camera button with `Camera` icon next to the "Mirror" button.
   - Triggers `CameraLaunchDialog` when clicked.
   - Shows "Camera" status badge when camera session is active.

3. **`src/components/ControlBar.tsx`**:
   - Adapts to session type:
     - When session `mode === "camera"`:
       - Torch Toggle (`Alt+T` / `Alt+Shift+T`).
       - Zoom In (`Alt+Up`) & Zoom Out (`Alt+Down`).
       - Flip Camera (stops session and restarts with flipped lens).
       - Stop Session button.
     - Hides touch navigation, swipe scroll, and flex display controls.

## Error Handling
- Android < 12 will fail scrcpy with an error message mentioning `Camera mirroring requires Android 12+`. Scrcpy error summarizer will display this error cleanly to the user.
- If scrcpy fails to query `--list-cameras`, the UI falls back gracefully to standard `--camera-facing=back` and `--camera-facing=front` selectors.

## Verification & Testing
1. Unit test Rust parser for `list_cameras` stdout.
2. Verify camera launch on connected device (`TECNO_CM7`).
3. Verify torch toggle and zoom shortcuts via ControlBar.
4. Verify stopping camera session leaves device and scrcpy in clean state.
