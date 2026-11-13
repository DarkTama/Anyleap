# Work Queue

Single source of truth for what is being built next and where each item stands.
Update the row whenever an item changes state. Workflow per item:
spec drafted → spec approved → implementation (Orca worker) → verified (tests + review) → shipped.

**Status legend:** `queued` · `spec-draft` · `spec-approved` · `in-progress` · `review` · `merged` · `shipped` · `blocked`

| # | Item | Spec / ADR | Status | Notes |
|---|------|-----------|--------|-------|
| 1 | KVM transport → QUIC + authenticated pairing | SPEC-19, ADR-0003 | merged | **Security-critical.** Unauthenticated `/ws` accepts input injection; file-transfer path traversal; plaintext traffic; unauthenticated UDP mouse. |
| 2 | Quick adb wins pack: link handoff, per-app windows, phone toggles (Wi-Fi/BT/DND), battery alerts, screen recording, place/end call | SPEC-20 | in-progress | All adb/scrcpy one-liners; no companion app. Owns `sh_quote`. |
| 3 | Dashboard patterns: device hero card, activity timeline, command palette (Ctrl+K), connection doctor | SPEC-18 | merged | Layout patterns only; keep the Graphite identity (SPEC-17). Owns `logActivity()`. |
| 4 | Phone files browser + photo gallery + read-only notifications over adb | SPEC-21 | in-progress | New "Phone" tab. Notifications are read-only. |
| 5 | Native virtual webcam (Media Foundation) | SPEC-15 | queued | Windows 11 only. Existing spec. |
| 6 | PC display streaming | SPEC-13 | queued | Reuse the QUIC connection from #1. |

**Merge order:** #1 → #2 → #3 → #4. Later branches rebase onto earlier ones; after #3 lands, #2 and #4 get a follow-up to call `logActivity()`.

## Out of scope (requires an Android companion app)

Notification replies, SMS/chat, answering calls, find-my-phone ringing, media session control, automatic phone→PC clipboard sync.
