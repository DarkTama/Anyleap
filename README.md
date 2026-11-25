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

> **Nothing to install on your phone** — AnyLeap uses Android's built-in USB/Wireless debugging.

> **Status & safety:** AnyLeap is in **beta** and is currently **Windows-first**. The Windows installer is not Authenticode-signed yet, so Windows SmartScreen may warn on first run. Because **Shared Mouse & Keyboard** uses low-level keyboard/mouse hooks and input injection, some antivirus tools may also flag it—the source code is open, and official builds come directly from GitHub Actions. See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md) for details.

## Features

### Phone
- **Mirroring Modes (Launch Matrix)** — Choose from **Phone Mirror** (1:1 low-latency portrait feed with wheel-to-swipe scrolling), **Desktop Flex** (secondary virtual display with custom DPI scaling and multi-window apps; uses the SDK keyboard so PC typing and the phone's own keyboard keep working), **Camera Studio** (stream front or rear phone lenses for calls and recording with torch, zoom, and lens flip), and **High Fidelity** (60 FPS, 20 Mbps AV1/HEVC).
- **Phone Hub (`Phone` tab)** — Labelled sidebar with **Overview** (battery, storage, RAM, Wi-Fi/cellular signal, screen state, uptime, and quick actions including **Mirror**, **Audio only**, and Wi-Fi/Bluetooth/Do Not Disturb toggles), **Messages** (memory-only read-only conversation threads grouped by app and cleared on disconnect), **Notifications** (grouped by app name with per-app mute filtering for both the list and desktop toasts), **Gallery** (infinite-scroll photo and video grid across all albums with an in-app photo viewer, direct video playback, and single/batch downloads), **Files** (`/storage/emulated/0` upload, download, and delete), and **Apps** (searchable user/system app launcher; **Open** reuses a running Flex window or Phone Mirror, otherwise opens a new window using the preset selected in Devices; per-device pinned apps stay at the top, keyed by hardware serial so pins survive wireless IP/port changes).
- **Per-Mode Mirroring Defaults (`Settings` tab, SPEC-28 / SPEC-30)** — A **Phone mirroring** card with **Mirror / Flex / Camera / High fidelity** tabs. Each mode keeps its own quality (Smooth / Balanced / Sharp / Maximum), audio, control, window, **Scrolling** (Smooth reading with density-independent **Auto** or **1x–5x** mouse wheel speed, separate **0.5x–3x** **Touchpad speed**, and optional **Smooth scrolling** vs. **Paged** short-video swipes), and Flex display settings, so picking Camera can never mute Mirror. Devices, the Command Palette and the control window always launch with the selected mode's settings; upgrades migrate existing settings once and never silently turn audio off.
- **Sectioned Settings (`Settings` tab, SPEC-28)** — A left rail with eight sections (General, Phone mirroring, Control bar, Shared mouse & keyboard, Clipboard & files, Audio, media & power, Advanced, About) and a **Filter settings** box that matches labels, helper text and keywords. Labels are positive and sentence-case. Crossing delay and corner dead zone are read-only here with an **Edit in Computers** link; the Computers tab is their single editor and shows a **Custom** chip for other stored values. Large-file limits, the diagnostics log, and **Delete logs** live in Advanced; **About → Privacy** holds the privacy summary, policy link, **Open data folder**, and **Delete logs**. Rail, tabs, switches and filter are fully keyboard accessible.
- **Default Transfer Folders (`Settings → Clipboard & files`, SPEC-31)** — Zero-prompt downloads across Phone Hub Gallery/Files, Command Palette screenshots, and multi-PC transfers saved to one configurable receive folder (default `Downloads\AnyLeap`), with an optional **Ask where to save each time** switch, a configurable phone upload folder (default `/sdcard/Download`) for mirror drops and pasted images, automatic collision renaming (`name (1).ext`), automatic fallback with notice if a folder is unavailable, and **Show in folder** actions on completed transfers.
- **Quick Actions & Adaptive Controls** — Embedded or floating control bar (60 px grouped icon sidebar with tooltips, keyboard access and a More popover) for navigation, volume, screen-off mirroring, screenshots, and MP4 screen recording (`Videos\AnyLeap`, with a "Show in folder" action when it stops), plus URL handoff from PC clipboard, per-app virtual display windows, Wi-Fi / Bluetooth / Do Not Disturb toggles, Windows battery alerts, and call placement/hangup.
- **USB & Wireless Pairing** — Plug in over USB or pair wirelessly via QR code or 6-digit mDNS pairing code with saved devices and live battery telemetry.

### Shared Mouse & Keyboard
- **Main PC & Linked PC Roles (`Computers` tab)** — Clear role model: the **Main PC** shares its physical mouse and keyboard; a **Linked PC** opts in via **Allow other PCs to control this PC**.
- **Encrypted QUIC Link** — Mutually authenticated QUIC transport (UDP 4177) with unreliable datagrams for low-latency pointer motion and audio, plus separate unidirectional streams for clipboard text/images and file transfers.
- **One-Time Pairing Code & Fingerprint Trust** — First link verifies a 6-digit **Pairing code**; subsequent links authenticate automatically by pinned SHA-256 certificate fingerprint (revoke anytime with **Forget**).
- **Offset-Aware 2D Layout** — Multi-monitor canvas to position the Linked PC along any screen edge (`left`, `right`, `top`, `bottom`) with pixel-offset alignment, edge deadzone/dwell tuning, and instant cursor release (`Scroll Lock`, `Pause`, `Ctrl+Alt+Esc`, or **Bring cursor back**).
- **Continuity — Cross-PC File Copy-Paste & Drag & Drop** — Copy and paste files between Main PC and Linked PC (auto-prefetched up to 100 MB by default; larger transfers wait for the **Fetch** button above, with **Cancel** on both PCs and a live **transfer bar**). Cross-PC drag & drop is available as **(Experimental, off by default)**.
- **Continuity — Audio Bridge & Media Keys** — Stream Linked PC audio to the Main PC with a volume slider and optional Linked-speaker mute (using Windows process loopback so bridged audio doesn't echo), while media and volume keys automatically follow whichever PC produced active audio.
- **Continuity — Session Sync, Open on Linked PC & Pointer Sync** — Optionally lock or sleep both PCs together (unlock is never automated), right-click files in Explorer (**Open on Linked PC**, under *Show more options* on Windows 11) or run **Open URL on Linked PC** from the Command Palette (including typing a URL directly), and optionally sync the Main PC's pointer scheme to the Linked PC.

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

> **Automatic wireless reconnect & single device identity:** Once paired, AnyLeap identifies your phone by hardware serial. If Android rotates your Wireless debugging port after toggling Wi-Fi or rebooting, AnyLeap discovers the updated port via mDNS and reconnects automatically without re-pairing; plugging in USB while on Wi-Fi keeps everything grouped as one device (preferring USB). Every 15 s a background health check compares saved wireless phones with the live adb list; a phone the OS silently dropped shows "Reconnecting…" and is re-attached via mDNS (with 15/30/60 s backoff), and Phone Hub "device not found/offline" errors trigger the same reconnect. AnyLeap prompts to pair again only if ADB authentication is revoked or the device is unreachable via mDNS.

### Link two PCs (Shared Mouse & Keyboard)
1. On the **Linked PC**, open the **Computers** tab and turn on **Allow other PCs to control this PC**.
2. On the **Main PC**, open **Computers**, select the discovered PC (or enter its IP), and click **Link**.
3. Enter the 6-digit **Pairing code** displayed on the Linked PC (required on first link only; future links recognize its certificate fingerprint).
4. Arrange the Linked PC in the 2D layout canvas and move your cursor across the shared edge. Press `Scroll Lock`, `Pause`, or `Ctrl+Alt+Esc` (or click **Bring cursor back**) anytime to return control to the Main PC.

## Privacy & Security

- **No accounts, cloud, or tracking:** AnyLeap runs directly on your devices with zero analytics, telemetry, or crash uploads. The only external internet requests are optional update checks to GitHub. See [PRIVACY.md](PRIVACY.md) for the full plain-language privacy policy and [docs/PRIVACY-AUDIT.md](docs/PRIVACY-AUDIT.md) for the audited data inventory.
- **Local network only:** Phone mirroring runs over USB or local ADB; Shared Mouse & Keyboard communicates strictly over your LAN with no cloud servers or relays.
- **Encrypted transport:** PC-to-PC input, clipboard, and file transfers run over mutually authenticated TLS 1.3 / QUIC (`anyleap-kvm/1`).
- **Pairing code & trust list:** Unknown peers can only send a pairing request with the one-time 6-digit pairing code (regenerated automatically after repeated failures). Trusted certificate fingerprints are stored locally in `kvm_trusted_peers.json` and can be removed anytime via **Forget**.
- **In-memory notifications & sanitized transfers:** Phone notifications are kept in memory only, incoming PC file transfers sanitize filenames to prevent path traversal, and opt-in diagnostics logs redact window titles, filenames, and user paths (with a 7-day retention cap and **Delete logs** control under **Settings → About → Privacy** and **Settings → Advanced**).

## Development

### Prerequisites (Windows)
- **Rust** (`stable-msvc` toolchain)
- **Visual Studio Build Tools** with *Desktop development with C++*
- **Node.js** LTS
- **WebView2 Runtime**

### Run locally
```bash
npm install          # installs dependencies and fetches pinned adb + scrcpy v5.0.1 binaries
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

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, architecture, and how to contribute, and [AGENTS.md](AGENTS.md) for the pull-request workflow, commit formatting, documentation sync rules, and quality gates.

## License

Apache-2.0 — see [LICENSE](LICENSE). Bundled third-party components (`scrcpy`, Android platform-tools, and the Plus Jakarta Sans font in `assets/brand/fonts/`) are attributed in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md) and [assets/brand/fonts/OFL.txt](assets/brand/fonts/OFL.txt).
