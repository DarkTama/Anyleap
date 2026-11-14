use super::input_logic::{Edge, TrackedKey};
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Mutex, OnceLock};

pub const DEBUG_LOG_FILE_NAME: &str = "kvm-debug.log";
pub const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
const CHANNEL_CAPACITY: usize = 2048;

static DEBUG_LOG_ENABLED: AtomicBool = AtomicBool::new(false);
static DROPPED_EVENTS: AtomicU64 = AtomicU64::new(0);
static SENDER: OnceLock<mpsc::SyncSender<DebugEvent>> = OnceLock::new();
static LOG_PATH_STATE: OnceLock<Mutex<(PathBuf, u64)>> = OnceLock::new();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyDecision {
    Pass,
    Block,
    Forward,
}

impl KeyDecision {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Block => "block",
            Self::Forward => "forward",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookKind {
    Keyboard,
    Mouse,
}

impl HookKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keyboard => "keyboard",
            Self::Mouse => "mouse",
        }
    }
}

pub const KEY_SNAPSHOT_CAP: usize = 16;

#[derive(Debug, Clone, Copy)]
pub struct KeySnapshot {
    pub lock_ok: bool,
    pub total_count: usize,
    pub len: u8,
    pub keys: [TrackedKey; KEY_SNAPSHOT_CAP],
}

impl KeySnapshot {
    pub fn lock_failed() -> Self {
        Self {
            lock_ok: false,
            total_count: 0,
            len: 0,
            keys: [TrackedKey {
                key_code: 0,
                scan_code: 0,
                extended: false,
            }; KEY_SNAPSHOT_CAP],
        }
    }

    pub fn from_slice(slice: &[TrackedKey]) -> Self {
        let mut keys = [TrackedKey {
            key_code: 0,
            scan_code: 0,
            extended: false,
        }; KEY_SNAPSHOT_CAP];
        let count = slice.len().min(KEY_SNAPSHOT_CAP);
        keys[..count].copy_from_slice(&slice[..count]);
        Self {
            lock_ok: true,
            total_count: slice.len(),
            len: count as u8,
            keys,
        }
    }

    pub fn format_vks(&self) -> String {
        if !self.lock_ok {
            return "lock_failed".to_string();
        }
        let mut parts: Vec<String> = self.keys[..self.len as usize]
            .iter()
            .map(|k| format!("0x{:02X}", k.key_code))
            .collect();
        if self.total_count > self.len as usize {
            parts.push(format!("...+{}", self.total_count - self.len as usize));
        }
        format!("[{}]", parts.join(","))
    }

