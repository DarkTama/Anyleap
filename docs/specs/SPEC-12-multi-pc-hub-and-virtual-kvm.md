# SPEC-12: Multi-PC Hub & Seamless Virtual KVM (Computer Link)

> **Status:** Draft / Active Design | Target Version: `v1.0.0`
> **Path:** Architectural (Phase 1 of Multi-PC Workstation Expansion)

## 1. System Overview & Objective

Expand AnyLeap beyond Android ADB mirroring by introducing a local peer-to-peer subsystem that connects multiple personal computers over local Wi-Fi or USB tethering.

Primary deliverable: **Virtual KVM (Computer Link)**.
- A single physical mouse and keyboard controls two or more PCs seamlessly.
- Crossing monitor perimeter on Primary PC smoothly warps cursor to Secondary PC.
- Keyboard focus automatically routes to whichever PC holds the cursor.
- Bidirectional clipboard synchronization across connected machines.

---

## 2. Network & Transport Architecture

```
[ Primary PC (Host) ]                             [ Secondary PC (Client) ]
   │                                                 │
   ├── mDNS Discovery (UDP 5353) ───────────────────►│ (Discovers Peer)
   │                                                 │
   │◄── HTTP POST /pair ─────────────────────────────┤ (Requests Pairing)
   ├─── HTTP POST /pair/confirm (6-digit PIN) ──────►│ (Verifies Identity)
   │                                                 │
   │◄════ WebSocket /ws (TCP 4177, TLS) ════════════►│ (Keystrokes, Clicks, Clipboard, State)
   │                                                 │
   │────── UDP Mouse Delta Datagrams (Port 4178) ───►│ (Sub-ms relative dx/dy cursor packets)
```

### 2.1 Listening Ports
- **`4177 TCP`**: Axum HTTP server and WebSocket `/ws` endpoint over local TLS. Carries pairing handshakes, keyboard events, mouse click state, clipboard synchronization, and topology configuration.
- **`4178 UDP`**: Raw UDP socket for high-frequency relative mouse movement deltas (`dx`, `dy`). Bypasses TCP head-of-line blocking for sub-millisecond cursor feel.

### 2.2 Security & Authentication
- Ephemeral TLS certificates generated per installation via `rcgen`.
- Pairing authenticated using a 6-digit cryptographically random numeric PIN displayed on host screen.
- Paired peer UUIDs and public keys persisted in local settings.
- Firewall helper checks and adds rules for ports 4177 (TCP) and 4178 (UDP).

---

## 3. Input Capture & Edge Leap Engine

### 3.1 Display Topology Model
```json
{
  "computers": [
    {
      "id": "pc-primary",
      "name": "Desktop PC",
      "isLocal": true,
      "bounds": { "x": 0, "y": 0, "width": 2560, "height": 1440 }
    },
    {
      "id": "pc-secondary",
      "name": "Laptop",
      "isLocal": false,
      "bounds": { "x": 2560, "y": 180, "width": 1920, "height": 1080 }
    }
  ],
  "links": [
    {
      "from": "pc-primary",
      "edge": "right",
      "to": "pc-secondary",
      "targetEdge": "left"
    }
  ]
}
```

### 3.2 Win32 Low-Level Input Hooks (`src-tauri/src/kvm/hooks.rs`)
1. **Mouse Traversal**:
   - `WH_MOUSE_LL` hook monitors cursor coordinates.
   - When cursor reaches configured monitor boundary (`x >= 2560`), cursor focus transitions to `pc-secondary`.
   - Local cursor is hidden (`ShowCursor(FALSE)`) and clipped to screen edge via `ClipCursor`.
   - Host begins streaming raw delta coordinates (`dx`, `dy`) over UDP port 4178 to secondary PC.
2. **Keyboard Suppression & Forwarding**:
   - `WH_KEYBOARD_LL` hook activates when focus is remote.
   - Intercepts all `WM_KEYDOWN`, `WM_KEYUP`, `WM_SYSKEYDOWN`, `WM_SYSKEYUP`.
   - Suppresses local execution (`return 1`).
   - Packages virtual key codes and scan codes into JSON payloads sent over WebSocket `/ws`.
3. **Remote Cursor Return**:
   - When secondary PC detects cursor touching reverse boundary (`x <= 0`), it signals host via WebSocket.
   - Host unclips and unhides local cursor, disengages keyboard hook suppression, and restores local control.

### 3.3 Target Input Injection (`src-tauri/src/kvm/injector.rs`)
- Secondary PC consumes packets and calls Win32 `SendInput` for mouse movements, button clicks, and keystrokes.
- Includes in-app "Elevate Helper" action allowing AnyLeap to restart with elevated administrator privileges if controlling elevated target applications.

---

## 4. Clipboard Synchronization (`src-tauri/src/kvm/clipboard.rs`)

- Host and client register `AddClipboardFormatListener` to receive `WM_CLIPBOARDUPDATE` messages.
- When clipboard content changes while linked:
  - Extracts UTF-8 plain text string.
  - Sends `{ "type": "clipboard", "text": "..." }` over `/ws`.
  - Receiving peer updates local Windows clipboard without triggering re-transmission loop (guarded by re-entrancy flag).

---

## 5. UI Architecture & Placement

1. **Top Navigation "Computers" Tab (`src/components/computers/ComputersView.tsx`)**:
   - Discovered nearby computers via mDNS.
   - "Pair Computer" dialog with 6-digit PIN prompt.
   - Interactive 2D drag-and-drop monitor layout canvas with edge snap guides.
2. **Studio Deck Quick Status Badge (`src/components/deck/ComputerLinkStatusBadge.tsx`)**:
   - Real-time indicator displaying active link state, peer RTT latency, and emergency unlock hotkey reminder (`ScrollLock` or `Ctrl+Alt+Shift+Escape`).

---

## 6. Error Handling & Failsafes

- **Emergency Escape Hotkey**: Pressing `ScrollLock` or `Ctrl+Alt+Escape` instantly breaks input trap, kills low-level hooks, and restores local mouse/keyboard control.
- **Heartbeat Timeout**: If WebSocket disconnects or fails heartbeat (> 3000ms), input hooks immediately release to prevent trapped input.
- **Firewall Prompt**: Diagnostic check warns user if port 4177/4178 inbound traffic is blocked.

---

## 7. Verification & Testing

- **T1: Discovery & Pairing**: Verify mDNS advertisement and 6-digit PIN handshake between two AnyLeap instances.
- **T2: Edge Leap**: Verify mouse moves across right edge of Primary PC and appears on left edge of Secondary PC.
- **T3: Keyboard Focus**: Verify typing while cursor on Secondary PC types into Secondary PC notepad, not Primary PC.
- **T4: Return Leap**: Verify moving cursor back to left edge immediately returns control to Primary PC.
- **T5: Clipboard Sync**: Verify copying text on Primary PC allows pasting on Secondary PC.
- **T6: Failsafe Hotkey**: Verify emergency hotkey frees cursor if network connection drops mid-session.
