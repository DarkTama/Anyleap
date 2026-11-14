use super::protocol::{write_frame, KvmPacket, Outbound, StreamHeader};
use crate::state::AppState;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::AsyncReadExt;

static OPEN_ON_LINKED_MENU: AtomicBool = AtomicBool::new(true);
static PENDING_OPEN_FILES: OnceLock<Mutex<PendingOpenState>> = OnceLock::new();

const CONTEXT_MENU_REG_PATH: &str = r"Software\Classes\*\shell\AnyLeapOpenOnLinked";
const CONTEXT_MENU_CMD_REG_PATH: &str = r"Software\Classes\*\shell\AnyLeapOpenOnLinked\command";
const MAX_URL_LEN: usize = 4096;
const MAX_PENDING_ENTRIES: usize = 64;
pub const OPEN_TEMP_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);

pub fn open_staging_root() -> PathBuf {
    #[cfg(windows)]
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        if !local_app_data.trim().is_empty() {
            return PathBuf::from(local_app_data).join("AnyLeap").join("staging");
        }
    }
    std::env::temp_dir().join("AnyLeap").join("staging")
}

pub fn open_staging_subdir_name(transfer_id: &str) -> Option<String> {
    let trimmed = transfer_id.trim();
    if trimmed.is_empty()
        || trimmed == "."
        || trimmed == ".."
        || trimmed.contains("..")
        || trimmed.chars().any(|c| c.is_control() || "/\\<>:\"|?*".contains(c))
    {
        return None;
    }
    Some(format!("open-{trimmed}"))
}

pub fn open_staging_dir_for(staging_root: &Path, transfer_id: &str) -> Option<PathBuf> {
    open_staging_subdir_name(transfer_id).map(|sub| staging_root.join(sub))
}

pub fn is_open_temp_expired(
    modified: std::time::SystemTime,
    now: std::time::SystemTime,
    max_age: std::time::Duration,
) -> bool {
    now.duration_since(modified).is_ok_and(|age| age > max_age)
}

pub fn cleanup_expired_open_staging(
    staging_root: &Path,
    max_age: std::time::Duration,
    now: std::time::SystemTime,
) -> usize {
    let Ok(entries) = std::fs::read_dir(staging_root) else {
        return 0;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        let is_open_entry = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("open-"));
        if !is_open_entry {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if is_open_temp_expired(modified, now, max_age) {
            let ok = if meta.is_dir() {
                std::fs::remove_dir_all(&path).is_ok()
            } else {
                std::fs::remove_file(&path).is_ok()
            };
            if ok {
                removed += 1;
            }
        }
    }
    removed
}

pub fn format_opened_temporary_notice(name: &str) -> String {
    format!("Opened {name} (temporary copy, removed after 24 h)")
}

const BLOCKED_EXECUTABLE_EXTENSIONS: &[&str] = &[
    "exe",
    "msi",
    "bat",
    "cmd",
    "ps1",
    "vbs",
    "js",
    "jse",
    "wsf",
    "lnk",
    "scr",
    "com",
    "hta",
    "pif",
    "cpl",
    "msc",
    "jar",
    "reg",
    "vbe",
    "wsh",
    "psm1",
    "psd1",
    "ps1xml",
    "scf",
    "url",
    "appref-ms",
    "application",
    "msix",
    "msixbundle",
    "appx",
    "appxbundle",
    "settingcontent-ms",
    "library-ms",
    "chm",
    "gadget",
    "inf",
    "msp",
    "mst",
    "cab",
    "iso",
    "img",
    "vhd",
    "vhdx",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceivedFileAction {
    OpenWithShell,
    RevealFolderOnly,
}

#[derive(Default)]
struct PendingOpenState {
    ready_files: HashMap<String, PathBuf>,
    requested_ids: HashSet<String>,
}

fn pending_open_files() -> &'static Mutex<PendingOpenState> {
    PENDING_OPEN_FILES.get_or_init(|| Mutex::new(PendingOpenState::default()))
}

