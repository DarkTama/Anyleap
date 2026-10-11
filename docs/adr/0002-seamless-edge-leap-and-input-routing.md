# ADR 0002: Seamless Edge Leap & Input Routing for Virtual KVM

## Status
Accepted

## Context
For multi-PC control, users need mouse and keyboard input to move seamlessly between physical computers across their desk.
When the mouse cursor touches the edge of a monitor adjacent to a linked secondary computer, control must transition without interrupting productivity or requiring manual hotkeys.

## Decision
1. **Edge Detection**:
   - Host runs a Windows low-level mouse hook (`WH_MOUSE_LL`) tracking cursor position relative to configured display topologies.
   - When cursor reaches configured monitor bounds, host captures the cursor, hides the local cursor (or clips it to boundary), and begins streaming relative coordinate deltas (`dx`, `dy`) and mouse clicks to the peer PC over WebSocket.
2. **Keyboard Follows Mouse Focus**:
   - As long as cursor focus resides on the target PC, host runs a low-level keyboard hook (`WH_KEYBOARD_LL`) that intercepts keystrokes, suppresses local execution (`return 1`), and forwards key codes to the target PC.
   - Target PC injects inputs via Win32 `SendInput`.
3. **Return Transition**:
   - When target PC reports cursor has crossed back over the reverse boundary, local hooks disengage suppression and restore local cursor control.
4. **Clipboard Forwarding**:
   - Host listens for `WM_CLIPBOARDUPDATE`. When copied data changes while connected, text payloads replicate to peer over the command channel.

## Consequences
### Positive
- Natural dual-PC workflow: mouse slides across desks, keyboard automatically types into whichever computer cursor is resting on.
- Zero extra hardware required.
### Negative
- Low-level hooks must be carefully managed to avoid thread blocking or missed inputs in Windows message loops.
- Requires elevation / admin awareness if target processes run elevated (UAC).
