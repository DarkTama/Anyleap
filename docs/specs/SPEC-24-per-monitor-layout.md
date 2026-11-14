# SPEC-24: Per-Monitor Layout Mapping (Display-Settings Style)

Status: Approved 2026-11-14

## Goal

The Linked PC's monitors appear to scale at their **real relative arrangement**, and the entire Linked PC block can be placed anywhere around the Main PC's monitors (with magnetic snapping, no gaps, and no overlaps). Every segment where a Linked PC monitor edge touches a Main PC monitor edge becomes a **seam**; the cursor can leap and return through **any** seam and lands on the target monitor at the matching coordinate along that seam. Inside each PC, cursor movement follows that PC's real monitor layout.

Out of scope:
- More than one Linked PC at a time
- Moving individual Linked PC monitors independently (the Linked PC block moves as one)
- Linked PC chaining

## Data Model

### `MonitorDisplay`
- Gains `device_name: String` (populated from `MONITORINFOEXW.szDevice` on Windows, e.g. `\\.\DISPLAY1`).
- `id` uses `device_name` so monitor identifiers remain stable across reconnects and enumeration order changes.
- Existing fields (`x`, `y`, `width`, `height`, `is_primary`) are preserved.

### `TopologyConfig`
- Gains `remote_origin: Option<Point { x: i32, y: i32 }>`, representing where the Linked PC's virtual-desktop origin `(0, 0)` sits in the Main PC's virtual-desktop coordinates (physical px).
- When present, `remote_origin` supersedes SPEC-23 link `edge` + `offset_px` placement.
- **Migration**: If `remote_origin` is absent and a SPEC-23 `TopologyLink` exists for the Linked PC, `remote_origin` is derived from that link's `edge` and offsets (`offset_px` / `offset_x` / `offset_y`) so existing behavior is preserved cleanly.

### `Seam` (Controller / Shared Pure Representation)
Computed deterministically from `main_monitors`, `linked_monitors`, and `remote_origin` (pure Rust function in backend; matching pure TypeScript helper in frontend for canvas rendering):
- Fields: `{ main_monitor_id, linked_monitor_id, side: left | right | top | bottom, start, end }`
  - `side` is the side of the **Main** monitor that touches the Linked monitor.
  - `start` / `end` define the strictly positive overlap span along that touching boundary in Main PC coordinates.
- Only real touching edges (shared boundary line with `end > start`) form seams.
- `dwell_ms` and `deadzone_px` apply **per seam** (deadzone trims both `start` and `end` ends of each seam).
- Overlapping placements (any Main monitor intersecting any Linked monitor at `remote_origin`) are invalid and rejected.

## Protocol Additions

All new fields carry `#[serde(default)]` and new variants are additive for wire compatibility with SPEC-23 peers:

- **`KvmPacket::CursorEnter`** gains:
  - `target_monitor: Option<String>` — `device_name` / `id` of the destination Linked monitor.
  - `along_px: Option<i32>` — cursor offset along the seam in the **target** monitor's local coordinates.
- **`KvmPacket::CursorReturn`** gains:
  - `target_monitor: Option<String>` — `device_name` / `id` of the destination Main monitor.
  - `along_px: Option<i32>` — cursor offset along the seam in the **Main** monitor's local coordinates.
- **`KvmPacket::Seams { seams: Vec<LinkedSeam> }`** (sent Main -> Linked on link establishment and whenever topology changes):
  - `LinkedSeam { linked_monitor_id, side, start, end, main_monitor_id, main_offset }`
  - `side` is the side of the **Linked** monitor (`left | right | top | bottom`).
  - `start` / `end` are the seam span in **Linked PC** virtual-desktop coordinates.
  - `main_offset` maps a coordinate along the Linked seam directly to the matching local coordinate on `main_monitor_id` (`main_along = perp + main_offset`).

## Runtime Behavior

### Controller (Main PC)
- Leaps only from Main-monitor edge segments covered by a `Seam` (outside that seam's `deadzone_px` and after per-seam `dwell_ms`).
- Sends `CursorEnter` populated with `target_monitor`, `along_px`, plus fallback `edge`/`relative_y`.
- On `CursorReturn`, restores the Main PC cursor onto the specified `target_monitor` at `along_px` along the opposite side (falling back to SPEC-23 placement if `target_monitor`/`along_px` are absent).

### Receiver (Linked PC)
- On receiving `KvmPacket::CursorEnter`, warps the cursor onto `target_monitor` (when present and valid) at `along_px` along the entry edge, falling back to SPEC-23 mapping if absent.
- When `Seams` have been received, returns only when the cursor crosses a Linked-monitor edge segment covered by a `LinkedSeam` (after per-seam `dwell_ms` and outside per-seam `deadzone_px`), populating `CursorReturn` with `target_monitor` and `along_px`. Crossing any edge segment with no seam clamps within the receiver desktop.
- Legacy SPEC-23 peers that never send `KvmPacket::Seams` retain SPEC-23 entry-edge return behavior.

### Frontend Canvas
- Draws both PCs' monitors to scale.
- Linked PC block is draggable and snaps magnetically (`<= 24` screen px) so at least one Linked edge touches a Main monitor edge with positive overlap.
- Overlapping placements are rejected (and `kvm_update_topology` rejects overlapping layouts).
- Active seams are highlighted in gold (`--gold-400`).
- Edge buttons (`Left` / `Right` / `Top` / `Bottom`) act as quick presets computing a `remote_origin`.
- Saving sends `remote_origin` via `kvm_update_topology`.

### Monitor Configuration Changes
- When monitor count or resolution changes on either PC while a `remote_origin` is configured, the layout falls back to a preset placement on the previously used side, updates seams, and emits `"kvm-layout-notice"` so the user is notified.

## Verification
- Pure Rust + TS unit tests covering:
  - Seam computation (multi-monitor asymmetric arrangements including 1920x1080 landscape + 1080x1920 portrait at `(1920, -420)` with Linked 1920x1200 placed left, bottom, and spanning under both Main monitors).
  - Snapping (`<= 24` screen px, gap/overlap rejection).
  - Controller leap & receiver `CursorEnter` target monitor/offset mapping.
  - Receiver return gating (returns only on seam segments, clamps on non-seam edges) & controller `CursorReturn` mapping onto non-primary Main monitors.
  - Migration from SPEC-23 `TopologyLink` edge/offset to `remote_origin`.
  - Monitor-change preset fallback + notice.
- Loopback QUIC integration test verifying `KvmPacket::Seams` delivery, `CursorEnter` onto a specific non-primary monitor, clamping on a non-seam edge, and `CursorReturn` through a non-primary seam landing on the right Main monitor.
- Quality gates: `cargo test`, `cargo check --all-targets`, `npm test`, `npm run build`.
