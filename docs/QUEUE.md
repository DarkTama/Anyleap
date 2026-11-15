# Work Queue

Single source of truth for what is being built next and where each item stands.
Update the row whenever an item changes state. Workflow per item:
spec drafted → spec approved → implementation (Orca worker) → verified (tests + review) → shipped.

**Status legend:** `queued` · `spec-draft` · `spec-approved` · `in-progress` · `review` · `merged` · `shipped` · `blocked`

| # | Item | Spec / ADR | Status | Notes |
|---|------|-----------|--------|-------|
| 1 | KVM transport → QUIC + authenticated pairing | SPEC-19, ADR-0003 | shipped | **Security-critical.** Unauthenticated `/ws` accepts input injection; file-transfer path traversal; plaintext traffic; unauthenticated UDP mouse. |
| 2 | Quick adb wins pack: link handoff, per-app windows, phone toggles (Wi-Fi/BT/DND), battery alerts, screen recording, place/end call | SPEC-20 | shipped | All adb/scrcpy one-liners; no companion app. Owns `sh_quote`. |
| 3 | Dashboard patterns: device hero card, activity timeline, command palette (Ctrl+K), connection doctor | SPEC-18 | shipped | Layout patterns only; keep the Graphite identity (SPEC-17). Owns `logActivity()`. |
| 4 | Phone files browser + photo gallery + read-only notifications over adb | SPEC-21 | shipped | New "Phone" tab. Notifications are read-only. |
| 0 | **Computer Link input fidelity & reliability fixes** (16 audit findings) | SPEC-22 | shipped | **Priority.** Part A (input core) and Part B (clipboard/discovery/settings/UI) run as parallel workers. |
| 5 | Native virtual webcam (Media Foundation) | SPEC-15 | queued | Windows 11 only. Existing spec. |
| 6 | PC display streaming | SPEC-13 | queued | Reuse the QUIC connection from #1. |
| 7 | Shared Mouse & Keyboard UX, role-aware links, fingerprint reconnect & offset-aware layout | SPEC-23 | review | This PR. |
| 8 | "Ink & Gold" app theme + interaction polish | SPEC-26 | shipped | Replaces Graphite palette (SPEC-17); flat tokens + design-engineering interaction polish. |
| 9 | Per-monitor layout mapping | SPEC-24 | shipped | Backend (`spec24/backend`) + Frontend (`spec24/frontend`) in parallel. |
| — | **Release v1.0.0** | — | shipped | Version bump + release after pre-release polish. |
| — | **Release v1.0.1 (notification parser fix, stable phone identity + auto reconnect)** | — | shipped | Version bump + patch release (#17, #18). |
| 10a | Continuity — Drag & drop files across PCs, file clipboard, pointer scheme sync | SPEC-25a, ADR-0004 | shipped | Edge catcher OLE drop target, pre-staging (`stagingLimitMb`), CF_HDROP clipboard, `SetSystemCursor` + `SPI_SETCURSORS` restore. |
| 10b | Continuity — Audio bridge + media keys follow active audio | SPEC-25b, ADR-0004 | shipped | WASAPI loopback 48 kHz stereo PCM over QUIC datagrams (`0x41`), jitter buffer, keyboard-hook media key routing. |
| 10c | Continuity — Session sync (lock/sleep together) + Open on Linked PC | SPEC-25c, ADR-0004 | shipped | Hidden window session/power notifications, HKCU Explorer context menu + single-instance `--open-on-linked`, URL/executable safety guards. |
| — | **Release v1.1.0 (Continuity: cross-PC files, audio bridge, session sync & Open on Linked PC)** | SPEC-25 | shipped | Version bump + feature release (#20, #21, #22, #23). |
| 11 | Phone hub | SPEC-27 | queued | Queued after SPEC-25 continuity work. |
| 12 | New app icons from the Ink & Gold logo kit | — | review | Brand assets (`assets/brand/`), app/tray/web icons, header logo, and README refresh (`feat/brand-ink-gold`). |
| 13 | Cross-platform Linux/macOS input adapters | — | queued | Blocked on testers; Windows first. |
| 14 | Android companion app (notification replies, SMS, calls, QUIC link reusing SPEC-19 pairing) | — | queued | Long-term post-v1.0.0. |

**Merge order:** #1 → #2 → #3 → #4. Later branches rebase onto earlier ones; after #3 lands, #2 and #4 get a follow-up to call `logActivity()`.

## Out of scope (requires an Android companion app)

Notification replies, SMS/chat, answering calls, find-my-phone ringing, media session control, automatic phone→PC clipboard sync.
