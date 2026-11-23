# SPEC-31: Default Transfer Folders

> **Status:** Implemented (2026-11-23)
> **Scope:** Phone Hub downloads/uploads (`src/components/phone/`, `src-tauri/src/phone_files.rs`), phone mirroring captures & drops (`src/App.tsx`, `src/components/EmbeddedMirrorWindow.tsx`, `src-tauri/src/commands.rs`), multi-PC KVM file receives (`src-tauri/src/kvm/`, `src/components/computers/`, `src/components/desktop/TransferBarWindow.tsx`), and Settings (`src/components/settings/`, `src/lib/settingsStore.ts`).
> **Related:** SPEC-21 (Phone Files & Photos), SPEC-25a (Files Across PCs), SPEC-25c (Open on Linked PC), SPEC-27 (Phone Hub), SPEC-28 (Settings Redesign).
> **Plan:** `docs/superpowers/plans/2026-11-23-spec31-default-transfer-folders.md`

---

## 1. Problem

Receiving or sending files across AnyLeap today has inconsistent, fragmented destination behaviour:

1. **Repeated folder prompts on Phone Hub downloads.**
   - Phone Hub **Gallery** (`src/components/phone/GalleryView.tsx:504-549`) and **Files** (`src/components/phone/FilesView.tsx:138-162`) both open `openDialog({ directory: true })` on every single download click. Gallery remembers the last folder only to pre-fill the next prompt (`phoneHubLastGalleryDownloadDir` in `src/stores/phoneHubStore.ts:11,171-192`); Files does not even pass `defaultPath`.
2. **Broken / unconfigured PC screenshot saving.**
   - Command Palette **Take screenshot** (`src/App.tsx:759-777`) receives PNG bytes from `take_screenshot` (`src-tauri/src/commands.rs:2362-2380`) and triggers an in-webview `<a download>` Blob click, relying on WebView2's default download handling with no configured folder, no returned path, and no "Show in folder" action.
3. **Hardcoded KVM destination and stale capture at startup.**
   - `get_download_dir()` (`src-tauri/src/kvm/commands.rs:112-120`) hardcodes `%USERPROFILE%\Downloads\AnyLeap_Transfers` for `StreamHeader::File` (`kvm/commands.rs:251`, `kvm/files.rs:520-547`), `DownloadsFallback` (`kvm/staging.rs:168-174,507`), and error/fallback notices (`kvm/commands.rs:451,812,3845`). That path is also duplicated in UI copy (`src/components/computers/TopologyCanvas.tsx:477`, `src/components/settings/registry.ts:100`).
   - `start_kvm_server` (`kvm/commands.rs:1817`, `kvm/server.rs:472`) and `KvmClientHandle::connect` (`kvm/commands.rs:2852`, `kvm/client.rs:159,223`) capture `download_dir` once at startup, so changing a folder setting mid-session would not take effect on `StreamHeader::File`.
4. **Hardcoded PC -> Phone upload targets.**
   - Mirror clipboard image paste (`src/components/EmbeddedMirrorWindow.tsx:171`, `src-tauri/src/commands.rs:2782-2845`) and the dormant `handle_dropped_files` command (`src-tauri/src/commands.rs:2227-2274`, wrapped in `src/lib/tauri.ts:126-127`) hardcode `/sdcard/Download/`.
   - `build_scrcpy_args` (`src-tauri/src/commands.rs:321-380`) passes no `--push-target`, leaving OS drops onto the mirror window to `scrcpy`'s built-in `/sdcard/Download/`.
5. **Missing "Show in folder" on almost all completed transfers.**
   - Only screen recordings offer `"Open recording folder"` via `revealItemInDir(savedPath)` (`src/components/ControlBar.tsx:27,194-198`). Gallery, Files, Command Palette screenshots, the KVM Transfer Bar (`src/components/desktop/TransferBarWindow.tsx:235-298`), and the Computers activity feed (`src/components/computers/ComputersView.tsx:556-560`) lack a "Show in folder" action.

---

## 2. Goals

