# SPEC-25a — Files Across PCs + Pointer Scheme

> **Status:** Approved 2026-11-15

## Goal

Extend the multi-PC virtual KVM (Windows) with seamless cross-PC file drag-and-drop, file copy-paste (`CF_HDROP`), and optional pointer scheme synchronization while preserving existing security boundaries, hook callback constraints, and per-monitor seam routing (SPEC-19, SPEC-23, SPEC-24).

Out of scope:
- Virtual/shell-only drag items (`CFSTR_FILEDESCRIPTOR` / `CFSTR_FILECONTENTS`) — real filesystem files and folders via `CF_HDROP` only in v1
- Writing cursor scheme entries to the Linked PC's registry
- More than one Linked PC at a time

## Drag & Drop Between PCs (Experimental, Default Off)

> **Experimental in v1.1.0 (`dragDrop` defaults to `false`):** Cross-PC file copy-paste (`CF_HDROP`) is the primary supported file transfer path. Cross-PC drag & drop is disabled by default; when disabled, no edge catcher is shown or polled, no drag-mode leap occurs, no `350 ms` return hold applies, and incoming `DragOffer` packets are rejected cleanly (`DragResult { outcome: "cancelled" }`).
>
> **Known limitations:**
> - Target apps (including Chromium/Electron web apps or elevated windows) may reject synthesized cross-PC OLE drops (`DROPEFFECT_NONE` / `effect = 0`) or block `DoDragDrop` initiation (`0x80004005`) if foreground/capture ownership changes mid-transfer.
> - Releasing the mouse button while files are still streaming/staging cancels the drag rather than completing a late drop.

### Source Side (Main -> Linked, and Linked -> Main Mirror)
- When `dragDrop` is explicitly enabled, a peer is linked, a mouse button is held, and the cursor approaches/reaches an active seam (or return seam on Linked PC), AnyLeap shows an invisible (`WS_EX_LAYERED` alpha=1), topmost (`WS_EX_TOPMOST`), non-activating (`WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`) 2-px **edge catcher** window along that seam registered as an OLE drop target (`RegisterDragDrop`). Outside an active button-held approach to a seam (or whenever `dragDrop` is `false`), the edge catcher is hidden (`HWND_NOTOPMOST` + `SW_HIDE`) and its OLE drop registration is revoked (`RevokeDragDrop`).
- If an OLE drag enters the edge catcher (`IDropTarget::DragEnter`), AnyLeap reads `CF_HDROP` (real files/folders only; non-`CF_HDROP` drags are ignored).
- On catching valid filesystem paths:
  - A helper thread injects `Escape` (`VK_ESCAPE`) to cancel the local `DoDragDrop` without releasing the physical mouse button (never injected from a low-level hook callback).
  - The controller leaps in **drag mode** (bypassing the SPEC-23 "no leap while a mouse button is held" rule only for this case, using `deadzone + 2px` seam tolerance for the 2-px strip) and sends `KvmPacket::DragOffer { drag_id, files, button }` followed by asynchronous `StreamHeader::DragFile { drag_id, rel_path, size }` unidirectional QUIC streams (`64 KiB` chunks read via `spawn_blocking`). It does not pre-inject a raw `MouseButton { is_down: true }` before staging finishes.
  - On Linked -> Main (mirror), the Linked PC hosts the 2-px edge catcher along its active return seams, cancels its local drag on catch, sends `DragOffer` + `DragFile` streams, and returns control to the Main PC in drag mode while preserving the Main PC's held button state. On the Main PC, the low-level mouse hook ignores injected events (`LLMHF_INJECTED` / `LLMHF_LOWER_IL_INJECTED`) for button-state tracking, never swallows the physical button-up ending the drag, and initializes the tracked remote button state on `CursorReturn` and `DragOffer` from the hook's physical held state rather than `GetAsyncKeyState` alone.

