# Implementation Plan: Cross-Platform Build & Sidecar Packaging

> **Goal:** Support building and packaging AnyLeap on macOS and Linux in addition to Windows by replacing PowerShell binary fetching with a cross-platform Node.js script, dynamically resolving target-triple sidecar names in Rust, and expanding Tauri bundle targets.

**Architecture:**
- Create `scripts/fetch-binaries.js` to download platform-specific official releases of scrcpy and adb, verify SHA-256 hashes against known checksums or official `SHA256SUMS.txt`, extract them, and stage them as `adb-<target-triple>[.exe]` and `scrcpy-<target-triple>[.exe]` inside `src-tauri/binaries/`.
- Update `package.json` to run `node scripts/fetch-binaries.js` across all environments.
- In `src-tauri/build.rs`, propagate `TARGET` environment variable to Rust via `cargo:rustc-env=TARGET=...`.
- In `src-tauri/src/commands.rs`, resolve `ADB_SIDECAR` using `concat!("adb-", env!("TARGET"), ...)` and expand `adb_path()` search candidates to include macOS `.app` bundle `../Resources/` directories, un-suffixed dev binaries, and `binaries/` roots.
- In `src-tauri/tauri.conf.json`, declare bundle targets: `["nsis", "app", "dmg", "deb", "appimage"]`.

**Tech Stack:** Node.js (fetch, crypto, child_process), Rust/Cargo build scripts, Tauri v2 bundler.

---

## Global Constraints

- Do not break existing Windows builds, DLL staging, or NSIS packaging.
- Retain existing npm scripts (`dev`, `build`, `preview`, `tauri`, `fetch-binaries`, `release`, `postinstall`).
- SHA256 integrity verification must guard all downloads.

---

## File Changes

- Create: `scripts/fetch-binaries.js`
- Modify: `package.json`
- Modify: `src-tauri/build.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/tauri.conf.json`

---

## Detailed Tasks

### Task 1: Create Cross-Platform Binary Fetcher (`scripts/fetch-binaries.js`)

**Files:**
- Create: `scripts/fetch-binaries.js`
- Modify: `package.json`

**Implementation Details:**
1. Detect host triple using `rustc --print host-tuple` or fallback from `process.platform` / `process.arch` (`x86_64-pc-windows-msvc`, `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`).
2. Resolve archive asset name for scrcpy release (e.g. `scrcpy-win64-v4.0.zip`, `scrcpy-macos-aarch64-v4.0.tar.gz`, `scrcpy-linux-x86_64-v4.0.tar.gz`).
3. Download archive to temp directory.
4. Verify SHA256 checksum from embedded checksum table or download and parse `SHA256SUMS.txt`.
5. Extract zip (PowerShell or unzip) or tar.gz (`tar -xzf`).
6. Copy `scrcpy` -> `src-tauri/binaries/scrcpy-<triple>[.exe]` and `chmod 0o755`.
7. Copy `adb` -> `src-tauri/binaries/adb-<triple>[.exe]` and `chmod 0o755`.
8. Copy `scrcpy-server` -> `src-tauri/binaries/scrcpy-server`.
9. On Windows, copy `*.dll` to `src-tauri/binaries/dll/`.
10. In `package.json`, update `"fetch-binaries"` script to `"node scripts/fetch-binaries.js"`.

**Verification:**
Run: `node scripts/fetch-binaries.js`
Expected: Binaries verified and staged in `src-tauri/binaries/`.

---

### Task 2: Expose `TARGET` Triple in `build.rs`

**Files:**
- Modify: `src-tauri/build.rs`

**Implementation:**
In `src-tauri/build.rs`:
```rust
fn main() {
    println!("cargo:rustc-env=TARGET={}", std::env::var("TARGET").unwrap_or_default());
    tauri_build::build()
}
```

**Verification:**
Run: `cargo build --manifest-path src-tauri/Cargo.toml`
Expected: PASS

---

### Task 3: Abstract `ADB_SIDECAR` and `adb_path` in `commands.rs`

**Files:**
- Modify: `src-tauri/src/commands.rs`

**Implementation:**
1. Dynamic sidecar constant:
   ```rust
   #[cfg(target_os = "windows")]
   const ADB_SIDECAR: &str = concat!("adb-", env!("TARGET"), ".exe");
   #[cfg(not(target_os = "windows"))]
   const ADB_SIDECAR: &str = concat!("adb-", env!("TARGET"));
   ```
2. Update `adb_path()` candidates:
   ```rust
   pub(crate) fn adb_path() -> Option<std::path::PathBuf> {
       let exe = std::env::current_exe().ok()?;
       let dir = exe.parent()?;
       let exe_name = if cfg!(windows) { "adb.exe" } else { "adb" };
       let candidates = [
           dir.join(exe_name),
           dir.join(ADB_SIDECAR),
           dir.join("..").join("Resources").join(ADB_SIDECAR),
           dir.join("..").join("Resources").join(exe_name),
           dir.join("..").join("..").join("binaries").join(ADB_SIDECAR),
           dir.join("..").join("..").join("binaries").join(exe_name),
       ];
       candidates.into_iter().find(|c| c.exists())
   }
   ```

**Verification:**
Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS

---

### Task 4: Expand Bundle Targets in `tauri.conf.json`

**Files:**
- Modify: `src-tauri/tauri.conf.json`

**Implementation:**
Update `bundle.targets`:
```json
"targets": [
  "nsis",
  "app",
  "dmg",
  "deb",
  "appimage"
]
```

**Verification:**
Run: `npm run build`
Expected: PASS