pub fn set_open_on_linked_menu_enabled(enabled: bool) {
    OPEN_ON_LINKED_MENU.store(enabled, Ordering::SeqCst);
}

pub fn open_on_linked_menu_enabled() -> bool {
    OPEN_ON_LINKED_MENU.load(Ordering::Relaxed)
}

pub fn format_context_menu_command(exe_path: &str) -> String {
    format!("\"{exe_path}\" --open-on-linked \"%1\"")
}

pub fn format_context_menu_icon(exe_path: &str) -> String {
    format!("\"{exe_path}\",0")
}

/// Parse `--open-on-linked <path>` or `--open-on-linked=<path>` from CLI args.
pub fn parse_open_on_linked_arg(args: &[String]) -> Option<String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--open-on-linked" {
            if let Some(next) = iter.next() {
                let trimmed = next.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            return None;
        }
        if let Some(rest) = arg.strip_prefix("--open-on-linked=") {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
            return None;
        }
    }
    None
}

pub fn resolve_cli_file_path(raw: &str, cwd: &str) -> PathBuf {
    let p = PathBuf::from(raw);
    if p.is_absolute() || cwd.trim().is_empty() {
        p
    } else {
        PathBuf::from(cwd).join(p)
    }
}

/// Allow only `http://`, `https://`, and `mailto:` URLs with no control characters.
pub fn validate_open_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("URL cannot be empty".into());
    }
    if trimmed.len() > MAX_URL_LEN {
        return Err("URL is too long".into());
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err("URL contains invalid control characters".into());
    }
    let Some(colon) = trimmed.find(':') else {
        return Err("URL must include an http, https, or mailto scheme".into());
    };
    let scheme = trimmed[..colon].to_ascii_lowercase();
    let after_colon = &trimmed[colon + 1..];
    match scheme.as_str() {
        "http" | "https" => {
            if !after_colon.starts_with("//") {
                return Err(format!("{scheme} URL must start with {scheme}://"));
            }
            let rest = &after_colon[2..];
            let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
            if authority.is_empty()
                || authority.starts_with('.')
                || authority.chars().any(char::is_whitespace)
            {
                return Err(format!("{scheme} URL is missing a valid host"));
            }
            Ok(trimmed.to_string())
        }
        "mailto" => {
            if after_colon.starts_with("//") || after_colon.trim().is_empty() {
                return Err("mailto URL must specify a recipient".into());
            }
            let addr = after_colon.split('?').next().unwrap_or("");
            if addr.trim().is_empty() || addr.chars().any(char::is_whitespace) {
                return Err("mailto URL has an invalid recipient".into());
            }
            Ok(trimmed.to_string())
        }
        _ => Err(format!(
            "Blocked URL scheme '{scheme}': only http, https, and mailto are allowed"
        )),
    }
}

pub fn is_blocked_executable(path: &Path) -> bool {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return false;
    };
    let normalized = ext.trim_end_matches([' ', '.']).to_ascii_lowercase();
    BLOCKED_EXECUTABLE_EXTENSIONS.contains(&normalized.as_str())
}

pub fn decide_received_file_action(path: &Path) -> ReceivedFileAction {
    if is_blocked_executable(path) {
        ReceivedFileAction::RevealFolderOnly
    } else {
        ReceivedFileAction::OpenWithShell
    }
}

