# SPEC-27 Phone hub Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn the Phone tab into a phone dashboard (overview, messages, notifications, gallery, files, apps, audio-only) fed by a backend phone-data service, and refocus the Devices tab on connect & mirror.

**Architecture:** A new Rust module `src-tauri/src/phone_hub/` owns all adb/scrcpy queries, a tiered poller, a memory-only message aggregator and media/app helpers, exposed through the Tauri commands/events in the design's §7 contract. The frontend consumes only that contract: 27b rebuilds `src/components/phone/**`, 27c tidies `src/components/deck/**`. Three Orca workers in parallel; merge order 27a → 27c → 27b.

**Tech Stack:** Tauri 2, Rust (tokio), React 19 + TypeScript + Tailwind (SPEC-26 Ink & Gold tokens), Vitest.

**Spec:** `docs/superpowers/specs/2026-11-16-spec27-phone-hub-design.md` (copy of the approved design; contract in §7).

## Global Constraints
- No app on the phone; adb + bundled scrcpy only. No root.
- Every overview leaf may be `null`; parsers never return an error for unknown/OEM formats.
- Poll tiers: static once per connection; fast 30 s while watched; slow 5 min while watched or on `refresh: true`; notifications every `notificationPollSecs` (default 10, min 5).
- Messages: memory only, ≤ 200 per conversation, cleared on disconnect and app exit; never in disk or debug log.
- App label cache may be on disk, keyed by the stable device identity (#18).
- Device paths always through `normalize_device_path` / `sh_quote`; `serial` through `validate_serial`.
- UI: SPEC-26 tokens, no gradients (enforced by `themeTokens.test.ts`), emil-design-eng rules, labelled navigation, "—" for null.
- Shared Rust files edited only inside `// --- SPEC-27a ---` blocks; 27b owns `src/components/phone/**`, `src/types/phone.ts`, `src/services/phoneHubService.ts`, `src/stores/phoneHubStore.ts`, the notification-polling part of `App.tsx`; 27c owns `src/components/deck/**`, `src/components/DevicesTab.tsx`, `src/components/DeviceRow.tsx`.
- Gates (AGENTS.md §2): `npm test`, `cargo fmt --check`, `cargo clippy --all-targets --locked -D warnings`, `cargo check`, `npm run build`, plus `cargo test` and `npm run check:tauri-versions`.
- Fixtures use generic names/messages only (no personal data, no third-party product names in prose).

## Review Focus
1. Phone disconnects mid-poll / wireless drop → poller stops cleanly, overview shows stale-marked data, messages cleared, no error toasts loop. (Task A5 test `poller_stops_and_clears_on_disconnect`.)
2. Two phones connected → watch/unwatch and messages are per-serial, never mixed. (Task A5 test `watch_is_per_serial`; Task A4 test `conversations_are_per_serial`.)
3. Notification with emoji / RTL / multi-line text or a group chat with same sender names → parsed and deduped correctly. (Task A4 test `dedupes_group_messages_with_unicode`.)
4. Gallery with thousands of items or file names with spaces/quotes → paged queries, safe paths. (Task A3 test `media_rows_with_quotes_and_spaces`; Task B4 virtualized/paged grid test.)
5. Audio-only while a mirror with audio is running, or started twice → refused/idempotent, never double audio. (Task A6 test `audio_only_refused_when_mirror_audio_active`.)

---

## Part 27a — Backend (worker: hydrogen-high, branch `feat/spec27a-phone-hub-backend`)

### Task A1: Overview parsers
**Files:** Create `src-tauri/src/phone_hub/mod.rs`, `src-tauri/src/phone_hub/overview.rs`; fixtures `src-tauri/tests/fixtures/phone_hub/{getprop,df,meminfo,wifi_status,wifi_unknown_ssid,telephony,power_on,power_off_locked,thermal}.txt`; Modify `src-tauri/src/lib.rs` (mod + commands, SPEC-27a block).
**Interfaces — Produces:** `pub struct PhoneOverview` (serde camelCase, fields exactly as design §7.1); `parse_getprop(&str) -> StaticInfo`, `parse_df_data(&str) -> Option<Bytes2>`, `parse_meminfo(&str) -> Option<Bytes2>`, `parse_wifi(&str) -> Option<WifiInfo>`, `parse_telephony(&str) -> Option<CellInfo>`, `parse_screen(power: &str, window: &str) -> Option<ScreenState>`, `parse_uptime(&str) -> Option<u64>`; reuse existing `parse_battery_info` from `commands.rs` (convert to the §7.1 battery shape).
- [ ] Step 1: tests per parser against each fixture (e.g. `wifi_unknown_ssid` → `ssid: None, signalLevel: Some(n)`; garbage input → `None`, never panic).
- [ ] Step 2: run `cargo test --manifest-path src-tauri/Cargo.toml phone_hub::overview` → FAIL.
- [ ] Step 3: implement parsers (line/regex scanning, tolerant to OEM ordering).
- [ ] Step 4: run → PASS. Step 5: commit.

### Task A2: Apps (labels + open app)
**Files:** Create `src-tauri/src/phone_hub/apps.rs`, fixture `scrcpy_list_apps.txt`.
**Interfaces — Produces:** `PhoneApp { package, label, system }`; `parse_scrcpy_app_list(&str) -> Vec<PhoneApp>`; `AppLabelCache` (`get(identity, package) -> Option<String>`, disk JSON in app data dir); commands `phone_list_apps(serial, refresh) -> Vec<PhoneApp>`, `phone_open_app(serial, package)` (reuse existing mirror launch path with the scrcpy start-app option; validate `package` against `^[A-Za-z0-9_.]+$`).
- [ ] Tests: parser (system/user split, labels with spaces/unicode), cache hit/miss, package validation rejects `;`/space.
- [ ] Run fail → implement → pass → commit.

### Task A3: Gallery (MediaStore)
**Files:** Create `src-tauri/src/phone_hub/media.rs`, fixtures `content_images.txt`, `content_videos.txt`.
**Interfaces — Produces:** `Album`, `MediaItem` (design §7.5); `parse_content_rows(&str, kind) -> Vec<MediaItem>`; commands `phone_list_albums`, `phone_list_media(serial, kind, album, offset, limit)` (newest first, `limit` ≤ 200), `phone_media_thumbnail(serial, id) -> Option<String>` (photos via existing photo cache/resize; videos `None`), `phone_open_media(serial, id)` (pull to size-capped cache, open with default app).
- [ ] Tests: `media_rows_with_quotes_and_spaces`, album aggregation counts, paging offset/limit, kind filter.
- [ ] Run fail → implement → pass → commit.

### Task A4: Messages aggregator
**Files:** Create `src-tauri/src/phone_hub/messages.rs`, fixtures `notif_messaging_style.txt`, `notif_group_chat.txt`, `notif_oem_variant.txt`.
**Interfaces — Consumes:** `PhoneNotification` from `phone_files.rs` (extend parser in a SPEC-27a block to expose messaging extras if needed). **Produces:** `Conversation`, `ChatMessage { sender, text, timestampMs }`; `MessageStore::ingest(serial, &[PhoneNotification], labels) -> Vec<String /*changed keys*/>`, `conversations(serial)`, `mark_read(serial, key)`, `clear(serial)`; commands `phone_get_conversations`, `phone_mark_conversation_read`; event `phone-messages-updated`.
- [ ] Tests: `dedupes_group_messages_with_unicode`, retention keeps last 200, `conversations_are_per_serial`, `clear` on disconnect empties, non-chat notifications ignored.
- [ ] Run fail → implement → pass → commit.

### Task A5: Poller + events
**Files:** Create `src-tauri/src/phone_hub/poller.rs`; Modify existing device-disconnect path (SPEC-27a block) to call `phone_hub::on_disconnect(serial)`.
**Interfaces — Produces:** commands `phone_hub_watch(serial, sections)`, `phone_hub_unwatch(serial)`, `phone_get_overview(serial, refresh)`; events `phone-overview`, `phone-notifications` (items include `appLabel`); setting `notificationPollSecs`. Pure scheduler `next_due(now, last, tier, watched) -> bool` for tests.
- [ ] Tests: tier schedule (static once, fast 30 s, slow 5 min, notifications 10 s/min 5), `watch_is_per_serial`, `poller_stops_and_clears_on_disconnect`, debug-mode poll counter increments.
- [ ] Run fail → implement → pass → commit.

### Task A6: Audio-only
**Files:** Create `src-tauri/src/phone_hub/audio_only.rs`.
**Interfaces — Produces:** `phone_audio_start(serial)`, `phone_audio_stop(serial)`, event `phone-audio-state { serial, playing }`; pure `can_start_audio_only(mirror_audio_active, already_playing) -> Result<(), &str>`.
- [ ] Tests: `audio_only_refused_when_mirror_audio_active`, idempotent start. Verify the exact scrcpy options with the bundled version manually (`--help`) and document them in the spec.
- [ ] Run fail → implement → pass → commit.

### Task A7: Docs + PR
Spec doc `docs/specs/SPEC-27-phone-hub.md` (design + contract), copy design to `docs/superpowers/specs/2026-11-16-spec27-phone-hub-design.md`, this plan to `docs/superpowers/plans/2026-11-16-spec27-phone-hub.md`, CONTEXT.md terms (Phone hub, poll tiers, conversation). Squash to one commit `feat(deck): phone hub data service (SPEC-27a)`, open PR.

## Part 27b — Phone UI (worker: hydrogen-medium, branch `feat/spec27b-phone-ui`) — starts after mockup approval

### Task B1: Types, service, store
**Files:** Create `src/types/phone.ts`, `src/services/phoneHubService.ts`, `src/stores/phoneHubStore.ts`, `src/stores/__tests__/phoneHubStore.test.ts`.
**Produces:** TS types mirroring §7 exactly; wrappers for every command/event; store reducers `applyOverview`, `applyNotifications`, `applyMessagesUpdated`, `setMutedPackages` (persisted), `isMuted(pkg)`.
- [ ] Tests for each reducer (null leaves render-safe, per-serial isolation, muted filter) → fail → implement → pass → commit.

### Task B2: Phone shell + Overview
**Files:** Modify `src/components/phone/PhoneView.tsx` (labelled vertical sidebar: Overview, Messages[unread], Notifications, Gallery, Files, Apps; header device selector + connection chip; calls `phone_hub_watch`/`unwatch`); Create `src/components/phone/OverviewView.tsx` (status cards, quick actions: Mirror, Audio only, Wi-Fi/BT/DND using existing toggle commands).
- [ ] Component tests: renders "—" for nulls, quick action disabled states, unwatch on unmount → implement → commit.

### Task B3: Notifications + Messages
**Files:** Modify `src/components/phone/NotificationsView.tsx` (app labels, per-app mute menu); Create `src/components/phone/MessagesView.tsx` (app → conversation list, thread view, unread, empty state explaining the capture limitation); Modify `src/App.tsx` (replace frontend notification polling with `phone-notifications` events; toasts respect muted packages and show `appLabel`).
- [ ] Tests: muted app excluded from list and toasts; conversation ordering newest first → implement → commit.

### Task B4: Gallery + Apps
**Files:** Create `src/components/phone/GalleryView.tsx` (replaces `PhotosView.tsx`; filter All/Photos/Videos, album picker, paged lazy grid with reserved tile size, video placeholder + duration, click opens), `src/components/phone/AppsView.tsx` (search, user/system toggle, open app).
- [ ] Tests: paging appends without duplicates, filter switches reset paging, app search → implement → commit.

### Task B5: Docs + PR
README Features + CONTEXT; squash `feat(deck): phone hub UI (SPEC-27b)`; PR with Before/After/Why table.

## Part 27c — Devices tidy (worker: hydrogen-medium, branch `feat/spec27c-devices-ui`) — starts after mockup approval

### Task C1: Split DeviceRack
**Files:** Split `src/components/deck/DeviceRack.tsx` (925 lines) into focused components under `src/components/deck/rack/` (device list item, connection controls, pairing entry, launch presets) without behaviour change.
- [ ] Existing tests stay green; add a render test per new component → commit.

### Task C2: Devices = connect & mirror
**Files:** `src/components/deck/StudioDeck.tsx`, `DeviceHeroCard.tsx`, `DevicesTab.tsx`, `DeviceRow.tsx`: phone condition reduced to chips (battery %, USB/wireless) using `phone-overview` data when available; detailed info links to Phone → Overview; timeline stays.
- [ ] Tests: chips render with/without overview data; "View phone details" navigates to Phone/Overview → commit.

### Task C3: PR
Squash `feat(deck): refocus Devices tab on connect and mirror (SPEC-27c)`; PR with Before/After/Why table.

## Integration (coordinator)
Merge 27a → rebase/merge 27c → rebase/merge 27b; combined test build; two-PC + phone manual test (wireless + USB, battery-drain sanity via debug poll counter); docs sync (§1.4); release.
