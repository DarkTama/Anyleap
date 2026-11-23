# SPEC-28: Settings Redesign and Per-Mode Mirroring Defaults

> **Status:** Approved (2026-11-17). Phases 1–3 implemented (per-mode defaults, migration, Phone mirroring section; sectioned Settings with rail, filter, relabels, Advanced; grouped accessible mirror control bar).
> **Scope:** `(deck)` Settings tab (`src/components/SettingsPanel.tsx`), persisted mirroring settings (`src/lib/settingsStore.ts`, `src/store/useAppStore.ts`), and every launch path that reads them (Devices tab, command palette, control window).
> **Related:** ADR-0005 (per-mode mirroring settings), SPEC-26 (Ink & Gold tokens), SPEC-25 (Continuity), SPEC-24 (Edge Leap).
> **Plan:** `docs/superpowers/plans/2026-11-17-spec28-settings-redesign.md`
> **Input:** the approved settings audit (findings, 7-section structure, label copy rules, "Phone mirroring" mockup with per-mode tabs).

## 1. Problem

The Settings tab is four stacked cards (`Settings`, `Control bar`, `Shared Mouse & Keyboard`, `Continuity`) holding about 50 controls in one scroll, with no navigation or search. It also misleads:

1. **Two preset systems rewrite one settings object.** Settings has a "Quality preset" (`PRESET_BUNDLES`, `src/lib/types.ts:84-89`; dropdown `src/components/SettingsPanel.tsx:92-105`). Devices has four Mirroring presets (`LaunchMatrix.handleSelect`, `src/components/deck/LaunchMatrix.tsx:16-53`, duplicated in the palette `startMode`, `src/App.tsx:697-716`). Picking a mode **overwrites the single global `settings`**: Camera sets `noAudio: true`, High Fidelity forces `av1` / 20 Mbps / 60 fps, Flex flips `flexDisplay`, every mode sets `embedded: true`. The values leak into the next launch of another mode and into the Settings page. This is the cause of the "no sound in Mirror/Flex" bug: choose Camera once and `noAudio: true` stays in the store used by the next Mirror launch.
2. **Duplicate controls for one thing.** "Flex display (desktop mode)" (`SettingsPanel.tsx:183-190`) duplicates the Desktop Flex preset.
3. **Negative labels** ("No audio", "View only", "Disable virtual keyboard"): ON means "something is off".
4. **Split and duplicated features** (§9): dwell/deadzone have two editors; file sync has three gates.
5. **Jargon and mixed casing** ("Floating Desktop HUD Pill", "Canvas Traveling Cursor Beacon", "Staging size limit").
6. **A third-party product name** in the helper text of the embedded-mirror toggle (`SettingsPanel.tsx:196-198`), which breaks `AGENTS.md` §1.3.
7. **Magic numbers** (`0 = native`, free-form bitrate field).

## 2. Goals

- G1. Each mirroring mode owns its settings. Switching modes never mutates another mode. A Devices launch uses the selected mode's settings.
- G2. Seven-section left rail with a filter box. Every existing control has exactly one home (§8).
- G3. Positive, sentence-case, jargon-free labels with one helper line where the effect is not obvious.
- G4. A single source for every duplicated setting (§9).
- G5. No data loss on upgrade. Audio is never silently forced off (§5.4).
- G6. Two independently shippable phases (§12).

## 3. Non-goals

- No change to the Rust IPC contract: `start_mirror` still receives a `CoreSettings`, `start_camera_mirror` a `CameraSettings` (§6).
- No new mirroring options or modes. No change to the Phone tab, control bar button behaviour, KVM wire protocol or Computers tab layout.
- No per-phone settings (modes are per app install). The persisted shape (§5.2) does not block adding it later.
- No re-theme: SPEC-26 tokens only.

## 4. Current model (research findings)

| Topic | Fact | Source |
|---|---|---|
| Type | `CoreSettings` (17 fields: `maxSize`, `videoBitRate`, `maxFps`, `videoCodec`, `stayAwake`, `turnScreenOff`, `fullscreen`, `showTouches`, `noAudio`, `noControl`, `noKeyboardIme`, `flexDisplay`, `flexDisplaySize`, `noWindowAspectRatioLock`, `renderFit`, `embedded?`, `hideVirtualTaskbar?`) | `src/lib/types.ts:56-74` |
| Presets | `QualityPreset = low \| medium \| high \| highest \| custom`; bundles touch only `maxSize`, `videoBitRate`, `maxFps`, `videoCodec`; default is `medium` | `types.ts:76-114` |
| Store | One `settings: CoreSettings`, one `preset`, plus `currentMode: StudioMode` (default `"flex"`, not persisted) | `src/store/useAppStore.ts:22-30, 57-58, 69-77, 142-143` |
| Persistence | `settings.json` via the store plugin (`STORE_PATH`), keys `settings` and `preset`, loaded as `{...DEFAULT_SETTINGS, ...settings}` and `null` if either key is missing. One-time import from legacy `config.json`. App prefs under `appPrefs`, control bar under `controlBar` | `src/lib/settingsStore.ts:25, 35-80, 99-112` |
| KVM settings | **Not** in `settings.json`: `localStorage["anyleap_kvm_settings"]`, pushed to Rust on change | `src/stores/kvmStore.ts:244-253, 1021-1093` |
| Load | App start: `loadQuality()` then `setSettings` / `setPreset` | `src/App.tsx:360-368` |
| Writers | Settings page (`update()` forces `preset = "custom"`), `LaunchMatrix`, palette `startMode`, inline toggles in the Devices deck (`StudioDeck.tsx:127-146, 433-546`: Flex size, hide taskbar, turn screen off, show touches) | `SettingsPanel.tsx:44-59`; `LaunchMatrix.tsx:16-53`; `App.tsx:700-716`; `StudioDeck.tsx` |
| Readers at launch | Devices rack and deck call `startMirror(serial, settings)` with the global object | `DeviceRack.tsx:55, 222, 231`; `StudioDeck.tsx:55, 201, 209` |
| Other readers | Existing-session window choice reads `settings.embedded` (`App.tsx:436`); the floating control window reads `settings.flexDisplay` from storage (`ControlWindow.tsx:34-36`); the camera dialog reads `settings.embedded` and keeps its own mic/bitrate/size state, not persisted (`CameraLaunchDialog.tsx:77-96`) | as cited |

