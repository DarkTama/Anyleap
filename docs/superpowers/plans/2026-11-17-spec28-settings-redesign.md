# SPEC-28 Settings Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give each mirroring mode its own persisted defaults (with a safe migration), then rebuild Settings as a 7-section rail with a filter and positive, jargon-free labels.

**Architecture:** A pure TS module `src/lib/modeSettings.ts` owns types, defaults, migration and `resolveLaunchSettings`. `settingsStore.ts` persists `modeDefaults`; `useAppStore` holds it; every launch path calls the resolver. Rust is unchanged. UI lives in `src/components/settings/**`.

**Tech Stack:** Tauri 2, React 19 + TypeScript + Tailwind (SPEC-26 tokens), Vitest.

**Spec:** `docs/specs/SPEC-28-settings-redesign.md`. **ADR:** `docs/adr/0005-per-mode-mirroring-settings.md`.

## Global Constraints
- No Rust changes. `start_mirror` still receives a full 17-field `CoreSettings`.
- Never set `noAudio = true` in migration. Legacy `settings` / `preset` keys are never deleted.
- Labels follow SPEC-28 §8.5. No third-party or competitor product names anywhere (`AGENTS.md` §1.3).
- SPEC-26 tokens only, no gradients, muted text never on `--ink-800`.
- Each phase ends green: `npx tsc --noEmit`, `npx vitest run`, `cargo test` (unchanged) and ships alone.

---

## Phase 1: data model, migration, Phone mirroring section

### Task 1.1: `modeSettings.ts` types, defaults, resolver (TDD)
**Files:** create `src/lib/modeSettings.ts`, `src/lib/modeSettings.test.ts`.
- [ ] Write failing tests: `resolveLaunchSettings` returns all 17 `CoreSettings` keys; `flexDisplay` true only for `flex`; editing one mode leaves the others deep-equal; fresh-install defaults per SPEC-28 §5.3.
- [ ] Implement `MirrorMode`, `ModeSettings`, `CameraModeSettings`, `ModeDefaultsV2`, `DEFAULT_MODE_DEFAULTS`, `resolveLaunchSettings`, `toCameraSettings(camera, dialogChoices)`.
- [ ] Run `npx vitest run src/lib/modeSettings.test.ts`; commit.

### Task 1.2: Migration (TDD)
**Files:** `src/lib/modeSettings.ts`, `src/lib/modeSettings.test.ts`.
- [ ] Failing tests (a)-(j) from SPEC-28 §11, including the property test "no migrated mode has `noAudio === true`" and idempotence.
- [ ] Implement `migrateLegacy(settings, preset)` per §5.4 steps 1-5 (pure, no I/O).
- [ ] Run tests; commit.

### Task 1.3: Quality and inversion helpers (TDD)
**Files:** `src/lib/modeSettings.ts` (+ test).
- [ ] Tests: bundle to Smooth/Balanced/Sharp/Maximum and Custom; resolution segments (Native/1080p/720p, odd values 800/1600 to Custom); bitrate segments (≤4, ≤12, above); inversion helpers for `noAudio`, `noControl`, `noKeyboardIme`, `noWindowAspectRatioLock`.
- [ ] Implement `deriveQuality`, `applyQuality`, `segmentFor*`, `invert*`; keep `PRESET_BUNDLES`, drop `PRESET_LABELS`/`PRESET_HINTS` use.
- [ ] Run tests; commit.

### Task 1.4: Persistence and store
**Files:** `src/lib/settingsStore.ts`, `src/store/useAppStore.ts`, `src/lib/persist.ts`, `src/App.tsx` (load block ~L360).
- [ ] Add `loadModeDefaults()` / `saveModeDefaults()` (keys `modeDefaults`, `modeDefaultsNotice`); on absent key run `migrateLegacy` from `loadSettings()` and write per §5.4 step 7. Keep `saveSettings` write of legacy keys off.
- [ ] Add KVM step 6: in `kvmStore` init, if `syncFileTransfer === false && continuityFileClipboard`, call `updateSettings({ continuityFileClipboard: false })` once (guard by `modeDefaultsNotice`-style flag `fileSyncMigrated`).
- [ ] `useAppStore`: add `modeDefaults`, `setModeDefaults`, `updateMode(mode, patch)`; keep `settings`/`preset` only until task 1.7.
- [ ] `App.tsx` startup loads `modeDefaults`. Test the store with a mocked plugin-store (idempotent second load).
- [ ] Commit.

