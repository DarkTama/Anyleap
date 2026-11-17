# ADR 0005: Per-Mode Mirroring Settings

## Status
Proposed (accompanies SPEC-28)

## Context
Mirroring settings are one global `CoreSettings` object plus one `preset` in `settings.json` (`src/lib/settingsStore.ts`). The Devices Mirroring presets (`LaunchMatrix.handleSelect`, palette `startMode` in `src/App.tsx`) overwrite fields of that object when a mode is picked (Camera sets `noAudio: true`, High Fidelity forces AV1, Flex flips `flexDisplay`). Values leak between modes and into the Settings page, which caused the "no sound in Mirror/Flex" defect. The persisted shape and its migration are hard to change later because every installed profile carries it.

## Decision
1. **Each mode owns a complete settings object** (`modeDefaults` in `settings.json`: `mirror`, `flex`, `hifi` as `Omit<CoreSettings, "flexDisplay">`, `camera` as a small camera object). Selecting a mode writes nothing; editing a mode writes only that mode.
2. **Launch settings are resolved, not stored:** `resolveLaunchSettings(mode, defaults)` yields a full `CoreSettings`; only `flexDisplay` is forced by the mode. The Rust IPC types and `build_scrcpy_args` do not change.
3. **The global "Quality preset" is removed.** The Low / Medium / High / Highest bundles remain as a per-mode Quality control; the preset is derived from the four quality fields, never stored.
4. **One-time migration** from legacy `settings` + `preset` into every mode, undoing known preset residue, never setting `noAudio = true`, and showing a one-time notice when audio is turned on. Legacy keys stay frozen for downgrade.
5. **`syncFileTransfer` is retired from the UI**; the Rust gate stays and is always sent `true` until a follow-up removes it.

## Alternatives considered
- *Keep one global object, make presets non-destructive overlays at launch.* Smaller change, but the Settings page would then show values that are not what a mode launches with, and "edit what Flex uses" has no home.
- *Per-device and per-mode settings.* More flexible, but multiplies stored objects and UI; deferred, the chosen shape can be keyed by device later.
- *Keep the global Quality preset next to per-mode tabs.* Reintroduces cross-mode coupling.

## Consequences
### Positive
- Switching modes cannot change another mode; the audio-leak defect class is removed by construction.
- Settings page shows exactly what each mode launches with.
- Rust side and its tests are untouched.
### Negative
- Settings persisted in two shapes during the downgrade window; edits made after upgrade do not flow back.
- Migration cannot tell a deliberate "no audio" from Camera residue, so affected users get audio on once, with a notice.
- Launch paths (rack, deck, palette, control window, session window choice) must all move to the resolver in one release or they read stale legacy data.