How the Mirroring presets rewrite settings today (`LaunchMatrix.tsx:21-49`):

| Mode | Writes into the global settings |
|---|---|
| `flex` | `flexDisplay: true`, `embedded: true`, `noWindowAspectRatioLock: true`, `renderFit: "unscaled"` |
| `mirror` | `flexDisplay: false`, `embedded: true`, `noWindowAspectRatioLock: false`, `renderFit: "letterbox"` |
| `camera` | `flexDisplay: false`, `embedded: true`, **`noAudio: true`**, `stayAwake: true` |
| `hifi` | keeps `flexDisplay`, `embedded: true`, `maxFps: 60`, `videoBitRate: 20_000_000`, `videoCodec: "av1"` |

The palette copy at `App.tsx:703-709` is the same data written a second time, and its `hifi` branch differs slightly (it does not touch `flexDisplay` explicitly). Both copies disappear in this spec.

## 5. Per-mode defaults model

### 5.1 Rules
1. Modes are `mirror`, `flex`, `camera`, `hifi` (the existing `StudioMode`, `LaunchMatrix.tsx:5`).
2. Each mode owns a complete settings object (**Mode defaults**). Only the Settings page's Phone mirroring section and the Devices deck's quick toggles (§8, rows M-21..M-24) write it, and they write only the **selected** mode's object.
3. Selecting a mode in Devices only sets `currentMode`. It writes no settings.
4. Launch settings are **resolved**, never stored: `resolveLaunchSettings(mode, defaults)` returns the mode's object with the one mode-fixed field forced: `flexDisplay = (mode === "flex")`. No other field is forced.
5. `flexDisplay` is no longer user-editable (§9.4).

### 5.2 Persisted shape
New key in `settings.json` (`src/lib/settingsStore.ts`):

```ts
type MirrorMode = "mirror" | "flex" | "camera" | "hifi"; // = StudioMode

// Same fields as CoreSettings minus flexDisplay (derived from the mode).
type ModeSettings = Omit<CoreSettings, "flexDisplay">;

interface CameraModeSettings {
  playAudio: boolean;                     // CameraSettings.noAudio = !playAudio
  bitrate: "low" | "balanced" | "high";   // 2 / 8 / 16 Mbps, as the launch dialog maps them
  embedded: boolean;
}

interface ModeDefaultsV2 {
  version: 2;
  mirror: ModeSettings;
  flex: ModeSettings;   // flexDisplaySize and hideVirtualTaskbar are meaningful only here
  hifi: ModeSettings;
  camera: CameraModeSettings;
}
// settings.json: { "modeDefaults": ModeDefaultsV2, "modeDefaultsNotice": boolean, ... }
```

Rationale:
- `ModeSettings` reuses `CoreSettings` field names, so the IPC type, `build_scrcpy_args` and its tests are untouched (§6). Positive phrasing is a UI concern: the UI shows "Play phone audio on this PC" and writes `noAudio = !checked`.
- Camera has its own small object because camera launch uses a different backend struct (`CameraSettings`, `src-tauri/src/commands.rs:36-70`) with different options. The launch dialog is pre-filled from it; per-launch dialog choices are not persisted back.
- `flexDisplaySize` and `hideVirtualTaskbar` stay in `ModeSettings` to keep one resolver. They are shown on the Flex tab only and ignored by the launcher when `flexDisplay` is false (`commands.rs:354-366`).
- The legacy keys `settings` and `preset` are **kept, frozen** (not deleted) so a downgrade still loads (§5.4).

### 5.3 The Settings "Quality preset": folded into per-mode quality
**Decision: remove the global "Quality preset" dropdown; keep the four bundles as a per-mode "Quality" choice.**

