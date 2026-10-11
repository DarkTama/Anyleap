# Multi-PC Hub & Seamless Virtual KVM (Computer Link) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Implement Phase 1 Multi-PC Virtual KVM (Computer Link) allowing seamless mouse/keyboard roaming across physical computer monitors, synchronized clipboard, and mDNS LAN discovery.

**Architecture:** Local peer-to-peer subsystem using an embedded Axum WebSocket server (port 4177) for reliable key/click/clipboard events and a raw UDP datagram listener (port 4178) for sub-millisecond mouse deltas. Host uses Win32 low-level hooks (`WH_MOUSE_LL`, `WH_KEYBOARD_LL`) to detect screen perimeter crossing, lock local cursor, suppress keystrokes, and forward events to the target PC which injects them via Win32 `SendInput`.

**Tech Stack:** Rust (Axum, tokio, tokio-tungstenite, windows crate) · Tauri 2 · React 19 + TypeScript · Tailwind v4 · Zustand · Vitest

**Spec:** `docs/specs/SPEC-12-multi-pc-hub-and-virtual-kvm.md`

## Global Constraints

- Mouse movements must feel instantaneous: use UDP datagrams on port 4178 for relative `dx`/`dy` cursor deltas to bypass TCP head-of-line blocking.
- Failsafe safety guarantee: pressing `ScrollLock` or `Ctrl+Alt+Escape` must immediately release all low-level hooks and unclip cursor, even if network disconnects.
- Socket heartbeat timeout: if heartbeat drops for > 3000ms, hooks automatically disengage and local control is restored.
- Loopback prevention on clipboard sync: re-entrancy flags prevent clipboard echo loops.
- Clean code architecture: follow `DeviceSource` modularity (ADR 0001) and atomic commit squashing per `AGENTS.md`.

---

## File Structure

```
src-tauri/
├── Cargo.toml                              # Modified: add axum, tokio-tungstenite, update windows features
├── src/
│   ├── lib.rs                              # Modified: register KVM IPC commands and initialize daemon
│   ├── state.rs                            # Modified: add KvmState handle to AppState
│   └── kvm/
│       ├── mod.rs                          # Created: module export and initialization
│       ├── protocol.rs                     # Created: packet formats (KvmPacket, MouseDelta, etc.)
│       ├── server.rs                       # Created: Axum WSS (4177) and UDP (4178) listener
│       ├── client.rs                       # Created: WSS connector and UDP sender to peer PC
│       ├── discovery.rs                    # Created: LAN UDP beacon for discovering peer AnyLeap hosts
│       ├── hooks.rs                        # Created: Win32 WH_MOUSE_LL / WH_KEYBOARD_LL hooks and edge leap
│       ├── injector.rs                     # Created: Win32 SendInput hardware input injection
│       ├── clipboard.rs                    # Created: WM_CLIPBOARDUPDATE listener and sync
│       └── commands.rs                     # Created: Tauri IPC handlers

src/
├── types/
│   └── kvm.ts                              # Created: TypeScript types for KVM packets and topology
├── stores/
│   ├── kvmStore.ts                         # Created: Zustand store for KVM peers, topology, and state
│   └── __tests__/
│       └── kvmStore.test.ts                # Created: Vitest tests for KVM store
├── services/
│   ├── kvmService.ts                       # Created: Frontend IPC wrapper service
│   └── __tests__/
│       └── kvmService.test.ts              # Created: Vitest tests for KVM service
└── components/
    └── computers/
        ├── ComputersView.tsx               # Modified: complete interactive KVM view
        ├── TopologyCanvas.tsx              # Created: 2D drag-and-drop monitor layout editor
        └── PairingPinModal.tsx             # Created: 6-digit mutual pairing PIN modal
```

---

### Task 1: Rust Dependencies & KVM Protocol Serialization

**Files:**
- Modify: `src-tauri/Cargo.toml`
- Create: `src-tauri/src/kvm/protocol.rs`
- Create: `src/types/kvm.ts`

**Interfaces:**
- Produces: `KvmPacket` enum (`MouseDelta`, `MouseButton`, `KeyEvent`, `ClipboardSync`, `Handshake`, `Heartbeat`, `CursorReturn`) with Serde JSON and bincode serialization.

