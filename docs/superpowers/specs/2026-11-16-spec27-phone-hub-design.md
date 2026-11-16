# SPEC-27 — Phone hub (design, draft for approval)

Date: 2026-11-16. Repo: AnyLeap (Tauri 2 + Rust + React/Tailwind, Windows-first). Phone side: Android over adb/scrcpy only — **no app on the phone**.

## 1. Intent
Make the PC the place where the user checks and controls the phone while working, so they pick the phone up less. Two tabs get clear, separate roles:
- **Devices** = "connect & mirror": phone list, pairing, connection status, mirroring presets.
- **Phone** = "everything about the phone's content and condition": overview, messages, notifications, gallery, files, apps, quick actions.

## 2. Goals
1. Phone **Overview**: battery (level, charging, temperature, health), model/brand/Android version/security patch, storage, RAM, Wi-Fi (name + signal), cellular (operator + signal), screen state, uptime; quick actions (mirror, audio-only, Wi-Fi/Bluetooth/DND toggles).
2. **App names instead of package ids** everywhere (notifications, messages, apps list), via the bundled scrcpy app list.
3. **Notifications**: app names, per-app filter (applies to the list AND to native Windows toasts).
4. **Messages**: chat history built from chat-style notifications, grouped app → conversation, deduplicated, **memory only**, cleared when the phone disconnects.
5. **Gallery**: photos + videos from all albums (MediaStore), filter All / Photos / Videos + album picker, lazy-loaded grid, click-to-open.
6. **Apps**: list of installed apps (name + package), search, one click opens a mirror of that app.
7. **Audio-only**: play the phone's audio on the PC without a mirror window.
8. **Devices tab tidy-up**: focus on connect & mirror; the 925-line `DeviceRack` split into smaller components; phone condition shown only as small chips (battery %, USB/wireless).

## 3. Non-goals
Replying to messages, notification images/rich content, app icons, typing into the phone without a mirror window, IMEI / phone number / location / call log / SMS database, root. (Reply + rich notifications belong to the future Android companion app.)

## 4. Anti-goals (failure even if it works)
- Noticeable extra battery drain or sluggish wireless adb from polling.
- Chat contents written to disk or to the debug log.
- Any regression in mirroring, launch presets, pairing or auto-reconnect.
- Generic "AI-slop" UI: gradients, decorative motion, icon-only navigation; must follow SPEC-26 Ink & Gold tokens and the emil-design-eng rules.

## 5. Constraints (verified in code)
- Today: `dumpsys battery` parser and `getprop ro.product.model` exist (`src-tauri/src/commands.rs`); notifications come from `dumpsys notification --noredact` (`src-tauri/src/phone_files.rs`), polled by the frontend in `App.tsx`, which also sends native toasts (title/body only, `app_label || package`); photos only from `DCIM/Camera` + `Pictures/Screenshots`; phone toggles exist but only in the command palette; Phone tab = icon-only segmented control (Files/Photos/Notifications) + device dropdown.
- Wireless adb: every query is a round-trip; keep polling tiered and pause it when nothing needs it.
- OEM `dumpsys` formats differ: parsers must be tolerant; unknown → `null` (UI shows "—"), never an error.
- Some Android versions redact the Wi-Fi name (`<unknown ssid>`) → show "Wi-Fi (name hidden)".
- Messages are only captured while connected and only if the notification is still present at poll time (documented limitation, shown in the empty state).

## 6. Approaches considered
- A. Incremental, one feature per PR — low risk but slow and visually inconsistent ("bolted on").
- **B. One spec, three parallel parts with a shared contract (chosen)** — 27a backend data service, 27b Phone UI, 27c Devices UI. Frontend parts touch only `src/**`, so no shared-backend-file conflicts (lesson from SPEC-25). Merge order 27a → 27c → 27b; one combined two-PC test.
- C. Two separate specs (Phone first, Devices later) — focused, but phone info is duplicated across tabs in the meantime.

## 7. Shared contract (all parts follow exactly)
Rust module: new `src-tauri/src/phone_hub/` (27a owns it). Tauri commands (JS args camelCase); events emitted to all windows.

### 7.1 Overview + poller
- `phone_hub_watch(serial: String, sections: Vec<String>)` — sections ⊆ `["overview","notifications"]`; the UI calls it when the Phone tab / hero chip needs data and `phone_hub_unwatch(serial)` when not. Notifications polling keeps running in the background while "desktop notifications" or Messages are enabled (they must not depend on the tab being open).
- Tiers: static (model, brand, Android, patch, RAM total, storage total) once per connection; fast (battery, Wi-Fi, cellular, screen) every 30 s while watched; slow (storage used, RAM used, uptime) on `phone_get_overview(serial, refresh: true)` and every 5 min while watched; notifications every 10 s (setting `notificationPollSecs`, min 5).
- `phone_get_overview(serial, refresh: bool) -> PhoneOverview`
- Event `phone-overview { serial, overview: PhoneOverview }`
- `PhoneOverview { model, brand, androidVersion, securityPatch, battery: { level, charging, tempC, health } | null, storage: { usedBytes, totalBytes } | null, ram: { usedBytes, totalBytes } | null, wifi: { ssid: string|null, signalLevel: 0-4|null } | null, cellular: { operator: string|null, signalLevel: 0-4|null } | null, screen: "on"|"off"|"locked"|null, uptimeSecs|null, updatedAtMs }` — every leaf may be null.

