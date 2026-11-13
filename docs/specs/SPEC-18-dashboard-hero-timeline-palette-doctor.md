# SPEC-18: Dashboard — Device Hero Card, Activity Timeline, Command Palette, Connection Doctor

> **Status:** Approved (2026-11-13) | Target Version: `v1.1.0`
> Scope: `(deck)`. Builds on SPEC-17 (Graphite). Adds layout patterns only; visual identity stays Graphite.

## 1. Overview
Four additions that make the Studio Deck easier to read and drive:

1. **Device hero card** — at-a-glance identity and health of the selected device.
2. **Activity timeline** — a local log of what AnyLeap did ("Mirrored TECNO CM7 · 12 min", "Sent 3 files").
3. **Command palette** — `Ctrl+K` launcher for every action.
4. **Connection doctor** — one-click diagnosis of why a device will not connect.

## 2. Device hero card
**Backend:** `get_device_details(serial) -> DeviceDetails { manufacturer, model, android_version, sdk, connection: "usb"|"wifi" }`
from one `adb shell getprop` call parsed for `ro.product.manufacturer`, `ro.product.model`, `ro.build.version.release`, `ro.build.version.sdk`. `connection` = `wifi` when the serial is `host:port`. Cache per serial in the frontend for the app run.

**UI** (top of the Studio Deck workspace stage, above the Launch Matrix, `src/components/deck/DeviceHeroCard.tsx`):
- Left: device name (from saved-device label, fallback model), `Android <ver>`, connection badge (USB / Wi-Fi), battery % + charging icon (existing battery data), live state badge (`DeviceStateBadge`).
- Right: primary actions — Mirror (current Launch Matrix mode), Reconnect (wireless only), Troubleshoot (opens Connection Doctor for this device).
- Empty state when no device is selected: "Connect a phone" + Pair button.

## 3. Activity timeline
**Store** `src/stores/activityStore.ts`:
```ts
type ActivityKind = "mirror" | "camera" | "screenshot" | "files" | "clipboard" | "app" | "kvm" | "device";
interface Activity { id: string; kind: ActivityKind; title: string; detail?: string; at: number; durationMs?: number }
logActivity(a: Omit<Activity, "id" | "at">): void
```
- Ring buffer, newest first, max 200 entries, persisted through the existing `src/lib/persist.ts` mechanism.
- `logActivity` is the single public entry point; later specs (SPEC-20, SPEC-21) call it.

**Emit points in this spec:** mirror/camera session start → end (log on end with duration; log start-failure with detail), screenshot taken, files dropped/pushed, clipboard image pushed, app launched, KVM peer connected/disconnected, device paired/connected/disconnected. Wire each at the place the existing code already handles the result (frontend side); do not add backend events for this.

**UI:**
- Deck side panel "Timeline": latest 5 entries, relative time ("2 m ago"), kind icon, "View all".
- "View all" opens a drawer with the full list, filter chips by kind, and "Clear history".

## 4. Command palette
- `src/components/shell/CommandPalette.tsx`, opened by `Ctrl+K` (global keydown in `App.tsx`; ignore when focus is in an input/textarea) and a "Commands" button in `AppHeader.tsx` showing the shortcut.
- Registry: a plain array built in one function `buildCommands(ctx)` returning `{ id, label, group, keywords?, run, disabled? }[]`. No plugin system.
- Initial commands: switch tab (Devices / Computers / Settings), start each Launch Matrix mode on the selected device, stop mirroring, take screenshot, pair new device, run Connection Doctor, open timeline, check for updates, start/stop KVM server.
- Filtering: case-insensitive substring match over `label` + `keywords`, ranked label-prefix first. Arrow keys + Enter + Esc. Disabled commands render dimmed with the reason.
- Accessible: `role="dialog"`, focus trap, `aria-activedescendant` on the list.

## 5. Connection doctor
**Backend:** `run_connection_doctor(serial: Option<String>) -> Vec<DoctorCheck>`
`DoctorCheck { id, label, status: "pass"|"warn"|"fail"|"skip", detail, fix_hint: Option<String> }`

Checks, in order (later checks `skip` when a prerequisite fails):
1. adb binary resolvable (`adb_path()`), `adb version` output.
2. adb server responds (`adb devices` exits 0).
3. scrcpy binary and `scrcpy-server` resource present; `scrcpy --version`.
4. Device present in `adb devices -l` (if serial given): state `device` = pass; `unauthorized` = fail with hint "Accept the USB debugging prompt on the phone"; `offline` = fail with hint "Reconnect cable or re-pair".
5. Wireless only: TCP connect to `host:port` within 2 s.
6. Device shell responds (`adb -s <serial> shell echo ok`).
7. Android version ≥ 11 for wireless pairing / ≥ 10 for audio (warn otherwise).

**UI:** modal `src/components/ConnectionDoctor.tsx` with a check list (status icon, label, detail, fix hint), "Run again", and "Copy report" (plain text to clipboard). Opened from the hero card, the command palette, and the device row when a connection fails.

## 6. Verification
Automated:
1. `getprop` parser (Rust unit test with fixture output).
2. Doctor check sequencing as a pure function (prerequisite fail → dependents `skip`).
3. `activityStore`: ring buffer cap, ordering, persistence round-trip (Vitest).
4. Palette filter/rank function (Vitest).
Plus AGENTS.md gates: `npm test`, `cargo check`, `npm run build`.

Manual: hero card with USB and Wi-Fi devices; timeline fills during a mirror session; `Ctrl+K` runs a mode; doctor reports `unauthorized` correctly when the prompt is declined.
