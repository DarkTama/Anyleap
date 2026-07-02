# Settings Map — scrcpy flags → GUI controls

_Planning doc. Last updated: 2026-06-22._

Goal: expose scrcpy's options as **typed GUI controls**, never raw command input. Settings are **tiered** so v1 ships a small, friendly "core" set and the full set arrives later behind an "Advanced" view.

> ⚠️ **Flag names change across scrcpy versions** (e.g. `--bit-rate` → `--video-bit-rate` in v2.0; codec/input options expanded in v2.x–3.x). The exact flags below must be verified against the **pinned bundled scrcpy version** when we implement. Treat this as the intended *mapping*, not a frozen contract.

## Quality presets (game-style)

For the wide range of users who don't want to think about individual flags, a single **Quality** selector picks a bundle of the performance-vs-fidelity settings — exactly like Low / Medium / High / Highest in games. Picking a preset fills in the underlying settings; touching any individual setting flips the selector to **Custom**.

| Preset | Max resolution (`--max-size`) | Bitrate (`--video-bit-rate`) | Max FPS (`--max-fps`) | Codec (`--video-codec`) | Best for |
|--------|------|---------|------|-------|----------|
| **Low** — _Smooth_ | 800 | 2 Mbps | 30 | h264 | Weak/busy Wi-Fi, lowest latency, older phones |
| **Medium** — _Balanced_ ★ default | 1280 | 8 Mbps | 60 | h264 | Most users on decent Wi-Fi |
| **High** | 1600 | 16 Mbps | 60 | h265 | Strong 5 GHz Wi-Fi or USB; sharper text |
| **Highest** — _Crisp_ | 0 (native) | 30 Mbps | 60 | h265 | USB or excellent Wi-Fi; max fidelity |
| **Custom** | — | — | — | — | Unlocks all manual controls |

Notes:
- Lower presets favor **latency and bandwidth** (best for wireless); higher presets favor **fidelity** (best for USB / strong Wi-Fi). For this app, the quality slider is effectively a "how good is your connection" slider.
- h265/av1 give better quality-per-bit but depend on the **phone's encoder** — fall back to h264 automatically if unsupported. AV1 (Android 14+, capable hardware) stays opt-in, not in any default preset.
- Numbers above are starting points — tune after real-world testing.
- Future: detect stutter / wireless-vs-USB and **suggest** stepping a preset down/up.

## Per-setting help text

Every setting carries plain-language help so users aren't guessing — surfaced as an ⓘ tooltip/popover next to each control: a one-line **what it does** plus a concrete **example/guidance**. Examples:

| Setting | Help (what it does) | Example / guidance |
|---------|---------------------|--------------------|
| Max resolution | Caps the long side of the mirrored video. | Lower = smoother & less lag. `1280` is smooth on Wi-Fi; `0` = phone's full resolution. |
| Video bitrate | How much data the video stream uses. | Higher = sharper but needs better Wi-Fi. `8 Mbps` is plenty for 1080p. |
| Max FPS | Frame-rate cap. | `60` is smooth; `30` saves bandwidth & battery. |
| Turn screen off | Blanks the phone's own screen while mirroring. | Saves battery; touch/control still works. |
| Keep awake | Stops the phone sleeping while connected. | Useful over USB during long sessions. |
| Codec | Video compression format. | `h264` = most compatible; `h265` = sharper at same bitrate if the phone supports it. |

Each quality preset also gets its own one-line "best for…" caption (the right-hand column in the preset table), shown under the selector.

---

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
| Disable virtual keyboard | `--keyboard=uhid` | toggle | Simulates a physical HID keyboard; Android hides on-screen keyboard. |

## Tier 2 — Video

| Setting | scrcpy flag | Control |
|---------|-------------|---------|
| Video codec | `--video-codec` | select: h264 / h265 / av1 |
| Crop | `--crop` | text (W:H:x:y) |
| Display to mirror | `--display-id` | select (enumerate displays) |
| Rotation / angle | `--orientation` / `--angle` | select / number |
| Disable video (audio-only) | `--no-video` | toggle |
| Virtual display | `--new-display` | text/size |
| Flex display (desktop mode) | `--new-display[=WxH/dpi] --flex-display` | toggle + text (size) | Android 10+; virtual display continuously resizes to match the window. Implemented. |
| Unlock aspect ratio | `--no-window-aspect-ratio-lock` | toggle | Allow any window shape. Implemented. |
| Render fit | `--render-fit` | select: letterbox / unscaled / stretched | Always passed explicitly (scrcpy defaults to unscaled under --flex-display). Implemented. |

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
| Rotate device | — (control bar button, adb) | Auto-rotate off + toggles `user_rotation`; physical display only. Implemented. |
| Swipe scroll | — (control bar toggle, adb `input swipe` via Win32 wheel hook) | Converts mouse wheel over the mirror into touch flings, for Reels/Shorts-style pagers that ignore mouse scroll. Display-targeted (`input -d`) in flex mode. Implemented. |

Flex (virtual display) sessions: the scrcpy display id is parsed from its
"New display" log line (fallback: `dumpsys display`), and Back/Home/Recents
key events are injected with `input -d <id>` so they act on the mirrored
display. Volume/power/screenshot/notifications stay global. The control strip
overlays *inside* the mirror's client edge (Snap-friendly) and collapses to a
floating round button.

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

## Finding settings (search & quick-jump)

With the full flag set exposed, a **search bar** at the top of the Settings view keeps it usable:

- Matches against each setting's **label, help text, and scrcpy flag** (incl. short aliases like `-b`, `-m`) — so both newcomers ("frame rate") and power users (`--max-fps`) find it.
- Typing **filters / highlights** matching settings live; selecting a result **scrolls to and highlights** that control (expanding its tier/section if collapsed).
- Optional **command-palette quick-jump** (e.g. Ctrl/Cmd+K) for keyboard users.
- Graceful empty state when nothing matches.

Like the tiering, help tooltips, and presets, this **falls out of the same schema** — the search index is just each entry's `label` + `help` + `flag`/aliases.

## Implementation note: schema-driven form

Rather than hand-code each widget, define settings as a **schema** (id, flag, `aliases`, type, default, min/max/options, tier, `help`, `example`, and per-preset values) and render the form from it. Adding a new scrcpy flag = one schema entry. This keeps us aligned with scrcpy releases and makes everything else trivial: core/advanced tiering, the ⓘ help tooltips, applying a quality preset (read each entry's value for the chosen preset), and **settings search** (index each entry's label/help/flag/aliases) all fall out of the same schema.

## To seed the "core" tier well

Open question for the author: **which scrcpy settings did you actually use day-to-day in the old Python wrapper?** Those should be promoted into Tier 1 regardless of the defaults above.
