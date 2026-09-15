# Implementation Plan: Session Lifecycle & Flex Mode Teardown

> **Goal:** Ensure clean scrcpy session restarts and teardowns by eliminating argument list duplication, restoring device launcher state on flex virtual display teardown, and integrating fallback restart logic with Windows dynamic screen-off shortcuts.

**Architecture:**
- Create pure helper `toggle_screen_off_args(args: &[String], off: bool) -> Vec<String>` with unit tests.
- Maintain compatibility with dynamic screen-off: Windows uses `keybd_event` (`MOD+o` / `MOD+Shift+o`) to toggle power without restarting scrcpy; if window is absent or on macOS/Linux, fallback cleanly to `restart_with_screen_off` using `toggle_screen_off_args`.
- In `stop_mirror`, if `session.display_id.is_some()` (flex mode), send Android `KEYCODE_BACK` (keycode 4) prior to SIGKILL, then pause 250ms via `tokio::time::sleep` to allow the device to restore its native density and launcher layout cleanly.
- Add `tokio` dependency with `features = ["time"]` in `src-tauri/Cargo.toml`.

**Tech Stack:** Rust, Tokio (timer), ADB keyevent injection, Tauri IPC.

---

## Global Constraints

- Do not alter Tauri command signature `restart_with_screen_off(app: AppHandle, serial: String, off: bool) -> Result<SessionInfo, String>`.
- `stop_mirror` must become an async command returning `Result<(), String>`.
- Preserves Android virtual display reset behaviors without hanging UI thread.

---

## File Changes

- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/commands.rs`

---

## Detailed Tasks

### Task 1: Add Tokio Timer Feature to `Cargo.toml`

**Files:**
- Modify: `src-tauri/Cargo.toml`

**Implementation:**
Add tokio dependency:
```toml
tokio = { version = "1", features = ["time"] }
```

**Verification:**
Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS

---

### Task 2: Implement `toggle_screen_off_args` and Unit Tests

**Files:**
- Modify: `src-tauri/src/commands.rs`

**Implementation:**
Add pure helper:
```rust
/// Mutate scrcpy argument list to add or remove `--turn-screen-off`.
fn toggle_screen_off_args(args: &[String], off: bool) -> Vec<String> {
    let mut out: Vec<String> = args
        .iter()
        .filter(|a| a.as_str() != "--turn-screen-off")
        .cloned()
        .collect();
    if off {
        out.push("--turn-screen-off".to_string());
    }
    out
}
```

Add unit tests in `mod tests` block:
```rust
#[test]
fn toggle_screen_off_adds_and_removes_flag() {
    let base = vec![
        "--serial".to_string(),
        "DEVICE123".to_string(),
        "--max-size=1280".to_string(),
        "--window-title=AnyLeap — DEVICE123".to_string(),
    ];

    let with_off = toggle_screen_off_args(&base, true);
    assert!(with_off.contains(&"--turn-screen-off".to_string()));
    assert_eq!(
        with_off.iter().filter(|a| *a == "--turn-screen-off").count(),
        1
    );

    let with_off_again = toggle_screen_off_args(&with_off, true);
    assert_eq!(
        with_off_again
            .iter()
            .filter(|a| *a == "--turn-screen-off")
            .count(),
        1
    );

    let without_off = toggle_screen_off_args(&with_off, false);
    assert!(!without_off.contains(&"--turn-screen-off".to_string()));
    assert_eq!(without_off, base);
}
```

**Verification:**
Run: `cargo test --manifest-path src-tauri/Cargo.toml test_toggle_screen_off`
Expected: PASS

---

### Task 3: Flex Mode Teardown in `stop_mirror`

**Files:**
- Modify: `src-tauri/src/commands.rs`

**Implementation:**
In `stop_mirror`:
```rust
#[tauri::command]
pub async fn stop_mirror(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
) -> Result<(), String> {
    // Take ownership out of the map, then kill outside the lock.
    let session = state.sessions.lock().map_err(poison_error)?.remove(&session_id);
    if let Some(s) = session {
        // For flex mode, send a BACK keypress before killing. This helps the
        // device restore its original density and launcher layout, which can
        // get stuck otherwise.
        if s.display_id.is_some() {
            let _ = send_keyevent(app, s.serial.clone(), 4).await;
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        s.child.kill().map_err(|e| e.to_string())
    } else {
        Err("no such session".into())
    }
}
```

**Verification:**
Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS
