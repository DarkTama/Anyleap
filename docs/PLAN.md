# Plan — AnyLeap *(working title)*

_Planning doc. Last updated: 2026-06-22._

## Vision

A friendly desktop app that connects an Android phone to a PC **wirelessly with as little friction as a vendor "PC connection" feature**, then mirrors and controls it via scrcpy — with **every scrcpy option available as a GUI control**, never as a typed command.

The differentiator is not mirroring (scrcpy already nails that) — it's the **connection experience**: scan a QR or tap once, and you're mirroring; reconnect automatically next time.

## Problem

- scrcpy is powerful but CLI-only; remembering flags and pairing wirelessly is tedious.
- Vendor solutions (Tecno OneLeap, `pcconnection.online`) are proprietary, locked to specific brands, and effectively deprecated/unreliable.
- The author had a Python GUI wrapper before but lost it and never pushed it.

## Goals

1. **Effortless wireless connect** — QR-code pairing and pairing-code pairing, mDNS auto-discovery, saved devices, auto-reconnect on launch.
2. **No command line, ever** — all scrcpy settings as typed GUI controls, tiered so beginners aren't overwhelmed.
3. **Universal** — works with any Android 11+ device over standard ADB; not tied to any vendor.
4. **Polished & cross-platform-ready** — Windows first (author's OS), but the stack (Tauri) keeps macOS/Linux open.

## Core architectural decisions (settled)

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Engine | **Use scrcpy as-is** (drive its CLI) | Rebuilding the mirroring engine = years of work; scrcpy is excellent & Apache-2.0. |
| Relationship | **Wrap, don't fork** | Keep our GUI in a separate layer; inherit every upstream scrcpy update for free. |
| App framework | **Tauri** (Rust core + web UI) | Small native binary, modern UI, cross-platform; our backend is thin (just process orchestration). |
| Frontend | **React + shadcn/ui** | Best accessible form-control ecosystem for a settings-heavy app + a pairing wizard. |
| Wireless pairing | **Delegate to adb CLI** (adb-qr model) | No need to implement SPAKE2/mDNS ourselves; works reliably incl. on Windows. |

## Scope

### v1 — "It just connects" (wireless-first)
- Device list: USB devices + wireless devices (via `adb mdns services`) in one view.
- Pairing wizard:
  - **QR code** (primary) — phone scans a QR shown by the app.
  - **Pairing code** (fallback) — enter the 6-digit code the phone shows.
- **Saved devices + auto-reconnect** on launch (the payoff: effortless after first time).
- **One-click Mirror** — launch scrcpy with a curated **core** settings subset.
- **Session management** — list running mirrors, stop/kill them.
- Per-device saved config + **import/export** of settings.

### v2+ — "Full control"
- Full **schema-driven settings form** covering the entire scrcpy flag set (see [SETTINGS-MAP.md](SETTINGS-MAP.md)).
- In-window **control bar** (rotate, screenshot, file push/APK install, etc.) — escrcpy-style.
- Reverse tethering (share PC internet to phone) via Gnirehtet.
- Profiles/presets ("gaming", "low-bandwidth", "recording").
- macOS / Linux builds.

### Non-goals
- **Not** forking or modifying scrcpy's engine.
- **Not** integrating with Tecno OneLeap's proprietary protocol (closed, vendor-locked — we build the universal equivalent instead).
- **Not** building notification/SMS/call mirroring (that's KDE-Connect territory; out of scope).
- **Not** rebuilding a video pipeline — scrcpy owns mirroring.

## Success criteria (v1)

- A first-time user can pair a phone wirelessly via QR in under a minute, no typing of IPs.
- On second launch, a previously-paired phone reconnects and is mirrorable in one click.
- A user can change common settings (resolution, bitrate, fps, stay-awake, turn-screen-off) without seeing a flag.

## Milestones

1. **M0 — Docs & decisions** (this commit): plan, architecture, wireless flows, settings map.
2. **M1 — Skeleton**: Tauri app boots; bundles/locates `adb` + `scrcpy`; lists USB devices; can launch scrcpy with defaults.
3. **M2 — Wireless core**: mDNS discovery; pairing-code flow; `adb connect`; saved devices + auto-reconnect.
4. **M3 — QR pairing**: generate QR; poll `adb mdns services`; pair + connect end-to-end.
5. **M4 — Core settings UI**: curated settings tier wired into the scrcpy launch.
6. **M5 — Polish**: session management, per-device config, import/export, error states.

## Open questions

- Final app name (AnyLeap is a placeholder).
- Which exact scrcpy settings the author used most → seed the "core" tier (see [SETTINGS-MAP.md](SETTINGS-MAP.md)).
- Bundle scrcpy/adb with the installer vs. fetch on first run (see [ARCHITECTURE.md](ARCHITECTURE.md)).