    pub fn format_detailed(&self) -> String {
        if !self.lock_ok {
            return "lock_failed".to_string();
        }
        let mut parts: Vec<String> = self.keys[..self.len as usize]
            .iter()
            .map(|k| {
                format!(
                    "0x{:02X}(scan=0x{:02X},ext={})",
                    k.key_code,
                    k.scan_code,
                    u8::from(k.extended)
                )
            })
            .collect();
        if self.total_count > self.len as usize {
            parts.push(format!("...+{}", self.total_count - self.len as usize));
        }
        format!("[{}]", parts.join(","))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElevationState {
    Elevated,
    NotElevated,
    Unknown,
    Err(u32),
}

impl ElevationState {
    pub fn format_val(self) -> String {
        match self {
            Self::Elevated => "1".to_string(),
            Self::NotElevated => "0".to_string(),
            Self::Unknown => "na".to_string(),
            Self::Err(code) => format!("err({code})"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct WindowDetails {
    pub proc_name: String,
    pub title: String,
    pub elevation: ElevationState,
}

#[derive(Debug, Clone)]
pub enum DebugEvent {
    Key {
        ts_us: u64,
        vk: u16,
        scan_code: u16,
        is_down: bool,
        extended: bool,
        injected: bool,
        is_trapped: bool,
        decision: KeyDecision,
        hwnd: isize,
    },
    Engage {
        ts_us: u64,
        phase: &'static str,
        edge: Edge,
        relative_y: f32,
        hwnd: isize,
    },
    EngagePerform {
        ts_us: u64,
        edge: Edge,
        relative_y: f32,
        hwnd: isize,
        tracker_before: KeySnapshot,
        released: KeySnapshot,
        send_input_us: u64,
    },
    Return {
        ts_us: u64,
        relative_y: f32,
        was_trapped: bool,
        hwnd: isize,
    },
    Release {
        ts_us: u64,
        reason: &'static str,
    },
    SlowHook {
        ts_us: u64,
        kind: HookKind,
        duration_us: u64,
    },
    HookGap {
        ts_us: u64,
        kind: HookKind,
        gap_us: u64,
    },
    ReceiverEnter {
        ts_us: u64,
        edge: Edge,
        relative_y: f32,
        pos_x: i32,
        pos_y: i32,
        hwnd: isize,
    },
    ReturnSent {
        ts_us: u64,
        edge: Edge,
        relative_y: f32,
        released_keys: usize,
        released_buttons: usize,
    },
    ReleaseAll {
        ts_us: u64,
        reason: &'static str,
        released_keys: usize,
        released_buttons: usize,
    },
    RemoteKey {
        ts_us: u64,
        vk: u16,
        scan_code: u16,
        is_down: bool,
        extended: bool,
        sent: u32,
        last_err: u32,
        hwnd: isize,
        focus_hwnd: Option<isize>,
    },
    RemoteMouseButton {
        ts_us: u64,
        button: u8,
        is_down: bool,
        sent: u32,
        last_err: u32,
        hwnd: isize,
        focus_hwnd: Option<isize>,
    },
    Note {
        ts_us: u64,
        hwnd: isize,
        text: String,
    },
}

pub fn log_path_for_dir(log_dir: &Path) -> PathBuf {
    log_dir.join(DEBUG_LOG_FILE_NAME)
}

#[inline]
pub fn is_enabled() -> bool {
    DEBUG_LOG_ENABLED.load(Ordering::Relaxed)
}

pub fn now_us() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

#[inline]
pub fn foreground_hwnd_raw() -> isize {
    #[cfg(windows)]
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow().0 as isize
    }
    #[cfg(not(windows))]
    {
        0
    }
}

#[inline]
pub fn foreground_focus_hwnd_raw(fg_hwnd_raw: isize) -> Option<isize> {
    #[cfg(windows)]
    unsafe {
        if fg_hwnd_raw == 0 {
            return Some(0);
        }
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
        };
        let fg_hwnd = HWND(fg_hwnd_raw as *mut core::ffi::c_void);
        let tid = GetWindowThreadProcessId(fg_hwnd, None);
        if tid == 0 {
            return None;
        }
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        if GetGUIThreadInfo(tid, &mut info).is_ok() {
            Some(info.hwndFocus.0 as isize)
        } else {
            None
        }
    }
    #[cfg(not(windows))]
    {
        let _ = fg_hwnd_raw;
        None
    }
}

fn path_state() -> &'static Mutex<(PathBuf, u64)> {
    LOG_PATH_STATE.get_or_init(|| Mutex::new((PathBuf::new(), 0)))
}

fn ensure_sender() -> &'static mpsc::SyncSender<DebugEvent> {
    SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::sync_channel::<DebugEvent>(CHANNEL_CAPACITY);
        let _ = std::thread::Builder::new()
            .name("kvm-debug-log".into())
            .spawn(move || writer_loop(rx));
        tx
    })
}

pub fn set_enabled(enabled: bool, log_path: PathBuf) {
    if enabled {
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut guard) = path_state().lock() {
            guard.0 = log_path;
            guard.1 = guard.1.wrapping_add(1);
        }
        let _ = ensure_sender();
        DEBUG_LOG_ENABLED.store(true, Ordering::Release);
    } else {
        DEBUG_LOG_ENABLED.store(false, Ordering::Release);
    }
}

