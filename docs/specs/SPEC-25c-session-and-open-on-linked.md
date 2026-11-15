# SPEC-25c: Continuity — Session Sync & Open on Linked PC

> **Status:** Implemented / Shipped in v1.1.0
> **Scope:** `(kvm)` — Part of SPEC-25 Continuity (Windows only). Builds on SPEC-19 (authenticated QUIC transport) and SPEC-23/SPEC-24 (Main PC / Linked PC link lifecycle).

## 1. Goal

Make two linked Windows PCs behave like one continuous workstation for session power/lock state and quick handoff of files and links:

1. **Lock / Sleep Together:** Locking or suspending one linked PC propagates to the authenticated peer (`lockTogether` default on, `sleepTogether` default off). Unlock is **never** automated.
2. **Open on Linked PC:** Right-click any file in Windows Explorer (`Open on Linked PC`) or invoke the Command Palette action (`Open URL on Linked PC`, bidirectional) to transfer/open a file or URL on the linked peer with strict scheme and executable safety guards.

---

## 2. Session Sync (`session_sync.rs`)

### Detection
- A dedicated thread (`kvm-session-sync`) creates an invisible, non-shown top-level message window (top-level rather than `HWND_MESSAGE` so `WM_POWERBROADCAST` is delivered alongside `WM_WTSSESSION_CHANGE`).
- Registers with `WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION)` and listens for:
  - `WM_WTSSESSION_CHANGE` → `WTS_SESSION_LOCK` / `WTS_SESSION_UNLOCK`
  - `WM_POWERBROADCAST` → `PBT_APMSUSPEND`
- The message loop never performs blocking network/UI operations directly.

### Policy & Echo Suppression
- Settings:
  - `lockTogether: bool` (default `true`)
  - `sleepTogether: bool` (default `false`)
- **Outbound:**
  - `WTS_SESSION_LOCK`: if linked and `lockTogether` is enabled and not inside an inbound lock-echo suppression window, release any locally trapped KVM inputs and send `KvmPacket::SessionLock`.
  - `PBT_APMSUSPEND`: if linked and `sleepTogether` is enabled and not inside an inbound sleep-echo suppression window, release any locally trapped KVM inputs and send `KvmPacket::SessionSleep`.
  - `WTS_SESSION_UNLOCK`: recorded for local state only; **never** sends any unlock packet.
- **Inbound** (only accepted over an authenticated, paired QUIC session):
  - `KvmPacket::SessionLock`: if `lockTogether` is enabled, record a short echo-suppression window, release any receiver inputs, and call `LockWorkStation()`.
  - `KvmPacket::SessionSleep`: if `sleepTogether` is enabled, record a short echo-suppression window, release any receiver inputs, and call `SetSuspendState(false, false, false)` on a background blocking thread.
  - **Unlock is never automated.**
- **Cursor Visibility Recovery (`injector.rs`):**
  - Because `SetCursorPos` on a receiver session does not count as user mouse input to clear `CURSOR_SUPPRESSED` after unlock/desktop switch, AnyLeap checks `GetCursorInfo` flags on local `WTS_SESSION_UNLOCK` and receiver `CursorEnter` (off hook callbacks); if `CURSOR_SHOWING` is unset or `CURSOR_SUPPRESSED` is set, it restores visibility and injects a single zero-delta relative `SendInput` mouse move.

---

## 3. Open on Linked PC (`open_on_linked.rs`)

### Explorer Context Menu & Single-Instance Forwarding
- Runtime HKCU registration under `HKCU\Software\Classes\*\shell\AnyLeapOpenOnLinked`:
  - Default value: `Open on Linked PC`
  - `Icon`: `"<exe_path>",0`
  - `command` subkey default value: `"<exe_path>" --open-on-linked "%1"`
- Controlled by `openOnLinkedMenu: bool` via `kvm_set_session_settings` (default `true`; disabling removes the key tree with `RegDeleteTreeW`).
- NSIS uninstaller (`src-tauri/nsis/hooks.nsh`, `NSIS_HOOK_POSTUNINSTALL`) deletes `HKCU\Software\Classes\*\shell\AnyLeapOpenOnLinked` cleanly on uninstall.
- `tauri-plugin-single-instance` (registered first in `lib.rs`) forwards CLI arguments (`--open-on-linked <path>` or `--open-on-linked=<path>`) from secondary launches to the running instance without spawning duplicate processes. If no peer is currently linked, AnyLeap emits `kvm-continuity-notice`.

