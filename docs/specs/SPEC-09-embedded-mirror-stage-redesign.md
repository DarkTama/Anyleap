# Implementation Plan: Embedded Mirror Stage Redesign

> **Goal:** Transform the embedded mirror window from a plain reparented box into a high-grade native "Stage" — featuring a refined glass title bar, smooth letterbox layout, interactive sidebar dock with tooltips and haptics, and instant drag-and-drop feedback.

**Architecture:**
- **Frameless Glass Title Bar:**
  - Micro status pill: device brand icon, nickname/model, battery %, transport badge (5GHz Wi-Fi / USB).
  - Floating action buttons: Always-on-top pin toggle, window maximize/restore, minimize, close with smooth micro-interactions.
  - Native double-click title bar to toggle maximize.
- **Stage Viewport (`#mirror-stage`):**
  - High performance Win32 child reparenting with automatic monitor DPI scaling synchronization.
  - Drag-and-drop visual dropzone overlay ("Drop APK to install / Drop files to push") with animated dashed border and pulse glow.
- **Precision Sidebar Dock (48px):**
  - Segmented vertical tool strips:
    1. *Android Navigation*: Back, Home, Recents.
    2. *Hardware Controls*: Sleep, Volume slider / mute toggle, Rotate.
    3. *Device Tools*: App Drawer modal launcher, Screenshot with instant thumbnail preview toast, File Explorer trigger.
    4. *Reels Mode*: Wheel-swipe toggle indicator with active scroll glow.
  - High-contrast tooltips showing shortcut keys (<kbd>Alt+O</kbd>, <kbd>Alt+B</kbd>).

**Tech Stack:** React 19, Tailwind CSS, Lucide Icons, Win32 API (`SetParent`, `SetWindowPos`), Tauri Event Bus.

---

## Global Constraints

- Never interfere with direct touch/click latency on the scrcpy SDL surface.
- The sidebar must not obscure or crop any part of the active Android aspect ratio.
- Must remain fully responsive on both portrait and landscape orientation flips.

---

## File Changes

- Modify: `src/components/EmbeddedMirrorWindow.tsx`
- Modify: `src/components/ControlBar.tsx`
- Create: `src/components/stage/StageDropzone.tsx`
- Create: `src/components/stage/ScreenshotPreviewToast.tsx`
- Modify: `src-tauri/src/embed.rs`

---

## Detailed Tasks

### Task 1: Refactor Win32 Embedded Viewport Alignment (`embed.rs`)

**Files:**
- Modify: `src-tauri/src/embed.rs`

**Implementation:**
- Synchronize client bounds accurately with Windows window frames.
- Handle multi-monitor DPI transitions smoothly without black bars.

---

### Task 2: Build Stage Title Bar & Dropzone Overlay

**Files:**
- Create: `src/components/stage/StageDropzone.tsx`
- Modify: `src/components/EmbeddedMirrorWindow.tsx`

**Features:**
- Native drag-over detection on the Tauri webview showing "Drop to install APK" or "Drop to transfer to device".
- Compact battery and connection status pill in the title bar.

---

### Task 3: Redesign Precision Vertical Toolbar (`ControlBar.tsx`)

**Files:**
- Modify: `src/components/ControlBar.tsx`

**Features:**
- Tactile icon buttons with rounded-lg slate backgrounds.
- Tooltips showing hotkey equivalents.
- Screenshot button triggers instant floating thumbnail toast with "Copied to clipboard".
