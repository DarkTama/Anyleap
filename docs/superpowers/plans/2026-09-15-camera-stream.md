# Android Camera Stream & Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Allow users to capture raw Android camera video using scrcpy (`--video-source=camera`), configure camera parameters, and trigger camera controls (torch toggle, zoom, flip) in AnyLeap.

**Architecture:** Extend backend IPC in Tauri/Rust (`commands.rs`) to query camera device lists, build camera-specific scrcpy arguments, and send Windows keystrokes to the scrcpy camera window. Update React frontend with camera types, a dedicated `CameraLaunchDialog`, a camera trigger button in `DeviceRow`, and adaptive camera tools in `ControlBar`.

**Tech Stack:** Rust (Tauri 2, windows-rs crate), TypeScript, React 19, Tailwind CSS, Lucide React icons.

**Spec:** `docs/superpowers/specs/2026-09-15-camera-stream-design.md`

## Global Constraints
- Pinned `scrcpy` 4.0 binary and `scrcpy-server` bundle must be used.
- scrcpy `--video-source=camera` requires Android 12+; errors must be gracefully propagated and summarized.
- Single source of truth for scrcpy flags remains in Rust (`commands.rs`), not injected raw from TypeScript.
- No regression in existing display mirroring or flex-mode features.

---

### Task 1: Camera Data Model & Backend Arguments Builder

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src/lib/types.ts`
- Test: `src-tauri/src/commands.rs` (unit tests in `mod tests`)

**Interfaces:**
- Consumes: Existing `spawn_session`, `AppState`, `Session`
- Produces:
  - Rust struct `CameraSettings { facing: String, camera_id: Option<String>, size: Option<String>, fps: Option<u32>, high_speed: bool, torch: bool, no_audio: bool }`
  - Rust struct `CameraDeviceOption { id: String, facing: String, resolution: String, fps: Vec<u32>, zoom_range: Option<(f32, f32)> }`
  - Rust function `build_scrcpy_camera_args(serial: &str, s: &CameraSettings) -> Vec<String>`
  - Rust function `parse_list_cameras_output(stdout: &str) -> Vec<CameraDeviceOption>`
  - Rust field `mode: String` on `SessionInfo` and `Session`
  - TypeScript types `CameraDeviceOption`, `CameraSettings`, and updated `SessionInfo`

- [ ] **Step 1: Write failing Rust unit tests for `parse_list_cameras_output` and `build_scrcpy_camera_args`**

Add unit tests in `src-tauri/src/commands.rs` within `mod tests`:
```rust
#[test]
fn test_parse_list_cameras_output() {
    let output = r#"
[server] INFO: List of cameras:
    --camera-id=0    (back, 4096x3072, fps={10, 15, 20, 24, 30}, zoom-range=[1, 15])
    --camera-id=1    (front, 4080x3072, fps={10, 15, 20, 24, 30}, zoom-range=[1, 4])
"#;
    let cameras = parse_list_cameras_output(output);
    assert_eq!(cameras.len(), 2);
    assert_eq!(cameras[0].id, "0");
    assert_eq!(cameras[0].facing, "back");
    assert_eq!(cameras[0].resolution, "4096x3072");
    assert_eq!(cameras[0].fps, vec![10, 15, 20, 24, 30]);
    assert_eq!(cameras[0].zoom_range, Some((1.0, 15.0)));

    assert_eq!(cameras[1].id, "1");
    assert_eq!(cameras[1].facing, "front");
}

