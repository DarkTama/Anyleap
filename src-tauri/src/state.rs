use crate::kvm::client::KvmClientHandle;
use crate::kvm::discovery::DiscoveryManager;
use crate::kvm::hooks::HookController;
use crate::kvm::identity::{Identity, TrustStore};
use crate::kvm::protocol::TopologyConfig;
use crate::kvm::server::KvmServerHandle;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tauri_plugin_shell::process::CommandChild;

#[derive(Default)]
pub struct KvmState {
    pub server: Option<KvmServerHandle>,
    pub client: Option<KvmClientHandle>,
    pub discovery: DiscoveryManager,
    pub hooks: HookController,
    pub topology: TopologyConfig,
    pub connected_peers: Vec<String>,
    /// Lazily created on first KVM use (needs the app data dir).
    pub security: Option<(Arc<Identity>, Arc<TrustStore>)>,
}

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
    pub mode: String, // "display" or "camera"
}

/// App-wide state held in Tauri's managed state.
///
/// Lock scopes must stay tiny — never hold this `Mutex` across an `.await`.
#[derive(Default)]
pub struct AppState {
    pub sessions: Mutex<HashMap<String, Session>>,
    pub kvm: Mutex<KvmState>,
    /// Session ids the user stopped on purpose; their exit must not surface an error.
    pub user_stopped: Mutex<std::collections::HashSet<String>>,
}

impl AppState {
    /// Mark a session as user-stopped before killing it.
    pub fn mark_user_stopped(&self, id: &str) {
        if let Ok(mut set) = self.user_stopped.lock() {
            set.insert(id.to_string());
        }
    }

    /// Consume the user-stopped flag for a session; true if it was set.
    pub fn take_user_stopped(&self, id: &str) -> bool {
        self.user_stopped
            .lock()
            .map(|mut set| set.remove(id))
            .unwrap_or(false)
    }
}

/// Format a PoisonError into a human-readable String for Tauri command results.
pub fn poison_error<T>(e: std::sync::PoisonError<T>) -> String {
    format!("Mutex lock poisoned: {e}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_poison_error_format() {
        let m = Mutex::new(10);
        let _ = std::panic::catch_unwind(|| {
            let _guard = m.lock().unwrap();
            panic!("poison trigger");
        });
        let err = m.lock().unwrap_err();
        let formatted = poison_error(err);
        assert!(formatted.starts_with("Mutex lock poisoned:"));
    }
}