/// Register or remove the HKCU Explorer context menu entry at runtime.
pub fn sync_context_menu(enabled: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
        use windows::Win32::System::Registry::{
            RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
            KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
        };

        fn to_wide(s: &str) -> Vec<u16> {
            s.encode_utf16().chain(std::iter::once(0)).collect()
        }

        unsafe fn set_reg_sz(hkey: HKEY, name: Option<&str>, value: &str) -> Result<(), String> {
            let name_w = name.map(to_wide);
            let name_ptr = name_w
                .as_ref()
                .map_or(PCWSTR::null(), |v| PCWSTR(v.as_ptr()));
            let val_w = to_wide(value);
            let bytes = std::slice::from_raw_parts(val_w.as_ptr() as *const u8, val_w.len() * 2);
            let status = RegSetValueExW(hkey, name_ptr, None, REG_SZ, Some(bytes));
            if status != ERROR_SUCCESS {
                return Err(format!("RegSetValueExW failed: {:?}", status));
            }
            Ok(())
        }

        if !enabled {
            let subkey_w = to_wide(CONTEXT_MENU_REG_PATH);
            let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(subkey_w.as_ptr())) };
            if status != ERROR_SUCCESS && status != ERROR_FILE_NOT_FOUND {
                return Err(format!("RegDeleteTreeW failed: {:?}", status));
            }
            return Ok(());
        }

        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe_str = exe.to_string_lossy();
        let command_val = format_context_menu_command(&exe_str);
        let icon_val = format_context_menu_icon(&exe_str);

        unsafe {
            let mut shell_key = HKEY::default();
            let shell_path_w = to_wide(CONTEXT_MENU_REG_PATH);
            let status = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(shell_path_w.as_ptr()),
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut shell_key,
                None,
            );
            if status != ERROR_SUCCESS {
                return Err(format!("RegCreateKeyExW (shell) failed: {:?}", status));
            }
            let res_label = set_reg_sz(shell_key, None, "Open on Linked PC");
            let res_icon = set_reg_sz(shell_key, Some("Icon"), &icon_val);
            let _ = RegCloseKey(shell_key);
            res_label?;
            res_icon?;

            let mut cmd_key = HKEY::default();
            let cmd_path_w = to_wide(CONTEXT_MENU_CMD_REG_PATH);
            let status = RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(cmd_path_w.as_ptr()),
                None,
                None,
                REG_OPTION_NON_VOLATILE,
                KEY_WRITE,
                None,
                &mut cmd_key,
                None,
            );
            if status != ERROR_SUCCESS {
                return Err(format!("RegCreateKeyExW (command) failed: {:?}", status));
            }
            let res_cmd = set_reg_sz(cmd_key, None, &command_val);
            let _ = RegCloseKey(cmd_key);
            res_cmd?;
        }
        Ok(())
    }

    #[cfg(not(windows))]
    {
        let _ = enabled;
        Ok(())
    }
}

pub fn emit_open_progress(
    app: Option<&AppHandle>,
    id: &str,
    name: &str,
    pct: u8,
    state: &str,
    size_bytes: u64,
) {
    if let Some(app_handle) = app {
        let _ = app_handle.emit(
            "kvm-transfer-progress",
            serde_json::json!({
                "id": id,
                "kind": "open",
                "name": name,
                "pct": pct,
                "state": state,
                "sizeBytes": size_bytes,
            }),
        );
    }
}

pub fn emit_continuity_notice(app: &AppHandle, text: impl Into<String>) {
    let _ = app.emit(
        "kvm-continuity-notice",
        serde_json::json!({ "text": text.into() }),
    );
}

/// Stream a file for `OpenFile` on a QUIC unidirectional stream.
pub async fn send_open_file(
    conn: &quinn::Connection,
    path: &Path,
    transfer_id: &str,
    app: Option<&AppHandle>,
) -> Result<(), String> {
    send_open_file_with_events(conn, path, transfer_id, app, None).await
}