- A global preset is exactly the cross-mode coupling this spec removes.
- The bundles (Low / Medium / High / Highest) remain the easiest way to say "how good is my connection". Each mode's Quality group opens with a segmented control **Smooth · Balanced · Sharp · Maximum** (= the Low, Medium, High, Highest bundles, `types.ts:84-89`). It writes the four quality fields into that mode only.
- The preset is **derived, not stored**: if the four fields equal a bundle, that segment is selected; otherwise the control shows "Custom" and the fine-tune fields (M-02..M-05) are expanded. `QualityPreset` and `preset` leave the live model; `PRESET_BUNDLES` stays as data; `PRESET_LABELS` / `PRESET_HINTS` are replaced by the new copy.
- Fresh-install defaults: `mirror` and `flex` = Balanced bundle; `hifi` = `{maxSize: 0, videoBitRate: 20_000_000, maxFps: 60, videoCodec: "h265"}`; `camera` = `{playAudio: true, bitrate: "balanced", embedded: true}`. The old High Fidelity default of AV1 is dropped because AV1 depends on the phone's encoder and stays opt-in (`docs/SETTINGS-MAP.md`); users can still choose it. All modes default `embedded: true` as the old presets did.

### 5.4 Migration (legacy `settings` + `preset` to `modeDefaults` v2)
Runs once at load in `loadModeDefaults()` (new, `src/lib/settingsStore.ts`) when `modeDefaults` is absent.

Let `S = {...DEFAULT_SETTINGS, ...legacy settings}` and `P = legacy preset` (`medium` if absent). If no legacy data exists, write the fresh-install defaults of §5.3.

