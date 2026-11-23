# SPEC-31 Default Transfer Folders Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate per-transfer folder prompts across Phone Hub, screenshots, and multi-PC KVM by introducing one configurable PC receive folder, an opt-in "Ask where to save each time" switch, a configurable phone upload folder, fallback notices, and "Show in folder" actions.

**Architecture:** A shared Rust module `src-tauri/src/transfer_paths.rs` owns live receive/upload path state, `name (1).ext` collision resolution, and missing/unwritable fallback logic. A pure TS module `src/lib/transferFolders.ts` owns defaults, validation, and normalization persisted under `transferFolders` in `settings.json` (`src/lib/settingsStore.ts`) and synced to Rust on startup and edit. UI controls live exclusively in **Settings → Clipboard & files** (`SPEC-28` single home rule).

**Tech Stack:** Tauri 2 (Rust, `tauri-plugin-store`, `tauri-plugin-dialog`, `tauri-plugin-opener`), React 19 + TypeScript + Tailwind (SPEC-26 tokens), Vitest, Cargo test.

**Spec:** `docs/specs/SPEC-31-default-transfer-folders.md`.

## Global Constraints
- Follow `AGENTS.md` §1.3: zero third-party or competitor product names in code, comments, UI copy, tests, or docs.
- Follow `SPEC-28` §8.5 & §9.1: single editor in Settings → Clipboard & files; positive sentence-case labels; no internal jargon (`KVM`, `staging`, `prefetch`, `adb`, `scrcpy`).
- Follow `SPEC-26` contrast rules: `--text-muted` only on `--ink-900` / `--ink-950`, never on `--ink-800`.
- Verify `scrcpy 4.0 --help` supports `--push-target` in Phase 2 before wiring it into `build_scrcpy_args`.
- Every phase ends green on all 5 `AGENTS.md` §2 gates (`npm test`, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo check`, `npm run build`) and ships alone.

---

## Phase 1: Path Resolver, Settings Model, and Settings UI

### Task 1.1: Shared Rust path resolver (`transfer_paths.rs`) (TDD)
**Files:** create `src-tauri/src/transfer_paths.rs`; modify `src-tauri/src/lib.rs`.
- [ ] Write failing unit tests in `transfer_paths.rs`:
  - `unique_dest_path`: handles clean names, `file (1).ext`, `file (2).ext`, extensionless `README (1)`, multi-dot `a.tar (1).gz`, and dotfiles `.env (1)`.
  - `prepare_receive_dir`: creates missing subdirectories under a writable parent (`fallback_from == None`); falls back to `default_receive_dir` → OS download dir → temp dir when configured path is unwritable or points to an existing regular file (`fallback_from == Some(bad)`).
  - `normalize_phone_upload_dir`: accepts `/sdcard/Download`, `/sdcard/Documents/Work/`, `/storage/emulated/0/Download`; rejects empty, `..`, relative paths, control chars, quotes, and paths outside `/sdcard` or `/storage/emulated/0`.
- [ ] Implement `TransferConfig` (`OnceLock<RwLock<...>>`), `init_defaults`, `set_config`, `get_default_folders`, `resolve_configured_receive_dir`, `prepare_receive_dir`, `unique_dest_path`, and `normalize_phone_upload_dir`.
- [ ] Export `pub mod transfer_paths;` in `src-tauri/src/lib.rs`, call `init_defaults` in `.setup()`, and register `set_transfer_folders` and `get_default_transfer_folders` Tauri commands.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml transfer_paths`; commit.

### Task 1.2: Frontend model, validation, and persistence (TDD)
**Files:** create `src/lib/transferFolders.ts`, `src/lib/transferFolders.test.ts`; modify `src/lib/settingsStore.ts`, `src/lib/persist.ts`, `src/store/useAppStore.ts`, `src/lib/tauri.ts`, `src/App.tsx`.
- [ ] Write failing Vitest tests in `transferFolders.test.ts`:
  - `DEFAULT_TRANSFER_FOLDERS` (`receiveDir: null`, `askWhereToSave: false`, `phoneUploadDir: "/sdcard/Download"`).
  - `normalizeTransferFolders` handles null/corrupt values, trims strings, rejects invalid `phoneUploadDir`.
  - `validatePhoneUploadDir` mirrors Rust rules for immediate UI feedback.
  - `loadTransferFoldersFrom(mockStore)` writes defaults once when `transferFolders` is absent, is idempotent, and leaves `phoneHubLastGalleryDownloadDir` untouched.
- [ ] Implement `transferFolders.ts`, `loadTransferFolders` / `saveTransferFolders` in `settingsStore.ts` (re-exported via `persist.ts`), `transferFolders` state + `setTransferFolders` / `updateTransferFolders` in `useAppStore.ts`, and `setTransferFoldersConfig` / `getDefaultTransferFolders` wrappers in `src/lib/tauri.ts`.
- [ ] On app launch in `src/App.tsx` (startup `useEffect`), load `transferFolders`, store in `useAppStore`, and sync to Rust via `setTransferFoldersConfig`.
- [ ] Run `npx vitest run src/lib/transferFolders.test.ts`; commit.

### Task 1.3: Settings UI in Clipboard & files (`F-01`, `F-02`, `F-03`)
**Files:** modify `src/components/settings/registry.ts`, `src/components/settings/ClipboardFilesSection.tsx`, `src/components/settings/__tests__/SettingsPanel.test.tsx`, `src/lib/settingsFilter.test.ts`.
- [ ] Add `F-01` ("Save received files to"), `F-02` ("Ask where to save each time"), and `F-03` ("Save files sent to phone in") to `REGISTRY` (`section: "clipboard"`) with SPEC-28 helpers and search keywords.
- [ ] Update `T-04` helper text in `registry.ts` from `"Downloads\\AnyLeap_Transfers"` to `"Files above this size (MB) are saved directly to your receive folder."`.
- [ ] Render the **"Transfer folders"** group in `ClipboardFilesSection.tsx`:
  - `F-01`: current path display (or resolved default `<Downloads>\AnyLeap`), **Change...** (`openDialog({ directory: true })`), **Open** (`revealItemInDir` / `openPath`), and **Reset** when custom.
  - `F-02`: `SwitchRow` bound to `askWhereToSave`.
  - `F-03`: accessible text input + quick preset chips (**Download**, **Documents**, **Pictures**) + inline validation + **Reset** when non-default.
- [ ] Ensure every change updates `useAppStore`, persists via `saveTransferFolders`, and calls `setTransferFoldersConfig`.
- [ ] Add component + filter tests in `SettingsPanel.test.tsx` and `settingsFilter.test.ts`; run all 5 §2 gates; commit.

---

## Phase 2: Phone Hub, Screenshots, and Mirror Upload Wiring + "Show in folder"

### Task 2.1: Backend `pull_device_file`, `save_screenshot`, and PC → Phone destinations (TDD)
**Files:** modify `src-tauri/src/phone_files.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src/lib/tauri.ts`.
- [ ] Verify bundled `scrcpy 4.0 --help` output for `--push-target` flag syntax (`src-tauri/binaries/scrcpy-x86_64-pc-windows-msvc.exe --help`).
- [ ] Update `pull_device_file(serial, device_path, dest_dir: Option<String>)`:
  - When `dest_dir` is `None` or empty, resolve via `transfer_paths::prepare_receive_dir()`.
  - When `dest_dir` is `Some(explicit)` (from `askWhereToSave: true`), validate/prepare with fallback via `transfer_paths::prepare_receive_dir_at(&explicit)`.
  - Return `PullResult { saved_path: String, fallback_from: Option<String> }` (update `src/lib/tauri.ts` and `GalleryAndApps.test.tsx` mock).
- [ ] Add `save_screenshot(app, serial, dest_file: Option<String>) -> Result<PullResult, String>` in `commands.rs`:
  - Runs `adb exec-out screencap -p`, writes to `dest_file` if explicitly chosen or `<receive_dir>/Screenshot_<sanitized_serial>_<timestamp>.png` via `unique_dest_path` (with fallback notice if `receive_dir` was unwritable).
- [ ] Wire `transfer_paths::phone_upload_dir()` into:
  - `build_scrcpy_args` (`--push-target=<dir>`, plus unit test in `commands.rs`).
  - `push_clipboard_image` (create target dir via `mkdir -p` if needed, push to `<dir>/<fname>`).
  - `handle_dropped_files` (push non-APKs to `<dir>/`).
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml`; commit.

### Task 2.2: Phone Hub Gallery & Files zero-prompt download + "Show in folder"
**Files:** modify `src/components/phone/GalleryView.tsx`, `src/components/phone/FilesView.tsx`, `src/stores/phoneHubStore.ts`, `src/components/phone/__tests__/GalleryAndApps.test.tsx`, create/update `FilesView` tests.
- [ ] In `GalleryView.tsx` (`handleDownloadItems`):
  - Read `askWhereToSave` and `receiveDir` from `useAppStore`.
  - If `askWhereToSave === false`: skip `openDialog`, call `pullDeviceFile(serial, item.path, null)` directly.
  - If `askWhereToSave === true`: prompt `openDialog({ directory: true, defaultPath: receiveDir ?? lastDownloadDir ?? await downloadDir() })`, pass chosen dir.
  - On completion, track `lastSavedPath` and `fallbackNotice`; render **"Show in folder"** (`revealItemInDir(lastSavedPath)`) in the status banner (`GalleryView.tsx:661-679`).
- [ ] In `FilesView.tsx` (`handleDownloadSelected`):
  - Same `askWhereToSave` branch (`pullDeviceFile(serial, entry.path, null)` vs `openDialog`).
  - Render **"Show in folder"** (`revealItemInDir`) in the status banner (`FilesView.tsx:342-346`) and surface any fallback notice.
- [ ] Keep `phoneHubLastGalleryDownloadDir` in `phoneHubStore.ts` frozen/non-destructive for downgrade safety.
- [ ] Add/update Vitest tests verifying zero-dialog download when `askWhereToSave: false`, dialog when `true`, and **"Show in folder"** calls `revealItemInDir`; commit.

### Task 2.3: Command Palette screenshot & Control Bar label alignment
**Files:** modify `src/App.tsx`, `src/components/ControlBar.tsx`.
- [ ] Replace `<a download>` Blob URL in `src/App.tsx:759-777` with:
  - If `askWhereToSave`: `saveDialog({ defaultPath: ...png })` then `saveScreenshot(selectedSerial, pickedPath)`.
  - If `!askWhereToSave`: `saveScreenshot(selectedSerial, null)`.
  - Show in-app toast `"Screenshot saved"` (or fallback notice) with **"Show in folder"** button (`revealItemInDir(res.savedPath)`).
- [ ] In `src/components/ControlBar.tsx:196`, rename `"Open recording folder"` → `"Show in folder"` for consistent copy (update any test referencing it).
- [ ] Run all 5 `AGENTS.md` §2 gates; commit.

---

## Phase 3: KVM Live Folder, Transfer Bar & Activity Feed "Show in folder"

### Task 3.1: Replace KVM `get_download_dir()` and static captures with `transfer_paths`
**Files:** modify `src-tauri/src/kvm/commands.rs`, `src-tauri/src/kvm/client.rs`, `src-tauri/src/kvm/server.rs`, `src-tauri/src/kvm/files.rs`, `src-tauri/src/kvm/staging.rs`.
- [ ] Remove hardcoded `get_download_dir()` from `kvm/commands.rs:112-120` and remove static `download_dir: PathBuf` capture from `start_kvm_server` / `KvmClientHandle::connect` (or resolve dynamically via `crate::transfer_paths::prepare_receive_dir()` whenever `StreamHeader::File` or `DownloadsFallback` runs).
- [ ] Update `TransferEvent::File` (`kvm/files.rs:546`, `kvm/commands.rs:243-252`) to carry the actual `saved_path: PathBuf` (not just the directory) and emit `kvm-transfer-progress` (`kind: "file"` or existing `kvm-activity`) plus `kvm-continuity-notice` if `fallback_from.is_some()`.
- [ ] Update `DownloadsFallback` notices in `kvm/commands.rs:451,812,3845` to print the actual resolved destination path instead of `"Downloads\\AnyLeap_Transfers"`, and include `saved_path` on `TransferCompletion` / `kvm-transfer-progress` `done` payloads for `DownloadsFallback`.
- [ ] Add Rust test confirming changing `transfer_paths` updates the destination of a subsequent `StreamHeader::File` receive without restarting the server/client.
- [ ] Run `cargo test --manifest-path src-tauri/Cargo.toml`; commit.

### Task 3.2: "Show in folder" in Transfer Bar, Activity Feed, and live TopologyCanvas copy
**Files:** modify `src/types/kvm.ts`, `src/components/desktop/transferBarReducer.ts`, `src/components/desktop/TransferBarWindow.tsx`, `src/components/computers/ComputersView.tsx`, `src/components/computers/TopologyCanvas.tsx`, plus their unit/component tests.
- [ ] Extend `KvmTransferProgressPayload` and `TransferBarItem` with optional `savedPath?: string` (only populated for transfers written directly to the PC receive folder, never for ephemeral clipboard/drag staging).
- [ ] Update `transferBarReducer.ts`: `done` items with `savedPath` use `TRANSFER_SAVED_LINGER_MS = 4500`; add tests in `transferBarReducer.test.ts`.
- [ ] Update `TransferBarWindow.tsx` & `TransferBarWindow.test.tsx`: render **"Show in folder"** button on `done` rows with `item.savedPath`, invoking `revealItemInDir(item.savedPath)`.
- [ ] Update `ComputersView.tsx:556-560`: add **"Show in folder"** button on activity rows with `act.savedPath` (`revealItemInDir(act.savedPath)`).
- [ ] Update `TopologyCanvas.tsx:477`: display the live resolved PC receive folder (`receiveDir ?? defaultReceiveDir`) instead of hardcoded `"Downloads/AnyLeap_Transfers"`.
- [ ] Update `README.md`, `docs/specs/SPEC-31-default-transfer-folders.md` (status), and `docs/MANUAL-TEST-CHECKLIST.md` per `AGENTS.md` §1.4.
- [ ] Run all 5 `AGENTS.md` §2 gates (`npm test`, `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo check`, `npm run build`); commit.
