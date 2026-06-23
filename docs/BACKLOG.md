# Backlog — captured from usage

Items observed during M2 testing, to be sequenced into future milestones.

> **Auto-discover was fixed in M2** by forcing adb's working mDNS backend
> (`ADB_MDNS_OPENSCREEN=0`). The Openscreen default (platform-tools r37) discovers
> nothing on Windows; the legacy backend lists services correctly. Native mDNS is
> therefore *not* needed.

## Wireless / pairing

- **QR-code pairing** (escrcpy-style). Show a QR the phone scans
  (`WIFI:T:ADB;S:<name>;P:<password>;;`); the phone then advertises its pairing
  service and we discover → `adb pair` → `adb connect`. Uses the same mDNS
  discovery that now works, so it's feasible. **Target: M3.**

## UI redesign (target: M4)

- **Tabbed layout**: split the window into **Devices** and **Settings** tabs.
- **Devices tab**: Saved devices on top; newly discovered / undiscovered devices
  below. Remove the separate "Running sessions" section — show per-device status
  inline (e.g. "TECNO_CM7 — Connected" / "Mirroring").
- **Mirror control bar / Android nav overlay**: a sidebar on the mirror window with
  the standard Android navigation (Back / Home / Recents) plus extras — volume
  up/down, screenshot, power, notifications, rotate, etc. scrcpy supports these via
  key injection; expose them as buttons.
