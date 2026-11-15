# Contributing to AnyLeap

Thank you for your interest in contributing to AnyLeap!

AnyLeap is a local-first desktop workstation built with **Tauri 2**, **Rust**, and **React / Vite** (Apache-2.0). It combines Android mirroring and phone companion tools (`adb` + `scrcpy`) with **Shared Mouse & Keyboard** (Virtual KVM) over an encrypted local QUIC link.

> **Project status:** **Beta**, **Windows-first** (Windows 10 / 11 x64). Linux and macOS support is not shipped yet; see the [Cross-platform guide](#cross-platform-guide) if you want to help port platform seams.

---

## Reporting Bugs & Proposing Features

- **Bugs:** Open an issue using the **Bug Report** template (`.github/ISSUE_TEMPLATE/bug_report.yml`). Include your AnyLeap version, OS, whether the issue is in Phone or Shared Mouse & Keyboard, reproduction steps, and logs/diagnostics (for phone issues, **Connection Doctor** has a plain-text report copy button).
- **Feature ideas:** Open an issue using the **Feature Request** template (`.github/ISSUE_TEMPLATE/feature_request.yml`).
- **Questions & general discussion:** Use [GitHub Discussions](https://github.com/DarkTama/Anyleap/discussions).
- **Security vulnerabilities:** Do **not** open a public issue. Follow [SECURITY.md](SECURITY.md).

---

## Development Setup (Windows)

### Prerequisites

- **Rust:** `stable-msvc` toolchain (installed via `rustup`; CI builds with `dtolnay/rust-toolchain@stable` on `windows-latest`).
- **Visual Studio Build Tools:** *Desktop development with C++* workload (MSVC + Windows SDK).
- **Node.js:** LTS release (CI uses `node-version: lts/*` with `npm`).
- **Microsoft Edge WebView2 Runtime:** Preinstalled on Windows 11 and most Windows 10 systems.

### Install Dependencies & Fetch Sidecars

```powershell
npm ci
```

`npm ci` installs frontend dependencies and automatically runs the `postinstall` hook (`npm run fetch-binaries` -> `node scripts/fetch-binaries.js`). That script downloads and SHA-256-verifies the pinned `scrcpy` v4.0 release (including `adb`, DLLs, and `scrcpy-server`) into `src-tauri/binaries/`, which is gitignored.

If you ever need to re-fetch or refresh the sidecars manually:

```powershell
npm run fetch-binaries
```

### Run in Development Mode

```powershell
npm run tauri dev
```

This starts the Vite dev server and launches the Tauri desktop application.

### Quality Gates

All checks must pass before opening a pull request:

```powershell
npm test
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml
npm run build
```

---

## Architecture Map

### Role Model (Shared Mouse & Keyboard)

- **Main PC (Controller):** Owns the physical mouse and keyboard. Runs low-level input hooks (`hooks.rs`) and the QUIC client (`client.rs`).
- **Linked PC (Receiver):** Opts in via **Allow other PCs to control this PC**. Runs the QUIC server (`server.rs`) on UDP `4177` and injects received input (`injector.rs`).
- **Trust & Discovery:** Peers discover each other on the LAN via UDP `4179` (`discovery.rs`). The first link requires a one-time 6-digit pairing code; subsequent links authenticate automatically using pinned SHA-256 certificate fingerprints stored in `kvm_trusted_peers.json` (`identity.rs`).

### Frontend (`src/`)

- **State (`src/stores/`, `src/store/`):** Zustand stores for Shared Mouse & Keyboard (`kvmStore.ts`), Activity Timeline (`activityStore.ts`), in-app updater (`appUpdateStore.ts`), and phone/session state (`useAppStore.ts`).
- **Services & IPC (`src/services/`, `src/lib/`):** `kvmService.ts`, `updateChecker.ts`, and `tauri.ts` wrap Tauri commands and events.
- **UI Components (`src/components/`):**
  - `deck/` — Studio Deck workstation (`StudioDeck`, `DeviceHeroCard`, `DeviceRack`, `LaunchMatrix`, `TimelinePanel`, `DpiVisualizer`).
  - `phone/` — ADB-only phone companion (`PhoneView`, `FilesView`, `PhotosView`, `NotificationsView`).
  - `computers/` — Shared Mouse & Keyboard setup and 2D monitor layout (`ComputersView`, `TopologyCanvas`, `PairingPinModal`).
  - `desktop/` — Overlay windows (`DesktopHudWindow`, `TransferBarWindow`).
  - `shell/` — Navigation, `CommandPalette` (`Ctrl+K`), and `UpdateBadge` / `UpdateModal`.

### Backend (`src-tauri/src/`)

- **Phone & Deck:**
  - `commands.rs` — ADB/scrcpy process orchestration, mirroring profiles, Camera Studio, Connection Doctor diagnostics, and quick actions.
  - `device_identity.rs` — Hardware-serial identity (`hw_serial`) and USB/Wi-Fi transport merging.
  - `phone_files.rs` — Scoped `/storage/emulated/0` file operations and 500 MB LRU photo cache.
  - `embed.rs` — Embedded mirror window parenting.
  - `wheel_swipe.rs` — Mouse wheel-to-swipe translation on mirrored windows.
- **Shared Mouse & Keyboard (`src-tauri/src/kvm/`):**
  - *Transport & Identity:* `client.rs`, `server.rs`, `protocol.rs` (`anyleap-kvm/1`), `identity.rs`, `discovery.rs`.
  - *Input & Layout:* `hooks.rs`, `injector.rs`, `input_logic.rs`, `monitors.rs`, `cursor_scheme.rs`.
  - *Continuity:* `clipboard.rs`, `files.rs`, `staging.rs`, `dragdrop.rs` (Experimental, off by default), `audio.rs`, `media_keys.rs`, `session_sync.rs`, `open_on_linked.rs`.
  - *Commands & Diagnostics:* `commands.rs`, `debug_log.rs`.

### Key Architecture Decision Records (`docs/adr/`)

- [ADR 0001](docs/adr/0001-modular-device-source-architecture.md) — Modular `DeviceSource` architecture (`AdbDevice` + `NetworkPcDevice`).
- [ADR 0002](docs/adr/0002-seamless-edge-leap-and-input-routing.md) — Edge leap and keyboard-follows-mouse input routing.
- [ADR 0003](docs/adr/0003-quic-transport-for-computer-link.md) — Authenticated QUIC (`quinn` + `rustls`, UDP 4177) transport with fingerprint pinning and per-file unidirectional streams.
- [ADR 0004](docs/adr/0004-continuity-transfers-and-audio-bridge.md) — Continuity file staging/prefetch/cancel, `0x41` audio datagrams, WASAPI process loopback with mute crash marker, and Experimental drag & drop.

---

## Cross-Platform Guide

Windows-only dependencies (`windows`, `windows-core`) live under `[target.'cfg(windows)'.dependencies]` in `src-tauri/Cargo.toml`. Platform-specific implementations are isolated behind `#[cfg(windows)]` with `#[cfg(not(windows))]` stub fallbacks so `cargo check` continues to pass on non-Windows targets.

When adding or modifying OS-specific code:
- Keep Windows API imports and types inside `#[cfg(windows)]` modules or blocks.
- Provide a `#[cfg(not(windows))]` fallback (returning a clear unsupported error or no-op stub) so non-Windows builds do not break.

A Linux or macOS port will need to implement the following seams:

| Seam | Current Windows Implementation | Location |
|---|---|---|
| **Input capture hooks** | `WH_MOUSE_LL`, `WH_KEYBOARD_LL`, `SetCursorPos`, `ClipCursor` | `src-tauri/src/kvm/hooks.rs`, `src-tauri/src/wheel_swipe.rs` |
| **Input injection** | Win32 `SendInput` | `src-tauri/src/kvm/injector.rs` |
| **Cursor control & pointer scheme** | Cursor hiding/clipping & Registry/Win32 cursor schemes | `src-tauri/src/kvm/hooks.rs`, `src-tauri/src/kvm/cursor_scheme.rs` |
| **Monitor enumeration** | `EnumDisplayMonitors`, `GetMonitorInfoW` | `src-tauri/src/kvm/monitors.rs` |
| **Clipboard formats & drag-and-drop** | `WM_CLIPBOARDUPDATE`, `CF_UNICODETEXT`/`CF_DIB`/`CF_HDROP`, layered edge catcher & OLE `DoDragDrop` | `src-tauri/src/kvm/clipboard.rs`, `src-tauri/src/kvm/dragdrop.rs` |
| **Audio capture/playback & media keys** | WASAPI `VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK`, `IAudioEndpointVolume::SetMute`, session activity & media key injection | `src-tauri/src/kvm/audio.rs`, `src-tauri/src/kvm/media_keys.rs` |
| **Session lock & sleep events** | `WM_WTSSESSION_CHANGE`, `WM_POWERBROADCAST`, `LockWorkStation`, `SetSuspendState` | `src-tauri/src/kvm/session_sync.rs` |
| **Shell open & Explorer context menu** | `ShellExecuteW`, `HKCU` context menu registration | `src-tauri/src/kvm/open_on_linked.rs` |
| **Window embedding & sidecar cleanup** | `SetParent` / `FindWindowW`, Win32 Job Objects | `src-tauri/src/embed.rs`, `src-tauri/src/commands.rs` |

---

## Contribution Workflow

See [AGENTS.md](AGENTS.md) for the full contributor protocol. In short:

1. **Specifications:** Non-trivial or architectural changes start with an approved spec in `docs/specs/SPEC-XX-<title>.md` (and a plan in `docs/superpowers/plans/` for multi-step work).
2. **Branching:** Branch from the latest `origin/main` using `<type>/<short-topic>` (for example, `fix/kvm-input-fidelity` or `docs/contributor-guide`). Never push directly to `main`.
3. **One squashed Conventional Commit:** Squash intermediate work into a single atomic commit before merging:
   ```text
   <type>(<scope>): <concise imperative description> [(<SPEC-ID>)]
   ```
   Allowed types: `feat`, `fix`, `docs`, `ci`, `test`, `refactor`.
   Common scopes: `updater`, `kvm`, `mirror`, `camera`, `deck`, `settings`.
4. **Pull requests:** Open a PR against `main` with a summary, the Spec ID (if applicable), results of the quality gates, and notes on what still needs manual testing.
5. **Documentation sync:** When completing a feature, keep `README.md`, `CONTEXT.md`, `docs/PLAN.md`, `docs/BACKLOG.md`, `docs/adr/`, and `docs/specs/` status in sync.
6. **Generic terminology:** Do not name competing or third-party proprietary products in commits, code comments, specs, or PR descriptions. Describe behaviour using generic engineering terms (such as "Peer Desktop Protocol" or "Reference Architecture").

---

## Good First Areas

These items from [docs/BACKLOG.md](docs/BACKLOG.md) are good places to explore (no timelines or promises implied):

- **Strict stream-to-offer binding:** Bind incoming `DragFile` and `ClipFile` unidirectional QUIC streams strictly to active offer IDs and declared file sizes (`src-tauri/src/kvm/files.rs`, `staging.rs`).
- **Intermittent receiver-side Cancel:** Investigate rare cases where transfer **Cancel** only took effect from the sender side.
- **Cursor smoothness during large outbound transfers:** Profile QUIC pacing and datagram scheduling when the Linked PC sends large files to the Main PC.
- **Cross-PC drag & drop reliability:** Investigate `DROPEFFECT_NONE` rejections and Linked-to-Main mouse capture/button ownership edge cases (`src-tauri/src/kvm/dragdrop.rs`).
- **Windows 11 top-level Explorer context menu:** Explore a packaged shell extension / sparse package so **Open on Linked PC** appears directly in the top-level Windows 11 context menu.
- **Non-Windows platform seams:** Prototype Linux or macOS backends for any of the `#[cfg(windows)]` seams listed above.
