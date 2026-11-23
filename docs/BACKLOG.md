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

## Continuity / Shared Mouse & Keyboard (post-SPEC-25)

- **Cross-PC drag & drop reliability (Experimental → stable)**: investigate drops rejected with `DROPEFFECT_NONE` (`effect = 0`) and Linked→Main mouse capture/button ownership edge cases so drag & drop can graduate from Experimental.
- **Cursor smoothness on Linked PC during large outbound transfers**: pointer motion feels less smooth when the Linked PC sends large files to the Main PC than in the Main→Linked direction; profile QUIC pacing / datagram scheduling under load.
- **Windows 11 top-level Explorer context menu entry**: add a packaged shell extension / sparse package so **Open on Linked PC** appears directly in the Windows 11 top-level context menu instead of under *Show more options*.
- **Strict stream-to-offer binding**: bind incoming `DragFile` and `ClipFile` unidirectional QUIC streams strictly to active offer IDs and declared file sizes.
- **Intermittent receiver-side Cancel**: investigate rare cases where transfer **Cancel** only took effect from the sender (could not reproduce during SPEC-25 verification).

## Settings redesign (SPEC-28)

- ~~Per-mode mirroring defaults, migration and Phone mirroring section~~ (SPEC-28 phase 1, done).
- ~~Seven-section rail with filter, relabelled rows, Advanced section, Computers "Custom" chip~~ (SPEC-28 phase 2, done).
- ~~**Phase 3:** mirror window control bar redesign~~ (SPEC-28 phase 3, done; live Mirror/Flex/Camera visual check still pending).
- **Follow-ups:** remove `syncFileTransfer` from the type and the Rust `SYNC_FILES` gate; move KVM settings from `localStorage` to `settings.json`; optional per-phone mode defaults.

## Scroll feel (SPEC-30)

- ~~**Phase 0 & Phase 1:** SDL3 precise-scroll verification spike, density-independent scroll speed (`Auto` / `1x–5x`), per-mode Scrolling group, More popover **Paged scrolling** rename~~ (SPEC-30 phase 1, done).
- ~~**Phase 2:** optional smooth scrolling (eased step interpolation, touchpad bypass).~~ (SPEC-30 phase 2, code done; manual device feel/CPU check pending).
- **Phase 3:** touchpad delta stream measurement and optional inertia.

## Dependencies

Planned manual major upgrades (require two-PC testing and/or release verification):

- **`windows` / `windows-core` `0.61` → `0.62`**: touches unsafe Win32/COM code across audio loopback, input hooks, and OLE drag & drop; verify `PROPVARIANT` and drop semantics with two-PC testing.
- **`rcgen` `0.13` → `0.14`**: pairing certificate generation and TLS identity (`identity.rs`); verify pairing and fingerprint pinning across two PCs.
- **`sha2` `0.10` → `0.11`**: certificate fingerprint hashing; verify pairing and existing trusted peer compatibility across two PCs.
- **`tauri-apps/tauri-action` `0` → `1`**: release bundling and updater signing; verify with a manual `release` workflow test build.
- **GitHub Actions (`actions/checkout`, `actions/setup-node`, `actions/upload-artifact`) `v4` → `v7`**: review workflow compatibility and verify with CI and a release test build.
