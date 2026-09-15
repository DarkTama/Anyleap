# Implementation Plan: Escrcpy Feature Additions for AnyLeap

> **Goal:** Extend AnyLeap with top-tier convenience features adapted from Escrcpy: an App Launcher drawer, Drag-and-Drop file/APK transfer, custom device nicknames, direct screenshot-to-clipboard, and Camera-as-Webcam mirroring.

**Architecture:**
- **App Launcher Drawer (`src-tauri/src/apps.rs` + `src/components/AppDrawer.tsx`)**:
  - Run `adb shell cmd package list packages -3` (or `scrcpy --list-apps`).
  - Parse package names and friendly app labels.
  - Clicking an app executes `adb shell monkey -p <pkg> -c android.intent.category.LAUNCHER 1` or launches directly onto a virtual display via `scrcpy --new-display --start-app=<pkg>`.
- **Drag-and-Drop File / APK Push (`src-tauri/src/transfer.rs`)**:
  - Intercept Tauri `tauri://drag-drop` file paths.
  - If `.apk`: execute `adb install -r <path>` and notify with install status toast.
  - If other files: execute `adb push <path> /sdcard/Download/` and notify.
- **Device Nicknames (`src/lib/settingsStore.ts`)**:
  - Store map `nicknames: Record<serial, nickname>` in `settings.json`.
  - Device list and mirror window title show nickname first, falling back to serial/IP.
- **Screenshot to PC Clipboard (`src-tauri/src/screenshot.rs`)**:
  - Execute `adb exec-out screencap -p` into byte buffer.
  - Write PNG bytes directly into system clipboard via Tauri clipboard plugin or `arboard` crate.
- **Camera-as-Webcam Mode**:
  - Preset mirroring option: `--video-source=camera` with camera selector (`--camera-facing=front|back`), torch toggle (`--camera-torch`), and zoom controls.

**Tech Stack:** Rust (ADB process execution, clipboard integration), React + Tailwind, Tauri file drop events.

---

## Global Constraints

- Do not block the UI during file transfers or app list queries (use async background tasks).
- Provide visual feedback (toasts / progress indicators) for file push and APK install.
- Keep dependency additions minimal.

---

## File Changes

- Create: `src-tauri/src/transfer.rs`
- Create: `src-tauri/src/apps.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src/components/AppDrawer.tsx`
- Modify: `src/components/ControlBar.tsx`
- Modify: `src/store/useAppStore.ts`

---

## Detailed Tasks

### Task 1: Implement Drag-and-Drop APK Install & File Push

**Files:**
- Create: `src-tauri/src/transfer.rs`
- Modify: `src-tauri/src/commands.rs`

**Implementation Outline:**
```rust
#[tauri::command]
pub async fn handle_dropped_files(app: AppHandle, serial: String, paths: Vec<String>) -> Result<String, String> {
    for path in paths {
        if path.ends_with(".apk") {
            let out = adb_cmd(&app)?
                .args(["-s", &serial, "install", "-r", &path])
                .output()
                .await
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(format!("APK install failed: {}", String::from_utf8_lossy(&out.stderr)));
            }
        } else {
            let out = adb_cmd(&app)?
                .args(["-s", &serial, "push", &path, "/sdcard/Download/"])
                .output()
                .await
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                return Err(format!("File push failed: {}", String::from_utf8_lossy(&out.stderr)));
            }
        }
    }
    Ok("Transfer complete".into())
}
```

**Verification:**
Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS

---

### Task 2: Implement App Listing & Launching

**Files:**
- Create: `src-tauri/src/apps.rs`
- Create: `src/components/AppDrawer.tsx`

**Implementation:**
Query third-party packages via `adb shell pm list packages -3 -f`, parse package names, and expose command `launch_app(serial, package_name, new_display: bool)`.

**Verification:**
Run: `npm run build`
Expected: PASS

---

### Task 3: Implement Screenshot to System Clipboard

**Files:**
- Modify: `src-tauri/src/commands.rs`

**Implementation:**
Capture PNG data via `adb exec-out screencap -p` and set into system clipboard.

**Verification:**
Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS
