# Work Queue

Single source of truth for what is being built next and where each item stands.
Update the row whenever an item changes state. Workflow per item:
spec drafted → spec approved → implementation (Orca worker) → verified (tests + review) → shipped.

**Status legend:** `queued` · `spec-draft` · `spec-approved` · `in-progress` · `review` · `merged` · `shipped` · `blocked`

| # | Item | Spec / ADR | Status | Notes |
|---|------|-----------|--------|-------|
| 1 | KVM transport → QUIC + authenticated pairing | SPEC-19, ADR-0003 | shipped | **Security-critical.** Shipped in v1.0.0. |
| 2 | Quick adb wins pack: link handoff, per-app windows, phone toggles (Wi-Fi/BT/DND), battery alerts, screen recording, place/end call | SPEC-20 | shipped | Shipped in v1.0.0. All adb/scrcpy one-liners; no companion app. Owns `sh_quote`. |
| 3 | Dashboard patterns: device hero card, activity timeline, command palette (Ctrl+K), connection doctor | SPEC-18 | shipped | Shipped in v1.0.0. Owns `logActivity()`. |
| 4 | Phone files browser + photo gallery + read-only notifications over adb | SPEC-21 | shipped | Shipped in v1.0.0. New "Phone" tab. Notifications are read-only. |
| 0 | **Computer Link input fidelity & reliability fixes** (16 audit findings) | SPEC-22 | shipped | **Priority.** Shipped in v1.0.0. Part A (input core) and Part B (clipboard/discovery/settings/UI). |
| 7 | Shared Mouse & Keyboard UX, role-aware links, fingerprint reconnect & offset-aware layout | SPEC-23 | shipped | Shipped in v1.0.0 (#10). Role-aware Main PC / Linked PC UX, certificate fingerprint reconnect, offset-aware layout. |
| 8 | "Ink & Gold" app theme + interaction polish | SPEC-26 | shipped | Shipped in v1.0.0 (#11). Replaces Graphite palette (SPEC-17); flat tokens + design-engineering interaction polish. |
| 12 | New app icons from the Ink & Gold logo kit | — | shipped | Shipped in v1.0.0 (#12). Brand assets (`assets/brand/`), app/tray/web icons, header logo, and README refresh. |
| 9 | Per-monitor layout mapping | SPEC-24 | shipped | Shipped in v1.0.0 (#14). Display-settings-style layout and seam-based cursor leaps. |
| — | **Release v1.0.0** | — | shipped | Version bump + release after pre-release polish (#15, #16). |
| — | **Release v1.0.1 (notification parser fix, stable phone identity + auto reconnect)** | — | shipped | Version bump + patch release (#17, #18, #19). |
| 10a | Continuity — Drag & drop files across PCs, file clipboard, pointer scheme sync | SPEC-25a, ADR-0004 | shipped | Shipped in v1.1.0 (#22). Edge catcher OLE drop target, pre-staging (`stagingLimitMb`), CF_HDROP clipboard, `SetSystemCursor` + `SPI_SETCURSORS` restore. |
| 10b | Continuity — Audio bridge + media keys follow active audio | SPEC-25b, ADR-0004 | shipped | Shipped in v1.1.0 (#23). WASAPI loopback 48 kHz stereo PCM over QUIC datagrams (`0x41`), jitter buffer, keyboard-hook media key routing. |
| 10c | Continuity — Session sync (lock/sleep together) + Open on Linked PC | SPEC-25c, ADR-0004 | shipped | Shipped in v1.1.0 (#20). Hidden window session/power notifications, HKCU Explorer context menu + single-instance `--open-on-linked`, URL/executable safety guards. |
| — | **Release v1.1.0 (Continuity: cross-PC files, audio bridge, session sync & Open on Linked PC)** | SPEC-25 | shipped | Version bump + feature release (#20, #21, #22, #23, #24, #25). |
| — | OSS readiness — Contributor, security & community docs | — | shipped | No version bump needed (#26). `CONTRIBUTING.md`, `SECURITY.md`, `CODE_OF_CONDUCT.md`, `CODEOWNERS`, issue templates. |
| — | OSS readiness — CI PR checks & PR template | — | shipped | No version bump needed (#27). `.github/workflows/ci.yml` + `.github/pull_request_template.md`. |
| — | OSS readiness — `rustfmt` & `clippy` baseline | — | shipped | No version bump needed (#44). Crate-wide formatting and CI enforcement. |
| — | OSS readiness — Dependency policy & Tauri version guard | — | shipped | No version bump needed (#43, #48). Dependabot grouping/policy, `scripts/check-tauri-versions.mjs`, crate/npm alignment. |
| — | Security & dependency patches (`undici`, `rustls`, `zip`, Tauri alignment, npm advisories) | — | shipped | Released in v1.1.1 (#42, #48, #54). |
| — | **Release v1.2.0 (Phone Hub, pinned apps, open apps in Flex/Mirror, per-mode Settings redesign and new mirror control bar, scrcpy 5.0.1, wireless auto-reconnect, KVM fixes: cursor return, monitor hot-plug, taskbar hover, admin/UAC input-blocked notices)** | — | shipped | Minor release. |
| — | **Release v1.1.1 (security & dependency patch: zip 9.5 in updater, rustls 0.23.46, Tauri plugins aligned, npm toolchain advisories)** | — | shipped | Version bump + security & dependency patch release (#56). |
| 11 | Phone hub | SPEC-27 | in-progress | In progress (27a backend data service; 27b/27c UI). |
| 5 | Native virtual webcam (Media Foundation) | SPEC-15 | queued | Windows 11 only; phone camera AND Linked PC webcam. Existing spec. |
| 6 | PC display streaming | SPEC-13 | queued | Reuse the QUIC connection from #1 (SPEC-19). |
| 13 | Cross-platform Linux/macOS input adapters | — | blocked | Blocked on contributors/testers; Windows first. |
| 14 | Android companion app (notification replies, SMS, calls, QUIC link reusing SPEC-19 pairing) | — | queued | Long-term post-v1.0.0. |

## Out of scope (requires an Android companion app)

Notification replies, SMS/chat, answering calls, find-my-phone ringing, media session control, automatic phone→PC clipboard sync.
