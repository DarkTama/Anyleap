# KVM Spatial Layout, Visual Indicators & Media Clipboard Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Enhance Virtual KVM with full 2D spatial monitor geometry, configurable edge resistance/deadzones, customizable cursor visual indicators (desktop ripple, floating HUD pill, canvas tracking dot), image and file drop clipboard synchronization with transfer progress tracker, and zero-latency keyboard/focus injection.

**Architecture:**
- Rust backend calculates true geometric boundary overlap and enforces corner deadzones and dwell resistance before engaging edge leap.
- Dedicated Win32 hook pump eliminates keyboard latency; target PC activates target window on pointer land.
- Bidirectional clipboard watcher syncs text and images (compressed PNG) with echo suppression; chunked file streamer transfers dropped files with progress reporting.
- Lightweight borderless transparent Tauri HUD pill displays active cursor ownership; Canvas renders blueprint layout with interactive deadzone preview and real-time cursor dot.

**Tech Stack:** Rust (Axum, tokio, windows crate, Win32 GDI/ClipCursor), React 19, TypeScript, Zustand, Tailwind CSS, Lucide icons.

**Spec:** `docs/specs/SPEC-12-multi-pc-hub-and-virtual-kvm.md`

## Global Constraints
- Zero mentions of third-party reference targets or competitor names anywhere (`AGENTS.md`).
- Atomic single-commit squashing rule before merge (`AGENTS.md`).
- Verification gates: `npm test`, `cargo check`, `cargo test`, `npm run build` must be 100% green.
- Terse caveman style for chat output.

---

### Task 1: Protocol, Types & Settings Schema

**Files:**
- Modify: `src-tauri/src/kvm/protocol.rs`
- Modify: `src/types/kvm.ts`
- Modify: `src/stores/settingsStore.ts`

**Interfaces:**
- Consumes: Existing `KvmPacket`, `KvmStatus`.
- Produces:
  - `KvmPacket::ClipboardImage { format: String, data: Vec<u8> }`
  - `KvmPacket::FileChunk { file_id: String, name: String, size: u64, offset: u64, data: Vec<u8>, is_last: bool }`
  - `KvmPacket::CursorPosition { norm_x: f32, norm_y: f32 }`
  - KVM Settings in `settingsStore.ts`: `edgeResistanceMs`, `cornerDeadzonePx`, `indicatorRipple`, `indicatorHud`, `indicatorHudCorner`, `indicatorCanvasDot`, `syncClipboardImage`, `syncFileTransfer`.

- [x] **Step 1: Write test for packet serialization** in `src-tauri/src/kvm/protocol.rs`.
- [x] **Step 2: Add image and file chunk packet types** to `src-tauri/src/kvm/protocol.rs`.
- [x] **Step 3: Mirror types** into `src/types/kvm.ts`.
- [x] **Step 4: Add KVM customization state and storage** to `src/stores/settingsStore.ts`.
- [x] **Step 5: Run tests** (`cargo test`, `npm test`) to verify cleanly passing.

---

### Task 2: 2D Spatial Overlap, Deadzones, Resistance & Instant Keyboard Loop

**Files:**
- Modify: `src-tauri/src/kvm/hooks.rs`
- Modify: `src-tauri/src/kvm/injector.rs`
- Modify: `src-tauri/src/kvm/commands.rs`

**Interfaces:**
- Consumes: `TopologyConfig` containing bounds, offsets, and edge settings.
- Produces:
  - 2D overlap calculator in `hooks.rs`.
  - Dwell timer / velocity resistance before engaging edge leap.
  - Corner deadzone threshold ignoring leaps in menu/tab strip regions.
  - Responsive Win32 hook event loop replacing sleep-based polling.
  - Focus synthesis via `WindowFromPoint` on pointer land in `injector.rs`.

- [x] **Step 1: Implement 2D bounding overlap and deadzone check** in `src-tauri/src/kvm/hooks.rs`.
- [x] **Step 2: Implement edge dwell resistance** (timer check before locking cursor) in `hooks.rs`.
- [x] **Step 3: Replace `PeekMessage + sleep` with responsive hook message pump** in `hooks.rs`.
- [x] **Step 4: Implement focus activation under landing cursor** in `src-tauri/src/kvm/injector.rs`.
- [x] **Step 5: Run `cargo check` and `cargo test`**.

---

### Task 3: Image Clipboard Sync & File Payload Transfer

**Files:**
- Modify: `src-tauri/src/kvm/clipboard.rs`
- Modify: `src-tauri/src/kvm/server.rs`
- Modify: `src-tauri/src/kvm/client.rs`
- Modify: `src-tauri/src/kvm/commands.rs`