/// Non-blocking enqueue from hook callbacks or hook thread.
#[inline]
pub fn record_event(event: DebugEvent) {
    if !is_enabled() {
        return;
    }
    if let Some(tx) = SENDER.get() {
        if tx.try_send(event).is_err() {
            DROPPED_EVENTS.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn sanitize_log_str(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\r' | '\n' | '\t' => ' ',
            '"' => '\'',
            _ => c,
        })
        .collect()
}

#[cfg(windows)]
fn query_process_elevation(handle: windows::Win32::Foundation::HANDLE) -> ElevationState {
    use windows::Win32::Foundation::{CloseHandle, GetLastError, HANDLE};
    use windows::Win32::Security::{
        GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
    };
    use windows::Win32::System::Threading::OpenProcessToken;

    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(handle, TOKEN_QUERY, &mut token).is_err() {
            return ElevationState::Err(GetLastError().0);
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut ret_len = 0u32;
        let res = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut core::ffi::c_void),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut ret_len,
        );
        let err = if res.is_err() { GetLastError().0 } else { 0 };
        let _ = CloseHandle(token);
        if res.is_ok() {
            if elev.TokenIsElevated != 0 {
                ElevationState::Elevated
            } else {
                ElevationState::NotElevated
            }
        } else {
            ElevationState::Err(err)
        }
    }
}

#[cfg(windows)]
fn resolve_window_details(hwnd_raw: isize, check_elevation: bool) -> WindowDetails {
    if hwnd_raw == 0 {
        return WindowDetails {
            proc_name: "none".to_string(),
            title: String::new(),
            elevation: ElevationState::Unknown,
        };
    }
    use windows::Win32::Foundation::{CloseHandle, GetLastError, HWND};
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
        PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowTextW, GetWindowThreadProcessId};

    unsafe {
        let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
        let mut title_buf = [0u16; 256];
        let title_len = GetWindowTextW(hwnd, &mut title_buf);
        let title = if title_len > 0 {
            sanitize_log_str(&String::from_utf16_lossy(&title_buf[..title_len as usize]))
        } else {
            String::new()
        };

        let mut pid: u32 = 0;
        let _ = GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return WindowDetails {
                proc_name: "unknown".to_string(),
                title,
                elevation: ElevationState::Unknown,
            };
        }

        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => {
                let mut path_buf = [0u16; 512];
                let mut len = path_buf.len() as u32;
                let ok = QueryFullProcessImageNameW(
                    handle,
                    PROCESS_NAME_FORMAT(0),
                    windows::core::PWSTR(path_buf.as_mut_ptr()),
                    &mut len,
                )
                .is_ok();
                let elevation = if check_elevation {
                    query_process_elevation(handle)
                } else {
                    ElevationState::Unknown
                };
                let _ = CloseHandle(handle);
                let proc_name = if ok && len > 0 {
                    let full = String::from_utf16_lossy(&path_buf[..len as usize]);
                    let name = Path::new(&full)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(&full);
                    sanitize_log_str(name)
                } else {
                    format!("pid:{pid}")
                };
                WindowDetails {
                    proc_name,
                    title,
                    elevation,
                }
            }
            Err(_) => {
                let err = GetLastError().0;
                WindowDetails {
                    proc_name: format!("pid:{pid}"),
                    title,
                    elevation: if check_elevation {
                        ElevationState::Err(err)
                    } else {
                        ElevationState::Unknown
                    },
                }
            }
        }
    }
}

#[cfg(not(windows))]
fn resolve_window_details(_hwnd_raw: isize, _check_elevation: bool) -> WindowDetails {
    WindowDetails {
        proc_name: "non-windows".to_string(),
        title: String::new(),
        elevation: ElevationState::Unknown,
    }
}