### 7.2 Apps
- `phone_list_apps(serial, refresh: bool) -> Vec<PhoneApp { package, label, system: bool }>` via the bundled scrcpy app listing; cached per device (disk cache keyed by the stable device identity from #18 — labels are not sensitive).
- `phone_open_app(serial, package)` — opens a mirror session that starts that app (reuse the existing launch path/presets).
- Notification and message payloads carry `appLabel` resolved from this cache (fallback: package id).

### 7.3 Notifications
- Event `phone-notifications { serial, items: Vec<PhoneNotification + appLabel> }` from the backend poller (replaces the frontend polling in `App.tsx`; the existing `get_device_notifications` command stays for manual refresh).
- Per-app filter is a persisted frontend setting `{ mutedPackages: string[] }` applied to the list and to toasts (toasts stay frontend-side).

### 7.4 Messages (memory only)
- Aggregator in 27a consumes every notification poll: chat-style notifications (MessagingStyle extras / conversation fields in the dump) → messages `{ sender, text, timestampMs }` keyed by `(package, conversationKey)`; dedupe by `(sender, text, timestampMs)`; keep the last 200 messages per conversation; nothing is written to disk or logs.
- `phone_get_conversations(serial) -> Vec<Conversation { package, appLabel, key, title, isGroup, messages, unread }>`
- `phone_mark_conversation_read(serial, key)`
- Event `phone-messages-updated { serial, conversationKey }`
- Cleared on device disconnect (and on app exit).

### 7.5 Gallery
- `phone_list_albums(serial) -> Vec<Album { name, photoCount, videoCount }>`
- `phone_list_media(serial, kind: "all"|"photo"|"video", album: Option<String>, offset: u32, limit: u32) -> { items: Vec<MediaItem { id, kind, name, album, path, dateTakenMs, sizeBytes, width, height, durationMs|null }>, total }` via `content query` on MediaStore images + video, newest first.
- `phone_media_thumbnail(serial, id) -> Option<String>` (cached local path; photos reuse the existing resize/cache path; videos may return `None` → UI placeholder with duration).
- `phone_open_media(serial, id)` — pull to cache and open with the default PC app; cache size-capped and cleaned like other caches.
- Paths come from MediaStore and must go through the existing `normalize_device_path` / quoting helpers.

### 7.6 Audio-only
- `phone_audio_start(serial)` / `phone_audio_stop(serial)` and event `phone-audio-state { serial, playing }` — plays phone audio on the PC without a visible mirror window, using bundled-scrcpy options verified to work with the bundled version.

### 7.7 Ownership
- 27a: `src-tauri/src/phone_hub/**`, additive registration in `lib.rs`, additive helpers in `phone_files.rs`/`commands.rs` inside `// --- SPEC-27a ---` blocks; parser fixtures (generic names only).
- 27b: `src/components/phone/**`, new store/service files for the contract, `App.tsx` changes for moving notification polling to events.
- 27c: `src/components/deck/**`, `src/components/DevicesTab.tsx`/`DeviceRow.tsx`.
- 27b and 27c must not edit each other's folders; shared types in `src/types/phone.ts` (27b owns, 27c imports).

## 8. UI (27b, 27c)
- Phone tab: labelled vertical sidebar (Overview, Messages [unread badge], Notifications, Gallery, Files, Apps), device selector + connection chip in the header. Overview = status cards + quick actions. Empty / loading / "—" states for every field. Lazy-loaded gallery grid with reserved space (no layout shift), click-to-play videos.
- Devices tab: connect & mirror focus; device list with small chips (battery %, USB/wireless); pairing; mirroring presets; activity timeline stays on Devices.
- Visual detail is the builder's within SPEC-26 tokens + emil-design-eng; mockups will be reviewed with the user before 27b/27c implementation starts (see open items).

## 9. Verification
Pure-function unit tests with fixtures for every parser (battery, getprop, df, meminfo, wifi, telephony, power/screen, uptime, scrcpy app list, MediaStore rows, messaging notifications incl. OEM variants and unknown formats), dedupe and retention of messages, poll tiers. Frontend reducer tests. All AGENTS.md §2 gates + `npm run check:tauri-versions`. Manual: two-PC + phone test (wireless and USB), battery-drain sanity check (poll counts per minute logged in debug mode).

## 10. Left to the builder
Exact dumpsys/content-query parsing strategy, cache locations and sizes, poll scheduler implementation, component breakdown and styling within the constraints above.
