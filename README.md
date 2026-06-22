# AnyLeap *(working title)*

A desktop GUI that makes connecting your Android phone to your PC — **wired or, especially, wirelessly** — effortless, then mirrors and controls it using [scrcpy](https://github.com/Genymobile/scrcpy) under the hood.

Think of it as "Tecno OneLeap, but universal": the easy one-tap/QR wireless pairing experience, working on *any* Android 11+ phone, not just one vendor's.

> **Status: planning.** No application code yet. This repository currently contains only the design/plan documents below. Nothing here is final — names, scope, and stack are open to change.

## Why this exists

scrcpy is an excellent mirroring/control engine, but it's command-line only. Vendor "PC connection" tools (e.g. Tecno OneLeap / pcconnection.online) are proprietary, device-locked, and in practice deprecated/unreliable. This project pairs scrcpy's engine with a friendly GUI whose **headline feature is painless wireless connection** (QR-code and pairing-code), plus a full settings UI so you never touch the command line.

## Core decisions

- **Wrap scrcpy, don't fork or rebuild it.** scrcpy is the engine; we drive its CLI. See [docs/PLAN.md](docs/PLAN.md).
- **Greenfield app in [Tauri](https://tauri.app/)** (Rust core + web frontend), frontend in **React + shadcn/ui**.
- **v1 is wireless-connection-first**: QR pairing, mDNS auto-discovery, saved devices + auto-reconnect.
- **All scrcpy settings exposed as GUI controls** (tiered core/advanced), never as raw command input.

## Documentation

| Doc | What's in it |
|-----|--------------|
| [docs/PLAN.md](docs/PLAN.md) | Vision, goals, scope (v1 vs later), non-goals, milestones |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | Tauri layers, binary bundling, Rust command surface, process lifecycle |
| [docs/WIRELESS.md](docs/WIRELESS.md) | The connection flows in detail — QR, pairing-code, USB, mDNS, reconnect |
| [docs/SETTINGS-MAP.md](docs/SETTINGS-MAP.md) | scrcpy flags → GUI controls, tiered into core/advanced |

## Prior art we're learning from

- [escrcpy](https://github.com/viarotel-org/escrcpy) — Electron/Vue; the closest existing app. Primary reference for UX (QR pairing, multi-device configs, import/export).
- [adb-qr](https://github.com/aleixrodriala/adb-qr) — proves QR pairing by *delegating to the adb CLI* (no custom crypto). The model we'll copy for wireless.
- [QtScrcpy](https://github.com/barry-ran/QtScrcpy) — C++/Qt; reimplements the client. Reference for UX, not architecture.
- [guiscrcpy](https://github.com/srevinsaju/guiscrcpy) — Python/PyQt; archived, but closest to the author's earlier prototype.