pub async fn send_open_file_with_events(
    conn: &quinn::Connection,
    path: &Path,
    transfer_id: &str,
    app: Option<&AppHandle>,
    transfer_tx: Option<&tokio::sync::broadcast::Sender<super::protocol::TransferEvent>>,
) -> Result<(), String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Invalid file name".to_string())?
        .to_string();
    let conn_id = conn.stable_id();

    let emit_step = |pct: u8, state: &str, size_bytes: u64| {
        emit_open_progress(app, transfer_id, &name, pct, state, size_bytes);
        if let Some(tx) = transfer_tx {
            let _ = tx.send(super::protocol::TransferEvent::Progress {
                id: transfer_id.to_string(),
                kind: "open".to_string(),
                name: name.clone(),
                pct,
                state: state.to_string(),
                size_bytes,
            });
        }
    };
    let emit_fail = |reason: &str| {
        if let Some(app_handle) = app {
            emit_continuity_notice(app_handle, format!("Failed to send \"{name}\": {reason}"));
        }
        if let Some(tx) = transfer_tx {
            let _ = tx.send(super::protocol::TransferEvent::Failed {
                id: transfer_id.to_string(),
                kind: "open".to_string(),
                name: name.clone(),
                reason: reason.to_string(),
            });
        }
    };

    let mut f = match tokio::fs::File::open(path).await {
        Ok(f) => f,
        Err(e) => {
            let msg = e.to_string();
            emit_fail(&msg);
            return Err(msg);
        }
    };
    let size = match f.metadata().await {
        Ok(m) => m.len(),
        Err(e) => {
            let msg = e.to_string();
            emit_fail(&msg);
            return Err(msg);
        }
    };
    let meta = super::files::TransferCancelMeta {
        id: transfer_id.to_string(),
        kind: "open".to_string(),
        name: name.clone(),
        size_bytes: size,
    };
    let handle_cancelled = |s: &mut quinn::SendStream| {
        let _ = s.reset(super::files::quic_cancel_varint());
        if super::files::emit_cancelled_once(conn_id, &meta, transfer_tx) {
            emit_open_progress(app, transfer_id, &name, 0, "cancelled", size);
        }
    };
    if super::files::global_cancel_registry().is_recently_cancelled(conn_id, transfer_id) {
        if super::files::emit_cancelled_once(conn_id, &meta, transfer_tx) {
            emit_open_progress(app, transfer_id, &name, 0, "cancelled", size);
        }
        return Err(super::files::TRANSFER_CANCELLED_ERR.to_string());
    }
    let (cancel_handle, _guard) =
        super::files::global_cancel_registry().register(conn_id, meta.clone());

    let mut s = match conn.open_uni().await {
        Ok(s) => s,
        Err(e) => {
            let msg = e.to_string();
            emit_fail(&msg);
            return Err(msg);
        }
    };
    if let Err(e) = write_frame(
        &mut s,
        &StreamHeader::OpenFile {
            transfer_id: transfer_id.to_string(),
            name: name.clone(),
            size,
        },
    )
    .await
    {
        let msg = e.to_string();
        emit_fail(&msg);
        return Err(msg);
    }

    let mut throttle = super::files::ProgressThrottle::new();
    if let Some(p) = throttle.should_emit(0, false) {
        emit_step(p, "active", size);
    }
    let mut buf = vec![0u8; super::files::TRANSFER_CHUNK_BYTES];
    let mut sent: u64 = 0;
    loop {
        if cancel_handle.is_cancelled() {
            handle_cancelled(&mut s);
            return Err(super::files::TRANSFER_CANCELLED_ERR.to_string());
        }
        let n = tokio::select! {
            biased;
            _ = cancel_handle.cancelled() => {
                handle_cancelled(&mut s);
                return Err(super::files::TRANSFER_CANCELLED_ERR.to_string());
            }
            res = f.read(&mut buf) => match res {
                Ok(n) => n,
                Err(e) => {
                    let msg = e.to_string();
                    emit_fail(&msg);
                    return Err(msg);
                }
            }
        };
        if n == 0 {
            break;
        }
        let write_res = tokio::select! {
            biased;
            _ = cancel_handle.cancelled() => {
                handle_cancelled(&mut s);
                return Err(super::files::TRANSFER_CANCELLED_ERR.to_string());
            }
            res = s.write_all(&buf[..n]) => res,
        };
        if let Err(e) = write_res {
            if super::files::is_quic_cancel_write_error(&e) || cancel_handle.is_cancelled() {
                handle_cancelled(&mut s);
                return Err(super::files::TRANSFER_CANCELLED_ERR.to_string());
            }
            let msg = e.to_string();
            emit_fail(&msg);
            return Err(msg);
        }
        sent += n as u64;
        if size > 0 {
            let pct = super::files::pct_for_bytes(sent, size).min(99);
            if let Some(p) = throttle.should_emit(pct, false) {
                emit_step(p, "active", size);
            }
        }
    }
    if cancel_handle.is_cancelled() {
        handle_cancelled(&mut s);
        return Err(super::files::TRANSFER_CANCELLED_ERR.to_string());
    }
    if let Err(e) = s.finish() {
        let msg = e.to_string();
        emit_fail(&msg);
        return Err(msg);
    }
    let stopped_res = tokio::select! {
        biased;
        _ = cancel_handle.cancelled() => {
            handle_cancelled(&mut s);
            return Err(super::files::TRANSFER_CANCELLED_ERR.to_string());
        }
        res = s.stopped() => res,
    };
    match stopped_res {
        Ok(Some(code))
            if code.into_inner() == super::files::QUIC_CANCEL_ERROR_CODE as u64 =>
        {
            handle_cancelled(&mut s);
            return Err(super::files::TRANSFER_CANCELLED_ERR.to_string());
        }
        Ok(_) | Err(_) => {
            if cancel_handle.is_cancelled() {
                handle_cancelled(&mut s);
                return Err(super::files::TRANSFER_CANCELLED_ERR.to_string());
            }
        }
    }
    if let Some(p) = throttle.should_emit(100, true) {
        emit_step(p, "done", size);
    }
    Ok(())
}

