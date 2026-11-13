# SPEC-20: ADB Quick Actions Pack

> **Status:** Implemented (merged to main 2026-11-13; ships in next release) | Target Version: `v1.1.0`
> Scope: `(deck)`, `(mirror)`. No companion app; everything runs over adb and the bundled scrcpy.

## 1. Overview
Six small capabilities, each a thin adb/scrcpy wrapper plus a UI entry point:

| # | Capability | Mechanism |
|---|-----------|-----------|
| A | Open link on phone | `am start -a android.intent.action.VIEW -d <uri>` |
| B | App in its own window | scrcpy `--new-display` + `--start-app=<package>` |
| C | Phone toggles: Wi-Fi, Bluetooth, Do Not Disturb | `svc wifi`, `svc bluetooth`, `cmd notification set_dnd` |
| D | Battery alerts | existing `get_battery_info` + Windows toast |
| E | Screen recording | scrcpy `--record=<file>` |
| F | Place / end a phone call | `am start -a android.intent.action.CALL`, `KEYCODE_ENDCALL` |

## 2. Shared rule: device-shell quoting (security)
`adb shell <args…>` joins arguments into one string that the **device** shell parses. Any user-supplied value (URL, phone number, path) must pass through one helper:

```rust
/// Wrap a value in single quotes for the Android shell; ' becomes '\''.
pub(crate) fn sh_quote(s: &str) -> String
```

Unit tests: plain text, spaces, `;`, `$()`, backticks, embedded `'`, empty string. Every new command in this spec uses it.

## 3. Backend (`src-tauri/src/commands.rs`, register in `lib.rs`)

### A. `open_url_on_device(serial, url) -> Result<(), String>`
- Accept only schemes `http`, `https`, `geo`, `mailto`; reject everything else (no `intent:`, `file:`, `content:`).
- `adb -s <serial> shell am start -a android.intent.action.VIEW -d <sh_quote(url)>`. Treat output containing `Error:` as failure.

### B. `launch_app_window(serial, package) -> Result<SessionInfo, String>`
- Validate `package` against `^[A-Za-z][A-Za-z0-9_]*(\.[A-Za-z][A-Za-z0-9_]*)+$`.
- Start a normal mirror session through the existing session machinery (`start_mirror` path, same process tracking and teardown as flex mode) with extra args `--new-display` and `--start-app=<package>`. Window title: `<package> — <device>`.
- Multiple app windows per device are allowed; each is its own session.

### C. Phone toggles
- `get_phone_toggles(serial) -> PhoneToggles { wifi: bool, bluetooth: bool, dnd: bool }`
  from `settings get global wifi_on`, `settings get global bluetooth_on`, `settings get global zen_mode` (`0` = off).
- `set_phone_toggle(serial, toggle: "wifi"|"bluetooth"|"dnd", on: bool)`:
  - wifi → `svc wifi enable|disable`
  - bluetooth → `svc bluetooth enable|disable`
  - dnd → `cmd notification set_dnd on|off`
- **Guard:** if the device serial is a network address (wireless adb), refuse `wifi` off with error `"Turning Wi-Fi off would disconnect this wireless session"`.

### D. Battery alerts (frontend-driven)
- Add `tauri-plugin-notification` (Rust + JS package, capability permission).
- Where battery is already polled (DeviceRack), fire one toast when a device crosses **≤ 20 % while not charging** and one when it reaches **100 % while charging**. Fire once per crossing (re-arm after leaving the band).
- Setting `batteryAlerts: boolean` (default `true`) in the existing settings store.

### E. Screen recording
- `start_recording(serial)` / `stop_recording(serial)`.
- Implementation follows the existing `restart_with_screen_off` pattern: restart the device's active session with `--record=<path>` added (or removed). Path: `%USERPROFILE%\Videos\AnyLeap\<model>-<YYYYMMDD-HHMMSS>.mp4`; create the directory.
- `SessionInfo` gains `recording_path: Option<String>` so the UI can show state and "Open folder".
- Stopping the session ends the recording (scrcpy finalises the file on exit) — make sure teardown sends a graceful stop, not a hard kill, so the MP4 is not truncated. Verify with a manual test.

### F. Calls
- `place_call(serial, number)`: validate `^[0-9+*#() -]{3,20}$`, strip spaces/dashes/parentheses, then `am start -a android.intent.action.CALL -d tel:<sh_quote(number)>`. If the output reports a permission error, fall back to `android.intent.action.DIAL` (opens the dialer prefilled) and return `Ok` with a note so the UI can say "Confirm on phone".
- `end_call(serial)`: reuse `send_keyevent` with `KEYCODE_ENDCALL` (6).

## 4. Frontend
- **Control Bar** (`src/components/ControlBar.tsx`): add buttons for Record (toggle, red dot while active), Wi-Fi / Bluetooth / DND toggles (state from `get_phone_toggles`, refreshed on open), End call.
- **Device row / rack** (`DeviceRack.tsx`): "Open link on phone" action → small input prefilled from the PC clipboard if it holds a URL; "Call…" action → number input.
- **App drawer** (`AppDrawer.tsx`): each app gets "Open in window" next to the existing launch action.
- **Settings**: `batteryAlerts` switch.
- Follow the Graphite visual language (SPEC-17); reuse `src/components/ui` primitives and existing icon set (lucide).

## 5. Verification
Automated (`cargo test`, `npm test`):
1. `sh_quote` cases (section 2).
2. URL scheme allow-list, package-name and phone-number validators.
3. Toggle-state parser for `settings get` output (`0`, `1`, `null`, garbage).
4. Battery crossing logic as a pure function `nextBatteryAlert(prev, current) -> "low"|"full"|null` with tests for re-arm.
5. scrcpy arg builder: app-window args contain `--new-display` and `--start-app=<pkg>`; recording args contain `--record=`.
Plus AGENTS.md gates: `npm test`, `cargo check`, `npm run build`.

Manual (real device): each of A–F once; recording file plays to the end.

## 6. Out of scope
Answering incoming calls, reading call state, mobile-data toggle (needs root on most devices), flashlight outside camera mode.
