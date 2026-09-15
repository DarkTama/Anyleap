# AnyLeap

A desktop GUI that makes connecting your Android phone to your PC — **wired or wirelessly** — effortless, then mirrors and controls it using [scrcpy](https://github.com/Genymobile/scrcpy) under the hood.

Think "Tecno OneLeap, but universal": the easy one-tap / QR-code wireless experience, on *any* Android 11+ phone, with all of scrcpy's power exposed as a GUI instead of command-line flags.

> **Status:** Active development. Latest release: **v0.9.0** (Studio Deck workstation, Android Camera Studio mode, embedded mirror container, wireless pairing, and adaptive control bars).

## Features

- **Camera Studio (PC Webcam)** — Stream your phone's front or back camera sensors directly to your PC with custom resolutions (1080p, 720p, 4K), configurable frame rates, microphone audio capture, and in-session controls (torch toggle, zoom in/out, lens flip).
- **Studio Deck Workstation** — Integrated control dashboard with the **Launch Matrix** supporting 4 distinct profiles:
  - *Desktop Flex Mode* — Dynamic virtual display container with live DPI presets and visualizer.
  - *Standard Mirror* — Low-latency native display replication.
  - *Camera Studio* — Dedicated raw camera sensor streaming for calls and content creation.
  - *High Fidelity* — 60 FPS, 20 Mbps AV1/HEVC pipeline for gaming and crisp media reproduction.
- **Embedded Mirror Window** — Borderless, single-window embedded workstation container with integrated flush control toolbar.
- **Effortless Wireless & USB** — One-tap pairing via **QR code** (scan with phone) or **6-digit mDNS auto-discovery**, persistent device storage, live battery telemetry, and automatic reconnects.
- **Power & Productivity Controls** — Seamless screen power toggling (keep mirroring while phone display stays dark), wheel-to-swipe scrolling for Reels/Shorts, clipboard image sharing, drag-and-drop file transfer, and app launcher drawer.
- **Adaptive Control Bar** — Context-aware floating or embedded controls for navigation, volume, sleep/wake, torch, and camera zoom.

## Download

Grab the latest build from the [**Releases**](https://github.com/DarkTama/Anyleap/releases) page (Windows x64):

- **`AnyLeap_*_x64-setup.exe`** — installer.
- **`AnyLeap-*-portable-win64.zip`** — portable; unzip and run `anyleap.exe`.

Builds are currently **unsigned**, so Windows SmartScreen may warn on first run ("More info" → "Run anyway"). Requires the WebView2 runtime (preinstalled on Windows 11 / most Windows 10).

## Usage

1. **USB:** enable *USB debugging* on the phone, plug in, authorize the prompt → the device appears under **Devices** → **Mirror**.
2. **Wireless:** on the phone, *Settings → Developer options → Wireless debugging*. In AnyLeap click **Add wireless device**:
   - **QR code** (default) — scan the shown QR with the phone's *Pair device with QR code*; it pairs and connects automatically.
   - **Auto-discover / Manual** — use the *Pair device with pairing code* screen and enter the 6-digit code.
3. **Launch Modes**:
   - **Mirror**: Launch display mirroring directly from the device list or Studio Deck.
   - **Camera Studio**: Click **Camera** on any connected device row or select **Camera Studio** in the Studio Deck Launch Matrix to configure resolution, lens, torch, and audio before streaming.
## Build from source

Prerequisites (Windows):

- **Rust** (MSVC toolchain): `rustup default stable-msvc`
- **Visual Studio Build Tools** with the *Desktop development with C++* workload
- **Node.js** LTS
- WebView2 (already on Windows 11)

```bash
npm install          # also runs scripts/fetch-binaries.ps1 (downloads pinned scrcpy + adb)
npm run tauri dev    # run in development
npm run tauri build  # produce the NSIS installer (src-tauri/target/release/bundle/nsis)
```

Notes:
- The bundled `adb`/`scrcpy` binaries are **not** in git; `npm install` (or `npm run fetch-binaries`) downloads a pinned scrcpy release into `src-tauri/binaries/`.
- Before re-running a build, stop any running app first (`adb kill-server` + close the app) so the staged sidecars aren't locked.

## How it works

AnyLeap is a thin **Tauri 2** (Rust) shell + **React/Vite** UI that drives the bundled `adb` and `scrcpy` as sidecar processes — it does **not** reimplement mirroring. Wireless discovery/pairing delegates to adb (`adb mdns services`, `adb pair`, `adb connect`); the control bar sends `adb shell input` key events.

## Tech stack

Tauri 2 · React 19 + TypeScript + Vite · Tailwind v4 · Zustand · qrcode.react · bundled scrcpy (Apache-2.0) + Android platform-tools (Apache-2.0).

## Docs

Design/architecture notes live in [`docs/`](docs/): [PLAN](docs/PLAN.md) · [ARCHITECTURE](docs/ARCHITECTURE.md) · [WIRELESS](docs/WIRELESS.md) · [SETTINGS-MAP](docs/SETTINGS-MAP.md) · [BACKLOG](docs/BACKLOG.md).

## Prior art

- [escrcpy](https://github.com/viarotel-org/escrcpy) — the closest existing GUI (Electron); a key reference for UX.
- [scrcpy](https://github.com/Genymobile/scrcpy) — the mirroring engine AnyLeap wraps.

## License

Apache-2.0 — see [LICENSE](LICENSE). Bundled third-party components are attributed in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).