fn record_transfer_ready_in(state: &mut PendingOpenState, transfer_id: &str, path: PathBuf) -> Option<PathBuf> {
    if state.requested_ids.remove(transfer_id) {
        Some(path)
    } else {
        if state.ready_files.len() >= MAX_PENDING_ENTRIES {
            state.ready_files.clear();
        }
        state.ready_files.insert(transfer_id.to_string(), path);
        None
    }
}

fn record_open_packet_in(state: &mut PendingOpenState, transfer_id: &str) -> Option<PathBuf> {
    if let Some(path) = state.ready_files.remove(transfer_id) {
        Some(path)
    } else {
        if state.requested_ids.len() >= MAX_PENDING_ENTRIES {
            state.requested_ids.clear();
        }
        state.requested_ids.insert(transfer_id.to_string());
        None
    }
}

pub fn handle_open_file_transfer_ready(app: &AppHandle, transfer_id: &str, saved_path: PathBuf) {
    let maybe_path = {
        let mut guard = pending_open_files()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        record_transfer_ready_in(&mut guard, transfer_id, saved_path)
    };
    if let Some(path) = maybe_path {
        execute_received_file(app, path);
    }
}

pub fn handle_open_file_packet(app: &AppHandle, transfer_id: &str) {
    let maybe_path = {
        let mut guard = pending_open_files()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        record_open_packet_in(&mut guard, transfer_id)
    };
    if let Some(path) = maybe_path {
        execute_received_file(app, path);
    }
}

pub fn cancel_pending_open(transfer_id: &str) {
    if let Ok(mut guard) = pending_open_files().lock() {
        guard.requested_ids.remove(transfer_id);
        if let Some(ready_path) = guard.ready_files.remove(transfer_id) {
            let _ = std::fs::remove_file(ready_path);
        }
    }
    if let Some(dir) = open_staging_dir_for(&open_staging_root(), transfer_id) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

pub fn handle_open_url_packet(app: &AppHandle, raw_url: &str) {
    let validated = match validate_open_url(raw_url) {
        Ok(u) => u,
        Err(reason) => {
            emit_continuity_notice(app, format!("Rejected link from peer: {reason}"));
            return;
        }
    };
    let app_h = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = shell_open_target(&validated) {
            emit_continuity_notice(&app_h, format!("Could not open link: {e}"));
        }
    });
}