fn format_focus_hwnd(focus_hwnd: Option<isize>) -> String {
    match focus_hwnd {
        Some(h) => format!("0x{h:X}"),
        None => "err".to_string(),
    }
}

fn format_event_line(
    event: &DebugEvent,
    window_resolver: impl Fn(isize, bool) -> WindowDetails,
) -> String {
    match event {
        DebugEvent::Key {
            ts_us,
            vk,
            scan_code,
            is_down,
            extended,
            injected,
            is_trapped,
            decision,
            hwnd,
        } => {
            let w = window_resolver(*hwnd, false);
            format!(
                "ts_us={ts_us} event=KEY vk=0x{vk:02X} scan=0x{scan_code:02X} state={} ext={} injected={} trapped={} decision={} hwnd=0x{hwnd:X} proc=\"{}\" title=\"{}\"\n",
                if *is_down { "DOWN" } else { "UP" },
                u8::from(*extended),
                u8::from(*injected),
                u8::from(*is_trapped),
                decision.as_str(),
                w.proc_name,
                w.title,
            )
        }
        DebugEvent::Engage {
            ts_us,
            phase,
            edge,
            relative_y,
            hwnd,
        } => {
            let w = window_resolver(*hwnd, false);
            format!(
                "ts_us={ts_us} event=ENGAGE phase={phase} edge={} rel_y={relative_y:.4} hwnd=0x{hwnd:X} proc=\"{}\" title=\"{}\"\n",
                edge.as_str(),
                w.proc_name,
                w.title,
            )
        }
        DebugEvent::EngagePerform {
            ts_us,
            edge,
            relative_y,
            hwnd,
            tracker_before,
            released,
            send_input_us,
        } => {
            let w = window_resolver(*hwnd, false);
            format!(
                "ts_us={ts_us} event=ENGAGE phase=perform edge={} rel_y={relative_y:.4} released_count={} released_vks={} tracker_before={} send_input_us={send_input_us} hwnd=0x{hwnd:X} proc=\"{}\" title=\"{}\"\n",
                edge.as_str(),
                released.total_count,
                released.format_vks(),
                tracker_before.format_detailed(),
                w.proc_name,
                w.title,
            )
        }
        DebugEvent::Return {
            ts_us,
            relative_y,
            was_trapped,
            hwnd,
        } => {
            let w = window_resolver(*hwnd, false);
            format!(
                "ts_us={ts_us} event=RETURN rel_y={relative_y:.4} was_trapped={} hwnd=0x{hwnd:X} proc=\"{}\" title=\"{}\"\n",
                u8::from(*was_trapped),
                w.proc_name,
                w.title,
            )
        }
        DebugEvent::Release { ts_us, reason } => {
            format!("ts_us={ts_us} event=RELEASE reason={reason}\n")
        }
        DebugEvent::SlowHook {
            ts_us,
            kind,
            duration_us,
        } => {
            format!(
                "ts_us={ts_us} event=SLOW_HOOK kind={} duration_us={duration_us}\n",
                kind.as_str(),
            )
        }
        DebugEvent::HookGap {
            ts_us,
            kind,
            gap_us,
        } => {
            format!(
                "ts_us={ts_us} event=HOOK_GAP kind={} gap_us={gap_us}\n",
                kind.as_str(),
            )
        }
        DebugEvent::ReceiverEnter {
            ts_us,
            edge,
            relative_y,
            pos_x,
            pos_y,
            hwnd,
        } => {
            let w = window_resolver(*hwnd, false);
            format!(
                "ts_us={ts_us} event=ENTER edge={} rel_y={relative_y:.4} x={pos_x} y={pos_y} hwnd=0x{hwnd:X} proc=\"{}\" title=\"{}\"\n",
                edge.as_str(),
                w.proc_name,
                w.title,
            )
        }
        DebugEvent::ReturnSent {
            ts_us,
            edge,
            relative_y,
            released_keys,
            released_buttons,
        } => {
            format!(
                "ts_us={ts_us} event=RETURN_SENT edge={} rel_y={relative_y:.4} released_keys={released_keys} released_buttons={released_buttons}\n",
                edge.as_str(),
            )
        }
        DebugEvent::ReleaseAll {
            ts_us,
            reason,
            released_keys,
            released_buttons,
        } => {
            format!(
                "ts_us={ts_us} event=RELEASE_ALL reason={reason} released_keys={released_keys} released_buttons={released_buttons}\n",
            )
        }
        DebugEvent::RemoteKey {
            ts_us,
            vk,
            scan_code,
            is_down,
            extended,
            sent,
            last_err,
            hwnd,
            focus_hwnd,
        } => {
            let w = window_resolver(*hwnd, true);
            format!(
                "ts_us={ts_us} event=REMOTE_KEY vk=0x{vk:02X} scan=0x{scan_code:02X} state={} ext={} sent={sent} last_err={last_err} hwnd=0x{hwnd:X} focus_hwnd={} elevated={} proc=\"{}\" title=\"{}\"\n",
                if *is_down { "DOWN" } else { "UP" },
                u8::from(*extended),
                format_focus_hwnd(*focus_hwnd),
                w.elevation.format_val(),
                w.proc_name,
                w.title,
            )
        }
        DebugEvent::RemoteMouseButton {
            ts_us,
            button,
            is_down,
            sent,
            last_err,
            hwnd,
            focus_hwnd,
        } => {
            let w = window_resolver(*hwnd, true);
            format!(
                "ts_us={ts_us} event=REMOTE_MOUSE_BTN button={button} state={} sent={sent} last_err={last_err} hwnd=0x{hwnd:X} focus_hwnd={} elevated={} proc=\"{}\" title=\"{}\"\n",
                if *is_down { "DOWN" } else { "UP" },
                format_focus_hwnd(*focus_hwnd),
                w.elevation.format_val(),
                w.proc_name,
                w.title,
            )
        }
        DebugEvent::Note { ts_us, hwnd, text } => {
            let w = window_resolver(*hwnd, false);
            let clean = sanitize_log_str(text);
            format!(
                "ts_us={ts_us} event=NOTE text=\"{clean}\" hwnd=0x{hwnd:X} proc=\"{}\" title=\"{}\"\n",
                w.proc_name,
                w.title,
            )
        }
    }
}

