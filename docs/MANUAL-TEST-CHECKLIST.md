# Manual Test Checklist — SPEC-18 to SPEC-22

Build under test: `main` @ `c28b9c0`. Tick each box; note failures with device/OS and steps.

Equipment: 2 Windows PCs on the same LAN (ideally one with 2 monitors of different resolution), 1 Android phone (USB + wireless adb).

---

## A. Computer Link / KVM (SPEC-19, SPEC-22) — needs 2 PCs

### Pairing & security
- [ ] PC-B: start receiver; PC-A: "Link PC" on the discovered PC-B → PIN dialog opens automatically
- [ ] Wrong PIN → "Incorrect PIN"; 5 wrong PINs → PIN changes on PC-B and pairing is refused for ~60 s, even with the new correct PIN
- [ ] Correct PIN → paired; disconnect + reconnect → no PIN asked
- [ ] "Forget peer" → next connect asks for PIN again
- [ ] Discovery still finds the other PC with Cloudflare WARP / VPN enabled
- [ ] Manual IP field starts empty

### Pointer feel
- [ ] Leap A → B at the configured edge; cursor appears at the same relative height on B
- [ ] Slow, precise movement on B feels like a local mouse (no double acceleration)
- [ ] Fast flick on B travels the expected distance (no truncation)
- [ ] Return B → A only when B's cursor really reaches the entry edge (no early return in mid-screen, no getting stuck at the edge)
- [ ] Back on A: cursor appears at matching height, visible, not clipped
- [ ] Holding a mouse button at the edge (drag) does **not** leap
- [ ] Receiver with 2 monitors: both reachable; cursor never escapes into a gap; returns only through the entry edge
- [ ] Controller with 2 monitors: leap only from the configured outer edge, not from the inner border between its own monitors
- [ ] Different resolutions (e.g. 1080p ↔ 1440p/4K): entry/return heights still line up

### Keyboard & stuck input
- [ ] Typing on B works, including Shift, Ctrl+C/V, Alt+Tab, arrow keys, numpad, AltGr (if used)
- [ ] Hold Ctrl on A, leap to B, release → Ctrl not stuck on A
- [ ] Hold Shift on B, return to A, release → Shift not stuck on B
- [ ] Scroll Lock / Pause / Ctrl+Alt+Esc → instant release, cursor visible on A, nothing stuck on B
- [ ] "Release Cursor Now" button works

### Clipboard & files
- [ ] Copy text on A → paste on B (and B → A)
- [ ] Copy cells from a spreadsheet / text from a browser page with images → pastes as **text**
- [ ] Copy a screenshot (image only) → pastes as image
- [ ] Settings: turn off text sync → copy no longer syncs and no activity entry appears; same for image
- [ ] Send a large file (≥ 500 MB) → arrives intact in the configured receive folder (default `Downloads\AnyLeap`); mouse/keyboard stay smooth during transfer
- [ ] Turn off file sync on the receiver → incoming file is rejected, no `.part` leftovers
- [ ] Sending a file whose name already exists → saved as `name (1).ext`

### Sessions
- [ ] Unplug network / close app on B while cursor is on B → A gets its cursor back automatically within ~10 s
- [ ] With two controllers linked to one receiver, disconnecting one leaves the other working

---

## B. Phone tab (SPEC-21) — needs phone

- [ ] Files: browse Internal storage, Download, DCIM; breadcrumb navigation
- [ ] Download a file to PC; upload a file from PC (button and drag-drop)
- [ ] Delete a test file and a test folder (confirmation shown); internal storage root cannot be deleted
- [ ] Folder/file names with spaces, quotes (`it's`), and `;` work
- [ ] Photos: grid loads newest first; "Load more" past 2 pages; lightbox + download
- [ ] Notifications: a fresh notification appears within ~10 s; "Copy" copies the OTP text; **check on your Android version — parser is the riskiest part (fixtures were synthetic)**
- [ ] Desktop pop-ups setting (default off) → when on, new notifications show a Windows toast

## C. Quick actions (SPEC-20) — needs phone

