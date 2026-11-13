# AnyLeap Domain Model & Glossary

## Core Concepts

### Workstation Hub
The desktop application container (Tauri 2 + React) managing connected external endpoints, video pipelines, and input routing.

### Device Connection (`DeviceSource`)
An abstract polymorphic handle representing an attached client device:
- **`AdbDevice`**: An Android device connected via USB or Wi-Fi (ADB pairing / QR code) running `scrcpy` subprocesses for mirroring, camera studio, and audio forwarding.
- **`NetworkPcDevice`**: A secondary PC or laptop connected over local network (mDNS / direct IP) running a native QUIC link for virtual KVM, remote desktop, and deck control.

### Virtual KVM (Computer Link)
A software-defined keyboard, video, and mouse multiplexer that allows a single physical mouse and keyboard to control multiple independent PCs by roaming across spatial screen boundaries.

### Trust Store
`kvm_trusted_peers.json` in the app data dir: the peers (`computer_id`, `name`, `fingerprint`) a PC has paired with. A connection from an unknown fingerprint may only send `Pair`; everything else closes it.

### Peer Fingerprint
SHA-256 (hex) of a peer's self-signed certificate DER. Pinned at PIN pairing and checked on every later connection; a changed fingerprint means "peer identity changed — re-pair".

### Edge Leap
The event trigger when a mouse pointer reaches the physical or logical perimeter of a display monitor, causing the local host to capture input and forward coordinates/events to an adjacent linked device.

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