struct RotatingLogWriter {
    path: PathBuf,
    writer: Option<BufWriter<File>>,
    written_bytes: u64,
    max_bytes: u64,
}

impl RotatingLogWriter {
    fn new(path: PathBuf, max_bytes: u64) -> Self {
        Self {
            path,
            writer: None,
            written_bytes: 0,
            max_bytes,
        }
    }

    fn set_path(&mut self, path: PathBuf) {
        if self.path != path {
            if let Some(ref mut w) = self.writer {
                let _ = w.flush();
            }
            self.writer = None;
            self.written_bytes = 0;
            self.path = path;
        }
    }

    fn ensure_open(&mut self) -> std::io::Result<()> {
        if self.writer.is_some() {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let existing_len = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.written_bytes = existing_len;
        self.writer = Some(BufWriter::new(file));
        Ok(())
    }

    fn rotate(&mut self) -> std::io::Result<()> {
        if let Some(mut w) = self.writer.take() {
            let _ = w.flush();
        }
        let backup = self.path.with_extension("log.1");
        let _ = std::fs::remove_file(&backup);
        if std::fs::rename(&self.path, &backup).is_err() {
            let _ = File::create(&self.path);
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        self.written_bytes = 0;
        self.writer = Some(BufWriter::new(file));
        Ok(())
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        if self.path.as_os_str().is_empty() {
            return Ok(());
        }
        self.ensure_open()?;
        if self.written_bytes > 0 && self.written_bytes + line.len() as u64 > self.max_bytes {
            self.rotate()?;
        }
        if let Some(ref mut w) = self.writer {
            w.write_all(line.as_bytes())?;
            self.written_bytes += line.len() as u64;
        }
        Ok(())
    }

    fn flush(&mut self) {
        if let Some(ref mut w) = self.writer {
            let _ = w.flush();
        }
    }
}

fn writer_loop(rx: mpsc::Receiver<DebugEvent>) {
    let mut writer = RotatingLogWriter::new(PathBuf::new(), MAX_LOG_BYTES);
    let mut seen_gen = 0u64;

    while let Ok(first) = rx.recv() {
        if let Ok(guard) = path_state().lock() {
            if guard.1 != seen_gen {
                seen_gen = guard.1;
                writer.set_path(guard.0.clone());
            }
        }

        let line = format_event_line(&first, resolve_window_details);
        let _ = writer.write_line(&line);

        while let Ok(ev) = rx.try_recv() {
            let line = format_event_line(&ev, resolve_window_details);
            let _ = writer.write_line(&line);
        }

        let dropped = DROPPED_EVENTS.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            let note = format!(
                "ts_us={} event=DROPPED count={dropped}\n",
                now_us()
            );
            let _ = writer.write_line(&note);
        }
        writer.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stub_window(_hwnd: isize, check_elevation: bool) -> WindowDetails {
        WindowDetails {
            proc_name: "notepad.exe".into(),
            title: "Untitled - \"Note\"".into(),
            elevation: if check_elevation {
                ElevationState::NotElevated
            } else {
                ElevationState::Unknown
            },
        }
    }

    #[test]
    fn format_event_lines_include_expected_fields() {
        let key_line = format_event_line(
            &DebugEvent::Key {
                ts_us: 123456,
                vk: 0x41,
                scan_code: 0x1E,
                is_down: true,
                extended: false,
                injected: false,
                is_trapped: true,
                decision: KeyDecision::Forward,
                hwnd: 0x1234,
            },
            stub_window,
        );
        assert!(key_line.contains("ts_us=123456"));
        assert!(key_line.contains("event=KEY"));
        assert!(key_line.contains("vk=0x41"));
        assert!(key_line.contains("state=DOWN"));
        assert!(key_line.contains("injected=0"));
        assert!(key_line.contains("trapped=1"));
        assert!(key_line.contains("decision=forward"));
        assert!(key_line.contains("hwnd=0x1234"));
        assert!(key_line.contains("proc=\"notepad.exe\""));

        let slow_line = format_event_line(
            &DebugEvent::SlowHook {
                ts_us: 999,
                kind: HookKind::Keyboard,
                duration_us: 25_100,
            },
            |_, _| unreachable!(),
        );
        assert!(slow_line.contains("event=SLOW_HOOK"));
        assert!(slow_line.contains("kind=keyboard"));
        assert!(slow_line.contains("duration_us=25100"));

        let gap_line = format_event_line(
            &DebugEvent::HookGap {
                ts_us: 2000,
                kind: HookKind::Mouse,
                gap_us: 310_000,
            },
            |_, _| unreachable!(),
        );
        assert!(gap_line.contains("event=HOOK_GAP"));
        assert!(gap_line.contains("kind=mouse"));
        assert!(gap_line.contains("gap_us=310000"));

        let snap = KeySnapshot::from_slice(&[TrackedKey {
            key_code: 0x5B,
            scan_code: 0x5B,
            extended: true,
        }]);
        let engage_perform = format_event_line(
            &DebugEvent::EngagePerform {
                ts_us: 3000,
                edge: Edge::Right,
                relative_y: 0.5,
                hwnd: 0xABCD,
                tracker_before: snap,
                released: snap,
                send_input_us: 420,
            },
            stub_window,
        );
        assert!(engage_perform.contains("event=ENGAGE phase=perform"));
        assert!(engage_perform.contains("released_count=1"));
        assert!(engage_perform.contains("released_vks=[0x5B]"));
        assert!(engage_perform.contains("tracker_before=[0x5B(scan=0x5B,ext=1)]"));
        assert!(engage_perform.contains("send_input_us=420"));

        let enter_line = format_event_line(
            &DebugEvent::ReceiverEnter {
                ts_us: 4000,
                edge: Edge::Left,
                relative_y: 0.25,
                pos_x: 2,
                pos_y: 270,
                hwnd: 0x55,
            },
            stub_window,
        );
        assert!(enter_line.contains("event=ENTER edge=left rel_y=0.2500 x=2 y=270 hwnd=0x55"));

        let return_sent = format_event_line(
            &DebugEvent::ReturnSent {
                ts_us: 5000,
                edge: Edge::Left,
                relative_y: 0.75,
                released_keys: 1,
                released_buttons: 0,
            },
            |_, _| unreachable!(),
        );
        assert!(return_sent.contains("event=RETURN_SENT edge=left rel_y=0.7500 released_keys=1 released_buttons=0"));

        let release_all = format_event_line(
            &DebugEvent::ReleaseAll {
                ts_us: 6000,
                reason: "release_all",
                released_keys: 2,
                released_buttons: 1,
            },
            |_, _| unreachable!(),
        );
        assert!(release_all.contains("event=RELEASE_ALL reason=release_all released_keys=2 released_buttons=1"));

        let remote_key = format_event_line(
            &DebugEvent::RemoteKey {
                ts_us: 7000,
                vk: 0x41,
                scan_code: 0x1E,
                is_down: true,
                extended: false,
                sent: 0,
                last_err: 5,
                hwnd: 0x100,
                focus_hwnd: Some(0x200),
            },
            stub_window,
        );
        assert!(remote_key.contains("event=REMOTE_KEY vk=0x41"));
        assert!(remote_key.contains("sent=0 last_err=5"));
        assert!(remote_key.contains("hwnd=0x100 focus_hwnd=0x200 elevated=0"));

        let remote_btn = format_event_line(
            &DebugEvent::RemoteMouseButton {
                ts_us: 8000,
                button: 1,
                is_down: true,
                sent: 1,
                last_err: 0,
                hwnd: 0x100,
                focus_hwnd: None,
            },
            stub_window,
        );
        assert!(remote_btn.contains("event=REMOTE_MOUSE_BTN button=1 state=DOWN sent=1 last_err=0"));
        assert!(remote_btn.contains("focus_hwnd=err elevated=0"));

        let note_line = format_event_line(
            &DebugEvent::Note {
                ts_us: 9000,
                hwnd: 0x300,
                text: "HUD_CREATE".into(),
            },
            stub_window,
        );
        assert!(note_line.contains("event=NOTE text=\"HUD_CREATE\" hwnd=0x300"));
    }

    #[test]
    fn rotating_log_writer_rotates_when_exceeding_max_bytes() {
        let dir = std::env::temp_dir().join(format!("anyleap-debuglog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let log_path = log_path_for_dir(&dir);

        let mut writer = RotatingLogWriter::new(log_path.clone(), 64);
        writer.write_line("line-1-aaaaaaaaaaaaaaaaaaaaaaaaaaaa\n").unwrap();
        writer.write_line("line-2-bbbbbbbbbbbbbbbbbbbbbbbbbbbb\n").unwrap();
        writer.flush();

        let backup = log_path.with_extension("log.1");
        assert!(backup.exists(), "backup log.1 must exist after rotation");
        assert!(log_path.exists(), "active log must exist after rotation");
        let active_content = std::fs::read_to_string(&log_path).unwrap();
        let backup_content = std::fs::read_to_string(&backup).unwrap();
        assert!(backup_content.contains("line-1"));
        assert!(active_content.contains("line-2"));
        assert!(!active_content.contains("line-1"));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
