use super::protocol::{KvmPacket, Outbound};
use crate::state::AppState;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

static LOCK_TOGETHER: AtomicBool = AtomicBool::new(true);
static SLEEP_TOGETHER: AtomicBool = AtomicBool::new(false);
static IS_LOCAL_LOCKED: AtomicBool = AtomicBool::new(false);
static SUPPRESS_LOCK_ECHO_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
static SUPPRESS_SLEEP_ECHO_UNTIL_MS: AtomicU64 = AtomicU64::new(0);
static SESSION_SYNC_STARTED: AtomicBool = AtomicBool::new(false);
static APP_HANDLE: OnceLock<AppHandle> = OnceLock::new();

const LOCK_ECHO_SUPPRESS_MS: u64 = 5_000;
const SLEEP_ECHO_SUPPRESS_MS: u64 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSessionEvent {
    Lock,
    Unlock,
    Suspend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundSyncAction {
    None,
    SendLock,
    SendSleep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundSyncAction {
    None,
    LockWorkstation,
    SleepWorkstation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionSyncPolicy {
    pub lock_together: bool,
    pub sleep_together: bool,
    pub is_linked: bool,
}

pub fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn set_session_sync_settings(lock_together: bool, sleep_together: bool) {
    LOCK_TOGETHER.store(lock_together, Ordering::SeqCst);
    SLEEP_TOGETHER.store(sleep_together, Ordering::SeqCst);
}

pub fn lock_together_enabled() -> bool {
    LOCK_TOGETHER.load(Ordering::Relaxed)
}

pub fn sleep_together_enabled() -> bool {
    SLEEP_TOGETHER.load(Ordering::Relaxed)
}

pub fn is_local_locked() -> bool {
    IS_LOCAL_LOCKED.load(Ordering::Relaxed)
}

/// Pure classifier for Win32 session/power notification messages.
pub fn classify_window_message(msg: u32, wparam: usize) -> Option<LocalSessionEvent> {
    const WM_POWERBROADCAST_VAL: u32 = 0x0218;
    const PBT_APMSUSPEND_VAL: usize = 0x0004;
    const WM_WTSSESSION_CHANGE_VAL: u32 = 0x02B1;
    const WTS_SESSION_LOCK_VAL: usize = 0x7;
    const WTS_SESSION_UNLOCK_VAL: usize = 0x8;

    match msg {
        WM_WTSSESSION_CHANGE_VAL => match wparam {
            WTS_SESSION_LOCK_VAL => Some(LocalSessionEvent::Lock),
            WTS_SESSION_UNLOCK_VAL => Some(LocalSessionEvent::Unlock),
            _ => None,
        },
        WM_POWERBROADCAST_VAL if wparam == PBT_APMSUSPEND_VAL => Some(LocalSessionEvent::Suspend),
        _ => None,
    }
}

/// Pure decision function for outbound session sync packets.
/// Unlock is NEVER synced to the peer.
pub fn decide_outbound_sync(
    event: LocalSessionEvent,
    policy: SessionSyncPolicy,
    now_ms: u64,
    suppress_lock_until_ms: u64,
    suppress_sleep_until_ms: u64,
) -> OutboundSyncAction {
    if !policy.is_linked {
        return OutboundSyncAction::None;
    }
    match event {
        LocalSessionEvent::Lock => {
            if policy.lock_together && now_ms >= suppress_lock_until_ms {
                OutboundSyncAction::SendLock
            } else {
                OutboundSyncAction::None
            }
        }
        LocalSessionEvent::Unlock => OutboundSyncAction::None,
        LocalSessionEvent::Suspend => {
            if policy.sleep_together && now_ms >= suppress_sleep_until_ms {
                OutboundSyncAction::SendSleep
            } else {
                OutboundSyncAction::None
            }
        }
    }
}

/// Pure decision function for inbound `SessionLock` / `SessionSleep` packets.
/// Unlock is NEVER automated.
pub fn decide_inbound_sync(is_lock_packet: bool, policy: SessionSyncPolicy) -> InboundSyncAction {
    if !policy.is_linked {
        return InboundSyncAction::None;
    }
    if is_lock_packet {
        if policy.lock_together {
            InboundSyncAction::LockWorkstation
        } else {
            InboundSyncAction::None
        }
    } else if policy.sleep_together {
        InboundSyncAction::SleepWorkstation
    } else {
        InboundSyncAction::None
    }
}

/// Pure decision function for whether a local session event should release any active controller trap.
pub fn should_release_trap_on_local_event(event: LocalSessionEvent) -> bool {
    matches!(event, LocalSessionEvent::Lock | LocalSessionEvent::Suspend)
}

enum TrapReleaseStatus {
    Released,
    Contended,
    NoState,
}

fn try_release_controller_trap_now(app: &AppHandle) -> TrapReleaseStatus {
    let Some(state) = app.try_state::<AppState>() else {
        return TrapReleaseStatus::NoState;
    };
    let status = match state.kvm.try_lock() {
        Ok(kvm) => {
            kvm.hooks.emergency_release();
            TrapReleaseStatus::Released
        }
        Err(std::sync::TryLockError::Poisoned(poisoned)) => {
            poisoned.into_inner().hooks.emergency_release();
            TrapReleaseStatus::Released
        }
        Err(std::sync::TryLockError::WouldBlock) => TrapReleaseStatus::Contended,
    };
    status
}

fn release_controller_trap_blocking(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        let kvm = state.kvm.lock().unwrap_or_else(|e| e.into_inner());
        kvm.hooks.emergency_release();
    }
}

fn is_kvm_linked(kvm: &crate::state::KvmState) -> bool {
    let client_linked = kvm
        .client
        .as_ref()
        .is_some_and(|c| c.is_connected.load(Ordering::Relaxed));
    let server_linked = kvm
        .server
        .as_ref()
        .is_some_and(|s| s.is_running.load(Ordering::Relaxed))
        && !kvm.connected_peers.is_empty();
    client_linked || server_linked
}

fn send_packet_with_kvm(kvm: &crate::state::KvmState, packet: KvmPacket) -> bool {
    if let Some(ref client) = kvm.client {
        if client.is_connected.load(Ordering::Relaxed) {
            return client.send_packet_tx.try_send(packet).is_ok();
        }
    }
    if let Some(ref server) = kvm.server {
        if server.is_running.load(Ordering::Relaxed) && !kvm.connected_peers.is_empty() {
            return server.outgoing_tx.send(Outbound::Packet(packet)).is_ok();
        }
    }
    false
}

fn apply_local_session_with_kvm(kvm: &crate::state::KvmState, event: LocalSessionEvent) {
    if should_release_trap_on_local_event(event) {
        kvm.hooks.emergency_release();
    }

    let policy = SessionSyncPolicy {
        lock_together: lock_together_enabled(),
        sleep_together: sleep_together_enabled(),
        is_linked: is_kvm_linked(kvm),
    };
    let action = decide_outbound_sync(
        event,
        policy,
        now_epoch_ms(),
        SUPPRESS_LOCK_ECHO_UNTIL_MS.load(Ordering::Relaxed),
        SUPPRESS_SLEEP_ECHO_UNTIL_MS.load(Ordering::Relaxed),
    );
    match action {
        OutboundSyncAction::None => {}
        OutboundSyncAction::SendLock => {
            let _ = send_packet_with_kvm(kvm, KvmPacket::SessionLock);
        }
        OutboundSyncAction::SendSleep => {
            let _ = send_packet_with_kvm(kvm, KvmPacket::SessionSleep);
        }
    }
}

fn handle_local_session_event(event: LocalSessionEvent) {
    match event {
        LocalSessionEvent::Lock => IS_LOCAL_LOCKED.store(true, Ordering::SeqCst),
        LocalSessionEvent::Unlock => {
            IS_LOCAL_LOCKED.store(false, Ordering::SeqCst);
            super::injector::mark_post_unlock_nudge_pending();
            super::injector::ensure_cursor_visible_with_nudge("wts_unlock");
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(120));
                super::injector::ensure_cursor_visible_with_nudge("wts_unlock_post");
            });
            return;
        }
        LocalSessionEvent::Suspend => {}
    }

    if !should_release_trap_on_local_event(event) {
        return;
    }

    let Some(app) = APP_HANDLE.get() else {
        return;
    };
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };

    // Never block the hidden-window message loop; retry once from a spawned thread if contended.
    match state.kvm.try_lock() {
        Ok(kvm) => apply_local_session_with_kvm(&kvm, event),
        Err(std::sync::TryLockError::Poisoned(poisoned)) => {
            let kvm = poisoned.into_inner();
            apply_local_session_with_kvm(&kvm, event);
        }
        Err(std::sync::TryLockError::WouldBlock) => {
            let app_h = app.clone();
            std::thread::spawn(move || {
                if let Some(state) = app_h.try_state::<AppState>() {
                    let kvm = state.kvm.lock().unwrap_or_else(|e| e.into_inner());
                    apply_local_session_with_kvm(&kvm, event);
                }
            });
        }
    };
}