- **G1. Zero-prompt default downloads.** Phone Hub Gallery/Files downloads, Command Palette PC screenshots, and KVM disk-saved receives save directly to one configured PC receive folder without prompting each time.
- **G2. Optional per-transfer prompt switch.** An **"Ask where to save each time"** toggle (default **off**) restores an OS folder/save dialog for user-initiated PC downloads when turned on.
- **G3. Configurable phone upload folder.** One **"Save files sent to phone in"** setting (default `/sdcard/Download`) controls where files dropped onto the mirror window and pasted clipboard images land on the phone.
- **G4. Predictable collision and failure handling.** Name collisions auto-rename (`name (1).ext`, `name (2).ext`); missing or unwritable PC folders fall back cleanly to the default folder (or OS temp) and notify the user without dropping the transfer.
- **G5. "Show in folder" everywhere.** Completed PC-side transfers surface a **"Show in folder"** action using `revealItemInDir`.
- **G6. SPEC-28 Settings IA compliance.** All new controls have a single editor home in **Settings → Clipboard & files**, follow SPEC-28 §8.5 copy rules, and contain zero third-party product names (`AGENTS.md` §1.3).

## 3. Non-goals

- **No per-source PC folder overrides (YAGNI).** One PC receive folder serves Phone Hub, screenshots, and KVM receives; adding separate pickers per source adds clutter with no researched need.
- **Screen recordings stay in `Videos\AnyLeap`.** `build_recording_path` (`src-tauri/src/commands.rs:3915-3938`) already saves MP4 recordings without prompting to `%USERPROFILE%\Videos\AnyLeap` (matching the OS Videos library) and already has `"Open recording folder"` (`src/components/ControlBar.tsx:196-197`). Moving multi-hundred-MB recordings into Downloads is out of scope.
- **Ephemeral staging & internal caches do not move.**
  - KVM clipboard/drag staging (`%LOCALAPPDATA%\AnyLeap\staging\<id>`, `src-tauri/src/kvm/staging.rs:177-181`), `"Open on Linked PC"` temp copies (`staging\open-<id>`, `src-tauri/src/kvm/open_on_linked.rs:19-48`), and Phone Hub media/thumbnail caches (`app_cache_dir()`, `src-tauri/src/phone_hub/media.rs:559-563`) remain internal caches.
- **No change to Phone Hub Files manual browsing uploads.** When a user browses to a specific folder in Phone Hub → **Files** and uploads or drops into that list, uploading into the open folder (`currentPath`) is expected file-manager behaviour.
- **No drag-out-of-mirror implementation.** Not implemented today (`EmbeddedMirrorWindow.tsx`, `src-tauri/src/embed.rs`); out of scope.
- **No migration of `localStorage["anyleap_kvm_settings"]` to `settings.json`.** Noted as a separate SPEC-28 follow-up; SPEC-31 stores its cross-cutting settings in `settings.json` directly.
- **No ADR.** Preferences in `settings.json` plus a Rust path-resolver module are standard, localized, and easily reversible without protocol changes.

---

## 4. Research: Every File-Transfer Flow Today

### 4.1 Phone → PC

| Flow | Trigger & Code Path | Current Destination Behaviour | Dialog? |
|---|---|---|---|
| **Phone Hub Gallery download** | Lightbox **Download** (`GalleryView.tsx:904`) or multi-select **Download N** (`:624`) → `handleDownloadItems` (`:504-549`) → `pullDeviceFile` (`phone_files.rs:922-953`) | Prompts every time; pre-fills `defaultPath` from `lastGalleryDownloadDir` (`phoneHubStore.ts:11,171-192`) or OS `downloadDir()` (`GalleryView.tsx:506-514`). Auto-renames `name (n).ext` via `unique_dest_path` (`phone_files.rs:128-152`). No "Show in folder". | **Yes (every time)** |
| **Phone Hub Files download** | **Download (N)** (`FilesView.tsx:258-266`) → `handleDownloadSelected` (`:138-162`) → `pullDeviceFile` (`phone_files.rs:922-953`) | Prompts every time with `openDialog({ directory: true })` (`FilesView.tsx:141`); does not pass `defaultPath` or remember last folder. Auto-renames via `unique_dest_path`. No "Show in folder". | **Yes (every time)** |
| **Mirror screenshot (Command Palette)** | **Take screenshot** (`commandPalette.ts:240-245`) → `App.tsx:759-777` → `take_screenshot` (`commands.rs:2362-2380`) | Triggers WebView2 `<a download="anyleap-...">` Blob URL (`App.tsx:763-769`). Unconfigured WebView2 download folder; no returned path; no "Show in folder". | **WebView2-dependent** |
| **Mirror screenshot (Control Bar)** | **Screenshot** button (`ControlBar.tsx:267-270`) | Sends `KEYCODE.SCREENSHOT` (`120`) to Android; saved on the phone (`/sdcard/Pictures/Screenshots/`), not on PC. | No (on-phone) |
| **Screen recording** | Control Bar **Record screen** (`ControlBar.tsx:185-203`) or palette (`App.tsx:734-737`) → `start_recording` (`commands.rs:3941-3990`) | Fixed `%USERPROFILE%\Videos\AnyLeap\<model>-YYYYMMDD-HHMMSS.mp4` (`commands.rs:3915-3938`). Stop toast offers `"Open recording folder"` via `revealItemInDir` (`ControlBar.tsx:194-198`). | **No (fixed)** |
| **Camera Studio capture** | `CameraLaunchDialog.tsx`, `build_scrcpy_camera_args` (`commands.rs:382-440`) | No still-photo capture flow exists. Recording an active camera session (`commands.rs:3948-3951`) uses `build_recording_path` (`Videos\AnyLeap`). | **N/A / Fixed** |
| **Drag out of mirror** | — | Not implemented in `EmbeddedMirrorWindow.tsx` or `embed.rs`. | N/A |
| **Gallery open / thumbs (internal cache)** | `phone_open_media` (`phone_hub/media.rs:546-592`), `phone_media_thumbnail` (`SPEC-27 §7.5`) | Internal LRU caches under `app_cache_dir()` (`phone_hub_media/`, 500 MB; `phone_hub_thumbs/`, 100 MB). Not user downloads. | No (cache) |

