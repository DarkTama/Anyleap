# AnyLeap Domain Model & Glossary

## Core Concepts

### Workstation Hub
The desktop application container (Tauri 2 + React) managing connected external endpoints, video pipelines, and input routing.

### Device Connection (`DeviceSource`)
An abstract polymorphic handle representing an attached client device:
- **`AdbDevice`**: An Android device connected via USB or Wi-Fi (ADB pairing / QR code) running `scrcpy` subprocesses for mirroring, camera studio, and audio forwarding.
- **`NetworkPcDevice`**: A secondary PC or laptop connected over local network (mDNS / direct IP) running a native QUIC link for virtual KVM, remote desktop, and deck control.

### Virtual KVM (Computer Link / Shared Mouse & Keyboard)
A software-defined keyboard, video, and mouse multiplexer that allows a single physical mouse and keyboard to control another PC by roaming across spatial screen boundaries.

#### UI-Term ↔ Code-Term Glossary (SPEC-23)
| User-Facing UI Term | Internal Code Term(s) | Meaning |
|---|---|---|
| **Shared Mouse & Keyboard** | `kvm`, Computer Link | Feature name across UI; backend module and commands retain the `kvm` prefix. |
| **Main PC** | Controller (`kvm.client`, `hooks`, `role: "main"`) | PC whose physical mouse and keyboard are used; runs input hooks and QUIC client. |
| **Linked PC** | Receiver (`kvm.server`, `injector`, `role: "linked"`) | PC being controlled; runs QUIC server and injects received input. |
| **Allow other PCs to control this PC** | `allow_control`, `kvm_set_allow_control`, `is_host_running` | Starts/stops the QUIC receiver server independently of any outgoing Main PC controller link. |
| **Pairing code** | `security_pin`, `pin` | 6-digit one-time code required only on first link before the peer's certificate fingerprint is pinned. |
| **Link** | `kvm_connect_peer`, `kvm_pair_peer` | Establishes the active single-peer link from Main PC to Linked PC. |
| **Unlink** | `kvm_disconnect_peer` | Role-aware disconnect: closes outgoing client + stops hooks on Main PC, or closes incoming controller sessions on Linked PC. |
| **Forget** | `kvm_forget_peer` | Removes a peer's pinned fingerprint from `TrustStore` by `computer_id` and closes any active session with it. |
| **Bring cursor back** | `kvm_emergency_release` | Un-traps the Main PC cursor and sends `EmergencyRelease` / `ReleaseAll`. |

### Trust Store
`kvm_trusted_peers.json` in the app data dir: the peers (`computer_id`, `name`, `fingerprint`) a PC has paired with. A connection from an unknown fingerprint may only send `Pair`; everything else closes it.

### Peer Fingerprint
SHA-256 (hex) of a peer's self-signed certificate DER. Pinned at PIN pairing and checked on every later connection; a changed fingerprint means "peer identity changed — re-pair".

### Edge Leap & Per-Monitor Layout (`SPEC-24`)
The spatial transition mechanism that lets a cursor move seamlessly between PCs across touching monitor boundaries:
- **Remote Origin (`remote_origin`)**: Optional `Point { x, y }` placement of the Linked PC's virtual-desktop origin `(0, 0)` inside the Main PC's virtual-desktop coordinates (physical px). Supersedes single-edge `TopologyLink` offsets (and auto-migrates from legacy links when absent).
- **Seam (`Seam` / `LinkedSeam`)**: A strictly positive overlapping segment where a Main PC monitor edge touches a Linked PC monitor edge with zero gap and zero overlap. Controller leaps (`CursorEnter`) and receiver returns (`CursorReturn`) trigger only across active seams (with per-seam `dwell_ms` and end-of-seam `deadzone_px`) and carry `target_monitor` (`device_name`) and `along_px` (local pixel coordinate along the target monitor's entry edge). Crossing an uncovered edge clamps inside the local desktop.

### Deck Macro Surface
A touch-grid interface (phone, tablet, or secondary window) presenting interactive buttons, hardware vital gauges, and application shortcut triggers.

### Media Source Pipeline
The capture, encoding, and transmission pipeline:
- **Android Display**: `scrcpy` video stream over local ADB forward socket.
- **PC Display**: DXGI Desktop Duplication API + Windows Media Foundation hardware encoder.
- **Camera Studio**: Camera sensor capture routed to native Windows virtual camera or OBS virtual camera interface.

### Studio Deck Diagnostics & Navigation
- **Device Hero Card**: Top-of-deck summary showing the selected device's identity, Android version, connection type (`usb`/`wifi`), battery telemetry, live state, and primary actions.
- **Activity Timeline**: Persistent local ring buffer (max 200 entries) recording mirroring sessions, screenshots, file/clipboard transfers, app launches, KVM peer events, and device connections.
- **Command Palette**: Keyboard-driven (`Ctrl+K`) action launcher across tabs, streaming profiles, diagnostics, and KVM controls.
- **Connection Doctor**: Sequenced diagnostic pipeline (`adb_binary` → `adb_server` → `scrcpy_binary` → `device_present` → `wireless_tcp` → `device_shell` → `android_version`) with actionable fix hints and plain-text report export.

### Phone Workspace (`PhoneView`)
The ADB-only companion view for a selected `AdbDevice` (requiring no companion app on the phone):
- **Device Storage Browser (`FilesView`)**: Sandboxed file operations scoped to `/storage/emulated/0` via `stat`, `adb pull`, `adb push`, and `rm -r`.
- **Photo Gallery (`PhotosView`)**: Paginated, lazy-loaded grid of `DCIM/Camera` and `Pictures/Screenshots` images backed by a 500 MB LRU/atime local disk cache.
- **Read-Only Notifications (`NotificationsView`)**: In-memory view of active notifications parsed from `dumpsys notification --noredact` (never persisted or logged), with optional desktop pop-ups.

### ADB Quick Actions
Direct device-control and session utilities executed over `adb` and `scrcpy`: URL handoff (`VIEW` intent), per-app virtual display windows (`--new-display --start-app`), system toggles (`svc`/`cmd notification`), MP4 screen recording (`--record`), Windows battery threshold notifications, and telephony intents (`CALL`/`DIAL`, `KEYCODE_ENDCALL`).

### Ink & Gold Design System (`SPEC-26`)
Flat, high-contrast workstation theme replacing the Graphite palette (`SPEC-17`):
- **Ink Surfaces**: `--ink-950` (`#12141F` canvas), `--ink-900` (`#1B1E2E` panels/tiles), `--ink-800` (`#2A2D40` raised controls), `--ink-700` (`#3A3D52` borders/hover).
- **Text Hierarchy**: `--text-strong` (`#ECEAE3`), `--text` (`#C9CBD6`), `--text-muted` (`#8B8FA3`).
- **Accents**: `--gold-400` (`#D8B46A`, used sparingly for primary CTAs, active/selected states, focus rings, active KVM edge highlight, and brand moments), `--gold-600` (`#A9822F` pressed/light accent), `--mist-300` (`#B7C9D6` info/secondary highlight).
- **Status Tokens**: `--status-success` / `--status-warning` / `--status-danger` (non-neon, `>= 4.5:1` contrast).
- **Motion Curve**: `--ease-out` (`cubic-bezier(0.23, 1, 0.32, 1)`), 160ms press feedback (`scale(0.97)`), zero gradients or backdrop-blur, zero animation on keyboard-initiated actions (`Ctrl+K`).