fn run_inbound_session_action(
    app: &AppHandle,
    action_fn: fn() -> Result<(), String>,
    err_prefix: &'static str,
) {
    let contended = matches!(
        try_release_controller_trap_now(app),
        TrapReleaseStatus::Contended
    );
    let app_h = app.clone();
    std::thread::spawn(move || {
        if contended {
            release_controller_trap_blocking(&app_h);
        }
        if let Err(e) = action_fn() {
            let _ = app_h.emit(
                "kvm-continuity-notice",
                serde_json::json!({ "text": format!("{err_prefix}: {e}") }),
            );
        }
    });
}

/// Handle an inbound `SessionLock` or `SessionSleep` from an authenticated peer.
pub fn handle_inbound_session_packet(app: &AppHandle, is_lock_packet: bool) {
    let policy = SessionSyncPolicy {
        lock_together: lock_together_enabled(),
        sleep_together: sleep_together_enabled(),
        is_linked: true,
    };
    match decide_inbound_sync(is_lock_packet, policy) {
        InboundSyncAction::None => {}
        InboundSyncAction::LockWorkstation => {
            SUPPRESS_LOCK_ECHO_UNTIL_MS.store(
                now_epoch_ms().saturating_add(LOCK_ECHO_SUPPRESS_MS),
                Ordering::SeqCst,
            );
            run_inbound_session_action(app, lock_local_workstation, "Failed to lock PC");
        }
        InboundSyncAction::SleepWorkstation => {
            SUPPRESS_SLEEP_ECHO_UNTIL_MS.store(
                now_epoch_ms().saturating_add(SLEEP_ECHO_SUPPRESS_MS),
                Ordering::SeqCst,
            );
            run_inbound_session_action(app, sleep_local_workstation, "Failed to put PC to sleep");
        }
    }
}

