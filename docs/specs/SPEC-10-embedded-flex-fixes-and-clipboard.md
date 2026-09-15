# Implementation Plan: Embedded Mirror Fixes, Flex Power/Shortcuts & Clipboard Paste

> **Goal:** Fix critical bugs in embedded mirror window controls and drag behavior, resolve physical vs virtual screen power toggles, fix sidebar shortcut targeting in flex display mode, add flex taskbar control, and implement image paste from Windows clipboard into Android.

---

## 1. Root Cause Analysis of Reported Bugs

### Bug 1: Embedded mirror window undraggable; minimize, maximize, and close buttons do not work
- **Root Cause**: 
  1. In `src/components/EmbeddedMirrorWindow.tsx`, `<div data-tauri-drag-region className="...">` wrapped the entire titlebar container including the `<button>` elements. In Tauri on Windows WebView2, child buttons inside a `data-tauri-drag-region` have their click events captured by the OS window drag handler.
  2. The reparented Win32 SDL window (`scrcpy_hwnd`) was positioned at `y = title_h`, but without `WS_CLIPCHILDREN` set on `tauri_hwnd`, mouse clicks near the top border or resize borders get swallowed by Win32 hit-testing.
  3. Window control actions must explicitly invoke `win.startDragging()` on pointerdown, and window action buttons must stop event propagation (`onPointerDown={(e) => e.stopPropagation()}`) with `data-tauri-drag-region="false"`.

### Bug 2 & 3: Screen on/off turns off mirror, feels like a reconnect, and opens outside embedded mirror
- **Root Cause**:
  1. `find_hwnd_by_pid(session.pid)` used `EnumWindows` (which only enumerates top-level desktop windows). Once scrcpy is reparented into the Tauri window via `SetParent`, it becomes a **child window**, so `EnumWindows` fails to find it.
  2. Failing to find the HWND caused `restart_with_screen_off` to drop into the fallback restart branch, killing the scrcpy process and spawning a brand-new scrcpy process with a new PID.
  3. The newly spawned scrcpy process opened as a standalone window outside the embedded mirror because nobody reparented the new PID.
  4. Scrcpy's `--turn-screen-off` controls the physical screen power mode (`SurfaceControl.setDisplayPowerMode`). To toggle without reconnecting, AnyLeap must query child HWNDs (`EnumChildWindows`) or track the cached HWND in `EMBEDDED_HWNDS`, and send the scrcpy shortcut `MOD+o` directly via `PostMessageW(hwnd, WM_SYSKEYDOWN, VK_O, ...)`.

### Tweak 1: Flex screen taskbar disappearing or toggling
- **Root Cause & Solution**:
  1. Android's virtual display taskbar is governed by scrcpy's `--no-vd-system-decorations` flag. When enabled, Android provides system decorations (taskbar/navigation). When `--no-vd-system-decorations` is passed, the taskbar is omitted.
  2. The taskbar disappears when the phone sleeps because Android locks the secondary display when Keyguard activates. Adding `--stay-awake` and `--no-vd-destroy-content` prevents the virtual display from freezing and losing its launcher state.
  3. We will expose an explicit toggle in Settings: "Flex Taskbar (System Decorations)" to let the user show or hide the taskbar on demand.

### Tweak 2: Fullscreen auto-detects system screen resolution
- **Solution**:
  In `SettingsPanel.tsx`, when "Fullscreen" or "Flex display" is selected, automatically detect and populate `flexDisplaySize` with the primary/active screen's physical pixel resolution (`${window.screen.width * window.devicePixelRatio}x${window.screen.height * window.devicePixelRatio}`).

### Tweak 3: Sidebar shortcuts don't work in Flex Display
- **Root Cause & Solution**:
  1. `DISPLAY_TARGETED_KEYCODES` in `commands.rs` only targeted keycodes `[3, 4, 187, 82, 111]`.
  2. In flex display mode, shortcuts like "Rotate" and "Notifications" act on physical display 0 because Android's `user_rotation` and `cmd statusbar expand-notifications` are display-0 exclusive.
  3. We will adapt the sidebar controls in flex mode: display-targeted Back, Home, and Recents will explicitly resolve `s.display_id` reliably; physical display operations (Rotate, Sleep) will clearly indicate they target the physical device, and virtual display window resizing will replace orientation toggles.

### Tweak 4: Pasting images from Windows clipboard into Android
- **Solution**:
  1. Scrcpy only syncs text clipboard data. It cannot push image binaries through text clipboard sync.
  2. Implement an IPC command `paste_clipboard_image(serial)`:
     - Check if the Windows clipboard contains image bitmap data (`GetClipboardData(CF_DIB)` / `CF_DIBV5` or `arboard`).
     - Save as temporary PNG `clipboard_paste.png`.
     - Push via `adb push clipboard_paste.png /sdcard/Download/`.
     - Broadcast media scanner: `adb shell am broadcast -a android.intent.action.MEDIA_SCANNER_SCAN_FILE -d file:///sdcard/Download/clipboard_paste.png`.
     - Toast notification: "Image pasted to device Downloads".

### Tweak 5: DPI Scale explanation & Live Icon Preview
- **Concept**:
  - Android density: **Lower DPI (e.g. 160) = smaller desktop-sized icons & more screen real estate**.
  - **Higher DPI (e.g. 320) = larger touch-friendly icons**.
  - Add descriptive helper labels and an interactive visual preview showing how icons scale at 120, 160, 200, 240, 320 DPI.

### Tweak 6: Flex Display + Embedded Mirror interaction
- **Intended Purpose**:
  - They SHOULD work together! Embedded mirror provides the unified window frame and sidebar, while Flex mode renders the Android desktop inside that frame.
  - When both are checked, scrcpy is launched with `--new-display --flex-display --window-borderless` and reparented cleanly into `EmbeddedMirrorWindow`.