pub fn execute_received_file(app: &AppHandle, path: PathBuf) {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    let action = decide_received_file_action(&path);
    let app_h = app.clone();
    match action {
        ReceivedFileAction::RevealFolderOnly => {
            emit_continuity_notice(
                app,
                format!("Blocked auto-open for \"{name}\" (executable/script) — revealed folder instead."),
            );
            std::thread::spawn(move || {
                if let Err(e) = reveal_in_explorer(&path) {
                    emit_continuity_notice(&app_h, format!("Could not reveal \"{name}\": {e}"));
                }
            });
        }
        ReceivedFileAction::OpenWithShell => {
            std::thread::spawn(move || {
                let target = path.to_string_lossy().into_owned();
                match shell_open_target(&target) {
                    Ok(()) => {
                        emit_continuity_notice(&app_h, format_opened_temporary_notice(&name));
                    }
                    Err(e) => {
                        emit_continuity_notice(&app_h, format!("Could not open \"{name}\": {e}"));
                    }
                }
            });
        }
    }
}

fn shell_open_target(target: &str) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let target_w: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
        let res = ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(target_w.as_ptr()),
            None,
            None,
            SW_SHOWNORMAL,
        );
        if (res.0 as isize) <= 32 {
            return Err(format!("ShellExecuteW failed (code {})", res.0 as isize));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = target;
        Err("ShellExecute is only supported on Windows".into())
    }
}

fn reveal_in_explorer(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    unsafe {
        use windows::core::{w, PCWSTR};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let args = format!("/select,\"{}\"", path.display());
        let args_w: Vec<u16> = args.encode_utf16().chain(std::iter::once(0)).collect();
        let res = ShellExecuteW(
            None,
            w!("open"),
            w!("explorer.exe"),
            PCWSTR(args_w.as_ptr()),
            None,
            SW_SHOWNORMAL,
        );
        if (res.0 as isize) <= 32 {
            return Err(format!(
                "ShellExecuteW (explorer.exe) failed (code {})",
                res.0 as isize
            ));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err("Explorer reveal is only supported on Windows".into())
    }
}

/// Dispatch `--open-on-linked <path>` received from CLI or single-instance forwarding.
pub fn handle_cli_open_on_linked(app: &AppHandle, path: PathBuf) {
    if !path.is_file() {
        emit_continuity_notice(
            app,
            format!("Cannot open on linked PC: \"{}\" is not a file.", path.display()),
        );
        return;
    }
    let app_h = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = send_and_open_file_from_app(&app_h, path).await {
            emit_continuity_notice(&app_h, format!("Open on Linked PC failed: {e}"));
        }
    });
}

pub async fn send_and_open_file_from_app(app: &AppHandle, path: PathBuf) -> Result<(), String> {
    if !path.is_file() {
        return Err("File does not exist".into());
    }
    let transfer_id = uuid::Uuid::new_v4().to_string();

    enum Transport {
        Client(quinn::Connection, tokio::sync::mpsc::Sender<KvmPacket>),
        Server(tokio::sync::broadcast::Sender<Outbound>),
    }

    let transport = {
        let state = app.state::<AppState>();
        let kvm = state.kvm.lock().map_err(|e| format!("Mutex lock poisoned: {e}"))?;
        if let Some(ref client) = kvm.client {
            if client.is_connected.load(Ordering::Relaxed) {
                Transport::Client(client.conn.clone(), client.send_packet_tx.clone())
            } else {
                return Err("No connected linked PC".into());
            }
        } else if let Some(ref server) = kvm.server {
            if server.is_running.load(Ordering::Relaxed) && !kvm.connected_peers.is_empty() {
                Transport::Server(server.outgoing_tx.clone())
            } else {
                return Err("No connected linked PC".into());
            }
        } else {
            return Err("No connected linked PC".into());
        }
    };

    match transport {
        Transport::Client(conn, pkt_tx) => {
            send_open_file(&conn, &path, &transfer_id, Some(app)).await?;
            pkt_tx
                .send(KvmPacket::OpenFile { transfer_id })
                .await
                .map_err(|e| format!("Failed to send OpenFile packet: {e}"))?;
        }
        Transport::Server(out_tx) => {
            out_tx
                .send(Outbound::OpenFile {
                    transfer_id: transfer_id.clone(),
                    path,
                })
                .map_err(|e| format!("Failed to queue OpenFile transfer: {e}"))?;
        }
    }
    Ok(())
}

