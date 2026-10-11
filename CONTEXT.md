# AnyLeap Domain Model & Glossary

## Core Concepts

### Workstation Hub
The desktop application container (Tauri 2 + React) managing connected external endpoints, video pipelines, and input routing.

### Device Connection (`DeviceSource`)
An abstract polymorphic handle representing an attached client device:
- **`AdbDevice`**: An Android device connected via USB or Wi-Fi (ADB pairing / QR code) running `scrcpy` subprocesses for mirroring, camera studio, and audio forwarding.
- **`NetworkPcDevice`**: A secondary PC or laptop connected over local network (mDNS / direct IP) running native Axum/WebSocket streams for virtual KVM, remote desktop, and deck control.

### Virtual KVM (Computer Link)
A software-defined keyboard, video, and mouse multiplexer that allows a single physical mouse and keyboard to control multiple independent PCs by roaming across spatial screen boundaries.

### Edge Leap
The event trigger when a mouse pointer reaches the physical or logical perimeter of a display monitor, causing the local host to capture input and forward coordinates/events to an adjacent linked device.

### Deck Macro Surface
A touch-grid interface (phone, tablet, or secondary window) presenting interactive buttons, hardware vital gauges, and application shortcut triggers.

### Media Source Pipeline
The capture, encoding, and transmission pipeline:
- **Android Display**: `scrcpy` video stream over local ADB forward socket.
- **PC Display**: DXGI Desktop Duplication API + Windows Media Foundation hardware encoder.
- **Camera Studio**: Camera sensor capture routed to native Windows virtual camera or OBS virtual camera interface.