fn lock_local_workstation() -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        windows::Win32::System::Shutdown::LockWorkStation()
            .map_err(|e| format!("LockWorkStation failed: {e}"))
    }
    #[cfg(not(windows))]
    {
        Err("LockWorkStation is only supported on Windows".into())
    }
}

fn sleep_local_workstation() -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        let ok = windows::Win32::System::Power::SetSuspendState(false, false, false);
        if ok {
            Ok(())
        } else {
            Err("SetSuspendState returned false".into())
        }
    }
    #[cfg(not(windows))]
    {
        Err("SetSuspendState is only supported on Windows".into())
    }
}

/// Start the hidden message window thread once during application setup.
pub fn init(app: &AppHandle) {
    let _ = APP_HANDLE.set(app.clone());
    if SESSION_SYNC_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }

    #[cfg(windows)]
    {
        let _ = std::thread::Builder::new()
            .name("kvm-session-sync".into())
            .spawn(run_hidden_message_loop);
    }
}

#[cfg(windows)]
fn run_hidden_message_loop() {
    use windows::core::w;
    use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::RemoteDesktop::{
        WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW,
        RegisterClassW, TranslateMessage, MSG, WINDOW_EX_STYLE, WM_DESTROY, WM_POWERBROADCAST,
        WNDCLASSW, WS_OVERLAPPED,
    };

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if let Some(ev) = classify_window_message(msg, wparam.0) {
            handle_local_session_event(ev);
            if msg == WM_POWERBROADCAST {
                return LRESULT(1);
            }
            return LRESULT(0);
        }
        if msg == WM_DESTROY {
            let _ = WTSUnRegisterSessionNotification(hwnd);
            return LRESULT(0);
        }
        DefWindowProcW(hwnd, msg, wparam, lparam)
    }

    unsafe {
        let hmodule = match GetModuleHandleW(None) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[KVM-SessionSync] GetModuleHandleW failed: {e}");
                return;
            }
        };
        let hinstance: HINSTANCE = hmodule.into();
        let class_name = w!("AnyLeapSessionSyncHiddenWindow");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wnd_proc),
            hInstance: hinstance,
            lpszClassName: class_name,
            ..Default::default()
        };
        RegisterClassW(&wc);

        // Top-level unshown window (NOT HWND_MESSAGE) so WM_POWERBROADCAST is delivered.
        let hwnd = match CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            w!("AnyLeapSessionSync"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance),
            None,
        ) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[KVM-SessionSync] CreateWindowExW failed: {e}");
                return;
            }
        };

        if let Err(e) = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION) {
            eprintln!("[KVM-SessionSync] WTSRegisterSessionNotification failed: {e}");
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        let _ = DestroyWindow(hwnd);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outbound_lock_and_sleep_decisions() {
        let linked_default = SessionSyncPolicy {
            lock_together: true,
            sleep_together: false,
            is_linked: true,
        };
        assert_eq!(
            decide_outbound_sync(LocalSessionEvent::Lock, linked_default, 10_000, 0, 0),
            OutboundSyncAction::SendLock
        );
        // Sleep is disabled by default
        assert_eq!(
            decide_outbound_sync(LocalSessionEvent::Suspend, linked_default, 10_000, 0, 0),
            OutboundSyncAction::None
        );
        // Unlock is NEVER sent
        assert_eq!(
            decide_outbound_sync(LocalSessionEvent::Unlock, linked_default, 10_000, 0, 0),
            OutboundSyncAction::None
        );

        let all_on = SessionSyncPolicy {
            lock_together: true,
            sleep_together: true,
            is_linked: true,
        };
        assert_eq!(
            decide_outbound_sync(LocalSessionEvent::Suspend, all_on, 10_000, 0, 0),
            OutboundSyncAction::SendSleep
        );
        // Echo suppression blocks re-broadcasting peer-triggered lock/sleep
        assert_eq!(
            decide_outbound_sync(LocalSessionEvent::Lock, all_on, 10_000, 12_000, 0),
            OutboundSyncAction::None
        );
        assert_eq!(
            decide_outbound_sync(LocalSessionEvent::Suspend, all_on, 10_000, 0, 15_000),
            OutboundSyncAction::None
        );
        // Unlinked never sends anything
        let unlinked = SessionSyncPolicy {
            lock_together: true,
            sleep_together: true,
            is_linked: false,
        };
        assert_eq!(
            decide_outbound_sync(LocalSessionEvent::Lock, unlinked, 10_000, 0, 0),
            OutboundSyncAction::None
        );
        assert_eq!(
            decide_outbound_sync(LocalSessionEvent::Suspend, unlinked, 10_000, 0, 0),
            OutboundSyncAction::None
        );
    }

    #[test]
    fn inbound_lock_and_sleep_decisions() {
        let default_policy = SessionSyncPolicy {
            lock_together: true,
            sleep_together: false,
            is_linked: true,
        };
        assert_eq!(
            decide_inbound_sync(true, default_policy),
            InboundSyncAction::LockWorkstation
        );
        assert_eq!(
            decide_inbound_sync(false, default_policy),
            InboundSyncAction::None
        );

        let sleep_enabled = SessionSyncPolicy {
            lock_together: false,
            sleep_together: true,
            is_linked: true,
        };
        assert_eq!(
            decide_inbound_sync(true, sleep_enabled),
            InboundSyncAction::None
        );
        assert_eq!(
            decide_inbound_sync(false, sleep_enabled),
            InboundSyncAction::SleepWorkstation
        );

        let unlinked = SessionSyncPolicy {
            lock_together: true,
            sleep_together: true,
            is_linked: false,
        };
        assert_eq!(decide_inbound_sync(true, unlinked), InboundSyncAction::None);
        assert_eq!(
            decide_inbound_sync(false, unlinked),
            InboundSyncAction::None
        );
    }

    #[test]
    fn win32_message_classification() {
        assert_eq!(
            classify_window_message(0x02B1, 0x7),
            Some(LocalSessionEvent::Lock)
        );
        assert_eq!(
            classify_window_message(0x02B1, 0x8),
            Some(LocalSessionEvent::Unlock)
        );
        assert_eq!(classify_window_message(0x02B1, 0x1), None);
        assert_eq!(
            classify_window_message(0x0218, 0x4),
            Some(LocalSessionEvent::Suspend)
        );
        assert_eq!(classify_window_message(0x0218, 0x7), None);
        assert_eq!(classify_window_message(0x0010, 0x7), None);
    }

    #[test]
    fn trap_release_decision_on_local_events() {
        assert!(should_release_trap_on_local_event(LocalSessionEvent::Lock));
        assert!(should_release_trap_on_local_event(
            LocalSessionEvent::Suspend
        ));
        assert!(!should_release_trap_on_local_event(
            LocalSessionEvent::Unlock
        ));
    }

    #[test]
    fn emergency_release_when_not_trapped_is_harmless() {
        let hooks = crate::kvm::hooks::HookController::new();
        assert!(!hooks.is_trapped());
        hooks.emergency_release();
        assert!(!hooks.is_trapped());
    }
}