### Target Side Staging & `DoDragDrop`
- Files are pre-staged on the target PC under `%LOCALAPPDATA%\AnyLeap\staging\<drag_id>\` using sanitized relative paths (`sanitize_rel_path`, rejecting traversal, drive prefixes, control/invalid characters, and Windows reserved names).
- Progress is emitted to the UI on both sender and receiver (throttled to `<= 10 events/s`) via `kvm-transfer-progress { id, kind, name, pct, state, sizeBytes }` (`state` in `"awaiting_fetch" | "active" | "done" | "failed" | "cancelled"`), with `kvm-continuity-notice` emitted on failure. During file streaming/staging and while awaiting the target drop, drag progress stays `"active"` (`<= 99%`).
- If total offered bytes exceed `stagingLimitMb` (default `2048` MiB):
  - No target-side `DoDragDrop` is started.
  - Files are saved into `Downloads\AnyLeap_Transfers`, `kvm-continuity-notice` is emitted, and `KvmPacket::DragResult { drag_id, outcome: Done, notice: None }` completes the sender progress bar.
- When staging completes within `stagingLimitMb`, the target PC sends `KvmPacket::DragReady { drag_id }` and starts its own OLE `DoDragDrop` from a transient `WS_EX_LAYERED` topmost hit-testable (`WM_NCHITTEST -> HTCLIENT`) window positioned under the live cursor. Before injecting any synthetic mouse-down, AnyLeap verifies `WindowFromPoint` hits its own drag window (aborting cleanly without clicking if blocked by another window), injects the click onto it, calls `SetCapture` on the drag window (logging any prior or conflicting capture owner), and only then adds `WS_EX_TRANSPARENT` before entering `DoDragDrop`.
- Target `DoDragDrop` termination & cleanup: concurrent target drags supersede and wait for the prior drag to exit cleanly; `QueryContinueDrag` cancels (`DRAGDROP_S_CANCEL`) on `Escape`, `DragCancel`/`TransferCancel`/`ReleaseAll`/`EmergencyRelease`/disconnect, if neither physical nor tracked remote button is down for `> 300 ms`, or after a `60 s` overall timeout (with a `50 ms` timer keeping `QueryContinueDrag` alive while the user pauses mid-drag). An independent watchdog thread monitors cancel signals, tracked button-up `> 300 ms`, `60 s` timeout, and `QueryContinueDrag` liveness stalls `> 1 s`. Forced termination (both `drag_source_wndproc` and the watchdog) **always cancels and never triggers a drop**: it posts only `WM_CANCELMODE`, `WM_KEYDOWN`/`WM_KEYUP` (`VK_ESCAPE`), and `WM_MOUSEMOVE` (never synthetic mouse button-up messages) to the drag and capture windows/thread (and `SendInput(VK_ESCAPE)` only if the drag window/thread owns foreground or capture) until `QueryContinueDrag` exits with `DRAGDROP_S_CANCEL`. Only a real tracked physical/remote button-up reaches `DRAGDROP_S_DROP`. Every exit path injects the matching button-up when a synthetic down was injected, calls `ReleaseCapture`, destroys the initiation window, calls `OleUninitialize`, clears target-drag state atomics, and logs `TARGET_DND_DONE` with reason. While KVM debug logging is enabled, `DND_STATE` is sampled every `5 s` on change (including foreground and AnyLeap-thread capture ownership), and `KEY`/`REMOTE_KEY` debug logs redact raw `vk`/`scan` to a key class except for modifiers, `Esc`, `Enter`, `Tab`, `Backspace`, function keys, media/volume keys, and Windows keys.
- Terminal outcome synchronization (`DragTransferRegistry` + `KvmPacket::DragResult`): on target `DoDragDrop` exit, the outcome is classified into `"done"` (`DRAGDROP_S_DROP` with `effect != 0`), `"failed"` (`effect == 0` / OLE error / `hittest_blocked` / button-lost or stall timeout — emitting a `kvm-continuity-notice` such as `"Drop was not accepted by the target app"`), or `"cancelled"` (`Escape`, `DragCancel`, `TransferCancel`, disconnect, disabled, etc.). Both target and source emit the terminal `kvm-transfer-progress` row via `KvmPacket::DragResult { drag_id, outcome, notice }`, and any active drags are transitioned to `"cancelled"` on disconnect.
- If the user cancels (`Escape` / `DragCancel` / `kvm_cancel_transfer`) or releases before staging completes, cancellation is sent (`DragCancel` / `TransferCancel` / `DragResult`), streams are reset, and staging/partial files are cleaned up with `state: "cancelled"` on both PCs.

### Staging Lifecycle, 4096 MB Cap, & Cleanup
- Total disk usage under `%LOCALAPPDATA%\AnyLeap\staging\` is capped at `4096 MB`. Before accepting new staging allocations, AnyLeap LRU-evicts the oldest **finished** drag/clip entries (`STAGING_EVICT`), never evicting active transfers, `open-*`, or `_cursors`. `OpenFile` (`open-*`) entries count toward the `4096 MB` cap and refuse new incoming opens when cap cannot be satisfied.
- Staged directories for a `drag_id` / `offer_id` are removed when the drag finishes, when replaced by a newer clip offer, when cancelled, on disconnect/unlink, and any staging entries older than 24 hours are swept automatically. Failed or cancelled transfers remove partial `.part` files.

### Transport Prioritization During Bulk Transfers
- Bulk file streams (`DragFile`, `ClipFile`, `OpenFile`, `CursorFile`) use `BULK_STREAM_PRIORITY = -10` so mouse datagrams stay prioritized over bulk transfers. Outbound `DragFile`/`ClipFile` streaming reads sequential `64 KiB` chunks in a `spawn_blocking` task over a bounded channel and yields cooperatively between writes, while the Linked PC's mouse-delta injection runs on a dedicated elevated-priority thread (`kvm-delta-inject`) with queue coalescing and `RecvError::Lagged` resilience so outbound bulk transfers never starve receiver cursor updates.
- QUIC transport settings are centralized in `identity::transport_tuning` (`CubicConfig`, `4 MiB` connection receive window, `1_250_000`-byte stream receive window, `4 MiB` send window), keeping buffer bloat bounded while sustaining LAN bulk throughput.

## Copy-Paste Files Between PCs

- When `fileClipboard` is enabled and `CF_HDROP` appears on the clipboard (excluding echo-suppressed local writes), AnyLeap enumerates real paths and sends `KvmPacket::FileClipOffer { offer_id, files }`.
- Only one clipboard file offer is held at a time: any new `FileClipOffer` (outbound or inbound) deletes/cancels the previous offer and its staging directory (`CLIP_REPLACE_PREV`).
- **Prefetch limit (`clipPrefetchLimitMb`, default `100` MiB; `0` disables auto-fetch):**
  - If total size `<= clipPrefetchLimitMb` (and `> 0`), the sender immediately streams `StreamHeader::ClipFile { offer_id, rel_path, size }` asynchronously in `64 KiB` chunks (`state: "active"`).
  - If total size `> clipPrefetchLimitMb` (or `clipPrefetchLimitMb == 0`), streaming is deferred (`CLIP_AWAIT_FETCH`): the receiver emits `kvm-transfer-progress` with `state: "awaiting_fetch"` and holds the metadata for up to 10 minutes (`600s`). When the user calls `kvm_fetch_clip(offerId)`, the receiver sends `KvmPacket::ClipFetchRequest { offer_id }` (`CLIP_FETCH_REQ`) and the sender starts streaming the deferred `ClipFile` streams.
- If total size `<= stagingLimitMb`, the peer stages the files into `%LOCALAPPDATA%\AnyLeap\staging\<offer_id>\` and only upon completion places `CF_HDROP` of the top-level staged paths onto its clipboard with echo suppression so `Ctrl+V` in Explorer pastes them. Failed or cancelled transfers clean up `.part`/staging files.
- If total size `> stagingLimitMb`, fetched files are saved to `Downloads\AnyLeap_Transfers` without overwriting the clipboard, and `kvm-continuity-notice` is emitted.

## User Cancellation on Both PCs

- Either sender or receiver can call `kvm_cancel_transfer(id)` to cancel an active, waiting-for-drop, or `awaiting_fetch` transfer (`drag`, `clip`, or `open`). For already-finished drag IDs (`"done"` / `"failed"` / `"cancelled"`), `kvm_cancel_transfer` cleans up any lingering staging directory without clobbering the terminal state.
- Both sides exchange `KvmPacket::TransferCancel { id }` (`TRANSFER_CANCEL_SEND` / `TRANSFER_CANCEL_RECV`), reset associated unidirectional QUIC streams (`QUIC_CANCEL_ERROR_CODE`), delete partial `.part` and staging directories, and emit `kvm-transfer-progress` with `state: "cancelled"` on both PCs.

## Pointer Scheme Sync (Optional, Default Off)

- Controlled by `pointerScheme` (default `false`).
- Once per link (and when toggled on while linked), the Main PC reads `HKCU\Control Panel\Cursors` cursor roles and their `.cur`/`.ani` file paths, sends `KvmPacket::CursorSchemeOffer { roles }`, and streams each cursor file as `StreamHeader::CursorFile { role, name, size }`.
- The Linked PC caches received cursor files in staging (never writing to the Linked PC's registry).
- While the remote cursor is entered on the Linked PC (`CursorEnter` active), Linked PC applies the received cursors via `SetSystemCursor(LoadCursorFromFileW(...), OCR_*)`.
- On cursor return, `EmergencyRelease` / `ReleaseAll`, disconnect/unlink, or crash recovery, Linked PC restores default cursors via `SystemParametersInfoW(SPI_SETCURSORS, 0, None, SPIF_SENDCHANGE)`.

## Protocol, Commands, and UI Events

- `KvmPacket` additions: `DragOffer`, `DragReady`, `DragCancel`, `DragResult { drag_id, outcome, notice }`, `FileClipOffer`, `ClipFetchRequest`, `TransferCancel`, `CursorSchemeOffer`, plus `FileMeta { rel_path, size, is_dir }` and `DragOutcome`.
- `StreamHeader` additions: `DragFile`, `ClipFile`, `CursorFile`.
- Tauri commands:
  - `kvm_set_files_settings(dragDrop: bool, fileClipboard: bool, stagingLimitMb: u32, pointerScheme: bool, clipPrefetchLimitMb?: u32)` (default `clipPrefetchLimitMb = 100`)
  - `kvm_fetch_clip(offerId: string)`
  - `kvm_cancel_transfer(id: string)`
- UI events:
  - `kvm-transfer-progress { id, kind: "drag" | "clip" | "open", name, pct, state: "awaiting_fetch" | "active" | "done" | "failed" | "cancelled", sizeBytes }`
  - `kvm-continuity-notice { text }`

## Verification

- Pure unit tests:
  - Edge-catcher trigger decisions, 2-px seam strip geometry, and target `DoDragDrop` termination decisions (buttons/time/cancel-signal -> continue/drop/cancel)
  - Debug-log key privacy classification/allowlist and `DND_STATE` change deduplication
  - Staging limit decisions (`Staging` vs `DownloadsFallback`), sanitized relative paths, top-level `CF_HDROP` path resolution, replace-previous clip decision, prefetch limit decision, `4096 MB` LRU eviction order (never `open-*`, `_cursors`, or active entries), and cleanup (by ID, all, and 24-hour expiry)
  - Clipboard action selection with `CF_HDROP` priority and `DROPFILES` layout construction
  - Cursor role to `OCR_*` mapping and `.cur`/`.ani` filtering
- Loopback QUIC integration tests:
  - `DragOffer` -> `DragFile` uni-stream -> `DragReady` roundtrip over QUIC, plus oversized `DragFile` rejection/notice behavior
  - `ClipFile` transfer cancellation mid-way from both receiver and sender sides (no partial files remain, both sides observe `cancelled`)
  - `>= 300 MB` `ClipFile` loopback throughput + `4 ms` mouse datagram one-way latency measurement (both Client->Server and Server->Client directions)
  - Ignored Windows desktop release-gate test (`target_drag_terminates_within_2s_when_wedged_or_capture_owned`) verifying target drag exits `<= 2 s` with `LOCAL_DRAG_SOURCE_ACTIVE = false` when external capture or missing button state occurs
- Quality gates: `cargo test`, `cargo check`, `npm test`, `npm run build`.