### 4.2 PC → Phone

| Flow | Trigger & Code Path | Current Destination Behaviour | Dialog? |
|---|---|---|---|
| **Phone Hub Files upload & drag-drop** | **Upload** button (`FilesView.tsx:164-169`) or OS drop on file list (`FilesView.tsx:117-136`) → `handleUploadPaths` (`:92-114`) → `pushToDevice` (`phone_files.rs:956-984`) | **Upload** opens a local PC file picker (`openDialog({ multiple: true })`, `:165`). Pushes via `adb push` into `currentPath` (`FilesView.tsx:99`), which defaults to `/storage/emulated/0` on open (`:23,59,89`). | PC file picker only |
| **Drag & drop onto mirror window** | OS drop onto scrcpy HWND (`commands.rs:321-380`) + dormant `handle_dropped_files` (`commands.rs:2227-2274`, `tauri.ts:126-127`) | `build_scrcpy_args` passes no `--push-target`, so scrcpy pushes non-APKs to `/sdcard/Download/` and installs `.apk`. `handle_dropped_files` (currently zero frontend callers) hardcodes `/sdcard/Download/` (`commands.rs:2258`). | **No (hardcoded)** |
| **Clipboard image paste onto mirror** | `Ctrl+V` in `EmbeddedMirrorWindow.tsx:160-190` → `pushClipboardImage` (`commands.rs:2782-2845`) | Writes temp PNG, runs `adb push` to hardcoded `/sdcard/Download/paste_<ms>.png` (`commands.rs:2804`), runs media scanner broadcast. | **No (hardcoded)** |

### 4.3 Multi-PC KVM (Receiving from Peer PC)

