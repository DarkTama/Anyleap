# AnyLeap Domain Model & Glossary

## Core Concepts

### Workstation Hub
The desktop application container (Tauri 2 + React) managing connected external endpoints, video pipelines, and input routing.

### Device Connection (`DeviceSource`)
An abstract polymorphic handle representing an attached client device:
- **`AdbDevice`**: An Android device connected via USB or Wi-Fi (ADB pairing / QR code) running `scrcpy` subprocesses for mirroring, camera studio, and audio forwarding.
- **Stable Device Identity (`hw_serial` & `transports`)**: Every Android phone is keyed by its hardware serial (`ro.serialno` / `ro.boot.serialno`, or parsed from mDNS `_adb-tls-connect._tcp` instance names `adb-<serial>-<suffix>`), rather than ephemeral wireless `IP:port` endpoints. USB and Wi-Fi transports for the same `hw_serial` merge into one logical device entry across `list_devices`, saved devices, Device Rack, Hero Card, and Timeline (preferring USB when both transports are live). When Wireless debugging rotates ports, AnyLeap matches `_adb-tls-connect._tcp` by `hw_serial`, calls `adb connect` on the new endpoint, removes stale offline `IP:port` duplicates, and only prompts to re-pair (`"This phone needs to be paired again"`) if no mDNS match exists or ADB rejects authentication.
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

### Continuity (`SPEC-25`)
Cross-PC workstation continuity features layered over the authenticated QUIC link (Windows):
- **Staging**: Receiving incoming cross-PC file transfers into a sanitized temporary staging directory over unidirectional QUIC streams before committing them to the local clipboard (`CF_HDROP`), OLE drop target, or shell-open handler.
- **Clip Offer / Prefetch Limit / Fetch**: File copy-paste broadcasts a `ClipFileOffer` metadata packet first; transfers within the configured **prefetch limit** (`stagingLimitMb`, default 100 MB) stream immediately, while larger offers wait for the user to click **Fetch** on the receiving PC.
- **Transfer Bar**: In-app progress banner tracking active cross-PC file transfers with **Cancel** support on both sender and receiver (`TransferCancel`).
- **Edge Catcher & Drag Mode**: An invisible, topmost, non-activating 2-px OLE drop-target strip (`WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE`) shown along active seams when **Drag & drop** (Experimental, off by default) is enabled and a mouse button is held; catching `CF_HDROP` cancels the local drag and leaps in **drag mode** across the seam.
- **Audio Bridge**: Unidirectional 48 kHz stereo PCM audio streaming from Linked PC to Main PC using tagged (`0x41`) QUIC datagrams and a ~40 ms sequence-ordered jitter buffer, with media/volume keys following whichever PC produced active audio within the last 10 seconds.
- **Process Loopback & Linked Mute**: WASAPI `VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK` (`PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE`) capture paired with `IAudioEndpointVolume::SetMute` (**Linked mute**, persisted via a crash marker in `%LOCALAPPDATA%\AnyLeap\audio_mute_marker.json`) so the Linked PC's local speakers stay silent without muting the bridged stream.
- **Session Sync**: Propagating Windows session lock (`WM_WTSSESSION_CHANGE`) and suspend (`WM_POWERBROADCAST`) events to the linked peer with echo-suppression windows; unlock is never automated.
- **Open on Linked PC**: Handoff of a local file (via the `HKCU` Windows Explorer context menu `--open-on-linked`, under *Show more options* on Windows 11) or URL (via Command Palette **Open URL on Linked PC**) to open on the linked peer with scheme and executable safety guards.

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

### Phone Hub (`src-tauri/src/phone_hub/`, `SPEC-27`)
The tiered backend phone-data service and Phone tab dashboard for a connected `AdbDevice` (ADB + bundled `scrcpy` only, no phone companion app):
- **Poll Tiers (`poller`)**: Per-serial polling scheduled at **static** (once per connection: model, brand, Android version, security patch), **fast** (30 s while watched: battery, Wi-Fi, cellular, screen state), **slow** (5 min while watched or manual refresh: `/data` storage, RAM, uptime), and **notifications** (default 10 s, min 5 s via `notificationPollSecs`), all paused on unwatch and cleaned up on disconnect.
- **App Label Cache (`AppLabelCache`)**: Disk-cached (`phone_hub_apps.json`) mapping of package IDs to human-readable app labels from `scrcpy --list-apps`, keyed by stable device identity (`hw_serial`) and applied to notifications, messages, and app lists.
- **Conversation (`MessageStore`)**: Memory-only chat history aggregated from `MessagingStyle` / `category=msg` notification dumps, keyed by `<package>::<conversationKey>`, deduplicated by `(sender, text, timestampMs)`, capped at 200 messages per conversation, never logged or written to disk, and cleared on device disconnect or app exit.
- **Audio-Only (`audio_only`)**: Headless PC audio playback via `scrcpy --no-video --no-window` (refused if a mirror session with audio is already active on the same device).

