# Implementation Plan: Screen Power Toggle & String Encoding Fix

> **Goal:** Fix the non-working screen on/off toggle by replacing fuzzy title string matching with PID-based window enumeration and direct message posting, and repair corrupted UTF-8 em-dashes in `Cargo.toml` and `commands.rs`.

**Architecture:**
- Repair mojibake: replace corrupted characters `â€”` and ` E` with standard UTF-8 em-dash `—`.
- Replace `FindWindowW` title lookup with `EnumWindows` searching for the window whose thread process ID matches `session.pid`. This guarantees 100% accurate window resolution regardless of serial formatting or title encoding.
- Replace fragile `keybd_event` global keyboard injection (which fails due to OS foreground lock restrictions) with direct Win32 message dispatch using `PostMessageW`:
  - Screen Off: `PostMessageW(hwnd, WM_SYSKEYDOWN, VK_O, lParamWithAlt)`.
  - Screen On: `PostMessageW(hwnd, WM_KEYDOWN, VK_SHIFT, ...)` followed by `PostMessageW(hwnd, WM_SYSKEYDOWN, VK_O, ...)`.
  - Direct message posting does not require changing foreground window focus, eliminating keystroke leaks to user applications.
- Lift `screenOff`, `asleep`, and `swipeScroll` states into Zustand store `useAppStore` so UI state does not reset to `false` when `ControlBar` is expanded or unmounted.

**Tech Stack:** Rust (`windows` crate 0.61: `EnumWindows`, `GetWindowThreadProcessId`, `PostMessageW`), React, Zustand.

---

## Global Constraints

- Never steal user foreground focus during background key event injection.
- Preserves scrcpy mirror continuity (no restart or process kill when toggling screen off).
- Ensure atomic state synchronization across multiple control instances.

---

## File Changes

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src/store/useAppStore.ts`
- Modify: `src/components/ControlBar.tsx`

---

## Detailed Tasks

### Task 1: Repair Corrupted UTF-8 Characters

**Files:**
- Modify: `src-tauri/Cargo.toml:4`
- Modify: `src-tauri/src/commands.rs:282, 558`

**Implementation:**
Restore clean UTF-8 em-dash:
1. `src-tauri/Cargo.toml`:
   ```toml
   description = "AnyLeap — effortless Android-to-PC mirroring (scrcpy GUI)"
   ```
2. `src-tauri/src/commands.rs`:
   ```rust
   a.push(format!("--window-title=AnyLeap — {}", serial));
   ```

**Verification:**
Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS

---

### Task 2: Implement PID-Based Window Resolution & Direct Message Dispatch

**Files:**
- Modify: `src-tauri/src/commands.rs`

**Implementation:**
1. Add window search callback using Win32 API:
   ```rust
   #[cfg(windows)]
   unsafe extern "system" fn enum_window_proc(hwnd: windows::Win32::Foundation::HWND, lparam: windows::Win32::Foundation::LPARAM) -> windows::Win32::Foundation::BOOL {
       use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;
       let target_pid = lparam.0 as u32;
       let mut process_id = 0u32;
       GetWindowThreadProcessId(hwnd, Some(&mut process_id));
       if process_id == target_pid {
           // Store result in user data or return false to stop enumeration
       }
       windows::Win32::Foundation::BOOL(1)
   }
   ```
2. In `restart_with_screen_off`:
   - Retrieve `session.pid`.
   - Enumerate top-level windows for `session.pid`.
   - Post `WM_SYSKEYDOWN` (`0x0104`) with `VK_O` (`0x4F`) and ALT bit `(1 << 29)` set in `lParam`.
   - If turning on (`!off`), post Shift key press or wake command.

**Verification:**
Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS

---

### Task 3: Lift ControlBar State to Zustand Store

**Files:**
- Modify: `src/store/useAppStore.ts`
- Modify: `src/components/ControlBar.tsx`

**Implementation:**
1. In `src/store/useAppStore.ts`:
   Add `deviceToggles: Record<string, { screenOff?: boolean; asleep?: boolean; swipeScroll?: boolean }>`
   Add action `setDeviceToggle: (serial: string, key: string, val: boolean) => void`.
2. In `src/components/ControlBar.tsx`:
   Read and update toggle state via `useAppStore`.

**Verification:**
Run: `npx tsc --noEmit`
Expected: PASS