- [ ] Open link on phone (http/https); `file:` / `intent:` URLs rejected
- [ ] App drawer → "Open in window" opens the app in its own window; two apps side by side
- [ ] Wi-Fi / Bluetooth / DND toggles reflect real state and work; Wi-Fi-off refused when connected over wireless adb
- [ ] Battery alert toast at ≤ 20 % (not charging) and at 100 % (charging), once per crossing; setting can disable it
- [ ] Screen recording → MP4 in `Videos\AnyLeap` plays to the end
- [ ] Toggle screen-off **while recording** → recording stops cleanly, file plays fully, no overwrite
- [ ] Switch mirror mode while recording → file plays fully
- [ ] Place a call (dialer may ask for confirmation); End call works

## D. Dashboard (SPEC-18)

- [ ] Hero card: model, Android version, USB / Wi-Fi badge, battery; empty state with no device
- [ ] Timeline logs mirror sessions with duration, screenshots, files, KVM connect/disconnect, phone file actions; never notification contents
- [ ] Ctrl+K opens palette (not while typing in an input); arrows/Enter/Esc; disabled items show a reason
- [ ] Connection Doctor: all pass with a healthy device; decline the USB-debugging prompt → "unauthorized" with fix hint; "Copy report" works

## E. Regression smoke

- [ ] Standard mirror, Desktop Flex, Camera Studio, High Fidelity modes still start and stop
- [ ] QR / mDNS wireless pairing still works
- [ ] In-app updater still detects updates (after the next version bump)

---

## Automated receiver harness

Single-PC check that QUIC-injected keys reach a focused Win32 `EDIT` control on the local AnyLeap receiver without physical input.

1. Start AnyLeap locally with **Allow other PCs to control this PC** enabled and copy the 6-digit **Pairing code**.
2. First run (pairs and caches its certificate/trust store under `%LOCALAPPDATA%\anyleap-kvm-harness`):
   ```powershell
   cargo run --example kvm_harness --manifest-path src-tauri/Cargo.toml -- --addr 127.0.0.1:4177 --pin <CODE> --scenario all --repeat 3
   ```
3. Subsequent runs reuse the cached pairing without `--pin`:
   ```powershell
   cargo run --example kvm_harness --manifest-path src-tauri/Cargo.toml -- --scenario hud --repeat 5
   ```
4. Options: `--scenario` (`basic`, `hud` [<=50 ms while HUD toast appears], `reenter`, `after-click`, or `all`), `--repeat <N>`, `--delay-ms <MS>`, `--addr <IP:PORT>`. Prints per-step foreground HWND/process and a summary table; exits non-zero on any `FAIL`.


---

## F. Per-mode mirroring settings (SPEC-28 phase 1)

- [ ] Upgrade a profile that had Camera residue (`noAudio: true`): Mirror and Flex have sound, and Settings > Phone mirroring shows the one-time audio notice; Dismiss removes it for good.
- [ ] Launch each mode (Mirror, Flex, High fidelity) from the Devices rack, the Devices deck, the Command Palette and Phone Hub > Mirror: each uses its own tab's values.
- [ ] Switching the mode in Devices changes no value on the Settings tabs; switching tabs in Settings changes nothing.
- [ ] Flex size, Hide Android taskbar and the DPI preview appear on the Flex tab only; the deck toggles edit the selected mode's values.
- [ ] Camera tab: microphone, bitrate and one-window toggles pre-fill the Camera launch dialog.
- [ ] Keyboard only: Tab to the mode tabs, Left/Right/Home/End select; Space toggles a switch; every control has a visible label.
- [ ] "File Transfer Drop Sync" is gone; a profile that had it off now has "Copy-paste files across PCs" off.
- [ ] Control window hides Rotate for a Flex session and shows it for Mirror.

## G. Sectioned Settings (SPEC-28 phase 2)

- [ ] Rail lists seven sections in order; Tab enters it once, Up/Down/Home/End move, Enter opens a section and focus lands on its heading.
- [ ] "Skip to settings" appears on focus after the filter box and jumps into the pane.
- [ ] Filter: "mute" and "sound" find the audio switches; Escape clears; a nonsense query shows "No settings match" with Clear; the match count is announced; Advanced expands while it has a match; no value changes.
- [ ] Shared mouse & keyboard: crossing delay and corner dead zone are read-only with "Edit in Computers", which opens the Computers tab. A stored value that is not a chip (for example 60 px) shows a "Custom: 60px" chip there.
- [ ] Each relabelled switch still changes behaviour (clipboard text/images, file copy and drop, pointer style, audio bridge, lock/sleep together, Explorer menu, cursor badge).
- [ ] Advanced: large-file limits and the diagnostics log toggle work; "Open log folder" opens the folder.
- [ ] Contrast spot check: helper text readable on every surface; selected rail item is dark text on gold.

