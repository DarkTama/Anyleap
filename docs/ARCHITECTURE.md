# Architecture — AnyLeap *(working title)*

_Planning doc. Last updated: 2026-06-22. No code yet; this describes the intended design._

## High-level shape

```
┌──────────────────────────────────────────────────────────┐
│  Frontend  (React + shadcn/ui, in the Tauri webview)      │
│   • Device list / pairing wizard / settings form / sessions│
│   • Calls Rust via Tauri `invoke`; listens for events      │
├──────────────────────────────────────────────────────────┤
│  Rust core  (Tauri commands + state)                       │
│   • Spawns & supervises adb / scrcpy child processes        │
│   • Parses adb text output (devices, mdns services)         │
│   • Persists saved devices + settings                       │
│   • Emits events (device found, pairing status, session end) │
├──────────────────────────────────────────────────────────┤
│  Bundled sidecar binaries                                  │
│   • adb(.exe)  • scrcpy(.exe)  • scrcpy-server  • DLLs       │
└──────────────────────────────────────────────────────────┘
                         │ USB / TCP-IP
                         ▼
                  Android device (scrcpy server runs here)
```

**Key principle:** the Rust core is *thin*. It does process orchestration, text parsing, and persistence — no networking protocols, no video. All the hard work lives in the bundled `adb` and `scrcpy`.

## Components

### Frontend (React + shadcn/ui)
- **Views:** Devices (USB + wireless), Pair (QR + code wizard), Settings (core tier in v1), Sessions (running mirrors).
- **State:** lightweight client store (e.g. Zustand) mirroring backend state; backend is source of truth.
- **Backend comms:** `@tauri-apps/api` `invoke()` for commands; `listen()` for push events (device discovered, pairing progress, session exited).
- QR rendering can be done client-side with a JS QR lib, or server-side with the Rust `qrcode` crate — TBD; client-side is simplest.

### Rust core (Tauri commands)
Proposed command surface (names provisional):

| Command | Purpose |
|---------|---------|
| `list_devices()` | Parse `adb devices -l` → connected devices (USB + already-connected TCP). |
| `discover_wireless()` | Parse `adb mdns services` → pairing/connect services on the LAN. |
| `start_qr_pairing()` | Generate name+password, return QR payload; begin polling for the advertised service. |
| `pair_with_code(host, port, code)` | Run `adb pair host:port code`. |
| `connect(host, port)` | Run `adb connect host:port`. |
| `start_mirror(serial, settings)` | Spawn scrcpy with mapped flags; track the child. |
| `stop_mirror(session_id)` | Kill a running scrcpy child. |
| `list_sessions()` | Running scrcpy children. |
| `save_device(device)` / `list_saved()` / `forget_device(id)` | Saved-device persistence. |
| `get_settings()` / `set_settings()` / `import_settings()` / `export_settings()` | Settings persistence. |

Long-running/streaming work (discovery polling, pairing progress, session lifecycle) is reported to the frontend via **Tauri events**, not blocking command returns.

### Process supervision
- scrcpy runs as a **long-lived child**; we keep a handle (PID/session id), surface stdout/stderr for diagnostics, and kill on user request or app exit.
- Concurrency: support multiple simultaneous mirrors (multi-device), each its own child + window.

## Bundling adb & scrcpy (a real packaging task)

scrcpy on Windows is **not a single exe** — it ships `scrcpy.exe` + `scrcpy-server` (the jar pushed to the phone) + SDL/FFmpeg DLLs + its own bundled `adb.exe`. Plan:

- Ship these as Tauri **sidecars** (`externalBin` in `tauri.conf.json`), with platform-suffixed names (e.g. `adb-x86_64-pc-windows-msvc.exe`).
- Keep the binaries **out of git** (large) — `src-tauri/binaries/` is gitignored; a `scripts/fetch-binaries` step downloads a pinned scrcpy release at setup/build time. (Decision pending: bundle in installer vs. fetch on first run.)
- **Pin a known adb/platform-tools version** and always invoke *our* adb (set `ADB` env / explicit path) to avoid version conflicts with any adb the user already runs.

## Data & persistence

- Saved devices, per-device configs, and global settings stored as JSON via `tauri-plugin-store` (or a small file in the app config dir).
- Saved device record: stable id (e.g. device serial / pairing fingerprint), label/notes, last-known host:port, per-device scrcpy settings.

## Cross-platform notes

- Windows-first (author's OS). Tauri keeps macOS/Linux viable; the only platform-specific parts are sidecar binary sets and a few path conventions.

## Open decisions

- QR rendering: client-side JS vs Rust `qrcode` crate.
- Binaries: bundled-in-installer vs fetched-on-first-run.
- Client state lib (Zustand vs. React context) — minor.
