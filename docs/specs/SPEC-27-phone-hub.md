# SPEC-27: Phone Hub

> **Status:** Approved
> **Scope:** `(deck)` — Backend phone-data service (`SPEC-27a`), Phone tab dashboard (`SPEC-27b`), and Devices tab connect-and-mirror tidy (`SPEC-27c`).

## 1. Summary

Turn the **Phone** tab into a full ADB-only phone companion dashboard (overview, memory-only messages, app-labelled notifications, all-album gallery, files, apps, and audio-only streaming) fed by a tiered backend service in `src-tauri/src/phone_hub/`, while keeping **Devices** focused on connection, pairing, and mirroring. No companion app is installed on the phone; all queries run over `adb` and the bundled `scrcpy`.

Design rationale and full task breakdown:
- Design: `docs/superpowers/specs/2026-11-16-spec27-phone-hub-design.md`
- Implementation plan: `docs/superpowers/plans/2026-11-16-spec27-phone-hub.md`

---

## 2. Verified Bundled Sidecar Options (`scrcpy 4.0`)

Verified directly against `src-tauri/binaries/scrcpy-x86_64-pc-windows-msvc.exe --version` (`scrcpy 4.0`) and `--help`:

- **Audio-only without a mirror window:** `--no-video --no-window`
  - `--no-video`: Disables video forwarding from the device.
  - `--no-window`: Disables the scrcpy window (implies `--no-video-playback`); audio forwarding remains active on the PC via SDL audio output.
- **Installed app listing:** `--list-apps`
  - Prints `[server] INFO: List of apps:` followed by ` * <label> <package>` (system apps) and ` - <label> <package>` (user apps).
- **Per-app mirror window launch:** `--new-display --start-app=<package>` (reuses existing `launch_app_window` / `build_app_window_args` path).

---

## 3. Shared Contract (§7 — Frozen Across 27a / 27b / 27c)

Rust module: `src-tauri/src/phone_hub/`. All Tauri command arguments and JSON payloads use **camelCase**; events are emitted globally to all windows.

### 7.1 Overview + Poller
- `phone_hub_watch(serial: String, sections: Vec<String>)` — `sections` ⊆ `["overview", "notifications"]`. Updates the watched sections for `serial` (passing `[]` unwatches). Notifications polling runs in the background while desktop notifications or Messages are enabled (independent of whether the Phone tab is open).
- `phone_hub_unwatch(serial: String)` — stops polling for `serial`.
- `phone_hub_set_settings(notificationPollSecs: u32)` — pushes the notification poll interval from the frontend store (default `10`, clamped to minimum `5` seconds). The backend does not read `settings.json` directly.
- Poll tiers:
  - **Static** (`model`, `brand`, `androidVersion`, `securityPatch`): once per connection.
  - **Fast** (`battery`, `wifi`, `cellular`, `screen`): every `30 s` while `"overview"` is watched.
  - **Slow** (`storage`, `ram`, `uptimeSecs`): on `phone_get_overview(serial, refresh: true)` and every `5 min` while `"overview"` is watched.
  - **Notifications**: every `notificationPollSecs` s (default `10`, min `5`) while `"notifications"` is watched.
- `phone_get_overview(serial: String, refresh: bool) -> PhoneOverview`
- Event `phone-overview`: `{ serial: string, overview: PhoneOverview }`
- `PhoneOverview` JSON shape (every leaf may be `null`):
  ```ts
  interface PhoneOverview {
    model: string | null;
    brand: string | null;
    androidVersion: string | null;
    securityPatch: string | null;
    battery: {
      level: number | null;
      charging: boolean | null;
      tempC: number | null;
      health: string | null;
    } | null;
    storage: { usedBytes: number; totalBytes: number } | null;
    ram: { usedBytes: number; totalBytes: number } | null;
    wifi: { ssid: string | null; signalLevel: number | null } | null; // signalLevel: 0..4
    cellular: { operator: string | null; signalLevel: number | null } | null; // signalLevel: 0..4
    screen: "on" | "off" | "locked" | null;
    uptimeSecs: number | null;
    updatedAtMs: number;
  }
  ```

