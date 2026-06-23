# AnyLeap

A desktop GUI that makes connecting your Android phone to your PC — **wired or wirelessly** — effortless, then mirrors and controls it using [scrcpy](https://github.com/Genymobile/scrcpy) under the hood.

Think "Tecno OneLeap, but universal": the easy one-tap / QR-code wireless experience, on *any* Android 11+ phone, with all of scrcpy's power exposed as a GUI instead of command-line flags.

> **Status:** working app. M1–M4 shipped (USB mirror → wireless core → QR pairing → tabbed UI + mirror control bar). M5 (control-bar customization, docs, and release builds) in progress.

## Features

- **Effortless wireless** — pair over Wi-Fi by **QR code** (scan it with the phone) or **6-digit code** (auto-discovered via mDNS, or entered manually), then mirror. Saved devices **auto-reconnect** on launch.
- **USB too** — plug in, authorize, mirror.
- **Quality presets** — Low / Medium / High / Highest (or Custom), plus the full core settings (resolution, bitrate, fps, codec, audio, etc.) as GUI controls — no command line.
- **Unified device view** — saved devices and discovered devices in one tab, each with live status (Offline / Connected / Mirroring) and one-click Mirror / Stop / Disconnect / Reconnect.
- **Mirror control bar** — Back / Home / Recents + Volume, Power, Screenshot, Notifications, Sleep/Wake, and scrcpy screen-off, as a floating always-on-top strip that **docks to the mirror window** (customizable: position, which buttons, size) plus inline controls in the app.

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
3. **Mirror** opens the scrcpy window. The **control bar** drives the phone (nav keys, volume, screenshot, etc.); customize it under **Settings → Control bar**.

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