**Interfaces:**
- Consumes: Win32 clipboard APIs (`CF_DIB`, `CF_UNICODETEXT`).
- Produces:
  - `get_clipboard_image()` and `set_clipboard_image(png_bytes)`.
  - Chunked file sender and receiver writing to `Downloads/AnyLeap_Transfers`.
  - Event emitter `kvm-transfer-progress` notifying UI of chunk completion.

- [x] **Step 1: Implement Win32 DIB/PNG image read and write** in `src-tauri/src/kvm/clipboard.rs`.
- [x] **Step 2: Implement chunked file transmitter and receiver** in `src-tauri/src/kvm/server.rs` and `client.rs`.
- [x] **Step 3: Connect clipboard watcher to emit image packets** when clipboard contains bitmap.
- [x] **Step 4: Expose IPC commands and progress events** in `commands.rs`.
- [x] **Step 5: Verify build with `cargo check`**.

---

### Task 4: Visual Indicators (Desktop Ripple, HUD Pill, Canvas Dot)

**Files:**
- Create: `src/components/computers/DesktopHudPill.tsx`
- Modify: `src/components/computers/TopologyCanvas.tsx`
- Modify: `src/stores/kvmStore.ts`
- Modify: `src-tauri/src/lib.rs` (transparent overlay window registration if enabled)

**Interfaces:**
- Consumes: Cursor transition events, `indicatorHud`, `indicatorRipple`, `indicatorCanvasDot`.
- Produces:
  - Visual ripple trigger on desktop entry.
  - Floating corner HUD pill showing current active machine name.
  - Real-time traveling cursor beacon dot on the blueprint canvas.

- [x] **Step 1: Add live cursor coordinate broadcast** from `hooks.rs` over UDP/IPC.
- [x] **Step 2: Implement traveling cursor beacon** inside `TopologyCanvas.tsx`.
- [x] **Step 3: Implement floating desktop indicator pill component**.
- [x] **Step 4: Connect indicator settings toggles in `kvmStore.ts`**.
- [x] **Step 5: Run `npm test`**.

---

### Task 5: Interactive 2D Blueprint Canvas with Visual Deadzones

**Files:**
- Modify: `src/components/computers/TopologyCanvas.tsx`

**Interfaces:**
- Consumes: Multi-monitor configuration, peer position, corner deadzone pixel count, active cursor status.
- Produces:
  - Interactive alignment canvas matching reference blueprint aesthetic.
  - Visualized active edge gateway (cyan/emerald) vs blocked corner deadzones (amber stripes).
  - Multi-direction placement (Left, Right, Top, Bottom) and smooth drag offset.

- [x] **Step 1: Implement full 2D coordinate positioning and visual deadzone stripes** in `TopologyCanvas.tsx`.
- [x] **Step 2: Add multi-directional edge selector and boundary alignment sliders/handles**.
- [x] **Step 3: Render live traveling beacon dot along monitor bounds**.
- [x] **Step 4: Run `npm run build` to verify clean JSX/TS compilation**.

---

### Task 6: Transfer Activity Feed, Progress Tracker & Settings Controls

**Files:**
- Modify: `src/components/computers/ComputersView.tsx`
- Modify: `src/components/SettingsPanel.tsx`
- Modify: `src/stores/kvmStore.ts`

**Interfaces:**
- Consumes: Transfer events (`text`, `image`, `file`), progress percentages, KVM settings.
- Produces:
  - Real-time Activity & Sync Feed in `ComputersView.tsx`.
  - Progress bar for ongoing file transfers.
  - Dedicated KVM settings section in `SettingsPanel.tsx` (Deadzone slider, Resistance slider, Indicator toggles, Clipboard options).

- [x] **Step 1: Add activity log array and file transfer state** in `src/stores/kvmStore.ts`.
- [x] **Step 2: Build Activity & Progress Feed UI** in `src/components/computers/ComputersView.tsx`.
- [x] **Step 3: Build KVM configuration panel with deadzone visualizer preview** in `src/components/SettingsPanel.tsx`.
- [x] **Step 4: Run Vitest test suite to verify store and UI functions**.

---

### Task 7: Verification Suite, Doc Sync & Branch Push

**Files:**
- Modify: `README.md`
- Modify: `docs/PLAN.md`
- Modify: `docs/specs/SPEC-12-multi-pc-hub-and-virtual-kvm.md`

- [x] **Step 1: Run complete verification gates** (`npm test`, `cargo test`, `npm run build`, `cargo check`).
- [x] **Step 2: Synchronize documentation** per `AGENTS.md`.
- [x] **Step 3: Squash/amend atomic semantic commit** and push branch to GitHub.