## H. Mirror control bar (SPEC-28 phase 3)

- [ ] Mirror and Flex: sidebar is 60 px; 40 px icon buttons grouped Navigate / Sound / Screen / Capture with dividers; More at the bottom.
- [ ] Tab through the bar: each button shows a tooltip on keyboard focus; More opens with Enter, arrows move, Escape closes and focus returns to More.
- [ ] Volume down shows the volume-down icon; no Sleep button; Rotate hidden in Flex.
- [ ] Screen-off, Record, Wi-Fi, Bluetooth, Do Not Disturb, Mouse wheel swipes show `aria-pressed` and a gold outline when on; Record is red while recording.
- [ ] Stopping a recording shows a toast with "Show in folder".
- [ ] End call appears (red) only during a call.
- [ ] Camera mode: torch, zoom, stop camera group; help text names no third-party apps.
- [ ] Title-bar buttons are 36 x 28 px; Close turns red on hover.

## I. Default transfer folders (SPEC-31)

- [ ] Phone Hub Gallery & Files download with default settings → saves straight to `Downloads\AnyLeap` without prompting; **Show in folder** opens Explorer at the file.
- [ ] Toggle **Ask where to save each time** on → Gallery and Files prompt once per click; cancelling leaves no file.
- [ ] Command Palette **Take screenshot** → saves PNG to the configured receive folder and shows **Show in folder** toast.
- [ ] Set **Save files sent to phone in** to `/sdcard/Documents`, drop a non-APK onto the mirror window and paste an image (`Ctrl+V`) → lands in `/sdcard/Documents/` on phone.
- [ ] Set PC receive folder to an unavailable path (e.g. `Z:\Missing`), trigger a phone download and a multi-PC transfer → falls back to `Downloads\AnyLeap` with a notice.
- [ ] Drop a file onto the Computers canvas or paste a large (`> stagingLimitMb`) file across PCs → Transfer Bar and Activity Feed show **Show in folder**.

## J. Tidy Notification View (SPEC-33 Phase 1) — needs phone

- [ ] Notifications appear under **Today / Yesterday / Earlier** with collapsed whitespace, 2-line clamp + **Show more** / **Show less**, and same-title stacks (`+N more`).
- [ ] App filter chips, search (Escape clears), collapsible app headers, and deterministic colored letter avatars work smoothly.
- [ ] Mark an app as sensitive via `Mute…` → **Mark `<App>` as sensitive**: notification body and desktop toast preview mask immediately; clicking **Sensitive content hidden — click to reveal** reveals in memory and switching away re-masks automatically.
- [ ] Click **Hide in AnyLeap** on a notification → removed locally with `<N> hidden · Show` restore control while remaining untouched in Android's notification shade.

## K. Per-App Retention & DPAPI Encrypted Vault (SPEC-33 Phase 2) — needs phone

- [ ] **Settings → General → Phone notifications & Money**: confirm **Save notification history & transactions on this PC** (`N-01`) and **Capture notifications while Phone tab is closed** (`N-03`) default to **off**, and `%LOCALAPPDATA%\id.co.dak.anyleap\phone_hub_history.v1.dpapi` does not exist while off.
- [ ] Turn on `N-01`, configure one app to **Keep history (30 days)** and one app to **Transactions only** with a Money source (`Bank` / `Credit card`), trigger notifications, and verify `phone_hub_history.v1.dpapi` and `phone_hub_transactions.v1.dpapi` contain zero plaintext strings and only the `Keep history` app is hydrated after restarting AnyLeap.
- [ ] Disconnect the phone while `N-01` is on → open **Phone → Notifications** and verify `"Offline — showing saved history"` renders saved notifications for the known device.
- [ ] Downgrade the app from **Keep history** to **Don't keep** → confirm the prompt `"Also delete existing saved data for <App>?"` (checked by default) clears its saved history; test **Search apps** (filters live, `Esc` clears, `Enter` never adds), **Other apps (N)** collapse/expand, **"Add an app that isn't listed"** package validation, **Remove from list**, and **Delete all saved data** (`N-08`) confirmation modal removes both `.dpapi` files.
