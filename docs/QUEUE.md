# Work Queue

Single source of truth for what is being built next and where each item stands.
Update the row whenever an item changes state. Workflow per item:
spec drafted → spec approved → implementation (Orca worker) → verified (tests + review) → shipped.

**Status legend:** `queued` · `spec-draft` · `spec-approved` · `in-progress` · `review` · `merged` · `shipped` · `blocked`

| # | Item | Spec / ADR | Status | Notes |
|---|------|-----------|--------|-------|
| 1 | KVM transport → QUIC + authenticated pairing | SPEC-19, ADR-0003 | merged | **Security-critical.** Unauthenticated `/ws` accepts input injection; file-transfer path traversal; plaintext traffic; unauthenticated UDP mouse. |
| 2 | Quick adb wins pack: link handoff, per-app windows, phone toggles (Wi-Fi/BT/DND), battery alerts, screen recording, place/end call | SPEC-20 | merged | All adb/scrcpy one-liners; no companion app. Owns `sh_quote`. |
| 3 | Dashboard patterns: device hero card, activity timeline, command palette (Ctrl+K), connection doctor | SPEC-18 | merged | Layout patterns only; keep the Graphite identity (SPEC-17). Owns `logActivity()`. |
| 4 | Phone files browser + photo gallery + read-only notifications over adb | SPEC-21 | merged | New "Phone" tab. Notifications are read-only. |
| 0 | **Computer Link input fidelity & reliability fixes** (16 audit findings) | SPEC-22 | merged | **Priority.** Part A (input core) and Part B (clipboard/discovery/settings/UI) run as parallel workers. |
| 5 | Native virtual webcam (Media Foundation) | SPEC-15 | queued | Windows 11 only. Existing spec. |
| 6 | PC display streaming | SPEC-13 | queued | Reuse the QUIC connection from #1. |
| 7 | Shared Mouse & Keyboard UX, role-aware links, fingerprint reconnect & offset-aware layout | SPEC-23 | review | This PR. |
| 8 | "Ink & Gold" app theme + interaction polish | SPEC-26 | merged | Replaces Graphite palette (SPEC-17); flat tokens + design-engineering interaction polish. |
| 9 | Per-monitor layout mapping | SPEC-24 | queued | Next. |
| — | **Release** | — | queued | Version bump + release after SPEC-23/24 manual tests pass. |
| 10 | Pointer scheme sync | SPEC-25 | queued | Optional toggle; uses `SetSystemCursor` + `SPI_SETCURSORS` restore, registry untouched. |
| 11 | New app icons from the Ink & Gold logo kit | — | review | Brand assets (`assets/brand/`), app/tray/web icons, header logo, and README refresh (`feat/brand-ink-gold`). |
| 12 | Cross-platform Linux/macOS input adapters | — | queued | Blocked on testers; Windows first. |

**Merge order:** #1 → #2 → #3 → #4. Later branches rebase onto earlier ones; after #3 lands, #2 and #4 get a follow-up to call `logActivity()`.

## Out of scope (requires an Android companion app)

Notification replies, SMS/chat, answering calls, find-my-phone ringing, media session control, automatic phone→PC clipboard sync.