pub fn send_open_url_from_state(state: &AppState, raw_url: &str) -> Result<(), String> {
    let url = validate_open_url(raw_url)?;
    let kvm = state.kvm.lock().map_err(|e| format!("Mutex lock poisoned: {e}"))?;
    if let Some(ref client) = kvm.client {
        if client.is_connected.load(Ordering::Relaxed) {
            return client
                .send_packet_tx
                .try_send(KvmPacket::OpenUrl { url })
                .map_err(|e| format!("Failed to send OpenUrl: {e}"));
        }
    }
    if let Some(ref server) = kvm.server {
        if server.is_running.load(Ordering::Relaxed) && !kvm.connected_peers.is_empty() {
            return server
                .outgoing_tx
                .send(Outbound::Packet(KvmPacket::OpenUrl { url }))
                .map(|_| ())
                .map_err(|e| format!("Failed to send OpenUrl: {e}"));
        }
    }
    Err("No connected linked PC".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_allow_list_checks() {
        assert_eq!(
            validate_open_url("https://example.com/path?a=1#frag").unwrap(),
            "https://example.com/path?a=1#frag"
        );
        assert_eq!(
            validate_open_url("  HTTP://localhost:8080/test  ").unwrap(),
            "HTTP://localhost:8080/test"
        );
        assert_eq!(
            validate_open_url("mailto:user@example.com?subject=Hi").unwrap(),
            "mailto:user@example.com?subject=Hi"
        );
        assert_eq!(
            validate_open_url("MAILTO:test@example.org").unwrap(),
            "MAILTO:test@example.org"
        );

        for bad in [
            "",
            "   ",
            "example.com",
            "http://",
            "https:///path",
            "http://.bad",
            "file:///C:/Windows/System32/cmd.exe",
            "javascript:alert(1)",
            "ftp://example.com/a",
            "cmd:calc",
            "ms-msdt:/id",
            "mailto:",
            "mailto://bad",
            "https://example.com/a\nb",
            "https://example.com/a\0b",
        ] {
            assert!(validate_open_url(bad).is_err(), "expected err for {bad:?}");
        }
    }

    #[test]
    fn executable_extension_blocklist() {
        for ext in [
            "exe",
            "EXE",
            "msi",
            "bat",
            "cmd",
            "ps1",
            "Ps1",
            "vbs",
            "js",
            "jse",
            "wsf",
            "lnk",
            "scr",
            "com",
            "hta",
            "pif",
            "cpl",
            "msc",
            "jar",
            "reg",
            "vbe",
            "wsh",
            "psm1",
            "psd1",
            "ps1xml",
            "Ps1Xml",
            "scf",
            "url",
            "appref-ms",
            "application",
            "msix",
            "msixbundle",
            "MsixBundle",
            "appx",
            "appxbundle",
            "settingcontent-ms",
            "library-ms",
            "chm",
            "gadget",
            "inf",
            "msp",
            "mst",
            "cab",
            "iso",
            "img",
            "vhd",
            "vhdx",
        ] {
            let p = PathBuf::from(format!(r"C:\Downloads\payload.{ext}"));
            assert!(is_blocked_executable(&p), "expected blocked for .{ext}");
            assert_eq!(
                decide_received_file_action(&p),
                ReceivedFileAction::RevealFolderOnly,
                "expected RevealFolderOnly for .{ext}"
            );
        }

        for ext in BLOCKED_EXECUTABLE_EXTENSIONS {
            let p = PathBuf::from(format!(r"C:\Downloads\payload.{ext}"));
            assert!(is_blocked_executable(&p), "expected blocked for .{ext}");
        }

        for safe in [
            "report.pdf",
            "notes.txt",
            "photo.png",
            "clip.mp4",
            "archive.zip",
            "README",
        ] {
            let p = PathBuf::from(format!(r"C:\Downloads\{safe}"));
            assert!(!is_blocked_executable(&p), "expected safe for {safe}");
            assert_eq!(
                decide_received_file_action(&p),
                ReceivedFileAction::OpenWithShell,
                "expected OpenWithShell for {safe}"
            );
        }
    }

    #[test]
    fn cli_parsing_open_on_linked() {
        assert_eq!(
            parse_open_on_linked_arg(&[
                "anyleap.exe".into(),
                "--open-on-linked".into(),
                r"C:\Users\Tama\doc.pdf".into()
            ]),
            Some(r"C:\Users\Tama\doc.pdf".into())
        );
        assert_eq!(
            parse_open_on_linked_arg(&[
                "anyleap.exe".into(),
                r"--open-on-linked=C:\Users\Tama\doc.pdf".into()
            ]),
            Some(r"C:\Users\Tama\doc.pdf".into())
        );
        assert_eq!(
            parse_open_on_linked_arg(&["anyleap.exe".into(), "--open-on-linked".into()]),
            None
        );
        assert_eq!(
            parse_open_on_linked_arg(&["anyleap.exe".into(), "--open-on-linked=   ".into()]),
            None
        );
        assert_eq!(parse_open_on_linked_arg(&["anyleap.exe".into()]), None);
        assert_eq!(
            format_context_menu_command(r"C:\Program Files\AnyLeap\anyleap.exe"),
            "\"C:\\Program Files\\AnyLeap\\anyleap.exe\" --open-on-linked \"%1\""
        );
    }

    #[test]
    fn pending_open_file_coordination_handles_both_orders() {
        let mut st = PendingOpenState::default();
        // Order A: stream finishes before packet
        assert_eq!(
            record_transfer_ready_in(&mut st, "id-1", PathBuf::from("a.pdf")),
            None
        );
        assert_eq!(
            record_open_packet_in(&mut st, "id-1"),
            Some(PathBuf::from("a.pdf"))
        );

        // Order B: packet arrives before stream finishes
        assert_eq!(record_open_packet_in(&mut st, "id-2"), None);
        assert_eq!(
            record_transfer_ready_in(&mut st, "id-2", PathBuf::from("b.pdf")),
            Some(PathBuf::from("b.pdf"))
        );
    }

    #[test]
    fn open_staging_path_and_24h_expiry_cleanup() {
        let root = std::env::temp_dir().join(format!("anyleap-open-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        assert_eq!(open_staging_subdir_name("abc-123").as_deref(), Some("open-abc-123"));
        assert_eq!(open_staging_subdir_name("../evil"), None);
        assert_eq!(open_staging_subdir_name(""), None);
        assert_eq!(
            open_staging_dir_for(&root, "tid-9"),
            Some(root.join("open-tid-9"))
        );
        assert_eq!(
            format_opened_temporary_notice("report.pdf"),
            "Opened report.pdf (temporary copy, removed after 24 h)"
        );

        let dir = open_staging_dir_for(&root, "tid-9").unwrap();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("report.pdf"), b"data").unwrap();

        let modified = std::fs::metadata(&dir).unwrap().modified().unwrap();
        let before_24h = modified + std::time::Duration::from_secs(23 * 3600);
        let at_24h = modified + OPEN_TEMP_MAX_AGE;
        let after_24h = modified + OPEN_TEMP_MAX_AGE + std::time::Duration::from_secs(5);
        assert!(!is_open_temp_expired(modified, before_24h, OPEN_TEMP_MAX_AGE));
        assert!(!is_open_temp_expired(modified, at_24h, OPEN_TEMP_MAX_AGE));
        assert!(is_open_temp_expired(modified, after_24h, OPEN_TEMP_MAX_AGE));

        assert_eq!(
            cleanup_expired_open_staging(&root, OPEN_TEMP_MAX_AGE, before_24h),
            0
        );
        assert!(dir.exists());
        assert_eq!(
            cleanup_expired_open_staging(&root, OPEN_TEMP_MAX_AGE, after_24h),
            1
        );
        assert!(!dir.exists());

        let _ = std::fs::remove_dir_all(&root);
    }
}
