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
    /// The scrcpy args this session was launched with (for restart-with-screen-off).
    pub args: Vec<String>,
    /// Android display id of the scrcpy virtual display (flex mode), parsed
    /// from scrcpy's "New display: WxH/dpi (id=N)" log line. None = mirroring
    /// the physical display; input injection then needs no `-d`.
    pub display_id: Option<u32>,
    /// Current size of the virtual display (flex mode resizes it with the
    /// window; refreshed from `dumpsys display`).
    pub virtual_size: Option<(u32, u32)>,
}

/// App-wide state held in Tauri's managed state.
///
/// Lock scopes must stay tiny — never hold this `Mutex` across an `.await`.
#[derive(Default)]
pub struct AppState {
    pub sessions: Mutex<HashMap<String, Session>>,
}