### Task 1.5: Launch paths use the resolver; Devices presets stop writing
**Files:** `src/components/deck/LaunchMatrix.tsx`, `src/components/deck/StudioDeck.tsx`, `src/components/deck/DeviceRack.tsx`, `src/App.tsx` (`startMode` ~L697-716), `src/components/CameraLaunchDialog.tsx`.
- [ ] `LaunchMatrix.handleSelect`: only `onSelectMode(mode)`; delete the per-mode `updated` blocks.
- [ ] `App.tsx` `startMode`: `startMirror(serial, resolveLaunchSettings(mode, store.modeDefaults))`; no `setSettings`.
- [ ] `StudioDeck` / `DeviceRack`: `startMirror(serial, resolveLaunchSettings(currentMode, modeDefaults))`. Deck toggles (hide taskbar, turn screen off, show touches, Flex size) call `updateMode(currentMode, ...)`; collapse the duplicated turn-screen-off row (`:465`, `:536`) into one.
- [ ] `CameraLaunchDialog`: pre-fill mic and bitrate from `modeDefaults.camera`, and `embedded` from it too.
- [ ] Component test: select Camera then Mirror; Mirror's resolved `noAudio` is unchanged.
- [ ] Commit.

### Task 1.6: Other readers
**Files:** `src/App.tsx` (~L436), `src/components/ControlWindow.tsx` (~L34).
- [ ] Session window choice reads `resolveLaunchSettings(sessionMode, modeDefaults).embedded`.
- [ ] Control window derives flex from the running session's mode (not legacy `settings.flexDisplay`).
- [ ] Commit.

### Task 1.7: Phone mirroring section UI
**Files:** create `src/components/settings/PhoneMirroringSection.tsx`, `ModeTabs.tsx`, `SegmentedControl.tsx`, `SwitchRow.tsx`; modify `src/components/SettingsPanel.tsx`.
- [ ] Mode tabs (`tablist`, arrow keys) for Mirror / Flex / Camera / High fidelity; selecting a tab changes the panel only.
- [ ] Groups per SPEC-28 §8.1: Quality (M-01..M-05), Audio & control (M-10..M-12), Phone while mirroring (M-06, M-07, M-09), Window options (M-08, M-14, M-20, M-20b), Flex display on the Flex tab (M-15..M-19). Camera tab: playAudio, bitrate, embedded.
- [ ] Positive labels via inversion helpers; M-14 helper text without any product name; M-20 disabled with a reason when M-14 is on.
- [ ] One-time audio notice from `modeDefaultsNotice` (dismiss clears the flag).
- [ ] Remove from `SettingsPanel`: Quality preset dropdown, Flex display toggle (M-13), old mirroring rows; keep General rows (M-25..M-29) in a small card.
- [ ] Remove `preset` / `setPreset` / `settings` / `setSettings` use; delete `QualityPreset`, `PRESET_LABELS`, `PRESET_HINTS` if unused; keep `PRESET_BUNDLES`.
- [ ] Contrast: muted helper text only on `--ink-900` / `--ink-950`.
- [ ] Commit.

### Task 1.8: Phase-1 cleanups that ship with it
**Files:** `src/components/SettingsPanel.tsx`, `src/components/computers/ComputersView.tsx`, `src/stores/kvmStore.ts`.
- [ ] Remove the K-08 toggle ("File Transfer Drop Sync"). Always pass `true` as the `files` argument of `kvmSetSyncSettings` (`kvmStore.ts` ~L1027, `ComputersView.tsx` ~L58); keep the field in the type for one release.
- [ ] Add a repo test scanning `src/**` and `docs/specs/SPEC-28*` for third-party product names (denylist stored obfuscated so the test file names none).
- [ ] Commit.

### Task 1.9: Phase-1 verification and docs
- [ ] `npx tsc --noEmit`, `npx vitest run`, `cd src-tauri && cargo test` all green.
- [ ] Manual: profile with Camera residue gets audio on Mirror plus the notice; each mode launched from rack, deck, palette uses its own object; Flex size only on Flex.
- [ ] Update `README.md` Features (per-mode defaults), `docs/MANUAL-TEST-CHECKLIST.md`; SPEC-28 status "Phase 1 implemented". Commit.

---

## Phase 2: sections, rail, filter

