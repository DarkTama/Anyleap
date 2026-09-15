# Implementation Plan: Integrated Embedded Mirror & Sidebar Window

> **Goal:** Replace the disconnected two-window floating control strip layout with a single, unified container window that embeds the scrcpy viewport alongside an integrated, flush right sidebar toolbar (Escrcpy-style "Inset Mirror").

**Architecture:**
- Instead of running scrcpy as an independent desktop window and polling its position from a secondary transparent window (`ControlWindow.tsx`), create a dedicated Tauri window (`mirror-container`) for each active mirror session.
- The `mirror-container` window layout consists of:
  - Custom frameless title bar displaying device model/serial, connection type badge, dark mode toggle, pin-on-top, minimize, maximize, close.
  - Main body divided into two horizontal sections:
    1. **Mirror Viewport Area (`#mirror-host`)**: Takes all remaining width, auto-centers or fits aspect ratio.
    2. **Integrated Sidebar Toolbar (`ControlBar`)**: Flush against the right edge (44px wide), styled with icons for Navigation (Back, Home, Recents), Quick Actions (Power, Screen Off, Volume, Screenshot, Rotate, Reels Swipe).
- **Win32 Reparenting Engine (`src-tauri/src/embed.rs`)**:
  - Scrcpy spawned with `--window-borderless`.
  - Upon window creation, backend obtains the Win32 `HWND` of the Tauri `#mirror-host` element and the `HWND` of scrcpy.
  - Calls Win32 `SetParent(scrcpy_hwnd, host_hwnd)` to reparent scrcpy as a genuine child window.
  - Modifies child window style via `SetWindowLongPtrW`: adds `WS_CHILD | WS_VISIBLE`, strips `WS_POPUP | WS_CAPTION | WS_THICKFRAME`.
  - On container resize, smoothly repositions and resizes the child viewport using `SetWindowPos` or allows scrcpy's native letterboxing to handle display aspect ratio.

**Tech Stack:** Rust (`windows` crate Win32 APIs: `SetParent`, `SetWindowLongPtrW`, `SetWindowPos`), React + Tailwind CSS, Tauri v2 Multi-window.

---

## Global Constraints

- Must work reliably across Windows 10 and Windows 11.
- Maintain scrcpy's native 60fps+ low-latency hardware-accelerated rendering.
- No polling intervals for window alignment: resizing and dragging must move as one native window.
- Gracefully tear down child scrcpy process when the container window is closed.

---

## File Changes

- Create: `src-tauri/src/embed.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Create: `src/components/EmbeddedMirrorWindow.tsx`
- Modify: `src/App.tsx`
- Delete / Deprecate: `src/components/ControlWindow.tsx`

---

## Detailed Tasks

### Task 1: Create Win32 Child Reparenting Backend (`embed.rs`)

**Files:**
- Create: `src-tauri/src/embed.rs`
- Modify: `src-tauri/src/lib.rs`

**Implementation Outline:**
```rust
#[cfg(windows)]
pub fn embed_scrcpy_window(scrcpy_pid: u32, parent_hwnd: windows::Win32::Foundation::HWND) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::*;
    // 1. Locate scrcpy HWND by PID
    let scrcpy_hwnd = find_hwnd_by_pid(scrcpy_pid).ok_or("scrcpy HWND not found")?;

    unsafe {
        // 2. Reparent window
        SetParent(scrcpy_hwnd, parent_hwnd);

        // 3. Update window styles to child
        let mut style = GetWindowLongPtrW(scrcpy_hwnd, GWL_STYLE);
        style &= !(WS_POPUP.0 as isize | WS_CAPTION.0 as isize | WS_THICKFRAME.0 as isize);
        style |= WS_CHILD.0 as isize | WS_VISIBLE.0 as isize;
        SetWindowLongPtrW(scrcpy_hwnd, GWL_STYLE, style);

        // 4. Initial layout fit
        let mut rect = windows::Win32::Foundation::RECT::default();
        GetClientRect(parent_hwnd, &mut rect);
        SetWindowPos(
            scrcpy_hwnd,
            windows::Win32::Foundation::HWND::default(),
            0, 0,
            rect.right - rect.left,
            rect.bottom - rect.top,
            SWP_NOZORDER | SWP_FRAMECHANGED | SWP_SHOWWINDOW,
        );
    }
    Ok(())
}
```

**Verification:**
Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS

---

### Task 2: Update Scrcpy Launch Flags for Borderless Embedding

**Files:**
- Modify: `src-tauri/src/commands.rs:build_scrcpy_args`

**Implementation:**
When launching embedded mirror, pass `--window-borderless` and disable window aspect ratio lock if controlled by the outer container.

**Verification:**
Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS

---

### Task 3: Build Embedded Mirror Frontend Container (`EmbeddedMirrorWindow.tsx`)

**Files:**
- Create: `src/components/EmbeddedMirrorWindow.tsx`
- Modify: `src/App.tsx`

**Implementation:**
1. Frameless title bar with drag region (`data-tauri-drag-region`), session name, status badge, window control buttons (minimize, maximize, close).
2. Flex container:
   - Left: `<div id="scrcpy-viewport" className="flex-1 h-full bg-black relative" />`.
   - Right: Integrated `<ControlBar orientation="vertical" />` (44px wide, dark zinc surface).
3. On mount, invoke `embed_mirror(serial, container_hwnd)`.

**Verification:**
Run: `npm run build`
Expected: PASS