| Flow | Trigger & Code Path | Current Destination Behaviour | Dialog? |
|---|---|---|---|
| **Clipboard file copy-paste (`<= stagingLimitMb`)** | `kvm/clipboard.rs:439-495` → `kvm/commands.rs:721-840,975-1039` → `kvm/staging.rs:487-622` | Streams to staging `%LOCALAPPDATA%\AnyLeap\staging\<offer_id>\` (`staging.rs:177-181`), sets `CF_HDROP` on clipboard (`kvm/commands.rs:1002`) so user pastes into target app/folder. Over `clipPrefetchLimitMb` (100 MB) waits in Transfer Bar for **Fetch** (`TransferBarWindow.tsx:215-221`, `kvm_fetch_clip` `kvm/commands.rs:3817`). | **No (clipboard)** |
| **Clipboard file copy-paste (`> stagingLimitMb`, `DownloadsFallback`)** | `choose_transfer_destination` (`staging.rs:168-174`) → `kvm/commands.rs:732,808-814,3824,3841-3848` | Bypasses staging/clipboard; saves directly to hardcoded `get_download_dir()` = `%USERPROFILE%\Downloads\AnyLeap_Transfers` (`kvm/commands.rs:112-120`). Top-level collisions auto-rename via `unique_path` (`staging.rs:513-529`, `files.rs:366-381`). Notice emitted; `savedPath` in `kvm-activity` only (`ComputersView.tsx:556-560`). | **No (hardcoded)** |
| **Cross-PC drag & drop (`<= stagingLimitMb`, Experimental)** | `kvm/dragdrop.rs` → `kvm/commands.rs:381-509,868-974` | Pre-stages in `%LOCALAPPDATA%\AnyLeap\staging\<drag_id>\`, then runs target OLE `DoDragDrop` onto cursor target (`kvm/commands.rs:945`). | **No (OLE drop)** |
| **Cross-PC drag & drop (`> stagingLimitMb`, `DownloadsFallback`)** | `kvm/commands.rs:402,437-454,893-903` | Skips `DoDragDrop`; saves directly to hardcoded `%USERPROFILE%\Downloads\AnyLeap_Transfers` with `unique_path` auto-rename + notice. | **No (hardcoded)** |
| **Direct drop onto Computers layout canvas** | `TopologyCanvas.tsx:172-203,470-480` → `kvmStore.ts:1007` → `kvm_send_file` (`kvm/commands.rs:3258-3273`) → `StreamHeader::File` (`kvm/files.rs:520-547,813-834`) | Saves directly to `dir` captured **once at server/client startup** from `get_download_dir()` (`kvm/commands.rs:1817,2852`; `kvm/client.rs:159,223`; `kvm/server.rs:472`). Auto-renames via `unique_path` (`files.rs:529`). Emits `kvm-activity` with `savedPath` (`kvm/commands.rs:251`). | **No (hardcoded at start)** |
| **"Open on Linked PC"** | Explorer menu / CLI `--open-on-linked` (`kvm/open_on_linked.rs:778-860`) → `StreamHeader::OpenFile` (`kvm/files.rs:560-717`) | Streams to `%LOCALAPPDATA%\AnyLeap\staging\open-<transfer_id>\` (`open_on_linked.rs:19-48`), opens with `ShellExecuteW("open")` or reveals blocked executables (`:681-715`), swept after 24 h (`OPEN_TEMP_MAX_AGE`, `:17`). | **No (temp staging)** |

### 4.4 Existing Settings Keys

| Store | Key | Current Purpose | Source |
|---|---|---|---|
| `settings.json` | `appPrefs` | `minimizeToTrayOnClose`, `checkUpdates`, `desktopNotifications`, `batteryAlerts` | `src/lib/settingsStore.ts:11-23` |
| `settings.json` | `modeDefaults` | Per-mode mirroring defaults (`SPEC-28`) | `src/lib/settingsStore.ts:114-143` |
| `settings.json` | `phoneHubLastGalleryDownloadDir` | Last picked directory for Gallery's folder picker `defaultPath` | `src/stores/phoneHubStore.ts:11,40,171-192` |
| `localStorage["anyleap_kvm_settings"]` | `continuityStagingLimitMb` (2048), `continuityClipPrefetchLimitMb` (100), `continuityFileClipboard` (true), `continuityDragDropExperimental` (false), `continuityOpenOnLinkedMenu` (true) | Continuity limits and feature toggles; **no folder keys exist** | `src/types/kvm.ts:126-151`, `src/stores/kvmStore.ts:125-147,241-257` |

---

## 5. Settings & Destination Model

### 5.1 Decisions & Rationale

1. **PC receive folder (`receiveDir`): default `<Downloads>\AnyLeap`.**
   - Resolved via Tauri's OS downloads directory (`app.path().download_dir()`, falling back to `$USERPROFILE\Downloads` / `$HOME\Downloads`) joined with `AnyLeap`.
   - **Why a subfolder instead of bare `Downloads`:** Bare `Downloads` mixes phone gallery batches, PNG screenshots, and large cross-PC drops with browser downloads. An `AnyLeap` subfolder keeps `Downloads` clean, matches the existing `Videos\AnyLeap` recording convention, and simplifies the old `Downloads\AnyLeap_Transfers` name into one unified folder users can easily change to bare `Downloads` (or any drive/folder) if they prefer.
2. **One PC receive folder (no per-source overrides).**
   - Following SPEC-28 minimalism and YAGNI, Phone Hub downloads, Command Palette PC screenshots, and KVM disk-saved transfers share one **Save received files to** setting.
3. **"Ask where to save each time" (`askWhereToSave`): default `false`.**
   - When **off** (default), user-initiated PC downloads (Phone Hub Gallery, Phone Hub Files, Command Palette screenshot) write directly into `receiveDir` with zero dialogs.
   - When **on**, user-initiated UI downloads prompt with the OS picker pre-seeded at the resolved `receiveDir` (`openDialog({ directory: true })` for Gallery/Files; `saveDialog` for single screenshot). Picking a one-off folder does **not** mutate `receiveDir`.
   - **Network/background KVM receives** (`StreamHeader::File`, `DownloadsFallback` for `drag`/`clip`) never block on a modal OS picker—they always save to `receiveDir` so peer QUIC streams never stall or time out.
4. **Phone upload folder (`phoneUploadDir`): default `/sdcard/Download`.**
   - Controls PC → Phone drops onto the mirror window (`scrcpy --push-target=<dir>` and `handle_dropped_files`) and clipboard image paste onto the mirror (`push_clipboard_image`).
   - Preserves Phone Hub **Files** browser uploads to `currentPath` (§3), and updates `FilesView` with a quick-root / status cue while leaving its initial path or quick navigation intact.
   - Validated and canonicalized so `/sdcard` and `/storage/emulated/0` forms are both accepted and normalized cleanly without traversal (`..`), control chars, or shell metacharacters.

### 5.2 Persisted Shape (`settings.json`)

New key `transferFolders` in `settings.json` (`src/lib/settingsStore.ts`), owned by a pure module `src/lib/transferFolders.ts`:

```ts
export interface TransferFoldersSettings {
  /** null = use OS default (<Downloads>/AnyLeap). Non-null = user-chosen absolute path. */
  receiveDir: string | null;
  /** Default false. Prompt on user-initiated PC downloads (Phone Hub, screenshot). */
  askWhereToSave: boolean;
  /** Android destination for mirror drops and pasted images. Default "/sdcard/Download". */
  phoneUploadDir: string;
}