1. **Base copy.** Every mode starts as a copy of `S` minus `flexDisplay`, so nothing the user chose is lost: quality, `stayAwake`, `turnScreenOff`, `fullscreen`, `showTouches`, `noControl`, `noKeyboardIme`, `embedded`, `hideVirtualTaskbar`, `flexDisplaySize`.
2. **Undo preset residue on quality.** If `S` has the High Fidelity signature (`videoCodec === "av1" && videoBitRate === 20_000_000 && maxFps === 60`), it was written by the old `hifi` preset: `hifi` keeps `S`'s values and `mirror` / `flex` quality = `PRESET_BUNDLES[P]` (`medium` if `P === "custom"`). Otherwise `mirror` and `flex` keep `S`'s quality, and `hifi` = `S` with `videoBitRate = max(S.videoBitRate, 20_000_000)` and `maxFps = max(S.maxFps, 60)`, codec kept.
3. **Undo preset residue on window fit.** The old presets rewrote `noWindowAspectRatioLock` and `renderFit` per mode, so the stored pair belongs to whichever mode was used last. If `S.flexDisplay` is true, `flex` keeps `S`'s pair and `mirror` / `hifi` get the old constants (`false` / `"letterbox"`). If false, `mirror` / `hifi` keep `S`'s pair and `flex` gets `true` / `"unscaled"`. An explicit `renderFit: "stretched"` is a deliberate choice (the presets never write it) and is kept in every mode.
4. **Audio is never silently forced off.** No migrated mode gets `noAudio = true`. `camera.playAudio = true` (the launch dialog's default). For `mirror` / `flex` / `hifi`, `noAudio = false` **even if `S.noAudio` is true**, because `true` cannot be told apart from the Camera preset's residue (§1 item 1). This turns audio on, never off, and it is not silent: if `S.noAudio` was true, the Phone mirroring section shows a one-time dismissible notice "Audio is now on in Mirror, Flex and High fidelity. Turn it off per mode here." (flag `modeDefaultsNotice`).
5. **Camera.** `camera.bitrate` = the nearest of low / balanced / high (2 / 8 / 16 Mbps) to `S.videoBitRate`; `camera.embedded = S.embedded ?? false`.
6. **File sync gate.** If the saved KVM settings have `syncFileTransfer === false` and `continuityFileClipboard === true`, set `continuityFileClipboard = false` (file copy-paste was effectively off; §9.3), through the existing `kvmStore.updateSettings` path.
7. **Write order and idempotence.** Write `modeDefaults`, then `modeDefaultsNotice`, then `store.save()`. Never delete `settings` / `preset`. If `modeDefaults` exists the migration does nothing. A half-written store (no `modeDefaults`) re-runs safely because the legacy keys are unchanged.
8. **Downgrade.** An older build ignores the new keys and reads the frozen `settings` / `preset`. Edits made in the newer build do not flow back. Documented limit.

## 6. Rust consumers the model must keep working

No Rust change in either phase. The frontend keeps sending a full `CoreSettings`.

| Consumer | What it reads | Source | Requirement |
|---|---|---|---|
| `start_mirror` → `build_scrcpy_args(serial, &CoreSettings)` | `max_size`, `video_bit_rate`, `max_fps`, `video_codec`, `stay_awake`, `turn_screen_off`, `fullscreen`, `show_touches`, `no_audio`, `no_control`, `no_keyboard_ime`, `flex_display` (+ `flex_display_size`, `hide_virtual_taskbar` only when `flex_display`), `embedded` (borderless, forces aspect-unlock), `no_window_aspect_ratio_lock`, `render_fit` | `src-tauri/src/commands.rs:83-106, 321-380, 1076-1086` | Resolver output includes all 17 fields. `renderFit` stays always-sent (comment at `commands.rs:371-373`: an omitted `letterbox` flips to `unscaled` under flex). `flexDisplay` comes from the mode only |
| `start_camera_mirror` → `build_scrcpy_camera_args` | `CameraSettings`: `facing`, `camera_id`, `size`, `fps`, `high_speed`, `torch`, `no_audio`, `embedded`, `orientation`, `video_bit_rate`, `video_buffer` | `commands.rs:36-70, 382-440, 1108-1115` | Camera defaults map to `noAudio`, `embedded`, `videoBitRate`; the other fields stay per-launch in the dialog |
| App windows | `launch_app_window` uses `default_app_window_settings()` (constants: 1280 px, 8 Mbps, 60 fps, h264, stay-awake) through `build_app_window_args` | `commands.rs:3572-3640, 4070` | **Independent of user settings today and stays so.** Not part of any mode; listed so it is not rewired by accident |
| Rust tests | `base_settings()` builds a `CoreSettings` | `commands.rs:2369` | Unchanged, the struct is unchanged |
| Control window | Reads `settings.flexDisplay` from `loadQuality()` | `src/components/ControlWindow.tsx:34-36` | Re-point to the mode of the running session (plan task 1.6); the legacy `settings` would be stale after migration |
| Existing-session window choice | `useAppStore.getState().settings.embedded` | `src/App.tsx:436` | Re-point to the resolved settings of the mode that launched the session |

## 7. Information architecture

Left rail (sticky, `nav` landmark "Settings sections") with a filter box above it:

| # | Section | Holds |
|---|---|---|
| 1 | General | Tray, updates, notifications, battery alerts |
| 2 | Phone mirroring | Tabs Mirror / Flex / Camera / High fidelity; per mode: Quality, Audio & control, Phone while mirroring, Window options; Flex tab only: Flex display |
| 3 | Control bar | Dock side, size, buttons (behaviour unchanged) |
| 4 | Shared mouse & keyboard | Crossing behaviour (read-only mirror of the Computers tab), cursor indicators |
| 5 | Clipboard & files | Text, images, files, drag & drop (Experimental), pointer scheme |
| 6 | Audio, media & power | Audio bridge, bridged volume, media keys, lock / sleep together, Explorer menu |
| 7 | Advanced | Large-file limits, diagnostics log, open log folder |

**Filter box.** Case-insensitive substring match over label, helper text and a per-control keyword list (for example "sound" and "mute" find the audio switch). While a query is active the rail lists only sections with matches plus a count, the pane shows matching rows under their section heading, and Advanced rows auto-expand. No match: "No settings match “<query>”" and a Clear button. Clearing restores the last section. Filtering never changes values.

## 8. Control mapping (every existing control)

Legend: **Move** = same control, new home. **Rename** = copy change. **Remove** / **Merge** state why. "Per mode" = stored in the selected mode's object (§5.2). Line numbers are in `src/components/SettingsPanel.tsx` unless noted.

### 8.1 Today's "Settings" card

| ID | Today (line) | Section | New label | Helper text | Control | Notes |
|---|---|---|---|---|---|---|
| M-01 | Quality preset (92) | Phone mirroring, per mode | Quality | "How good is your connection? Smooth saves bandwidth; Maximum needs USB or strong Wi-Fi." | Segmented: Smooth / Balanced / Sharp / Maximum (+ derived Custom) | **Merge** into per-mode quality (§5.3). Global dropdown removed |
| M-02 | Max resolution (109) | Phone mirroring, per mode | Resolution | "Long side of the video. Native is sharpest." | Segmented: Native / 1080p / 720p (other stored values shown as "Custom: N px") | **Rename**; removes the `0 = native` magic number. Maps `maxSize` 0 / 1920 / 1280 |
| M-03 | Bitrate (Mbps) (115) | Phone mirroring, per mode | Bitrate | "Higher is sharper but needs a strong connection." | Segmented Low / Balanced / High showing the Mbps ("High · 30 Mbps"); Stepper in Custom fine-tune | **Rename.** Low ≤ 4, Balanced ≤ 12, High above |
| M-04 | Max FPS (121) | Phone mirroring, per mode | Frame rate | "60 is smoother; 30 saves bandwidth." | Segmented 30 / 60 fps | **Rename**; unusual values shown as Custom |
| M-05 | Codec (127) | Phone mirroring, per mode | Codec | "H.265 saves bandwidth. Needs a phone that supports it." | Select: H.264 / H.265 / AV1 | **Rename** (display names, not `h264`) |
| M-06 | Keep awake (144) | Phone mirroring, per mode | Keep phone awake | "Stops the phone sleeping while mirroring." | Switch | **Rename** |
| M-07 | Turn screen off (149) | Phone mirroring, per mode | Turn phone screen off | "Saves battery. Mirroring keeps working." | Switch | **Rename.** Also edited from the Devices deck (M-22) |
| M-08 | Fullscreen (154) | Phone mirroring, per mode, Window options | Start in fullscreen | none | Switch | **Rename** |
| M-09 | Show touches (159) | Phone mirroring, per mode | Show touches on the phone | "Draws a dot where a finger or click lands." | Switch | **Rename.** Also edited from the deck (M-23) |
| M-10 | No audio (164) | Phone mirroring, per mode | Play phone audio on this PC | "Off: sound stays on the phone." | Switch (`noAudio = !on`) | **Rename**, positive. Camera tab: "Use the phone microphone on this PC" |
| M-11 | View only (169) | Phone mirroring, per mode | Allow control from this PC | "Off: watch only." | Switch (`noControl = !on`) | **Rename**, positive |
| M-12 | Disable virtual keyboard (174) | Phone mirroring, per mode | Use Android on-screen keyboard | "Off: type with this PC's keyboard directly." | Switch (`noKeyboardIme = !on`) | **Rename.** In Mirror and High fidelity, Off sends `--keyboard=uhid`. On the Flex tab the switch is disabled (unchecked) with helper "Not used in Flex. PC typing works, and the phone's keyboard may appear on the phone while you type in Flex." because `--new-display` sessions always use the SDK keyboard (no `--keyboard=uhid`, no `--display-ime-policy`) so the phone's own keyboard keeps working |
| M-13 | Flex display (desktop mode) (184) | none | none | none | none | **Remove.** Duplicates the Desktop Flex mode (§9.4); `flexDisplay` is derived |
| M-14 | Embedded mirror (integrated sidebar) (194) | Phone mirroring, per mode, Window options | Show phone in one AnyLeap window | "Keeps the phone view and its toolbar together." | Switch | **Rename + remove third-party name** from the helper text (the old text names an external product; not repeated here) |
| M-15 | Flex display size text (217) | Phone mirroring, Flex tab | Virtual display size | "Match this PC's monitor for a desktop-like layout. Format 1920x1080/240 (pixels / dpi). Empty means automatic." | Text | **Move**, Flex only |
| M-16 | Auto PC Res button (224-233) | Phone mirroring, Flex tab | Use this PC's resolution | "Detects this PC's main monitor." | Button | **Rename** |
| M-17 | DPI icon-size preview (237-255) | Phone mirroring, Flex tab | (preview) | Existing descriptive line per DPI band | Read-only | **Move** unchanged; graphic `aria-hidden`, text carries the meaning |
| M-18 | Size preset chips (258-282) | Phone mirroring, Flex tab | Size presets | none | Segmented (4 chips) | **Move.** Chip names in sentence case, e.g. "1080p compact (160 dpi)" |
| M-19 | Hide virtual taskbar (288) | Phone mirroring, Flex tab | Hide Android taskbar | "Hides the navigation bar on the virtual display." | Switch | **Rename.** Also edited from the deck (M-21) |
| M-20 | Unlock aspect ratio (299) | Phone mirroring, per mode, Window options | Keep phone aspect ratio | "Off: the window can be resized to any shape." | Switch (`noWindowAspectRatioLock = !on`) | **Rename**, positive. Disabled with a reason when M-14 is on (the launcher already forces unlocked, `commands.rs:368-370`) |
| M-20b | Render fit (304) | Phone mirroring, per mode, Window options | Picture fit | "Letterbox keeps proportions. Stretched fills the window." | Select: Letterbox / Unscaled / Stretched | **Rename** |
| M-21 | Deck toggle: hide taskbar (`StudioDeck.tsx:433`) | Devices deck (stays) | unchanged | unchanged | Switch | **Single source kept:** same key as M-19, writes `modeDefaults[currentMode]` |
| M-22 | Deck toggle: turn screen off (`StudioDeck.tsx:465`, duplicated at `:536`) | Devices deck (stays) | unchanged | unchanged | Switch | Same key as M-07. The two deck rows collapse to one (task 1.5) |
| M-23 | Deck toggle: show touches (`StudioDeck.tsx:504`) | Devices deck (stays) | unchanged | unchanged | Switch | Same key as M-09 |
| M-24 | Deck Flex size editor (`StudioDeck.tsx:127-146`) | Devices deck (stays) | unchanged | unchanged | existing | Same key as M-15; writes the Flex object |
| M-25 | Minimize to tray on close (320) | General | Minimize to tray when closing the window | none | Switch | **Rename** |
| M-26 | Check for updates on launch (325) | General | Check for updates on launch | none | Switch | Move |
| M-27 | Desktop pop-ups for phone notifications (330) | General | Show phone notifications on this PC | none | Switch | **Rename** |
| M-28 | Battery alerts (low ≤20% & full 100%) (335) | General | Battery alerts | "Alerts at 20% and when full." | Switch | **Rename**; thresholds move to the helper |
| M-29 | Application updates (342-377) | General | Updates | "Current version vX" / "vY available" | Button: Check now / View update | Move, sentence case |

### 8.2 "Control bar" card

| ID | Today (line) | Section | New label | Helper | Control | Notes |
|---|---|---|---|---|---|---|
| C-01 | Dock position (388) | Control bar | Dock side | none | Select | Move, rename |
| C-02 | Size (404) | Control bar | Button size | none | Select | Move, rename |
| C-03 | Buttons (422) | Control bar | Buttons | "Choose which buttons appear." | Existing list of switches | Move, behaviour unchanged |

### 8.3 "Shared Mouse & Keyboard" card

| ID | Today (line) | Section | New label | Helper | Control | Notes |
|---|---|---|---|---|---|---|
| K-01 | Edge Push Resistance (Dwell Delay) (501) | Shared mouse & keyboard | Crossing delay | "How long the cursor must push at the edge before it crosses. Edit it in Computers." | Read-only value + Link "Edit in Computers" | **Merge** into the Computers editor (§9.2); slider removed |
| K-02 | Corner Deadzone Margin (527) | Shared mouse & keyboard | Corner dead zone | "Top and bottom corner zones where crossing is blocked (tabs, title bars, taskbars). Edit it in Computers." | Read-only value + Link | **Merge** (§9.2) |
| K-03 | Floating Desktop HUD Pill (556) | Shared mouse & keyboard | Show which PC has the cursor | "A small floating badge." | Switch | **Rename** (internal name removed) |
| K-04 | HUD corner select (575) | Shared mouse & keyboard | Badge position | none | Select: Top right / Top left / Bottom right / Bottom left | **Rename**; shown only when K-03 is on |
| K-05 | Canvas Traveling Cursor Beacon (563) | Shared mouse & keyboard | Show cursor position in the layout map | "Marks the live cursor on the Computers layout." | Switch | **Rename** |
| K-06 | Sync Text Clipboard (624) | Clipboard & files | Share copied text | "Text copied on one PC can be pasted on the other." | Switch | **Rename**, grouped with files |
| K-07 | Sync Image Clipboard (Screenshots & Bitmaps) (631) | Clipboard & files | Share copied images | "Includes screenshots." | Switch | **Rename** |
| K-08 | File Transfer Drop Sync (638) | none | none | none | none | **Remove.** Third file gate that duplicates "Copy-paste files across PCs", mislabelled (§9.3) |
| K-09 | KVM diagnostics log (654) | Advanced | Record a diagnostics log | "Records input decisions and slow events for support. Rotates at 5 MB." | Switch | **Move + rename** (internal term removed) |
| K-10 | Open log folder (668) | Advanced | Open log folder | none | Button | Move |

### 8.4 "Continuity" card

| ID | Today (line) | Section | New label | Helper | Control | Notes |
|---|---|---|---|---|---|---|
| T-01 | Drag & drop files across PCs (705) | Clipboard & files | Drag and drop files between PCs (Experimental) | "Drag files across the edge. Some apps reject drops; copy and paste is more reliable." | Switch | **Rename**; stays Experimental (ADR-0004) |
| T-02 | Copy-paste files across PCs (714) | Clipboard & files | Copy and paste files between PCs | "Large files wait for Fetch (see Advanced)." | Switch | **Rename.** Becomes the only file switch (§9.3) |
| T-03 | Sync pointer scheme to Linked PC (721) | Clipboard & files | Match pointer style on the Linked PC | none | Switch | **Rename** |
| T-04 | Staging size limit (MB) (730) | Advanced | Large files: save to Downloads above | "Files above this size (MB) are saved to Downloads\AnyLeap_Transfers." | Stepper (MB) | **Rename** |
| T-05 | Clipboard prefetch limit (MB) (740) | Advanced | Large files: ask before copying above | "Bigger copies wait until you click Fetch. Default 100 MB." | Stepper (MB) | **Rename** |
| T-06 | Audio bridge (Linked PC → Main PC) (767) | Audio, media & power | Play Linked PC audio on this PC | none | Switch | **Rename** |
| T-07 | Mute Linked PC while bridging (774) | Audio, media & power | Silence the Linked PC's speakers meanwhile | "Prevents echo. Restored on stop or after a crash." | Switch | **Rename** |
| T-08 | Bridged audio volume (801) | Audio, media & power | Volume | none | Slider 0-100% with visible value | **Rename** |
| T-09 | Media keys follow active audio (827) | Audio, media & power | Media keys control the PC that is playing | none | Switch | **Rename** |
| T-10 | Lock together (843) | Audio, media & power | Lock both PCs together | none | Switch | **Rename** |
| T-11 | Sleep together (850) | Audio, media & power | Sleep both PCs together | none | Switch | **Rename** |
| T-12 | Explorer "Open on Linked PC" menu (857) | Audio, media & power | Add "Open on Linked PC" to the right-click menu | none | Switch | **Rename** |

Totals: 53 rows (29 Settings-card rows incl. 4 deck-owned, 3 control bar, 10 mouse & keyboard, 12 continuity). 2 removed (M-13, K-08), 3 merged into another editor (M-01, K-01, K-02), 1 third-party name removed (M-14), the rest moved or renamed.

### 8.5 Copy rules
1. Say what turning it on does, in positive form.
2. Sentence case. No internal names (HUD, staging, prefetch, beacon, KVM).
3. One helper line under any setting whose effect is not obvious, with the unit and the safe default.
4. A setting lives in exactly one place; elsewhere it is read-only with a link.
5. No third-party or competitor product names in labels, helpers, code comments, tests or docs (`AGENTS.md` §1.3).

## 9. Single-source rules for duplicated settings

### 9.1 Rule
One editor per setting. Other surfaces may show the value read-only with a link to the editor.

### 9.2 Dwell and deadzone: same values, two editors
- The Settings sliders "Edge Push Resistance (Dwell Delay)" and "Corner Deadzone Margin" (`SettingsPanel.tsx:497-545`) write `kvmSettings.edgeResistanceMs` and `kvmSettings.cornerDeadzonePx` through `useKvmStore.updateSettings`.
- The Computers tab "Dwell" and "Deadzone" chips (`src/components/computers/TopologyCanvas.tsx:688-745`; deadzone chips call `updateSettings({ cornerDeadzonePx })`, dwell chips call `updateSettings({ edgeResistanceMs })`) use the **same store and keys**.
- `updateSettings` copies both into every topology link as `deadzone_px` / `resistance_ms` (`src/stores/kvmStore.ts:1083-1090`).
- **Answer: same values, two editors.** Ranges differ (sliders 0-300 ms step 10 and 0-120 px step 4; chips 0/80/120/200 ms and 24/48/80 px), so a slider value such as 60 px lights no chip.
- **Decision:** the Computers tab is the single editor (next to the seams). Settings shows both read-only with an "Edit in Computers" link (K-01, K-02). A stored value that is not a chip value appears as an extra "Custom: N" chip so no user loses a value. The existing behaviour that one global value overwrites per-seam values (SPEC-24) is out of scope.

### 9.3 "File Transfer Drop Sync" (`syncFileTransfer`): wired, but a third file gate
- **Still wired.** TS: default `true` (`kvmStore.ts:133`), saved with the KVM settings (`:244-253`), pushed by `kvmSetSyncSettings` (`:1027-1036`, `src/components/computers/ComputersView.tsx:58-63`). Rust: `kvm_set_sync_settings(text, image, files)` stores `SYNC_FILES` (`src-tauri/src/kvm/clipboard.rs:9, 23-32`), read by `sync_files_enabled()` (`:19`).
- **Rust consumers:** copy-paste is gated by `file_clipboard_enabled() && sync_files_enabled()` (`clipboard.rs:144, 435`; `kvm/commands.rs:726`), and incoming `StreamHeader::File` is rejected with "file transfer sync disabled" when false (`kvm/files.rs:462-466, 480-484, 520-523`).
- **Versus drag & drop:** `continuityDragDropExperimental` feeds `drag_drop_enabled()` (`kvm/dragdrop.rs:20`; gates at `kvm/commands.rs:388, 1964`), a different flag. `continuityFileClipboard` feeds `file_clipboard_enabled()` (`dragdrop.rs:24`).
- **Answer:** it does not duplicate drag & drop. It is an older, third gate ANDed with `continuityFileClipboard` for copy-paste and additionally blocking incoming single-file streams. In effect it duplicates "Copy-paste files across PCs", and its label ("Drop Sync") describes neither.
- **Decision:** remove the control (K-08). T-02 is the only user-facing file switch. `syncFileTransfer` stays in the persisted type for one release, is always passed as `true` to `kvm_set_sync_settings` (no Rust change), and migration step 6 keeps the intent of users who had it off. Removing the field and the Rust gate is a follow-up (plan, "Follow-ups").

### 9.4 Flex display
M-13 is removed. Flex is selected by the Flex mode; `flexDisplay` is derived (§5.1). The Devices deck, palette and control window read the mode, never a stored flag.

## 10. Accessibility

- **Visible labels.** Every control has a visible `<label>` bound by `htmlFor` / `id` (or `aria-labelledby` for segmented controls and switches). No placeholder-only or title-only labelling. Helper text is linked with `aria-describedby`.
- **Rail.** A `nav` landmark "Settings sections" with a list of buttons; the current one has `aria-current="page"`. Roving focus: Tab enters the rail once; Up/Down and Home/End move between sections; Enter or Space opens one. Opening a section moves focus to its heading (`tabindex="-1"`). A skip link "Skip to settings" follows the filter box.
- **Mode tabs.** `role="tablist"` / `tab` / `tabpanel`; Left/Right arrows and Home/End select; selecting only changes the visible panel (no settings write, §5.1 rule 3).
- **Switches.** `role="switch"` with `aria-checked`; Space toggles. State is shown by thumb position as well as colour.
- **Filter.** `role="search"`, labelled "Filter settings"; result count in an `aria-live="polite"` region; Escape clears.
- **Focus order.** Filter, rail, pane heading, mode tabs, controls in DOM order, Advanced disclosure last. Focus ring: SPEC-26 gold 2 px `:focus-visible` (`src/index.css:70`).
- **Contrast (4.5:1 for text, SPEC-26 tokens).** Computed ratios: `--text-muted` `#8B8FA3` on `--ink-950` 5.72, on `--ink-900` 5.15, **on `--ink-800` 4.24 (fails)**; `--text` on `--ink-800` 8.40; `--gold-400` on `--ink-900` 8.38, on `--ink-800` 6.88; `--ink-950` on `--gold-400` 9.30. Rules: muted helper text only on `--ink-900` / `--ink-950`, never on `--ink-800` (use `--text` there); selected tab or segment = `--ink-950` on `--gold-400`, or gold text on `--ink-900`; no gradients (SPEC-26 gate). `--ink-700` hairlines are decorative; control state is also conveyed by label, value text and focus ring.
- **Motion.** No new animation beyond SPEC-26 press feedback; respect `prefers-reduced-motion`.
- **Targets.** Rail items and switches at least 32 px high.

## 11. Test plan

**Unit (Vitest, `src/lib/modeSettings.test.ts`, new):**
- `migrateLegacy`: (a) default legacy `{medium}` gives four modes with equal quality and audio on; (b) Camera residue (`noAudio: true`) gives `noAudio=false` in mirror/flex/hifi, `camera.playAudio=true`, notice flag set; (c) Hi-fi signature gives mirror/flex = preset bundle, hifi = legacy values; (d) legacy `flexDisplay: true` + `unscaled` stays on flex only, mirror gets letterbox/false; (e) `stretched` kept everywhere; (f) `preset: "custom"` with Hi-fi signature falls back to the `medium` bundle; (g) missing or partial legacy data filled from defaults; (h) **no migrated mode has `noAudio === true`** (property test over random legacy objects); (i) idempotent, legacy keys untouched; (j) camera bitrate nearest-of-three.
- `resolveLaunchSettings`: returns all 17 `CoreSettings` keys; `flexDisplay` true only for `flex`; **switching modes never mutates another mode** (select B then A, all four objects deep-equal before and after); editing mode A leaves B, C, D deep-equal.
- Quality derivation: the four bundles map to Smooth / Balanced / Sharp / Maximum; any deviation maps to Custom; resolution and bitrate segment mapping, including odd stored values (800, 1600).
- Inversion helpers for `noAudio`, `noControl`, `noKeyboardIme`, `noWindowAspectRatioLock` round-trip.
- Filter: label, helper and keyword matches; empty result; Advanced auto-expand.
- KVM migration step 6 (`syncFileTransfer=false` turns file copy-paste off).

**Component/UI (Vitest + Testing Library):** rail renders seven sections in order; arrow-key roving; filter narrows rail and pane; mode tabs switch panels without calling any setter; Camera then Mirror leaves Mirror audio unchanged; dwell/deadzone rows are read-only and link to Computers; no label contains the old jargon strings; a grep test asserts no third-party product name in `src/components/settings/**` and `docs/specs/SPEC-28*`.

**Must stay green:** Rust `commands.rs` tests for `build_scrcpy_args` / `build_app_window_args` (no Rust change), `themeTokens.test.ts`.

**Manual (`docs/MANUAL-TEST-CHECKLIST.md`):** upgrade a profile with Camera residue and confirm Mirror has sound; launch each mode from Devices and the palette and compare resulting settings; Flex size and taskbar on the Flex tab only; keyboard-only walk through rail, tabs and a switch; contrast spot check in the running app.

## 12. Phasing

Each phase merges and ships alone.

- **Phase 1: data model, migration and the Phone mirroring section.** `modeDefaults` store + migration + resolver; every launch path uses the resolver; Devices presets stop writing settings; "Quality preset" and "Flex display" controls removed; a Phone mirroring card with per-mode tabs replaces the mirroring rows of the old "Settings" card (M-01..M-24); General rows (M-25..M-29) stay in a small "General" card. The Control bar, Shared Mouse & Keyboard and Continuity cards are unchanged except K-08 removal and the third-party-name fix (M-14), shipped here as correctness and compliance fixes.
- **Phase 2: remaining sections, rail and filter.** Sectioned layout with rail and filter; relabel C-*, K-*, T-*; dwell/deadzone read-only plus the Computers "Custom" chip (§9.2); Advanced section; accessibility pass. No data-model change.

- **Phase 3: mirror window control bar.** Redesign the control bar in the mirror wrapper window (`src/components/ControlBar.tsx`, `src/components/EmbeddedMirrorWindow.tsx`), approved 2026-11-17 together with this spec:
  - Width 60 px; icon-only 40 x 40 px buttons, each with an `aria-label` and a styled tooltip shown on hover **and** keyboard focus (no 9 px text labels, no `title`-only buttons).
  - Groups with dividers, top to bottom: **Navigate** (Back, Home, Recent apps), **Sound** (Volume up, Volume down with a volume-down icon, not the mute icon), **Screen** (Power, Turn phone screen off (keep mirroring) as a toggle, Rotate), **Capture** (Screenshot, Record toggle, red while recording). A **More** button at the bottom opens a popover with Notifications, Wi-Fi, Bluetooth, Do Not Disturb, and Mouse wheel swipes.
  - Remove **Sleep** (duplicates Power). **End call** appears only during an active call, in danger color. Camera mode keeps torch, zoom, and stop camera in their own group.
  - Every on/off button gets a pressed style (gold outline) and `aria-pressed`.
  - "Open recording folder" moves into the toast shown when a recording stops.
  - Generic copy without third-party product names (AGENTS.md §1.3), for example "Mouse wheel swipes like a finger (for short-video feeds)" and "Use AnyLeap Camera in any video-call or streaming app".
  - Title-bar minimize, maximize, and close get 36 x 28 px hit areas, and Close gets a red hover.
  - The Settings → Control bar card lists the buttons by these groups. Collapsed mode applies to the floating control window only and shows Navigate and More only; the embedded mirror sidebar always shows every enabled group.

## 13. Risks and open questions

- Audio residue ambiguity (§5.4 step 4): a user who muted on purpose hears sound once and sees a notice. Accepted: it is the reported bug and the notice states the per-mode fix.
- Downgrade after edits loses new values (§5.4 step 8). Accepted.
- KVM settings live in `localStorage`, not `settings.json`. Moving them is out of scope; flag for a later spec.
- Removing the `syncFileTransfer` Rust gate entirely is deferred.