### File Transfer & Execution Safety
- Sender generates a UUID `transfer_id`, streams the file asynchronously in `256 KiB` chunks over a QUIC unidirectional stream with `StreamHeader::OpenFile { transfer_id, name, size }` (emitting throttled `kvm-transfer-progress { id, kind: "open", name, pct, state, sizeBytes }` at `<= 10 events/s` ending at `100` with `state: "done"` on both client and server paths, plus `kvm-continuity-notice` on failure), and sends `KvmPacket::OpenFile { transfer_id }` on the control stream.
- Either PC can cancel an active `open` transfer via `kvm_cancel_transfer(id)`, which sends `KvmPacket::TransferCancel { id }`, resets/stops the QUIC stream, removes partial `.part` / `open-<id>` staging state on the receiver, and transitions both sides to `state: "cancelled"` without an error notice.
- On WTS unlock and `CursorEnter`, `injector::ensure_cursor_visible_with_nudge` logs `CURSOR_INFO` (only when `flags` or `hCursor` change, including `desktop`) and sends an unconditional net-zero 1-px relative `SendInput` nudge (`+1, -1`), backed up by a one-shot atomic post-unlock nudge on the first receiver mouse delta. Debug log rotation renames the active `kvm-debug.log` to `kvm-debug.log.1` when exceeding 5 MB.
- Receiver accepts `StreamHeader::OpenFile` only on an authenticated, trusted QUIC session, validates `name` via `files::sanitize_name` into a temporary staging folder `%LOCALAPPDATA%\AnyLeap\staging\open-<transfer_id>\<name>` (never deleting on open; expired entries older than 24 h are swept on startup), emits throttled `kvm-transfer-progress` ending at `100` (or `kvm-continuity-notice` on failure), and coordinates stream completion + `KvmPacket::OpenFile` arrival in either order.
- Safe shell-opened files emit `kvm-continuity-notice` (`Opened <name> (temporary copy, removed after 24 h)`).
- **Executable/script blocklist:** Before calling `ShellExecuteW(..., "open", ...)`, the receiver inspects the file extension (case-insensitive):
  - Blocked extensions: `.exe`, `.msi`, `.bat`, `.cmd`, `.ps1`, `.vbs`, `.js`, `.jse`, `.wsf`, `.lnk`, `.scr`, `.com`, `.hta`, `.pif`, `.cpl`, `.msc`, `.jar`, `.reg`, `.vbe`, `.wsh`, `.psm1`, `.psd1`, `.ps1xml`, `.scf`, `.url`, `.appref-ms`, `.application`, `.msix`, `.msixbundle`, `.appx`, `.appxbundle`, `.settingcontent-ms`, `.library-ms`, `.chm`, `.gadget`, `.inf`, `.msp`, `.mst`, `.cab`, `.iso`, `.img`, `.vhd`, `.vhdx`.
  - Any file matching the blocklist is **never** auto-opened; instead Windows Explorer reveals and selects the file (`explorer.exe /select,"<path>"`) and emits `kvm-continuity-notice`.

### URL Handoff Safety
- `kvm_open_url_on_linked(url)` sends `KvmPacket::OpenUrl { url }` (works bidirectionally between Main PC and Linked PC).
- Both sender and receiver enforce a strict URL allow-list:
  - Allowed schemes (case-insensitive): `http://` (with non-empty host), `https://` (with non-empty host), `mailto:` (with non-empty recipient).
  - Control characters (`\r`, `\n`, `\0`, etc.), empty URLs, lengths > 4096, and all other schemes (`file:`, `javascript:`, `cmd:`, UNC paths, etc.) are rejected.
- Receiver opens validated URLs via `ShellExecuteW` on a blocking worker thread.

---

## 4. Shared Contract Interface

### Protocol Additions (`protocol.rs`)
- `KvmPacket`:
  - `SessionLock`
  - `SessionSleep`
  - `OpenUrl { url: String }`
  - `OpenFile { transfer_id: String }`
  - `TransferCancel { id: String }`
- `StreamHeader`:
  - `OpenFile { transfer_id: String, name: String, size: u64 }`

### Tauri Commands (camelCase JS args)
- `kvm_set_session_settings(lockTogether: bool, sleepTogether: bool, openOnLinkedMenu: bool)`
- `kvm_open_url_on_linked(url: String)`
- `kvm_open_file_on_linked(path: String)`
- `kvm_cancel_transfer(id: String)`

### UI Events
- `kvm-transfer-progress { id, kind: "open", name, pct, state, sizeBytes }`
- `kvm-continuity-notice { text }`

---

## 5. Verification

- **Unit tests (`cargo test`):**
  - Lock/sleep outbound & inbound decision matrix (defaults, echo suppression, unlock never synced, unlinked ignored) and Win32 message classification.
  - URL allow-list (`http`, `https`, `mailto` accepted; `file`, `javascript`, `ftp`, control chars, empty host rejected).
  - Executable/script blocklist (all 43 extensions case-insensitively → `RevealFolderOnly`; safe documents/media → `OpenWithShell`).
  - Command-line parsing for `--open-on-linked` (separate arg, `=` form, missing/empty arg) and HKCU command formatting.
- **Loopback QUIC tests (`cargo test`):**
  - `SessionLock` and `OpenUrl` end-to-end delivery over authenticated loopback QUIC.
  - `OpenFile` stream + `KvmPacket::OpenFile` coordination over authenticated loopback QUIC.
- **Quality gates:** `cargo test`, `cargo check`, `npm test`, `npm run build`.