### 7.2 Apps
- `phone_list_apps(serial: String, refresh: bool) -> Vec<PhoneApp>` via bundled `scrcpy --list-apps`, persisted per stable device identity (`hw_serial`, fallback `serial`) in `phone_hub_apps.json` under the app data dir:
  ```ts
  interface PhoneApp {
    package: string;
    label: string;
    system: boolean;
  }
  ```
- `phone_open_app(serial: String, package: String) -> SessionInfo` — validates `package` and launches an app mirror window via the existing `launch_app_window` path (`--new-display --start-app=<package>`).
- Notification and message payloads carry `appLabel` resolved from the app label cache (fallback: `package`).

### 7.3 Notifications
- Event `phone-notifications`: `{ serial: string, items: PhoneHubNotification[] }` emitted by the backend poller:
  ```ts
  interface PhoneHubNotification {
    key: string;
    package: string;
    appLabel: string;
    title: string;
    text: string;
    postedUnix: number;
  }
  ```
  *(Note: Existing `get_device_notifications` and its snake_case `PhoneNotification` stay unchanged for backwards compatibility.)*
- Per-app mute filtering (`{ mutedPackages: string[] }`) is persisted and applied frontend-side for both the notification list and native Windows toasts.

### 7.4 Messages (Memory-Only)
- Consumes every notification poll: extracts `MessagingStyle` / `category=msg` notifications into per-serial conversations keyed by `<package>::<conversationKey>`, deduplicated by `(sender, text, timestampMs)`, retaining the last `200` messages per conversation. Never written to disk or logs. Cleared on device disconnect and app exit.
- `phone_get_conversations(serial: String) -> Vec<Conversation>`:
  ```ts
  interface ChatMessage {
    sender: string;
    text: string;
    timestampMs: number;
  }
  interface Conversation {
    package: string;
    appLabel: string;
    key: string;
    title: string;
    isGroup: boolean;
    messages: ChatMessage[];
    unread: number;
  }
  ```
- `phone_mark_conversation_read(serial: String, key: String)`
- Event `phone-messages-updated`: `{ serial: string, conversationKey: string }`

### 7.5 Gallery
- `phone_list_albums(serial: String) -> Vec<Album>`:
  ```ts
  interface Album {
    name: string;
    photoCount: number;
    videoCount: number;
  }
  ```
- `phone_list_media(serial: String, kind: "all" | "photo" | "video", album: string | null, offset: number, limit: number) -> MediaPage`:
  ```ts
  interface MediaItem {
    id: string;
    kind: "photo" | "video";
    name: string;
    album: string;
    path: string;
    dateTakenMs: number;
    sizeBytes: number;
    width: number;
    height: number;
    durationMs: number | null;
  }
  interface MediaPage {
    items: MediaItem[];
    total: number;
  }
  ```
  Queries Android MediaStore (`content query` on `images/media` and `video/media`), sorted newest-first, `limit` capped at `200`, paths validated via `normalize_device_path`.
- `phone_media_thumbnail(serial: String, id: String) -> string | null` — produces a JPEG thumbnail (longest side ≤ 256 px, quality 80) cached at `<app cache>/phone_hub_thumbs/<serial-hash>/<id>-<date_modified>.jpg` (100 MB LRU cap), extracting embedded EXIF/JFIF thumbnails from a ~64 KiB `adb exec-out` head read before falling back to full pull + downscale with max 4 concurrent device reads; videos return `null`.
- `phone_open_media(serial: String, id: String)` — pulls to a size-capped cache (`phone_hub_media/`, 500 MB LRU cap) and opens with the default PC application. Enforces a kind-specific lowercase extension allow-list (after trimming trailing dots/spaces, judging double extensions by the last one): photos (`jpg`, `jpeg`, `png`, `gif`, `webp`, `heic`, `heif`, `bmp`, `avif`), videos (`mp4`, `m4v`, `mov`, `3gp`, `mkv`, `webm`). Refuses any other extension or kind mismatch with `"Unsupported media type"` and always writes the cached file with the allow-listed extension.

### 7.6 Audio-Only
- `phone_audio_start(serial: String)` — spawns `scrcpy --serial <serial> --no-video --no-window`. Refused if a mirror session with audio is already active for `serial`; idempotent if audio-only is already playing.
- `phone_audio_stop(serial: String)` — kills the audio-only process (idempotent).
- Event `phone-audio-state`: `{ serial: string, playing: boolean }`.
