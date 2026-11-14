# SPEC-22: Computer Link — Input Fidelity & Reliability Fixes

> **Status:** Implemented / Shipped in v1.0.0
> Scope: `(kvm)`. Follows SPEC-19 (QUIC transport). Fixes defects found in a code audit of the Computer Link feature.

## 1. Audit findings

| # | Defect | Location (pre-fix) | Part |
|---|--------|--------------------|------|
| 1 | Double pointer acceleration: controller deltas come from accelerated `pt`, receiver injects relative `MOUSEEVENTF_MOVE`, which is accelerated again | `hooks.rs`, `injector.rs` | A |
| 2 | Fast motion truncated: cursor clipped to a 60×60 rect, so any event > 30 px is clamped | `hooks.rs` `engage_leap` | A |
| 3 | Return-to-local decided from a controller-side virtual X that drifts from the real receiver cursor → early returns / stuck at edge | `hooks.rs` mouse proc | A |
| 4 | Peer resolution hard-coded 1920×1080 (manual connect, receiver topology, primary-monitor-only beacon) | `commands.rs`, `kvmStore.ts`, `discovery.rs` | A |
| 5 | Receiver always enters on the primary monitor (`SM_CXSCREEN`); vertical offset ignored | `commands.rs` CursorEnter | A |
| 6 | Stuck keys/buttons: keys held across a transition stay down on one side; dragging across an edge leaves a button down | `hooks.rs` | A |
| 7 | `Mutex::lock().unwrap()` inside low-level hook callbacks (panic across FFI = abort; slow hooks get silently removed by Windows) | `hooks.rs` | A |
| 8 | `CursorPosition` sent on every mouse event over the reliable stream and emitted to the UI per event | `hooks.rs`, `commands.rs` | A |
| 9 | Edge detection only at the virtual-desktop bounding box; recenter point clamped to `60..1020` | `hooks.rs` | A |
| 10 | Settings toggles (sync text / image / files, emergency combo) never reach the backend; clipboard always syncs | `SettingsPanel.tsx`, `clipboard.rs` | B |
| 11 | Clipboard prefers image over text, so copying from spreadsheets/documents/browsers sends a bitmap | `clipboard.rs` | B |
| 12 | Discovery broadcasts only to `255.255.255.255` (one adapter); fails with VPN / virtual adapters | `discovery.rs` | B |
| 13 | "Link PC" on a discovered, unpaired peer shows an error instead of the PIN dialog | `ComputersView.tsx` | B |
| 14 | Developer IP hard-coded as manual-connect default | `ComputersView.tsx` | B |
| 15 | One peer disconnecting on the receiver clears all peers and drops this PC's own controller session | `commands.rs` | A |
| 16 | Computer id derived from hostname (`<host>-host`) — collides across machines with equal names | `commands.rs` | A |

## 2. Part A — Input core (controller ↔ receiver)

### 2.1 Roles
Controller = PC whose physical mouse/keyboard is used (runs hooks, QUIC client). Receiver = PC being controlled (QUIC server, injects input).

### 2.2 Monitor layout exchange
- `Pair` and `PairAck` gain `monitors: Vec<MonitorDisplay>` (all monitors, virtual-desktop coordinates). Both sides store the peer's layout for the session.
- Frontend topology uses the received layout for the peer node size (union of monitors) instead of 1920×1080 defaults. Remove the hard-coded defaults in `kvmStore.ts` and the receiver's `ComputerNode` push.

### 2.3 Controller capture
- **Edge detection per monitor:** a leap triggers when `pt` is within 2 px of the link edge of the monitor containing `pt` **and** there is no local monitor beyond that edge at that `y` (`MonitorFromPoint(..., MONITOR_DEFAULTTONULL)` on the neighbouring pixel). Deadzone and dwell resistance keep their current meaning.
- **No leap while any mouse button is held** (track button state in the hook).
- **On leap:** remember `relative_y` = position along the edge monitor's height (0..1). Move the cursor to the **centre of that monitor**, hide it, `ClipCursor` to that monitor's rect (not 60×60). While trapped, every move event is blocked and `delta = pt − centre`, then the cursor is reset to centre. Max delta per event is therefore half a monitor.
- Deltas go out as QUIC datagrams only. The controller no longer sends `CursorPosition` and no longer tracks a virtual remote X/Y.
- **Held keys on leap:** the keyboard hook tracks keys currently down (ignore events with `LLKHF_INJECTED`). On leap, inject key-up locally for every tracked key (these are injected, so the hook lets them pass) so nothing stays stuck on the controller.