export const DEFAULT_TRANSFER_FOLDERS: TransferFoldersSettings = {
  receiveDir: null,
  askWhereToSave: false,
  phoneUploadDir: "/sdcard/Download",
};
```

**Why `receiveDir: null` for default:** Storing `null` instead of expanding `C:\Users\<user>\Downloads\AnyLeap` on first boot lets the default follow OS profile moves / localization cleanly and lets Settings show a clear **Reset to default** action whenever `receiveDir !== null`.

### 5.3 Rust Shared Path Resolver & Live Sync

Introduce `src-tauri/src/transfer_paths.rs` as the single backend source of truth (replacing duplicated `get_download_dir`, `unique_path` / `unique_dest_path` logic, and hardcoded `/sdcard/Download`):

- **Process-wide live state (`RwLock`)**:
  - `TRANSFER_CONFIG: OnceLock<RwLock<TransferConfig>>` holding `{ default_receive_dir: PathBuf, configured_receive_dir: Option<PathBuf>, phone_upload_dir: String }`.
  - Initialized at Tauri `.setup()` using `app.path().download_dir()` (fallback `$USERPROFILE/Downloads` / `$HOME/Downloads` / `temp_dir`) + `"AnyLeap"`.
  - Updated at frontend app startup and on every Settings edit via Tauri command `set_transfer_folders(receive_dir: Option<String>, phone_upload_dir: String)` (plus `get_default_transfer_folders()` returning `{ defaultReceiveDir, defaultPhoneUploadDir }`).
  - KVM server (`kvm/server.rs`), KVM client (`kvm/client.rs`), `StreamHeader::File` (`kvm/files.rs`), `DownloadsFallback` (`kvm/commands.rs`), `push_clipboard_image`, `handle_dropped_files`, `save_screenshot`, and `build_scrcpy_args` all read the live resolver at transfer/launch time—fixing the stale startup capture in `client.rs` / `server.rs`.

### 5.4 Edge-Case Behaviour

1. **Name collisions (`name (1).ext`)**:
   - Unified `unique_dest_path(dir: &Path, filename: &str) -> PathBuf` in `transfer_paths.rs` (consolidating `phone_files.rs:128-152` and `kvm/files.rs:366-381`).
   - If `<dir>/report.pdf` exists, try `report (1).pdf`, `report (2).pdf`, …; for extensionless files (`README`), `README (1)`.
   - Top-level directories in KVM `DownloadsFallback` continue remapping via `top_name_map` (`staging.rs:513-529`) using `unique_dest_path`, so `Photos` becomes `Photos (1)`.
   - Phone-side `adb push` keeps standard `adb push` overwrite semantics (matches `scrcpy` native drop and `FilesView`).
2. **Missing or unwritable PC folder (fall back + notice)**:
   - On every PC-side receive (via `prepare_receive_dir()` in `transfer_paths.rs`):
     1. Take `configured = configured_receive_dir.unwrap_or(default_receive_dir)`.
     2. Attempt `create_dir_all(&configured)` and verify writability (create & remove a short-lived `.anyleap-write-test-<pid>` probe or handle `create_dir_all`/file-create error).
     3. If `configured` fails (disconnected external drive, missing volume, permission denied, read-only path), fall back in order:
        `default_receive_dir` (`<Downloads>\AnyLeap`) → OS `download_dir` → `std::env::temp_dir().join("AnyLeap")`.
     4. Return `ResolvedReceiveDir { path, fallback_from: Option<PathBuf> }`.
     5. Whenever `fallback_from` is `Some(bad_path)`, emit a visible user notice (in-app status/toast for Phone Hub & screenshot; `kvm-continuity-notice` for KVM):
        `"Could not save to \"<bad_path>\" — saved to \"<actual_path>\" instead."`
     6. **Never overwrite** the saved `receiveDir` setting on a transient fallback (e.g., an unplugged USB drive may be reconnected later).
3. **Missing / invalid phone upload folder**:
   - `normalize_phone_upload_dir(raw: &str) -> Result<String, String>` accepts `/sdcard` or `/storage/emulated/0` prefixes (rejecting empty, `..`, `\0`, `\r`, `\n`, quotes, and non-shared-storage roots), trims trailing slashes, and ensures the directory exists before `adb push` via `adb shell mkdir -p <sh_quote(dir)>` (non-fatal if `scrcpy --push-target` handles creation, explicit for `adb push` flows).
   - If the user enters an invalid path in Settings, inline validation blocks saving with a clear message: `"Enter a folder inside /sdcard or /storage/emulated/0 (for example /sdcard/Download)."`
4. **"Show in folder" after transfer**:
   - Every completed PC-side transfer returns/emits its concrete `savedPath: string` (full file path for single-file pulls, screenshots, and `StreamHeader::File`; first/top-level saved path or folder for multi-item pulls and `DownloadsFallback`).
   - UI calls `revealItemInDir(savedPath)` (`@tauri-apps/plugin-opener`, already permitted in `src-tauri/capabilities/default.json:45`).
   - Surfaces:
     - **Phone Hub Gallery** completion banner (`GalleryView.tsx:661-679`): **"Show in folder"** button.
     - **Phone Hub Files** completion banner (`FilesView.tsx:342-346`): **"Show in folder"** button.
     - **Command Palette Screenshot** toast (`App.tsx:759-777`): **"Show in folder"** button.
     - **KVM Transfer Bar** (`TransferBarWindow.tsx:235-298`): completed (`status === "done"`) items carrying `savedPath` show a **"Show in folder"** button and linger for `4500 ms` (`TRANSFER_SAVED_LINGER_MS`) instead of `1500 ms`.
     - **Computers Activity Feed** (`ComputersView.tsx:556-560`): items with `savedPath` show a **"Show in folder"** button beside `"Saved: <path>"`.
     - **Control Bar Recording** toast (`ControlBar.tsx:194-198`): rename `"Open recording folder"` → `"Show in folder"` for consistent copy across the app.

---

## 6. Settings UI in SPEC-28 IA

### 6.1 Single Home Rule (`SPEC-28 §9.1`)

All three new controls live in **Settings → Clipboard & files** (`section: "clipboard"` in `src/components/settings/registry.ts` and `src/components/settings/ClipboardFilesSection.tsx`), under a **"Transfer folders"** group heading below the existing switches (`K-06`, `K-07`, `T-01`, `T-02`, `T-03`).

Other surfaces obey the single-editor rule:
- **Phone Hub Gallery & Files** toolbars may show a compact read-only destination pill (e.g., `Saving to Downloads\AnyLeap` or `Ask each time`) that acts as a link to **Settings → Clipboard & files** (matching `ReadOnlyLinkRow` / `"Edit in Computers"` from SPEC-28 §9.2), never a second folder picker setting.
- **TopologyCanvas** (`TopologyCanvas.tsx:477`) drop overlay displays the live configured receive folder path instead of hardcoded `"Downloads/AnyLeap_Transfers"`.
- **Advanced (`T-04`)** (`registry.ts:100`) updates its helper text to: `"Files above this size (MB) are saved directly to your receive folder."` (removing the stale `Downloads\AnyLeap_Transfers` string).

### 6.2 Control Mapping & SPEC-28 Copy Rules (`SPEC-28 §8.5`)

| ID | Section | Label | Helper text | Control | Keywords |
|---|---|---|---|---|---|
| `F-01` | `clipboard` | Save received files to | `Used for phone downloads, screenshots, and large files from another PC. Default: Downloads\AnyLeap.` | Read-only monospace path + **Change...** (`openDialog({ directory: true })`) + **Reset** (when custom) + **Open** (`revealItemInDir` / `openPath`) | `["folder", "download", "save", "path", "directory", "transfer"]` |
| `F-02` | `clipboard` | Ask where to save each time | `Off: saves straight to your folder without asking. Transfers from another PC always use your folder.` | Switch (`role="switch"`, default `false`) | `["prompt", "dialog", "ask", "download", "save"]` |
| `F-03` | `clipboard` | Save files sent to phone in | `Folder on your phone for files dropped onto the mirror window or pasted images. Default: /sdcard/Download.` | Text input + quick preset chips (**Download**, **Documents**, **Pictures**) + **Reset** (when non-default) | `["phone", "upload", "android", "sdcard", "push", "mirror"]` |

Copy compliance check (`SPEC-28 §8.5`):
- Positive phrasing, sentence case, safe defaults in helper lines.
- Zero internal terms (`KVM`, `staging`, `prefetch`, `adb`, `scrcpy`, `CF_HDROP`).
- Zero third-party product names (`AGENTS.md` §1.3).
- Contrast: helper text (`--text-muted`) placed on `--ink-900` / `--ink-950`, never `--ink-800` (`SPEC-28 §10`).

---

## 7. Migration

1. **First-load initialization of `transferFolders` in `settings.json`:**
   - When `transferFolders` is absent on load (`loadTransferFolders()` in `src/lib/settingsStore.ts`): write `DEFAULT_TRANSFER_FOLDERS` (`receiveDir: null`, `askWhereToSave: false`, `phoneUploadDir: "/sdcard/Download"`).
2. **Legacy `phoneHubLastGalleryDownloadDir` (`src/stores/phoneHubStore.ts:11`):**
   - **Do NOT blindly copy `phoneHubLastGalleryDownloadDir` into `receiveDir`.** A folder picked once for a single photo download would silently hijack all future Phone Hub, screenshot, and KVM transfers.
   - Keep `phoneHubLastGalleryDownloadDir` untouched/frozen in `settings.json` for downgrade safety (matching SPEC-28 §5.4).
   - When `askWhereToSave` is `true`, the Gallery/Files folder picker seeds `defaultPath` from the configured `receiveDir` (falling back to `lastGalleryDownloadDir` or OS `downloadDir()` only if `receiveDir === null`).
3. **Legacy `Downloads\AnyLeap_Transfers`:**
   - Do not delete or move existing files in `Downloads\AnyLeap_Transfers`.
   - Update all UI copy (`registry.ts:100`, `TopologyCanvas.tsx:477`) and Rust fallback notices (`kvm/commands.rs:451,812,3845`) to display the live resolved receive folder path.

---

## 8. Test Plan

### 8.1 Rust Unit & Integration Tests (`cargo test`)
- **`transfer_paths.rs`:**
  - `unique_dest_path`: no collision, `name (1).ext`, `name (2).ext`, extensionless `README (1)`, multi-dot `archive.tar (1).gz`, dotfiles `.env (1)`.
  - `prepare_receive_dir`: succeeds on valid writable directory (`fallback_from == None`); falls back cleanly when configured path is missing/unwritable or points to an existing regular file (`fallback_from == Some(configured)`).
  - `normalize_phone_upload_dir`: accepts `/sdcard/Download`, `/sdcard/Documents/Work/`, `/storage/emulated/0/Download`; rejects empty, `..` traversal, relative paths, quotes/newlines/control chars, and `/data/local/tmp`.
- **`commands.rs`:**
  - `build_scrcpy_args`: passes `--push-target=<dir>` when `phone_upload_dir` is set (verified against bundled `scrcpy 5.0.1 --help`).
  - `save_screenshot_to_dir` helper: writes PNG into resolved dir with `unique_dest_path` collision handling and returns `savedPath` + `fallbackFrom`.
- **`kvm/staging.rs` & `kvm/files.rs`:**
  - Updating `transfer_paths` live config changes the destination used by subsequent `StreamHeader::File` and `DownloadsFallback` transfers without restarting the KVM client or server.

### 8.2 Frontend Unit & Component Tests (Vitest)
- **`transferFolders.test.ts`:**
  - `normalizeTransferFolders` handles missing/partial/corrupt objects, trims paths, rejects invalid `phoneUploadDir`, preserves `null` default for `receiveDir`.
  - `loadTransferFoldersFrom` writes defaults once when absent and is idempotent on subsequent loads; never modifies `phoneHubLastGalleryDownloadDir`.
- **`SettingsPanel.test.tsx` & `settingsFilter.test.ts`:**
  - Filter finds `F-01`, `F-02`, `F-03` by label, helper, and keywords (`"folder"`, `"download"`, `"upload"`, `"sdcard"`).
  - Rendering **Clipboard & files** shows `F-01..F-03` with accessible bound labels, `aria-describedby` helpers, and `role="switch"` on `F-02`.
  - Preset chips (`Download`, `Documents`, `Pictures`) update `phoneUploadDir`; invalid phone path shows inline error without calling `set_transfer_folders`.
  - `T-04` no longer mentions `AnyLeap_Transfers`.
- **`GalleryAndApps.test.tsx` & `FilesView.test.tsx`:**
  - With `askWhereToSave: false` (default): clicking Download in Gallery or Files calls `pullDeviceFile` directly **without** calling `openDialog`, and renders **"Show in folder"** calling `revealItemInDir`.
  - With `askWhereToSave: true`: clicking Download calls `openDialog` seeded with the configured folder; cancelling `openDialog` aborts cleanly.
- **`transferBarReducer.test.ts` & `TransferBarWindow.test.tsx`:**
  - `done` payload with `savedPath` sets `TRANSFER_SAVED_LINGER_MS` (`4500 ms`) and renders **"Show in folder"** button calling `revealItemInDir(savedPath)`; `done` without `savedPath` (staged clipboard/drag) does not show the button.
- **`noThirdPartyNames.test.ts`:**
  - Expand spec glob (if desired) or run to confirm zero third-party names in `src/**` and specs.

### 8.3 Manual Test Checklist (before merging implementation)
- Download photo from Phone Hub Gallery and file from Phone Hub Files with default settings → lands in `Downloads\AnyLeap` without any dialog; **"Show in folder"** selects it in Explorer.
- Toggle **Ask where to save each time** on → Gallery and Files prompt once per click; cancelling leaves no partial file.
- Run **Take screenshot** from `Ctrl+K` Command Palette → saves PNG to receive folder and shows **"Show in folder"** toast.
- Drag a non-APK file onto the mirror window and paste an image (`Ctrl+V`) with `phoneUploadDir` set to `/sdcard/Documents` → arrives in `/sdcard/Documents/` on phone.
- Set PC receive folder to an invalid/disconnected drive (e.g. `Z:\Missing`), trigger a download and a KVM transfer → falls back to `Downloads\AnyLeap` and shows the fallback notice.
- Drop a file onto the Computers canvas and paste a large (`> stagingLimitMb`) file across KVM → Transfer Bar and Activity Feed show **"Show in folder"** pointing to the configured folder.

---

## 9. Phasing

Each phase compiles, passes all five quality gates (`AGENTS.md` §2), and ships independently:

1. **Phase 1 — Path resolver, settings model, and Settings UI.**
   - `src-tauri/src/transfer_paths.rs` (`resolve_receive_dir`, `prepare_receive_dir`, `unique_dest_path`, `normalize_phone_upload_dir`, live `RwLock` state, `set_transfer_folders` / `get_default_transfer_folders`).
   - `src/lib/transferFolders.ts`, `settingsStore.ts` persistence, `useAppStore.ts` + startup sync in `App.tsx`.
   - `F-01`, `F-02`, `F-03` in `ClipboardFilesSection.tsx` + `registry.ts`, updated `T-04` copy.
2. **Phase 2 — Phone Hub & Mirroring flows + "Show in folder".**
   - `phone_files.rs` uses `transfer_paths::prepare_receive_dir` when `dest_dir` is omitted (`pull_device_file` returns `{ savedPath, fallbackFrom }`).
   - `GalleryView.tsx` & `FilesView.tsx` respect `askWhereToSave` and add **"Show in folder"** + fallback notice.
   - New `save_screenshot` Rust command replacing `<a download>` in `App.tsx` + toast with **"Show in folder"**.
   - PC → Phone: `--push-target` in `build_scrcpy_args` (verified against `scrcpy 5.0.1`), `push_clipboard_image` and `handle_dropped_files` read live `phoneUploadDir`.
   - Consistent `"Show in folder"` label on `ControlBar.tsx` recording stop toast.
3. **Phase 3 — KVM live destination, Transfer Bar & Activity Feed "Show in folder".**
   - Replace `kvm/commands.rs::get_download_dir()` and static `download_dir` captures in `kvm/client.rs` / `kvm/server.rs` with live calls to `transfer_paths::prepare_receive_dir()`.
   - Update `KvmPacket` / `kvm-transfer-progress` `done` events for `StreamHeader::File` and `DownloadsFallback` to carry `savedPath` (and emit fallback notices when applicable).
   - Add **"Show in folder"** button to `TransferBarWindow.tsx` and `ComputersView.tsx` activity rows; update `TopologyCanvas.tsx:477` to show the live configured receive folder.
