use std::collections::HashMap;
use std::sync::Mutex;

use tauri_plugin_shell::process::CommandChild;

/// A running scrcpy mirror session.
pub struct Session {
    pub id: String,
    pub serial: String,
    pub pid: u32,
    pub started_at: i64,
    /// Handle to the spawned scrcpy child, used to kill it on demand.
    pub child: CommandChild,
}

/// App-wide state held in Tauri's managed state.
///
/// Lock scopes must stay tiny — never hold this `Mutex` across an `.await`.
#[derive(Default)]
pub struct AppState {
    pub sessions: Mutex<HashMap<String, Session>>,
}