### Phone Workspace (`PhoneView`, `SPEC-27`)
The ADB-only companion dashboard for a selected `AdbDevice` (requiring no companion app on the phone):
- **Overview (`OverviewView`)**: Status cards for battery, storage/RAM, Wi-Fi/cellular signal, and device info, plus quick actions (Mirror, Audio-only, and Wi-Fi/Bluetooth/DND toggles with wireless Wi-Fi-off confirmation).
- **Messages (`MessagesView`)**: Read-only chat history aggregated in memory from chat notifications, grouped by app and conversation, and cleared on disconnect or app exit.
- **Notifications (`NotificationsView`)**: Active notifications grouped by resolved app label with persisted per-app mute filtering (`mutedPackages`) applied to both the list and native desktop toasts.
- **Gallery (`GalleryView`)**: Infinite-scroll (pages of 60) windowed grid of MediaStore photos and videos across all albums, with an in-app photo viewer, direct PC playback for videos, and single/multi-select downloads via `pull_device_file`.
- **Device Storage Browser (`FilesView`)**: Sandboxed file operations scoped to `/storage/emulated/0` via `stat`, `adb pull`, `adb push`, and `rm -r`.
- **Apps Launcher (`AppsView`)**: Searchable list/grid of installed user and system apps (resolved via cached app labels) with one-click per-app mirror launch.

### ADB Quick Actions
Direct device-control and session utilities executed over `adb` and `scrcpy`: URL handoff (`VIEW` intent), per-app virtual display windows (`--new-display --start-app`), system toggles (`svc`/`cmd notification`), MP4 screen recording (`--record`), Windows battery threshold notifications, and telephony intents (`CALL`/`DIAL`, `KEYCODE_ENDCALL`).

### Ink & Gold Design System (`SPEC-26`)
Flat, high-contrast workstation theme replacing the Graphite palette (`SPEC-17`):
- **Ink Surfaces**: `--ink-950` (`#12141F` canvas), `--ink-900` (`#1B1E2E` panels/tiles), `--ink-800` (`#2A2D40` raised controls), `--ink-700` (`#3A3D52` borders/hover).
- **Text Hierarchy**: `--text-strong` (`#ECEAE3`), `--text` (`#C9CBD6`), `--text-muted` (`#8B8FA3`).
- **Accents**: `--gold-400` (`#D8B46A`, used sparingly for primary CTAs, active/selected states, focus rings, active KVM edge highlight, and brand moments), `--gold-600` (`#A9822F` pressed/light accent), `--mist-300` (`#B7C9D6` info/secondary highlight).
- **Status Tokens**: `--status-success` / `--status-warning` / `--status-danger` (non-neon, `>= 4.5:1` contrast).
- **Motion Curve**: `--ease-out` (`cubic-bezier(0.23, 1, 0.32, 1)`), 160ms press feedback (`scale(0.97)`), zero gradients or backdrop-blur, zero animation on keyboard-initiated actions (`Ctrl+K`).

### Mode defaults (`SPEC-28`, ADR-0005)
The per-mode mirroring settings persisted as `modeDefaults` in `settings.json`. Each **mirroring mode** (`mirror`, `flex`, `camera`, `hifi`; code type `StudioMode`) owns a complete object, so switching modes never changes another mode. A Devices launch builds its `CoreSettings` with `resolveLaunchSettings(mode, defaults)`, which forces only `flexDisplay` (true for `flex`).
- **Quality** (UI): per-mode choice Smooth / Balanced / Sharp / Maximum, the former Low / Medium / High / Highest preset bundles. Derived from `maxSize`, `videoBitRate`, `maxFps`, `videoCodec`; shown as Custom when no bundle matches. Replaces the global "Quality preset".
- **Settings rail**: seven sections (General, Phone mirroring, Control bar, Shared mouse & keyboard, Clipboard & files, Audio, media & power, Advanced) with a filter box.
- **Single-source rule**: a setting has one editor; other places show it read-only with a link (for example dwell and deadzone are edited in the Computers tab).
