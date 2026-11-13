# SPEC-23: Shared Mouse & Keyboard — Streamlined UX, Role-Aware Link Lifecycle & Offset-Aware Edge Mapping

> **Status:** Approved (`2026-11-14`)
> **Scope:** `(kvm)` — Follows SPEC-19 (QUIC transport & pairing) and SPEC-22 (input fidelity & fixes).

## 1. Naming & Roles (User-Facing Text Only)

Internal code names (`kvm`, `receiver`, `controller`, `hooks`, `KvmStatus`, etc.) remain unchanged in code; user-facing UI strings use the streamlined terminology below (see `CONTEXT.md` for the full UI-term ↔ code-term glossary):

- **Feature name:** `Shared Mouse & Keyboard`
- **Roles:**
  - **Main PC** — the PC whose physical mouse and keyboard are used (runs input hooks, QUIC client / controller).
  - **Linked PC** — the PC being controlled (runs QUIC server / receiver, injects input).
- **Terms:**
  - `Pairing code` (replaces `PIN`)
  - `Link` / `Unlink` / `Forget`
  - `Bring cursor back` (replaces `Release Cursor Now`)
- **Receiver toggle:** `Allow other PCs to control this PC` replaces `Start/Stop KVM Receiver`.
  - Default **ON** while the app runs (persisted per machine).
  - **No** app autostart and **no** auto-reconnect: a link only starts when the Main PC explicitly clicks `Link`.
  - Pairing code is required only on the first link; subsequent links recognise the peer by certificate fingerprint.
- **Cardinality:** One Linked PC at a time.

## 2. Per-Role UI (Computers Tab)

- **Not linked:** status card + allow-control toggle + pairing code + discovered PCs with `Link`.
- **Main PC (active outgoing link):** layout canvas + edge side (`left` / `right` / `top` / `bottom`) + deadzone + dwell + `Unlink` + `Bring cursor back`.
- **Linked PC (being controlled):** `Controlled by <name>` card + Trusted PCs list with `Forget` + `Unlink` (actually closes the active incoming session on the server). No layout canvas, no controller-only controls, and no file drop zone.

## 3. Layout & Offset-Aware Edge Mapping (Path A)

- The layout canvas renders every monitor of both PCs to scale at their real virtual-desktop positions (fix M4).
- The Linked PC can be placed `left` / `right` / `top` / `bottom` of the Main PC union, with a physical-pixel offset along the shared edge (`offset_y` for `left`/`right`, `offset_x` for `top`/`bottom`), representing the peer union position relative to the local union along the shared edge.
- **Mapping rule (backend):**
  - `CursorEnter.relative_y` (leap) is the position along the shared edge normalised over the **receiver union** (`0.0..=1.0`) after applying the link's pixel offset (`offset_y` for `left`/`right`, `offset_x` for `top`/`bottom`). The receiver maps it within its union onto the entry-edge monitor.
  - `CursorReturn.relative_y` (return) is normalised over the **receiver union** (`0.0..=1.0`) when crossing the receiver's entry edge; the controller converts it back using the link offset and receiver union size, then clamps to the appropriate controller edge monitor (`left`/`right`/`top`/`bottom`).
- Canvas state syncs from the active link on reconnect (M2); drag-end submits the final offset (fix H1 stale closure).

## 4. Audit Bug Fixes

- **H1:** Drag-end submits the final offset and backend applies offsets to both leap and return mapping across all four edges (`left`, `right`, `top`, `bottom`).
- **H2:** Recognise peers by certificate fingerprint, not IP address; update the stored `address` in the trust store on fingerprint match; require the pairing code only when the fingerprint is unknown.
- **H3:** `Unlink` refreshes status; turning `allow_control` OFF stops only the receiver server and **never** stops controller hooks or closes an active outgoing client link while this PC is a Main PC.
- **M1:** `refreshStatus` avoids read-modify-write races and prunes peers absent from `status.peers`.
- **M2:** Changing edge or offset refreshes `status.controller_edge` and syncs canvas state on reconnect.
- **M5:** `Forget` uses the peer's real `computer_id` and closes any active session (in either role) with that peer.
- **L1:** Clipboard/file sync toggles in Settings apply immediately (`kvm_set_sync_settings` on change); remove unused `indicatorRipple` and `emergencyKeyCombo` settings.

**Out of scope:** Per-monitor arrangement (future SPEC-24), multiple simultaneous Linked PCs, app autostart.

## 5. Backend ↔ Frontend Contract

### Tauri Commands

- `kvm_set_allow_control(enabled: bool) -> KvmStatus` — starts or stops the receiver server (`allow_control`). Never touches controller hooks or an active outgoing client link.
- `kvm_disconnect_peer() -> ()` — role-aware `Unlink`: if this PC has an outgoing client link, closes it and stops controller hooks; otherwise closes every incoming controller session on the server while leaving the receiver server listening.
- `kvm_forget_peer(computer_id: String) -> ()` — removes the trust entry for `computer_id` and closes any active session (either role) with that peer.
- `kvm_list_trusted() -> Vec<TrustedPeerInfo { computer_id: String, name: String, address: String }>` — returns all paired peers from the trust store.
- `kvm_start_server` / `kvm_stop_server` — retained for compatibility; UI uses `kvm_set_allow_control`.

### Data Types

- `KvmStatus` additions (existing fields `controller_edge`, `receiver_entered`, `peers`, etc. remain):
  - `allow_control: bool` — receiver server running.
  - `linked_peer: Option<LinkedPeerInfo { computer_id: String, name: String, role: "main" | "linked" }>` — the single active link and this PC's role in it (`None` when unlinked).
- `TopologyLink`:
  - `from_edge` / `to_edge`: `"left" | "right" | "top" | "bottom"`.
  - `offset_x: Option<i32>` alongside `offset_y: Option<i32>` (physical pixels; peer union position relative to local union along the shared edge).

## 6. Verification

- **Backend unit tests (`cargo test`):**
  - Fingerprint-based reconnect when the peer's IP changes, updating stored trust addresses on both sides.
  - Offset-aware leap and return mapping across `left`, `right`, `top`, and `bottom` edges (including out-of-span clamping and multi-monitor controller edge selection).
  - Turning `allow_control` OFF preserves active controller hooks and outgoing client link.
  - `kvm_forget_peer` and role-aware `kvm_disconnect_peer` close active sessions in both Main PC and Linked PC roles.
- **Quality gates:** `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo check --manifest-path src-tauri/Cargo.toml`, `npm test`, `npm run build`.