#[test]
fn test_build_scrcpy_camera_args() {
    let s = CameraSettings {
        facing: "back".into(),
        camera_id: None,
        size: Some("1920x1080".into()),
        fps: Some(30),
        high_speed: false,
        torch: true,
        no_audio: false,
    };
    let args = build_scrcpy_camera_args("SER123", &s);
    assert!(args.contains(&"--video-source=camera".to_string()));
    assert!(args.contains(&"--camera-facing=back".to_string()));
    assert!(args.contains(&"--camera-size=1920x1080".to_string()));
    assert!(args.contains(&"--camera-fps=30".to_string()));
    assert!(args.contains(&"--camera-torch".to_string()));
    assert!(!args.contains(&"--no-audio".to_string()));
    assert!(args.contains(&"--window-title=AnyLeap Camera — SER123".to_string()));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri; cargo test test_parse_list_cameras_output`
Expected: FAIL due to missing types/functions.

- [ ] **Step 3: Implement data structures and argument builders in `src-tauri` and `src/lib/types.ts`**

In `src-tauri/src/state.rs`:
Add `pub mode: String` to `Session`.

In `src-tauri/src/commands.rs`:
Add `CameraSettings`, `CameraDeviceOption`, `build_scrcpy_camera_args`, `parse_list_cameras_output`, and update `SessionInfo` with `pub mode: String`.
Update `spawn_session` to store and return the mode (`"display"` or `"camera"`).

In `src/lib/types.ts`:
Add `CameraSettings`, `CameraDeviceOption`, and `mode: "display" | "camera"` on `SessionInfo`.

- [ ] **Step 4: Run Rust tests to verify they pass**

Run: `cd src-tauri; cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/state.rs src-tauri/src/commands.rs src/lib/types.ts
git commit -m "feat(camera): add camera models, args builder, and list-cameras parser"
```

---

### Task 2: Backend IPC Commands for Camera (List Cameras, Launch, Shortcuts)

**Files:**
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs` (tauri generate_handler registration)
- Modify: `src/lib/tauri.ts`
- Test: Manual check via Tauri invocation & `cargo test`

**Interfaces:**
- Consumes: `parse_list_cameras_output`, `build_scrcpy_camera_args`, `spawn_session`
- Produces:
  - Tauri command `list_device_cameras(app: AppHandle, serial: String) -> Result<Vec<CameraDeviceOption>, String>`
  - Tauri command `start_camera_mirror(app: AppHandle, serial: String, settings: CameraSettings) -> Result<SessionInfo, String>`
  - Tauri command `send_camera_shortcut(app: AppHandle, serial: String, action: String) -> Result<(), String>`
  - Frontend bindings in `src/lib/tauri.ts`: `listDeviceCameras`, `startCameraMirror`, `sendCameraShortcut`

- [ ] **Step 1: Implement Tauri commands in `src-tauri/src/commands.rs`**

Add:
- `list_device_cameras`: runs `scrcpy` with `--list-cameras` and returns parsed vector.
- `start_camera_mirror`: uses `build_scrcpy_camera_args` and calls `spawn_session(&app, serial, args, false)` with mode `"camera"`.
- `send_camera_shortcut`: locates window `AnyLeap Camera — <serial>` and injects:
  - `"torch_on"`: `Alt + T`
  - `"torch_off"`: `Alt + Shift + T`
  - `"zoom_in"`: `Alt + Up`
  - `"zoom_out"`: `Alt + Down`

- [ ] **Step 2: Register commands in `src-tauri/src/lib.rs`**

Add `list_device_cameras`, `start_camera_mirror`, `send_camera_shortcut` to `tauri::generate_handler![...]`.

- [ ] **Step 3: Add typed invocations in `src/lib/tauri.ts`**

Export `listDeviceCameras`, `startCameraMirror`, and `sendCameraShortcut`.

- [ ] **Step 4: Verify Rust compilation and unit tests**

Run: `cd src-tauri; cargo test`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands.rs src-tauri/src/lib.rs src/lib/tauri.ts
git commit -m "feat(camera): expose list_device_cameras, start_camera_mirror, and send_camera_shortcut IPC"
```

---

### Task 3: Camera Launch Dialog Component

**Files:**
- Create: `src/components/CameraLaunchDialog.tsx`
- Modify: `src/components/DeviceRow.tsx`
- Test: `npm run build`

**Interfaces:**
- Consumes: `listDeviceCameras`, `startCameraMirror` from `src/lib/tauri.ts`
- Produces:
  - `<CameraLaunchDialog isOpen={open} onOpenChange={setOpen} serial={device.serial} deviceName={device.model} />`
  - Dedicated "Camera" button in `DeviceRow.tsx` with `Camera` icon from `lucide-react`.

- [ ] **Step 1: Create `src/components/CameraLaunchDialog.tsx`**

Build dialog with:
- Camera lens selector (Populated by `listDeviceCameras(serial)` or default Front/Back).
- Resolution presets: `1920x1080` (1080p FHD), `1280x720` (720p HD), Native Sensor (`""`).
- FPS options: 30, 60, Auto (`0`).
- Torch toggle checkbox (`torch`).
- Microphone audio checkbox (`noAudio: !micEnabled`, default mic checked).
- "Start Camera" button that calls `startCameraMirror(serial, settings)` and closes dialog.

- [ ] **Step 2: Connect Dialog to `src/components/DeviceRow.tsx`**

Add a button next to "Mirror":
```tsx
<Button
  size="sm"
  variant="outline"
  onClick={() => setCameraOpen(true)}
  disabled={busy || device.state !== "device"}
  title="Use as Camera / Webcam"
>
  <Camera className="h-3.5 w-3.5" />
  Camera
</Button>
```
Mount `CameraLaunchDialog` in `DeviceRow`.

- [ ] **Step 3: Run frontend build to verify TypeScript and JSX compilation**

Run: `npm run build`
Expected: PASS with 0 errors.

- [ ] **Step 4: Commit**

```bash
git add src/components/CameraLaunchDialog.tsx src/components/DeviceRow.tsx
git commit -m "feat(camera): add CameraLaunchDialog and camera launch button in DeviceRow"
```

---

### Task 4: In-Session Camera Controls in ControlBar

**Files:**
- Modify: `src/components/ControlBar.tsx`
- Modify: `src/components/DeviceRow.tsx`
- Test: `npm run build`

**Interfaces:**
- Consumes: `sendCameraShortcut`, `stopMirror`, `startCameraMirror`
- Produces:
  - Adaptive camera controls inside `ControlBar` when active session is `mode === "camera"`:
    - Torch button (toggle on/off)
    - Zoom + / Zoom − buttons
    - Flip Lens button (Front ⟷ Back)
    - Stop button
  - Hide non-camera controls (back, home, recents, swipe scroll, flex display) when in camera mode.

- [ ] **Step 1: Update `ControlBar.tsx` to handle camera mode**

Add prop `sessionMode?: "display" | "camera"`.
When `sessionMode === "camera"`:
- Render camera tool buttons: Torch, Zoom In, Zoom Out, Flip Lens.
- Wire actions to `sendCameraShortcut(serial, action)`:
  - Torch: `torch_on` / `torch_off`
  - Zoom: `zoom_in` / `zoom_out`

- [ ] **Step 2: Update `DeviceRow.tsx` to pass `session?.mode` to `ControlBar` and show "Camera Streaming" badge**

Pass `sessionMode={session?.mode}` to `ControlBar`.
When `status === "mirroring" && session?.mode === "camera"`, show badge "Camera Streaming" in amber/emerald styling.

- [ ] **Step 3: Run frontend build**

Run: `npm run build`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/components/ControlBar.tsx src/components/DeviceRow.tsx
git commit -m "feat(camera): add in-session camera controls to ControlBar"
```

---

### Task 5: End-to-End Verification with Live Device

**Files:**
- Test: Live testing on connected test device `192.168.0.175:38281` (TECNO_CM7) and automated tests.

- [ ] **Step 1: Run full unit test suite**

Run: `cd src-tauri; cargo test`
Expected: PASS (all 13+ tests pass).

- [ ] **Step 2: Run full production frontend build**

Run: `npm run build`
Expected: PASS.

- [ ] **Step 3: Test camera launch on connected device**

Run Tauri dev or test command invoking camera stream on `192.168.0.175:38281` to verify scrcpy window opens, displays sensor feed, torch shortcut toggles, and session stops cleanly.

- [ ] **Step 4: Final commit and cleanup**

```bash
git status
git commit -m "feat: complete android camera streaming and studio controls (issue #1 part 1)"
```
