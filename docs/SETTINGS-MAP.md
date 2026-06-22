# Settings Map — scrcpy flags → GUI controls

_Planning doc. Last updated: 2026-06-22._

Goal: expose scrcpy's options as **typed GUI controls**, never raw command input. Settings are **tiered** so v1 ships a small, friendly "core" set and the full set arrives later behind an "Advanced" view.

> ⚠️ **Flag names change across scrcpy versions** (e.g. `--bit-rate` → `--video-bit-rate` in v2.0; codec/input options expanded in v2.x–3.x). The exact flags below must be verified against the **pinned bundled scrcpy version** when we implement. Treat this as the intended *mapping*, not a frozen contract.

## Control types

`toggle` (bool flag) · `select` (enum) · `number` (with min/max/step) · `text` · `file` (path picker) · `size` (W×H).

---

## Tier 1 — Core (v1)

The everyday knobs, shown on the main mirror screen.

| Setting | scrcpy flag | Control | Notes |
|---------|-------------|---------|-------|
| Max resolution | `--max-size` (`-m`) | number | 0 = no limit; e.g. 1024, 1920. |
| Video bitrate | `--video-bit-rate` (`-b`) | number | e.g. 8M. |
| Max FPS | `--max-fps` | number | e.g. 60. |
| Keep awake | `--stay-awake` (`-w`) | toggle | While plugged/mirroring. |
| Turn screen off | `--turn-screen-off` (`-S`) | toggle | Blank phone screen while mirroring. |
| Fullscreen | `--fullscreen` (`-f`) | toggle | |
| Always on top | `--always-on-top` | toggle | |
| Show touches | `--show-touches` (`-t`) | toggle | |
| No audio | `--no-audio` | toggle | Audio on by default (Android 11+). |
| Read-only (view, no control) | `--no-control` (`-n`) | toggle | |

## Tier 2 — Video

| Setting | scrcpy flag | Control |
|---------|-------------|---------|
| Video codec | `--video-codec` | select: h264 / h265 / av1 |
| Crop | `--crop` | text (W:H:x:y) |
| Display to mirror | `--display-id` | select (enumerate displays) |
| Rotation / angle | `--orientation` / `--angle` | select / number |
| Disable video (audio-only) | `--no-video` | toggle |
| Virtual display | `--new-display` | text/size |

## Tier 2 — Audio

| Setting | scrcpy flag | Control |
|---------|-------------|---------|
| Audio codec | `--audio-codec` | select: opus / aac / flac / raw |
| Audio bitrate | `--audio-bit-rate` | number |
| Audio source | `--audio-source` | select: output / mic / playback |
| Audio buffer | `--audio-buffer` | number (ms) |

## Tier 2 — Window

| Setting | scrcpy flag | Control |
|---------|-------------|---------|
| Window title | `--window-title` | text |
| Position / size | `--window-x/y/width/height` | number ×4 |
| Borderless | `--window-borderless` | toggle |
| Disable screensaver | `--disable-screensaver` | toggle |

## Tier 2 — Control & input

| Setting | scrcpy flag | Control |
|---------|-------------|---------|
| Keyboard mode | `--keyboard` | select: sdk / uhid / aoa |
| Mouse mode | `--mouse` | select: sdk / uhid / aoa |
| Gamepad | `--gamepad` | select |
| OTG mode | `--otg` | toggle |
| Power off on close | `--power-off-on-close` | toggle |
| Don't power on at start | `--no-power-on` | toggle |
| Clipboard autosync | `--no-clipboard-autosync` | toggle (inverted) |

## Tier 2 — Recording

| Setting | scrcpy flag | Control |
|---------|-------------|---------|
| Record to file | `--record` (`-r`) | file picker |
| Record format | `--record-format` | select: mp4 / mkv |
| No playback (record only) | `--no-playback` | toggle |
| Time limit | `--time-limit` | number (s) |

## Tier 3 — Connection (mostly handled by the pairing UI)

| Setting | scrcpy flag | Control |
|---------|-------------|---------|
| Target serial | `--serial` (`-s`) | (auto from selected device) |
| TCP/IP mode | `--tcpip` | (handled by wireless flows) |
| Port range | `--port` | number |

## Implementation note: schema-driven form

Rather than hand-code each widget, define settings as a **schema** (id, flag, type, default, min/max/options, tier) and render the form from it. Adding a new scrcpy flag = one schema entry. This keeps us aligned with scrcpy releases and makes the core/advanced tiering trivial (just a field on each entry).

## To seed the "core" tier well

Open question for the author: **which scrcpy settings did you actually use day-to-day in the old Python wrapper?** Those should be promoted into Tier 1 regardless of the defaults above.
