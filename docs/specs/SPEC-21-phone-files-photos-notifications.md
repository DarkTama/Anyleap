# SPEC-21: Phone Files, Photos & Read-Only Notifications

> **Status:** Implemented (merged to main 2026-11-13; ships in next release) | Target Version: `v1.2.0`
> Scope: `(deck)`. adb only; no companion app.

## 1. Overview
- **Files** — browse shared storage on the phone, download to PC, upload from PC, delete.
- **Photos** — grid of camera and screenshot images, newest first, open/download.
- **Notifications** — read-only list of the phone's active notifications.

New shell tab **Phone** (`ShellTab` gains `"phone"`) with three sub-views: Files · Photos · Notifications, scoped to the selected device.

## 2. Security rules (apply to every command here)
- All device paths go through `sh_quote` (single-quote escaping for the Android shell). If SPEC-20 has not landed, add the helper here in the same form: `pub(crate) fn sh_quote(s: &str) -> String`.
- Device paths are normalised and must stay under `/storage/emulated/0` (reject `..` segments after normalisation, reject other roots).
- PC-side destination names use only the final path component of the device file; reject empty, `.`, `..`, and Windows reserved names; on collision append ` (n)`.

## 3. Files
**Backend** (`src-tauri/src/phone_files.rs`, commands registered in `lib.rs`):
- `list_device_dir(serial, path) -> Vec<DeviceEntry { name, path, is_dir, size, modified_unix }>`
  via `adb shell stat -c '%F|%s|%Y|%n' <quoted path>/*` (toybox `stat`). Parse line-wise; tolerate the literal `*` result of an empty directory and permission-denied lines (skip them). Sort dirs first, then name.
- `pull_device_file(serial, device_path, dest_dir) -> String` (final PC path) via `adb pull`.
- `push_to_device(serial, local_paths: Vec<String>, device_dir)` via `adb push`.
- `delete_device_path(serial, device_path)` via `rm -r` (files and folders). Frontend confirms first.

**UI** `src/components/phone/FilesView.tsx`: breadcrumb path bar, quick roots (Internal storage, Download, DCIM, Documents), list with name / size / modified, double-click to enter folder, multi-select → Download (folder picker via the existing dialog plugin) / Delete, Upload button + drag-and-drop onto the list. Show a busy indicator per operation; adb gives no progress, so no progress bar.

## 4. Photos
- `list_device_photos(serial, offset, limit) -> Vec<DeviceEntry>`: entries from `DCIM/Camera` and `Pictures/Screenshots` (image extensions `jpg jpeg png webp heic`), merged, sorted by `modified_unix` desc, paginated (page size 60).
- `cache_device_photo(serial, device_path) -> String`: `adb pull` into `<app cache dir>/photos/<serial>/<hash(device_path+modified)>.<ext>`; return the local path. The UI shows it via `convertFileSrc` with `object-fit: cover`.
- Cache cap 500 MB per app: after each pull, delete oldest files by access time until under the cap. `ponytail:` note: full images, not thumbnails — add on-device thumbnailing only if grid loading is measurably slow.
- **UI** `PhotosView.tsx`: virtualised-or-paginated grid ("Load more" button is fine), lazy-load images when visible (`IntersectionObserver`), click → lightbox with Download and "Open folder".
- Videos are out of scope.

## 5. Notifications (read-only)
- `get_device_notifications(serial) -> Vec<PhoneNotification { key, package, app_label?, title, text, posted_unix }>`
  via `adb shell dumpsys notification --noredact`. Parse `NotificationRecord` blocks: `pkg=`, `key=`, `android.title=`, `android.text=`, `when=`/`postTime`. Skip ongoing/group-summary records (`flags` with `FLAG_ONGOING_EVENT` 0x2 or `FLAG_GROUP_SUMMARY` 0x200). Unknown formats return an empty list, never an error.
- Frontend polls every 10 s only while the Notifications view is open **or** the "Desktop pop-ups" setting is on; pop-ups use the notification plugin (add it if SPEC-20 has not) for keys not seen before. Default: pop-ups **off**.
- Data stays in memory; never persisted, never logged.
- **UI** `NotificationsView.tsx`: list grouped by app, newest first, "Copy" button on each (useful for one-time codes), empty state explains "Read-only — replies need the phone".

## 6. Verification
Automated (`cargo test`, `npm test`):
1. `stat` output parser: files, dirs, names with `|` and spaces, empty dir `*`, permission-denied lines.
2. Path normaliser: traversal, other roots, trailing slashes.
3. PC destination-name sanitiser.
4. `dumpsys notification` parser with at least two fixtures (Android 12-ish and 14+). Capture real output from a device if one is attached (`adb shell dumpsys notification --noredact`), redact personal text, and store under `src-tauri/tests/fixtures/`; note the source Android version in the file name.
5. Photo cache eviction as a pure function over `(path, size, atime)` lists.
Plus AGENTS.md gates: `npm test`, `cargo check`, `npm run build`.

Manual: browse to Download, pull and push a file, delete a test folder, scroll photos past two pages, see a fresh notification appear within ~10 s.

## 7. Out of scope
Replying to or dismissing notifications, SMS, resume of interrupted transfers, synced folders, automatic photo backup.
