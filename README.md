<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/brand/anyleap-horizontal-reversed.svg" />
  <source media="(prefers-color-scheme: light)" srcset="assets/brand/anyleap-horizontal.svg" />
  <img alt="AnyLeap" src="assets/brand/anyleap-horizontal-reversed.svg" height="56" />
</picture>

<p><strong>Local-first Android mirroring, phone companion tools, and Shared Mouse &amp; Keyboard for Windows PCs.</strong></p>

[![Latest Release](https://img.shields.io/github/v/release/DarkTama/Anyleap?color=D8B46A&labelColor=1B1E2E)](https://github.com/DarkTama/Anyleap/releases/latest)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-A9822F?labelColor=1B1E2E)](LICENSE)

</div>

AnyLeap connects your Android phone and Windows PCs over USB or local Wi-Fi in a single desktop workstation. Mirror and control your phone, browse device files, photos, and notifications without a companion app, and glide one physical mouse and keyboard across multiple Windows PCs. Everything runs directly between your devices—local-first, encrypted, and with no cloud accounts or relays.

## Features

### Phone
- **Mirroring Modes (Launch Matrix)** — Choose from **Phone Mirror** (1:1 low-latency portrait feed with wheel-to-swipe scrolling), **Desktop Flex** (secondary virtual display with custom DPI scaling and multi-window apps), **Camera Studio** (stream front or rear phone lenses for calls and recording with torch, zoom, and lens flip), and **High Fidelity** (60 FPS, 20 Mbps AV1/HEVC).
- **Files, Photos & Notifications (`Phone` tab)** — Browse `/storage/emulated/0` with upload, download, and delete; view Camera and Screenshot photos in a lazy-loaded grid backed by a 500 MB local disk cache; and read active phone notifications in memory (read-only, with optional desktop pop-ups).
- **Quick Actions & Adaptive Controls** — Embedded or floating control bar for navigation, volume, screen-off mirroring, screenshots, and MP4 screen recording (`Videos\AnyLeap`), plus URL handoff from PC clipboard, per-app virtual display windows, Wi-Fi / Bluetooth / Do Not Disturb toggles, Windows battery alerts, and call placement/hangup.
- **USB & Wireless Pairing** — Plug in over USB or pair wirelessly via QR code or 6-digit mDNS pairing code with saved devices and live battery telemetry.

### Shared Mouse & Keyboard
- **Main PC & Linked PC Roles (`Computers` tab)** — Clear role model: the **Main PC** shares its physical mouse and keyboard; a **Linked PC** opts in via **Allow other PCs to control this PC**.
- **Encrypted QUIC Link** — Mutually authenticated QUIC transport (UDP 4177) with unreliable datagrams for low-latency pointer motion and separate streams for clipboard text/images and drag-and-drop file transfers.
- **One-Time Pairing Code & Fingerprint Trust** — First link verifies a 6-digit **Pairing code**; subsequent links authenticate automatically by pinned SHA-256 certificate fingerprint (revoke anytime with **Forget**).
- **Offset-Aware 2D Layout** — Multi-monitor canvas to position the Linked PC along any screen edge (`left`, `right`, `top`, `bottom`) with pixel-offset alignment, edge deadzone/dwell tuning, and instant cursor release (`Scroll Lock`, `Pause`, `Ctrl+Alt+Esc`, or **Bring cursor back**).

### Desk
- **Device Hero Card** — At-a-glance phone name, Android version, USB/Wi-Fi badge, battery level, live status, and primary actions.
- **Activity Timeline** — Local history ring buffer (up to 200 entries) logging mirroring sessions, screenshots, transfers, app launches, and computer links.
- **Command Palette (`Ctrl+K`)** — Zero-latency keyboard launcher for switching tabs, starting profiles, running diagnostics, and toggling Shared Mouse & Keyboard.
- **Connection Doctor** — Step-by-step diagnostics for `adb`, `scrcpy`, device authorization, wireless TCP reachability, shell responsiveness, and Android version, with fix hints and report copy.

### App
- **In-App Updater** — Background update checks and downloads verified with Minisign Ed25519 signatures, commit changelog viewer, and one-click relaunch.
- **Ink & Gold Theme** — Flat, high-contrast dark surface hierarchy (`#12141F` / `#1B1E2E` / `#2A2D40`) with restrained gold accents, tactile press physics, keyboard focus rings, and `prefers-reduced-motion` support.

## Requirements

- **PC:** Windows 10 or Windows 11 (x64) with Microsoft Edge WebView2 Runtime (preinstalled on Windows 11 and most Windows 10 systems).
- **Android:** Phone with **USB debugging** enabled (Android 11+ recommended for wireless QR/pairing-code setup and audio forwarding).
- **Shared Mouse & Keyboard:** Two Windows PCs running AnyLeap on the same local network (UDP 4177 for QUIC, UDP 4179 for LAN discovery).

## Install

Download the latest build from [**GitHub Releases**](https://github.com/DarkTama/Anyleap/releases):

- **`AnyLeap_<version>_x64-setup.exe`** — Windows installer.
- **`AnyLeap-<version>-portable-win64.zip`** — Portable archive (unzip and run `anyleap.exe`).

> Builds are currently unsigned for Authenticode, so Windows SmartScreen may warn on first run (*More info → Run anyway*).

## Quick Start

### Mirror a phone
1. Enable **USB debugging** under *Developer options* on your Android phone.
2. Connect via USB (or enable **Wireless debugging**, click **Pair Device** in AnyLeap, and scan the QR code or enter the 6-digit pairing code).
3. Select your phone in **Devices**, choose a profile in the Launch Matrix, and click **Mirror**.

### Link two PCs (Shared Mouse & Keyboard)
1. On the **Linked PC**, open the **Computers** tab and turn on **Allow other PCs to control this PC**.
2. On the **Main PC**, open **Computers**, select the discovered PC (or enter its IP), and click **Link**.
3. Enter the 6-digit **Pairing code** displayed on the Linked PC (required on first link only; future links recognize its certificate fingerprint).
4. Arrange the Linked PC in the 2D layout canvas and move your cursor across the shared edge. Press `Scroll Lock`, `Pause`, or `Ctrl+Alt+Esc` (or click **Bring cursor back**) anytime to return control to the Main PC.

## Privacy & Security

- **Local network only:** Phone mirroring runs over USB or local ADB; Shared Mouse & Keyboard communicates strictly over your LAN with no cloud servers or relays.
- **Encrypted transport:** PC-to-PC input, clipboard, and file transfers run over mutually authenticated TLS 1.3 / QUIC (`anyleap-kvm/1`).
- **Pairing code & trust list:** Unknown peers can only send a pairing request with the one-time 6-digit pairing code (regenerated automatically after repeated failures). Trusted certificate fingerprints are stored locally in `kvm_trusted_peers.json` and can be removed anytime via **Forget**.
- **In-memory notifications & sanitized transfers:** Phone notifications are never persisted to disk, and incoming PC file transfers sanitize filenames to prevent path traversal.

## Development

### Prerequisites (Windows)
- **Rust** (`stable-msvc` toolchain)
- **Visual Studio Build Tools** with *Desktop development with C++*
- **Node.js** LTS
- **WebView2 Runtime**

### Run locally
```bash
npm install          # installs dependencies and fetches pinned adb + scrcpy binaries
npm run tauri dev    # starts Vite + Tauri in development mode
```

### Quality gates
All checks must pass before committing:
```bash
npm test
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

## Contributing

See [AGENTS.md](AGENTS.md) for the pull-request workflow, commit formatting, documentation sync rules, and quality gates.

## License

Apache-2.0 — see [LICENSE](LICENSE). Bundled third-party components (`scrcpy`, Android platform-tools, and the Plus Jakarta Sans font in `assets/brand/fonts/`) are attributed in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) and [assets/brand/fonts/OFL.txt](assets/brand/fonts/OFL.txt).