### 2.4 Receiver positioning (authoritative)
- `CursorEnter { edge, relative_y }`: pick the receiver monitor on `edge` side (leftmost/rightmost monitor whose vertical span contains the mapped point; nearest if none), place the cursor 2 px inside that edge at `relative_y` of that monitor's height.
- On each delta: `next = GetCursorPos() + (dx, dy)`. If `next` crosses the **entry edge** outward (no receiver monitor beyond it at that `y`), send `CursorReturn { edge, relative_y }` to the controller (relative to the monitor it leaves from) and stop injecting until the next `CursorEnter`. Otherwise clamp `next` to the monitor nearest to it (`MonitorFromPoint` DEFAULTTONEAREST rect) and place it with `SetCursorPos` (absolute — no second acceleration).
- Receiver sends `CursorPosition { norm_x, norm_y }` (normalised over the union of its monitors) to the controller at most **20 Hz**, only while the cursor is entered. The controller emits it to its UI (`kvm-cursor-pos`). The receiver no longer emits `kvm-cursor-pos` per event.

### 2.5 Controller return
On `CursorReturn` from the receiver: untrap, `ClipCursor(None)`, show the cursor, place it 2 px inside the link edge of the local edge monitor at `relative_y`. Emergency release keeps working as today and additionally sends `ReleaseAll`.

### 2.6 Stuck-input safety on the receiver
The receiver tracks every key and mouse button it injected as down. On `CursorReturn` (sent by itself), `ReleaseAll`, `EmergencyRelease`, or connection loss it injects the matching up events and clears the set.

### 2.7 Hook robustness
- No `unwrap()`/`expect()` in hook callbacks or code they call. Use atomics, or `try_lock()` and fall through to `CallNextHookEx` when contended.
- Wrap each hook callback body in `std::panic::catch_unwind`; on panic, untrap, restore the cursor, and call `CallNextHookEx`.
- No allocation-heavy work in callbacks (no `String` clones per event: store the edge as an atomic enum).

### 2.8 Session bookkeeping
- `handle_connection` knows the peer `computer_id`; on close it emits `PeerDisconnected { computer_id }`. The receiver removes only that peer and never touches `kvm.client`.
- Computer id = first 16 hex chars of the local identity certificate fingerprint (stable, unique). Display name stays the hostname.

### 2.9 Protocol changes (summary)
Add `KvmPacket::ReleaseAll`, `PeerDisconnected { computer_id }` (internal), `monitors` field on `Pair`/`PairAck`. `CursorReturn` and `CursorPosition` now flow receiver → controller.

## 3. Part B — Clipboard, discovery, settings, UI

### 3.1 Sync settings reach the backend
- New command `kvm_set_sync_settings { text: bool, image: bool, files: bool }` (defined next to the clipboard code; register in `lib.rs`). Values held in atomics.
- Clipboard watcher sends text only if `text`, images only if `image`.
- Receiving side: an incoming file stream is rejected (stream reset, `.part` removed, no event) when `files` is false; incoming clipboard text/image is ignored when its toggle is false.
- Frontend calls it on startup (with persisted settings) and whenever a toggle changes — from `SettingsPanel.tsx` and the Computers view mount, not from `kvmStore.ts`.
- Emergency combo becomes fixed: Scroll Lock, Pause, or Ctrl+Alt+Esc. Replace the free-text setting with a read-only line listing them.

### 3.2 Clipboard priority
If `CF_UNICODETEXT` is available, send text (and not the image). Send an image only when no text format is present.

### 3.3 Discovery on every adapter
Send the beacon to the directed broadcast address of every up, non-loopback IPv4 interface (and to `255.255.255.255` as before). A small dependency such as `if-addrs` is acceptable. Pure helper `broadcast_addr(ip, netmask) -> Ipv4Addr` with tests.

### 3.4 UI fixes
- "Link PC" on a discovered peer: if the error says the peer is not paired, open the PIN dialog for that peer (same as the manual path).
- Manual IP input starts empty with placeholder `192.168.x.x`.
- Copy fixes: drop the "PHASE 1" badge; rename "UDP Mouse Acceleration" to "Low-latency pointer (QUIC datagrams)".

## 4. Verification
Automated (`cargo test`, `npm test`) — pure functions extracted so they test without Win32:
1. Receiver step: `(cursor, delta, monitors, entry_edge) -> Move(pos) | Return(relative_y)`; cases: inside move, clamp at non-entry edge, return through entry edge, multi-monitor with different heights, gap between monitors.
2. Controller edge detection: `(pt, monitors, link_edge) -> Option<relative_y>`; cases: non-rectangular layouts, shorter side monitor, deadzone.
3. Receiver mapping of `CursorEnter` onto the correct monitor.
4. Pressed-key tracker: down/up, injected events ignored, release-all list.
5. Loopback QUIC: controller enters, sends deltas, receiver returns `CursorReturn` when crossing the entry edge; `ReleaseAll` delivered; `PeerDisconnected` removes only that peer.
6. Clipboard priority function; sync-settings gating; `broadcast_addr`.
Plus AGENTS.md gates: `npm test`, `cargo check`, `npm run build`.

Manual (two PCs): fast flicks reach across the whole remote screen; no early/late returns; multi-monitor receiver reachable; Ctrl/Shift held across an edge do not stick; drag across an edge is blocked; copy from a spreadsheet pastes text.