- [x] **Step 1: Add Rust crate dependencies to `src-tauri/Cargo.toml`**

Add to `[dependencies]`:
```toml
axum = { version = "0.8", default-features = false, features = ["ws", "tokio", "http1", "json"] }
tokio-tungstenite = "0.30"
```
Update `tokio`:
```toml
tokio = { version = "1", features = ["time", "net", "sync", "rt", "macros"] }
```
Add windows features:
```toml
windows = { version = "0.61", features = [
    "Win32_Foundation",
    "Win32_UI_WindowsAndMessaging",
    "Win32_Graphics_Gdi",
    "Win32_System_LibraryLoader",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_System_DataExchange",
] }
```

- [x] **Step 2: Implement protocol types in `src-tauri/src/kvm/protocol.rs`**

Define serializable structs:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KvmPacket {
    Handshake { computer_id: String, name: String, pin: String },
    HandshakeAck { accepted: bool, session_token: String },
    Heartbeat { timestamp: u64 },
    MouseButton { button: u8, is_down: bool },
    KeyEvent { key_code: u16, scan_code: u16, is_down: bool, extended: bool },
    CursorEnter { x: f32, y: f32 },
    CursorReturn { edge: String },
    ClipboardText { text: String },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct UdpMouseDelta {
    pub dx: i32,
    pub dy: i32,
    pub seq: u32,
}
```

- [x] **Step 3: Define TypeScript interface matching in `src/types/kvm.ts`**

Mirror types for frontend store.

- [x] **Step 4: Verify Cargo check**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: SUCCESS (0 errors).

- [x] **Step 5: Commit Task 1**

Run: `git commit -am "feat(kvm): add KVM protocol serialization and network dependencies"`

---

### Task 2: Network Transport Daemon (Axum WSS & UDP Listeners)

**Files:**
- Create: `src-tauri/src/kvm/server.rs`
- Create: `src-tauri/src/kvm/client.rs`
- Create: `src-tauri/src/kvm/discovery.rs`
- Create: `src-tauri/src/kvm/mod.rs`

**Interfaces:**
- Produces:
  - `start_server(port: u16, udp_port: u16) -> Result<ServerHandle>`
  - `connect_to_peer(addr: &str, session_token: &str) -> Result<ClientHandle>`
  - `broadcast_discovery_beacon()` & `listen_discovery_beacons()`

- [x] **Step 1: Implement discovery beacon in `src-tauri/src/kvm/discovery.rs`**

Use UDP broadcast on port 4177 to periodically broadcast `AnyLeap-Beacon: {id, name, port}` and collect nearby machines.

- [x] **Step 2: Implement Axum WSS server & UDP listener in `src-tauri/src/kvm/server.rs`**

- `/pair` and `/pair/confirm` routes for 6-digit PIN handshake.
- `/ws` WebSocket route streaming `KvmPacket` events.
- Tokio UDP socket listener receiving `UdpMouseDelta` packets on port 4178.

- [x] **Step 3: Implement outbound client connector in `src-tauri/src/kvm/client.rs`**

Connects outbound WebSocket to target peer and initializes UDP socket for sending relative mouse packets.

- [x] **Step 4: Verify Cargo check**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: SUCCESS

- [x] **Step 5: Commit Task 2**

Run: `git add src-tauri/src/kvm/ && git commit -m "feat(kvm): implement Axum WSS server, UDP delta listener, and LAN beacon"`

---

### Task 3: Remote Hardware Input Injection (`injector.rs`)

**Files:**
- Create: `src-tauri/src/kvm/injector.rs`

**Interfaces:**
- Consumes: `KvmPacket`, `UdpMouseDelta`
- Produces: `inject_mouse_delta(dx: i32, dy: i32)`, `inject_mouse_button(btn: u8, down: bool)`, `inject_key(key: u16, scan: u16, down: bool, ext: bool)`

- [x] **Step 1: Write input injection logic using Win32 `SendInput`**

```rust
use windows::Win32::UI::Input::KeyboardAndMouse::*;

pub fn inject_mouse_delta(dx: i32, dy: i32) {
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT {
                dx,
                dy,
                mouseData: 0,
                dwFlags: MOUSEEVENTF_MOVE,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
}
```

- [x] **Step 2: Implement mouse button and keyboard scan code injection**

Support Left, Right, Middle mouse buttons, wheel scroll, and extended keyboard keys.

- [x] **Step 3: Verify Cargo check**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: SUCCESS

- [x] **Step 4: Commit Task 3**

Run: `git add src-tauri/src/kvm/injector.rs && git commit -m "feat(kvm): implement Win32 SendInput hardware input injection"`

---

### Task 4: Low-Level Input Hooks & Edge Leap Engine (`hooks.rs`)

**Files:**
- Create: `src-tauri/src/kvm/hooks.rs`

**Interfaces:**
- Consumes: Monitor bounds & topology links
- Produces: `install_hooks(topology: TopologyConfig)`, `uninstall_hooks()`, emergency escape handler

- [x] **Step 1: Implement `WH_MOUSE_LL` hook and edge boundary checking**

- Hook tracks cursor `(pt.x, pt.y)`.
- When `pt.x >= screen_max_x` on an edge linked to a peer PC:
  - Trigger `EdgeLeap` transition.
  - Call `ClipCursor` to keep pointer trapped at perimeter boundary.
  - Hide local cursor via `ShowCursor(FALSE)`.
  - Begin transmitting relative mouse movement over UDP.

- [x] **Step 2: Implement `WH_KEYBOARD_LL` hook suppression**

When remote PC has active focus:
- Intercept all keyboard strokes.
- Transmit `KeyEvent` to peer over WSS.
- Return `1` in hook callback to suppress local execution.

- [x] **Step 3: Implement failsafe emergency escape handler**

If `VK_SCROLL` (`ScrollLock`) or `Ctrl+Alt+Escape` is pressed:
- Instantly invoke `uninstall_hooks()`.
- Call `ClipCursor(NULL)`.
- Restore cursor visibility with `ShowCursor(TRUE)`.
- Return focus locally immediately.

- [x] **Step 4: Verify Cargo check**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: SUCCESS

- [x] **Step 5: Commit Task 4**

Run: `git add src-tauri/src/kvm/hooks.rs && git commit -m "feat(kvm): implement low-level Win32 hooks and edge leap transition"`

---

### Task 5: Bidirectional Clipboard Synchronization (`clipboard.rs`)

**Files:**
- Create: `src-tauri/src/kvm/clipboard.rs`

**Interfaces:**
- Consumes: Win32 message loop / clipboard notifications
- Produces: `start_clipboard_listener()`, `set_clipboard_text(text: &str)`

- [x] **Step 1: Implement `AddClipboardFormatListener` and notification loop**

Listens for `WM_CLIPBOARDUPDATE`.
Reads CF_UNICODETEXT from Windows clipboard when content changes.

- [x] **Step 2: Implement echo suppression guard**

Maintain an atomic sequence/hash flag so applying received remote clipboard text does not trigger re-broadcast back to peer.

- [x] **Step 3: Verify Cargo check**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: SUCCESS

- [x] **Step 4: Commit Task 5**

Run: `git add src-tauri/src/kvm/clipboard.rs && git commit -m "feat(kvm): implement Win32 clipboard update listener with echo guard"`

---

### Task 6: Tauri IPC Commands & Backend State

**Files:**
- Create: `src-tauri/src/kvm/commands.rs`
- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Produces: Tauri IPC commands:
  - `kvm_get_status`
  - `kvm_start_pairing`
  - `kvm_confirm_pairing`
  - `kvm_connect_peer`
  - `kvm_disconnect_peer`
  - `kvm_save_layout`

- [x] **Step 1: Define `KvmState` in `src-tauri/src/state.rs`**

Add mutex handles for active topology, connected peers, and hook controller.

- [x] **Step 2: Expose IPC handlers in `src-tauri/src/kvm/commands.rs`**

Wire commands to backend server, client, and hook routines.

- [x] **Step 3: Register commands in `src-tauri/src/lib.rs`**

Add commands to `tauri::generate_handler![...]`.

- [x] **Step 4: Verify Cargo check**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: SUCCESS

- [x] **Step 5: Commit Task 6**

Run: `git commit -am "feat(kvm): register Tauri IPC commands and state management"`

---

### Task 7: Frontend Zustand Store & Service Layer

**Files:**
- Create: `src/services/kvmService.ts`
- Create: `src/services/__tests__/kvmService.test.ts`
- Create: `src/stores/kvmStore.ts`
- Create: `src/stores/__tests__/kvmStore.test.ts`

**Interfaces:**
- Consumes: Tauri IPC invokes
- Produces: `useKvmStore` hook with `peers`, `discoveredComputers`, `topology`, `activeLink`, `connectPeer()`, `updateTopology()`

- [x] **Step 1: Write failing tests in `src/stores/__tests__/kvmStore.test.ts`**

Test adding peers, selecting active edge link, and updating 2D monitor layout.

- [x] **Step 2: Implement `src/stores/kvmStore.ts`**

Manage connected peers, discovery status, and spatial coordinates.

- [x] **Step 3: Run Vitest to verify tests pass**

Run: `npx vitest run src/stores/__tests__/kvmStore.test.ts`
Expected: PASS

- [x] **Step 4: Commit Task 7**

Run: `git add src/stores/ src/services/ && git commit -m "feat(kvm): implement frontend Zustand store and IPC service layer"`

---

### Task 8: Interactive Topology Canvas & Computers View

**Files:**
- Create: `src/components/computers/TopologyCanvas.tsx`
- Create: `src/components/computers/PairingPinModal.tsx`
- Modify: `src/components/computers/ComputersView.tsx`

**Interfaces:**
- Consumes: `useKvmStore`
- Produces: Interactive 2D drag-and-drop monitor layout canvas with edge snap guides, PIN pairing dialog, and live KVM status.

- [x] **Step 1: Implement `src/components/computers/TopologyCanvas.tsx`**

Build 2D canvas where users can drag Primary PC and Secondary PC monitor rectangles to align them side-by-side or stacked, displaying edge connection lines.

- [x] **Step 2: Implement `src/components/computers/PairingPinModal.tsx`**

Modal prompting user with 6-digit numeric PIN for new peer computer linking.

- [x] **Step 3: Integrate into `src/components/computers/ComputersView.tsx`**

Replace static scaffolding with live peer list, topology editor, and link toggle button.

- [x] **Step 4: Verify frontend build & tests**

Run: `npm test && npm run build`
Expected: PASS (0 errors).

- [x] **Step 5: Commit Task 8**

Run: `git add src/components/computers/ && git commit -m "feat(ui): implement interactive 2D topology canvas and KVM pairing modal"`

---

### Task 9: End-to-End Verification, Documentation & Atomic Squash

**Files:**
- Modify: `README.md`
- Modify: `docs/specs/SPEC-12-multi-pc-hub-and-virtual-kvm.md`

**Interfaces:**
- Produces: 100% test coverage, verified production build, updated documentation, and atomic squash commit.

- [x] **Step 1: Run complete test suite and compilation verification**

Run: `npm test && npm run build && cargo check --manifest-path src-tauri/Cargo.toml`
Expected: All pass with 0 errors.

- [x] **Step 2: Update `README.md`**

Add Virtual KVM (Computer Link) to Features section with usage guide.

- [x] **Step 3: Update `SPEC-12` status to Completed**

Mark `SPEC-12` as Implemented / Target Version `v1.0.0`.

- [x] **Step 4: Squash feature commits into single atomic commit per `AGENTS.md`**

Squash into:
`feat(kvm): implement multi-PC virtual KVM and edge leap input routing (SPEC-12)`

---

## Plan Verification Checklist

- [x] `npm test` passes 100% of test suites.
- [x] `cargo check --manifest-path src-tauri/Cargo.toml` compiles with 0 errors.
- [x] `npm run build` succeeds without TypeScript errors.
- [x] Emergency hotkey (`ScrollLock` / `Ctrl+Alt+Escape`) releases cursor trap immediately.
- [x] Bidirectional clipboard synchronization works without echo loops.
- [x] All commits squashed into single atomic feature commit per `AGENTS.md`.