### Task 2.1: Settings registry and filter (TDD)
**Files:** create `src/components/settings/registry.ts`, `src/lib/settingsFilter.ts`, `src/lib/settingsFilter.test.ts`.
- [ ] Registry: one entry per control (ids from SPEC-28 §8: section, label, helper, keywords).
- [ ] Failing tests: label / helper / keyword matching, case-insensitivity, empty result, section counts, Advanced auto-expand.
- [ ] Implement `filterSettings(query)`; commit.

### Task 2.2: Rail and layout shell
**Files:** create `src/components/settings/SettingsShell.tsx`, `SettingsRail.tsx`; modify `src/components/SettingsPanel.tsx`.
- [ ] `nav` "Settings sections", roving focus (Up/Down/Home/End), `aria-current`, focus to section heading on open, "Skip to settings" link, `role="search"` filter with live result count and Escape to clear.
- [ ] Narrow window: rail becomes a horizontal scroller; filter stays visible.
- [ ] Component tests for keyboard navigation and filter narrowing; commit.

### Task 2.3: General and Control bar sections
- [ ] General (M-25..M-29) with new labels; Control bar (C-01..C-03) relabelled, behaviour unchanged (`emit("control-config")` kept). Commit.

### Task 2.4: Shared mouse & keyboard (single source)
**Files:** `src/components/settings/MouseKeyboardSection.tsx`, `src/components/computers/TopologyCanvas.tsx`.
- [ ] K-01 / K-02 become read-only values with an "Edit in Computers" link (switches tab).
- [ ] K-03..K-05 relabelled; K-04 shown only when K-03 is on.
- [ ] TopologyCanvas: extra "Custom: N" chip when the stored dwell or deadzone is not a chip value; test.
- [ ] Commit.

### Task 2.5: Clipboard & files, Audio/media/power, Advanced
**Files:** `src/components/settings/ClipboardFilesSection.tsx`, `AudioMediaPowerSection.tsx`, `AdvancedSection.tsx`.
- [ ] K-06, K-07, T-01..T-03 in Clipboard & files (drag and drop labelled Experimental).
- [ ] T-06..T-12 in Audio, media & power; volume slider with visible value and bound label.
- [ ] Advanced (collapsed `details`): T-04, T-05, K-09, K-10.
- [ ] Commit.

### Task 2.6: Accessibility and visual pass
- [ ] Every input has a visible bound label and `aria-describedby` helper; switches use `role="switch"`.
- [ ] Contrast audit with SPEC-28 §10 ratios; `themeTokens.test.ts` stays green; honour `prefers-reduced-motion`.
- [ ] Label test: none of the old jargon strings ("HUD Pill", "Beacon", "Staging", "Prefetch", "KVM diagnostics") in settings components.
- [ ] Commit.

### Task 2.7: Phase-2 verification and docs
- [ ] `npx tsc --noEmit`, `npx vitest run`, `cargo test`.
- [ ] Manual keyboard-only walk (rail, tabs, switches, filter) and contrast spot check.
- [ ] Update `README.md`, `docs/MANUAL-TEST-CHECKLIST.md`, `docs/SETTINGS-MAP.md` (quality bundles are per mode); mark SPEC-28 "Implemented" and ADR-0005 "Accepted". Commit.

## Phase 3: mirror window control bar

1. Extract the button list into a pure, grouped model (`controlBarGroups(config, sessionMode, state)`) with unit tests: group order, Sleep removed, End call only during a call, camera group only in camera mode, More-menu contents. `collapsed` (Navigate + More only) applies to the floating control window only (`surface: "floating"`); the embedded sidebar always shows every enabled group.
2. Rebuild `ControlBar.tsx`: 40 px icon buttons, accessible tooltip component (hover and focus), `aria-label`, `aria-pressed` for toggles, and dividers between groups. Fix the volume-down icon.
3. Add the More popover (keyboard reachable, Escape closes, focus returns to the More button).
4. Widen the wrapper sidebar to 60 px in `EmbeddedMirrorWindow.tsx`, enlarge the title-bar controls, and add a red hover on Close.
5. Move "Open recording folder" into the stop-recording toast.
6. Replace third-party product names in camera-mode help and the swipe tooltip.
7. Update the Settings → Control bar card to list buttons by group, and migrate the stored `controlBar.buttons.sleep` (drop it).
8. Gates plus a UI check in the real mirror window (Mirror, Flex, Camera).

## Follow-ups (not in this plan)
- Remove `syncFileTransfer` from the type and the Rust `SYNC_FILES` gate (`kvm/clipboard.rs`, `kvm/files.rs`).
- Move KVM settings from `localStorage` to `settings.json`.
- Optional per-phone mode defaults.
