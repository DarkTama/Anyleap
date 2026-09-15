# Implementation Plan: Backend Mutex Hardening & Poison Handling

> **Goal:** Eliminate process panics caused by poisoned `std::sync::Mutex` locks across `src-tauri` by propagating errors gracefully as `Result<T, String>` to Tauri IPC callers and logging in background threads.

**Architecture:** Replace all `.lock().unwrap()` calls with structured error mapping. Define a centralized `poison_error` mapping function in `crate::state`. Map mutex lock failures in Tauri command handlers to `Result::Err(String)` so the frontend promise rejects cleanly without crashing the host process. In background loops (event handlers, window exit hooks, wheel-swipe thread), log poison errors to `eprintln!` and recover or gracefully skip ticks.

**Tech Stack:** Rust, Tauri v2 (`AppHandle`, `State`, commands).

---

## Global Constraints

- Do not add external crates for mutex management (use `std::sync::Mutex` and standard library error propagation).
- Tauri command signatures that touch `state.sessions` must return `Result<T, String>`.
- Lock scopes must remain minimal; never hold guards across async calls or `.await` boundaries.

---

## File Changes

- Modify: `src-tauri/src/state.rs`
- Modify: `src-tauri/src/commands.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/wheel_swipe.rs`

---

## Detailed Tasks

### Task 1: Add Centralized Poison Error Helper in `state.rs`

**Files:**
- Modify: `src-tauri/src/state.rs`

**Implementation:**
Add public helper function `poison_error`:
```rust
pub fn poison_error<T>(e: std::sync::PoisonError<T>) -> String {
    format!("Mutex lock poisoned: {e}")
}
```

**Verification:**
Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS

---

### Task 2: Harden Session Mutex in `commands.rs`

**Files:**
- Modify: `src-tauri/src/commands.rs`

**Changes:**
1. Import `poison_error` from `crate::state`.
2. Update `note_virtual_display`:
   ```rust
   fn note_virtual_display(app: &AppHandle, session_id: &str, id: u32, size: (u32, u32)) -> Result<(), String> {
       if let Some(st) = app.try_state::<AppState>() {
           if let Some(s) = st.sessions.lock().map_err(poison_error)?.get_mut(session_id) {
               s.display_id = Some(id);
               s.virtual_size = Some(size);
           }
       }
       Ok(())
   }
   ```
3. Update `spawn_session`:
   - Session insertion: `app.state::<AppState>().sessions.lock().map_err(poison_error)?.insert(...)`
   - Calls to `note_virtual_display(&app2, &id2, did, size)` wrapped in `let _ = ...;`
   - In `CommandEvent::Terminated` handler, avoid `.unwrap()`:
     ```rust
     if let Some(st) = app2.try_state::<AppState>() {
         match st.sessions.lock() {
             Ok(mut s) => { s.remove(&id2); }
             Err(e) => eprintln!("Sessions mutex poisoned on session exit: {}", e),
         }
     }
     ```
4. Update `restart_with_screen_off`:
   - Both in original restart block and any lock acquisition sites, replace `.lock().unwrap()` with `.lock().map_err(poison_error)?`.
5. Update `stop_mirror`:
   - Signature: `pub async fn stop_mirror(app: AppHandle, state: State<'_, AppState>, session_id: String) -> Result<(), String>`
   - Map lock error: `let session = state.sessions.lock().map_err(poison_error)?.remove(&session_id);`
6. Update `list_sessions`:
   - Signature: `pub fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionInfo>, String>`
   - Map lock: `state.sessions.lock().map_err(poison_error)?.values().map(...).collect()`
7. Update `send_keyevent`:
   - Replace `.lock().unwrap()` with `.lock().map_err(poison_error)?`.

**Verification:**
Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS

---

### Task 3: Harden Window Exit Hook in `lib.rs`

**Files:**
- Modify: `src-tauri/src/lib.rs`

**Implementation:**
In `WindowEvent::CloseRequested` / `Destroyed` handler, prevent panics if mutex is poisoned while draining child processes:
```rust
if let Some(state) = app.try_state::<AppState>() {
    match state.sessions.lock() {
        Ok(mut sessions) => {
            for (_, session) in sessions.drain() {
                let _ = session.child.kill();
            }
        }
        Err(e) => {
            eprintln!("Sessions mutex poisoned, could not kill child processes: {}", e);
        }
    }
}
```

**Verification:**
Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: PASS

---

### Task 4: Harden Wheel-Swipe Thread & Registry in `wheel_swipe.rs`

**Files:**
- Modify: `src-tauri/src/wheel_swipe.rs`

**Implementation:**
1. Update `lock_registry`:
   ```rust
   fn lock_registry() -> Result<std::sync::MutexGuard<'static, Registry>, String> {
       registry().lock().map_err(poison_error)
   }
   ```
2. Update callers in `set_enabled`:
   - Lock sessions: `state.sessions.lock().map_err(poison_error)?`
   - Lock registry: `let mut reg = lock_registry()?;`
3. Update background refresher threads:
   - Match `state.sessions.lock()` and log on `Err`, continuing the loop rather than panicking.
   - Use `let Ok(mut reg) = lock_registry() else { continue };` in loops.

**Verification:**
Run: `cargo test --manifest-path src-tauri/Cargo.toml`
Expected: PASS
