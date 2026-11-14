//! Receiver-side KVM test harness.
//!
//! Acts as a headless KVM controller over QUIC against a locally running AnyLeap
//! receiver, creates a Win32 window with a single-line EDIT control, sends
//! `CursorEnter` / mouse / `KeyEvent` sequences, and verifies with `WM_GETTEXT`
//! that injected keystrokes reach the focused edit control.

#[cfg(not(windows))]
fn main() {
    eprintln!("kvm_harness is only supported on Windows.");
}

#[cfg(windows)]
fn main() {
    std::process::exit(win::run());
}

#[cfg(windows)]
mod win {
    use anyleap_lib::kvm::client::KvmClientHandle;
    use anyleap_lib::kvm::identity::{Identity, TrustStore};
    use anyleap_lib::kvm::injector::get_cursor_pos;
    use anyleap_lib::kvm::monitors::get_local_monitors;
    use anyleap_lib::kvm::protocol::KvmPacket;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicIsize, Ordering};
    use std::sync::{mpsc, Arc, Mutex};
    use std::time::{Duration, Instant};
    use tokio::time::timeout;
    use windows::core::{w, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{CloseHandle, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows::Win32::Graphics::Gdi::{COLOR_WINDOW, HBRUSH};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::{
        AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
        PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetFocus, MapVirtualKeyW, SetFocus, MAPVK_VK_TO_VSC, VK_A, VK_B, VK_C,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, CreateWindowExW, DefWindowProcW,
        DestroyWindow, DispatchMessageW, GetForegroundWindow, GetMessageW, GetWindowRect,
        GetWindowTextW, GetWindowThreadProcessId, IsIconic, PeekMessageW, PostMessageW,
        PostQuitMessage, RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowTextW,
        ShowWindow, TranslateMessage, ASFW_ANY, CS_HREDRAW, CS_VREDRAW, ES_AUTOHSCROLL, ES_LEFT,
        HMENU, MSG, PM_REMOVE, SW_RESTORE, SW_SHOW, WM_APP, WM_CLOSE, WM_CREATE, WM_DESTROY,
        WM_GETTEXT, WM_GETTEXTLENGTH, WM_QUIT, WM_SETFOCUS, WM_SETTEXT, WNDCLASSW, WS_BORDER,
        WS_CHILD, WS_EX_APPWINDOW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    };

    const WM_HARNESS_PREPARE: u32 = WM_APP + 1;
    const WM_HARNESS_READ_TEXT: u32 = WM_APP + 2;
    const WM_HARNESS_EDIT_CENTER: u32 = WM_APP + 3;

    const EDIT_CONTROL_ID: isize = 1001;
    const EXPECTED_TEXT: &str = "abc";

    static EDIT_HWND_RAW: AtomicIsize = AtomicIsize::new(0);
    static SHARED_TEXT: Mutex<String> = Mutex::new(String::new());
    static SHARED_POINT: Mutex<(i32, i32)> = Mutex::new((0, 0));

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Scenario {
        Basic,
        Hud,
        Reenter,
        AfterClick,
    }

    impl Scenario {
        fn name(self) -> &'static str {
            match self {
                Self::Basic => "basic",
                Self::Hud => "hud",
                Self::Reenter => "reenter",
                Self::AfterClick => "after-click",
            }
        }

        fn parse_list(s: &str) -> Result<Vec<Self>, String> {
            match s.to_ascii_lowercase().as_str() {
                "basic" => Ok(vec![Self::Basic]),
                "hud" => Ok(vec![Self::Hud]),
                "reenter" => Ok(vec![Self::Reenter]),
                "after-click" | "after_click" => Ok(vec![Self::AfterClick]),
                "all" => Ok(vec![Self::Basic, Self::Hud, Self::Reenter, Self::AfterClick]),
                other => Err(format!(
                    "Unknown scenario '{other}'. Expected: basic, hud, reenter, after-click, or all."
                )),
            }
        }
    }

    struct Config {
        ip: String,
        port: u16,
        pin: Option<String>,
        scenarios: Vec<Scenario>,
        repeat: usize,
        delay_ms: Option<u64>,
    }

    fn print_usage() {
        eprintln!(
            "\
Usage: kvm_harness [OPTIONS]

Options:
  --addr <IP:PORT>      Receiver QUIC address (default: 127.0.0.1:4177)
  --pin <CODE>          Pairing PIN (required on first run; cached thereafter)
  --scenario <NAME>     basic | hud | reenter | after-click | all (default: all)
  --repeat <N>          Number of iterations per scenario (default: 1)
  --delay-ms <MS>       Delay after CursorEnter before typing (hud caps at 50 ms)
  -h, --help            Show this help message"
        );
    }

    fn parse_args() -> Result<Config, i32> {
        let mut addr = "127.0.0.1:4177".to_string();
        let mut pin: Option<String> = None;
        let mut scenario_arg = "all".to_string();
        let mut repeat: usize = 1;
        let mut delay_ms: Option<u64> = None;

        let args: Vec<String> = std::env::args().skip(1).collect();
        let mut i = 0;
        while i < args.len() {
            match args[i].as_str() {
                "-h" | "--help" => {
                    print_usage();
                    return Err(0);
                }
                "--addr" => {
                    i += 1;
                    addr = args.get(i).cloned().ok_or_else(|| {
                        eprintln!("Missing value for --addr");
                        2
                    })?;
                }
                "--pin" => {
                    i += 1;
                    pin = Some(args.get(i).cloned().ok_or_else(|| {
                        eprintln!("Missing value for --pin");
                        2
                    })?);
                }
                "--scenario" => {
                    i += 1;
                    scenario_arg = args.get(i).cloned().ok_or_else(|| {
                        eprintln!("Missing value for --scenario");
                        2
                    })?;
                }
                "--repeat" => {
                    i += 1;
                    let raw = args.get(i).ok_or_else(|| {
                        eprintln!("Missing value for --repeat");
                        2
                    })?;
                    repeat = raw.parse::<usize>().map_err(|_| {
                        eprintln!("Invalid --repeat value: {raw}");
                        2
                    })?;
                    if repeat == 0 {
                        eprintln!("--repeat must be >= 1");
                        return Err(2);
                    }
                }
                "--delay-ms" => {
                    i += 1;
                    let raw = args.get(i).ok_or_else(|| {
                        eprintln!("Missing value for --delay-ms");
                        2
                    })?;
                    delay_ms = Some(raw.parse::<u64>().map_err(|_| {
                        eprintln!("Invalid --delay-ms value: {raw}");
                        2
                    })?);
                }
                other => {
                    eprintln!("Unknown argument: {other}");
                    print_usage();
                    return Err(2);
                }
            }
            i += 1;
        }

        let (ip, port_str) = addr.rsplit_once(':').ok_or_else(|| {
            eprintln!("Invalid --addr '{addr}'; expected <ip>:<port>");
            2
        })?;
        let port = port_str.parse::<u16>().map_err(|_| {
            eprintln!("Invalid port in --addr '{addr}'");
            2
        })?;
        let scenarios = Scenario::parse_list(&scenario_arg).map_err(|e| {
            eprintln!("{e}");
            2
        })?;

        Ok(Config {
            ip: ip.to_string(),
            port,
            pin,
            scenarios,
            repeat,
            delay_ms,
        })
    }

    fn harness_state_dir() -> PathBuf {
        let base = std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        base.join("anyleap-kvm-harness")
    }

    #[derive(Clone, Debug)]
    struct ForegroundSnapshot {
        hwnd_raw: isize,
        is_harness: bool,
        proc_name: String,
        title: String,
    }

    impl ForegroundSnapshot {
        fn capture(harness_hwnd: HWND) -> Self {
            unsafe {
                let fg = GetForegroundWindow();
                let hwnd_raw = fg.0 as isize;
                if fg.0.is_null() {
                    return Self {
                        hwnd_raw: 0,
                        is_harness: false,
                        proc_name: "none".to_string(),
                        title: String::new(),
                    };
                }

                let mut title_buf = [0u16; 256];
                let tlen = GetWindowTextW(fg, &mut title_buf);
                let title = if tlen > 0 {
                    String::from_utf16_lossy(&title_buf[..tlen as usize])
                } else {
                    String::new()
                };

                let mut pid = 0u32;
                let _ = GetWindowThreadProcessId(fg, Some(&mut pid));
                let proc_name = if pid == 0 {
                    "unknown".to_string()
                } else {
                    match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                        Ok(handle) => {
                            let mut path_buf = [0u16; 512];
                            let mut len = path_buf.len() as u32;
                            let ok = QueryFullProcessImageNameW(
                                handle,
                                PROCESS_NAME_FORMAT(0),
                                PWSTR(path_buf.as_mut_ptr()),
                                &mut len,
                            )
                            .is_ok();
                            let _ = CloseHandle(handle);
                            if ok && len > 0 {
                                let full = String::from_utf16_lossy(&path_buf[..len as usize]);
                                std::path::Path::new(&full)
                                    .file_name()
                                    .and_then(|n| n.to_str())
                                    .unwrap_or(&full)
                                    .to_string()
                            } else {
                                format!("pid:{pid}")
                            }
                        }
                        Err(_) => format!("pid:{pid}"),
                    }
                };

                Self {
                    hwnd_raw,
                    is_harness: fg == harness_hwnd,
                    proc_name,
                    title,
                }
            }
        }

        fn summary(&self) -> String {
            format!(
                "hwnd=0x{:X}{} proc={} title={:?}",
                self.hwnd_raw,
                if self.is_harness { "(harness)" } else { "" },
                self.proc_name,
                self.title
            )
        }
    }

    unsafe fn focus_edit_with_attach(main_hwnd: HWND, edit_hwnd: HWND) -> bool {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
        if IsIconic(main_hwnd).as_bool() {
            let _ = ShowWindow(main_hwnd, SW_RESTORE);
        } else {
            let _ = ShowWindow(main_hwnd, SW_SHOW);
        }

        let fg = GetForegroundWindow();
        let cur_tid = GetCurrentThreadId();
        let fg_tid = if !fg.0.is_null() {
            GetWindowThreadProcessId(fg, None)
        } else {
            0
        };
        let attached =
            fg_tid != 0 && fg_tid != cur_tid && AttachThreadInput(cur_tid, fg_tid, true).as_bool();

        let _ = BringWindowToTop(main_hwnd);
        let _ = SetForegroundWindow(main_hwnd);
        let _ = SetFocus(Some(edit_hwnd));

        if attached {
            let _ = AttachThreadInput(cur_tid, fg_tid, false);
        }

        GetForegroundWindow() == main_hwnd && GetFocus() == edit_hwnd
    }

    unsafe fn drain_pending_messages(hwnd: HWND) {
        let _ = hwnd;
        let mut msg = MSG::default();
        while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
            if msg.message == WM_QUIT {
                PostQuitMessage(msg.wParam.0 as i32);
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    unsafe extern "system" fn wnd_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        match msg {
            WM_CREATE => {
                let hinst = GetModuleHandleW(None).unwrap_or_default();
                let edit = CreateWindowExW(
                    Default::default(),
                    w!("EDIT"),
                    w!(""),
                    WS_CHILD | WS_VISIBLE | WS_BORDER | windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE(ES_LEFT as u32 | ES_AUTOHSCROLL as u32),
                    16,
                    24,
                    430,
                    32,
                    Some(hwnd),
                    Some(HMENU(EDIT_CONTROL_ID as *mut core::ffi::c_void)),
                    Some(HINSTANCE(hinst.0)),
                    None,
                )
                .unwrap_or_default();
                EDIT_HWND_RAW.store(edit.0 as isize, Ordering::SeqCst);
                let _ = SetFocus(Some(edit));
                LRESULT(0)
            }
            WM_SETFOCUS => {
                let edit = HWND(EDIT_HWND_RAW.load(Ordering::SeqCst) as *mut core::ffi::c_void);
                if !edit.0.is_null() {
                    let _ = SetFocus(Some(edit));
                }
                LRESULT(0)
            }
            WM_HARNESS_PREPARE => {
                let edit = HWND(EDIT_HWND_RAW.load(Ordering::SeqCst) as *mut core::ffi::c_void);
                if !edit.0.is_null() {
                    let empty = [0u16; 1];
                    let _ = SendMessageW(
                        edit,
                        WM_SETTEXT,
                        Some(WPARAM(0)),
                        Some(LPARAM(empty.as_ptr() as isize)),
                    );
                    let _ = SetWindowTextW(edit, PCWSTR(empty.as_ptr()));
                    let ok = focus_edit_with_attach(hwnd, edit);
                    return LRESULT(if ok { 1 } else { 0 });
                }
                LRESULT(0)
            }
            WM_HARNESS_READ_TEXT => {
                drain_pending_messages(hwnd);
                let edit = HWND(EDIT_HWND_RAW.load(Ordering::SeqCst) as *mut core::ffi::c_void);
                let text = if !edit.0.is_null() {
                    let len = SendMessageW(edit, WM_GETTEXTLENGTH, None, None).0.max(0) as usize;
                    let mut buf = vec![0u16; len + 1];
                    let copied = SendMessageW(
                        edit,
                        WM_GETTEXT,
                        Some(WPARAM(buf.len())),
                        Some(LPARAM(buf.as_mut_ptr() as isize)),
                    )
                    .0
                    .max(0) as usize;
                    String::from_utf16_lossy(&buf[..copied.min(buf.len())])
                } else {
                    String::new()
                };
                if let Ok(mut guard) = SHARED_TEXT.lock() {
                    *guard = text;
                }
                LRESULT(0)
            }
            WM_HARNESS_EDIT_CENTER => {
                let edit = HWND(EDIT_HWND_RAW.load(Ordering::SeqCst) as *mut core::ffi::c_void);
                let mut rc = RECT::default();
                if !edit.0.is_null() && GetWindowRect(edit, &mut rc).is_ok() {
                    let cx = (rc.left + rc.right) / 2;
                    let cy = (rc.top + rc.bottom) / 2;
                    if let Ok(mut guard) = SHARED_POINT.lock() {
                        *guard = (cx, cy);
                    }
                    return LRESULT(1);
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }

    struct HarnessWindow {
        hwnd_raw: isize,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl HarnessWindow {
        fn spawn() -> Result<Self, String> {
            let (tx, rx) = mpsc::channel::<Result<isize, String>>();
            let thread = std::thread::spawn(move || unsafe {
                let hinst = GetModuleHandleW(None).unwrap_or_default();
                let class_name = w!("AnyLeapKvmHarnessWindow");
                let wc = WNDCLASSW {
                    style: CS_HREDRAW | CS_VREDRAW,
                    lpfnWndProc: Some(wnd_proc),
                    hInstance: HINSTANCE(hinst.0),
                    hbrBackground: HBRUSH((COLOR_WINDOW.0 + 1) as *mut core::ffi::c_void),
                    lpszClassName: class_name,
                    ..Default::default()
                };
                if RegisterClassW(&wc) == 0 {
                    let _ = tx.send(Err("RegisterClassW failed".to_string()));
                    return;
                }

                // Center on the primary (or first) monitor so the window never sits on the
                // screen entry edge or overlaps the corner HUD toast.
                let mons = get_local_monitors();
                let mon = mons
                    .iter()
                    .find(|m| m.is_primary)
                    .or_else(|| mons.first())
                    .cloned()
                    .unwrap_or(anyleap_lib::kvm::monitors::MonitorDisplay {
                        id: "mon-1".into(),
                        x: 0,
                        y: 0,
                        width: 1920,
                        height: 1080,
                        is_primary: true,
                    });
                let win_w = 480;
                let win_h = 120;
                let win_x = mon.x + (mon.width - win_w) / 2;
                let win_y = mon.y + (mon.height - win_h) / 2;

                let hwnd = match CreateWindowExW(
                    WS_EX_APPWINDOW,
                    class_name,
                    w!("AnyLeap KVM Receiver Harness"),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    win_x,
                    win_y,
                    win_w,
                    win_h,
                    None,
                    None,
                    Some(HINSTANCE(hinst.0)),
                    None,
                ) {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = tx.send(Err(format!("CreateWindowExW failed: {e}")));
                        return;
                    }
                };

                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = tx.send(Ok(hwnd.0 as isize));

                let mut msg = MSG::default();
                while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            });

            let hwnd_raw = rx
                .recv()
                .map_err(|e| format!("Window thread failed to start: {e}"))??;
            Ok(Self {
                hwnd_raw,
                thread: Some(thread),
            })
        }

        fn hwnd(&self) -> HWND {
            HWND(self.hwnd_raw as *mut core::ffi::c_void)
        }

        fn prepare(&self) -> bool {
            unsafe { SendMessageW(self.hwnd(), WM_HARNESS_PREPARE, None, None).0 != 0 }
        }

        fn read_edit_text(&self) -> String {
            unsafe {
                let _ = SendMessageW(self.hwnd(), WM_HARNESS_READ_TEXT, None, None);
            }
            SHARED_TEXT.lock().map(|g| g.clone()).unwrap_or_default()
        }

        fn edit_center(&self) -> Option<(i32, i32)> {
            let ok = unsafe { SendMessageW(self.hwnd(), WM_HARNESS_EDIT_CENTER, None, None).0 != 0 };
            ok.then(|| SHARED_POINT.lock().map(|g| *g).unwrap_or((0, 0)))
        }

        fn snapshot(&self) -> ForegroundSnapshot {
            ForegroundSnapshot::capture(self.hwnd())
        }
    }

    impl Drop for HarnessWindow {
        fn drop(&mut self) {
            unsafe {
                let _ = PostMessageW(Some(self.hwnd()), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    async fn send_abc(client: &KvmClientHandle) -> Result<(), String> {
        let keys = [VK_A.0, VK_B.0, VK_C.0];
        for vk in keys {
            let scan_code = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC) } as u16;
            client
                .send_packet(KvmPacket::KeyEvent {
                    key_code: vk,
                    scan_code,
                    is_down: true,
                    extended: false,
                })
                .await?;
            tokio::time::sleep(Duration::from_millis(8)).await;
            client
                .send_packet(KvmPacket::KeyEvent {
                    key_code: vk,
                    scan_code,
                    is_down: false,
                    extended: false,
                })
                .await?;
            tokio::time::sleep(Duration::from_millis(12)).await;
        }
        Ok(())
    }

    async fn wait_for_cursor_pos_pkt(
        rx: &mut tokio::sync::broadcast::Receiver<KvmPacket>,
        budget: Duration,
    ) -> bool {
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
            if rem.is_zero() {
                return false;
            }
            match timeout(rem, rx.recv()).await {
                Ok(Ok(KvmPacket::CursorPosition { .. })) => return true,
                Ok(Ok(_)) => continue,
                _ => return false,
            }
        }
    }

    async fn wait_for_cursor_return(
        rx: &mut tokio::sync::broadcast::Receiver<KvmPacket>,
        budget: Duration,
    ) -> bool {
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            let rem = deadline.saturating_duration_since(tokio::time::Instant::now());
            if rem.is_zero() {
                return false;
            }
            match timeout(rem, rx.recv()).await {
                Ok(Ok(KvmPacket::CursorReturn { .. })) => return true,
                Ok(Ok(_)) => continue,
                _ => return false,
            }
        }
    }

    fn drain_incoming(rx: &mut tokio::sync::broadcast::Receiver<KvmPacket>) {
        while rx.try_recv().is_ok() {}
    }

    async fn send_cursor_enter(client: &KvmClientHandle) -> Result<(), String> {
        client
            .send_packet(KvmPacket::CursorEnter {
                edge: "left".to_string(),
                relative_y: 0.5,
                dwell_ms: 0,
                deadzone_px: 0,
            })
            .await
    }

    /// Push left past the receiver entry edge so the receiver emits `CursorReturn`
    /// (and closes its HUD) cleanly between scenarios; fall back to `ReleaseAll`.
    async fn cleanup_receiver_return(
        client: &KvmClientHandle,
        rx: &mut tokio::sync::broadcast::Receiver<KvmPacket>,
    ) {
        drain_incoming(rx);
        for _ in 0..3 {
            client.send_mouse_delta(-10_000, 0);
            if wait_for_cursor_return(rx, Duration::from_millis(350)).await {
                tokio::time::sleep(Duration::from_millis(150)).await;
                return;
            }
        }
        let _ = client.send_packet(KvmPacket::ReleaseAll).await;
        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    struct RunResult {
        iteration: usize,
        scenario: &'static str,
        passed: bool,
        got: String,
        final_fg: ForegroundSnapshot,
        notes: String,
    }

    async fn run_one(
        client: &KvmClientHandle,
        win: &HarnessWindow,
        scenario: Scenario,
        iteration: usize,
        delay_override: Option<u64>,
    ) -> RunResult {
        let mut rx = client.incoming_tx.subscribe();
        let focused = win.prepare();
        tokio::time::sleep(Duration::from_millis(30)).await;
        let snap_prepare = win.snapshot();
        println!(
            "[{}:#{}] step=prepare focused={} {}",
            scenario.name(),
            iteration,
            focused,
            snap_prepare.summary()
        );

        let mut notes = Vec::new();
        if !focused {
            notes.push("initial focus failed".to_string());
        }

        drain_incoming(&mut rx);

        match scenario {
            Scenario::Basic => {
                let delay = delay_override.unwrap_or(200);
                if let Err(e) = send_cursor_enter(client).await {
                    return RunResult {
                        iteration,
                        scenario: scenario.name(),
                        passed: false,
                        got: String::new(),
                        final_fg: win.snapshot(),
                        notes: e,
                    };
                }
                let got_pos = wait_for_cursor_pos_pkt(&mut rx, Duration::from_millis(800)).await;
                if !got_pos {
                    notes.push("no CursorPosition ack".to_string());
                }
                tokio::time::sleep(Duration::from_millis(delay)).await;
                let snap_enter = win.snapshot();
                println!(
                    "[{}:#{}] step=after_enter(delay={}ms) {}",
                    scenario.name(),
                    iteration,
                    delay,
                    snap_enter.summary()
                );

                if let Err(e) = send_abc(client).await {
                    notes.push(e);
                }
            }
            Scenario::Hud => {
                let delay = delay_override.unwrap_or(25).min(50);
                let t0 = Instant::now();
                if let Err(e) = send_cursor_enter(client).await {
                    return RunResult {
                        iteration,
                        scenario: scenario.name(),
                        passed: false,
                        got: String::new(),
                        final_fg: win.snapshot(),
                        notes: e,
                    };
                }
                tokio::time::sleep(Duration::from_millis(delay)).await;
                let snap_enter = win.snapshot();
                println!(
                    "[{}:#{}] step=after_enter_hud(elapsed={}ms) {}",
                    scenario.name(),
                    iteration,
                    t0.elapsed().as_millis(),
                    snap_enter.summary()
                );
                if let Err(e) = send_abc(client).await {
                    notes.push(e);
                }
            }
            Scenario::Reenter => {
                let delay = delay_override.unwrap_or(200);
                if let Err(e) = send_cursor_enter(client).await {
                    return RunResult {
                        iteration,
                        scenario: scenario.name(),
                        passed: false,
                        got: String::new(),
                        final_fg: win.snapshot(),
                        notes: e,
                    };
                }
                let _ = wait_for_cursor_pos_pkt(&mut rx, Duration::from_millis(500)).await;
                let snap_e1 = win.snapshot();
                println!(
                    "[{}:#{}] step=after_enter1 {}",
                    scenario.name(),
                    iteration,
                    snap_e1.summary()
                );

                drain_incoming(&mut rx);
                client.send_mouse_delta(-10_000, 0);
                let returned = wait_for_cursor_return(&mut rx, Duration::from_millis(1500)).await;
                let snap_ret = win.snapshot();
                println!(
                    "[{}:#{}] step=after_return(ack={}) {}",
                    scenario.name(),
                    iteration,
                    returned,
                    snap_ret.summary()
                );
                if !returned {
                    notes.push("missing CursorReturn".to_string());
                }
                tokio::time::sleep(Duration::from_millis(100)).await;

                drain_incoming(&mut rx);
                if let Err(e) = send_cursor_enter(client).await {
                    notes.push(e);
                } else {
                    let _ = wait_for_cursor_pos_pkt(&mut rx, Duration::from_millis(500)).await;
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    let snap_e2 = win.snapshot();
                    println!(
                        "[{}:#{}] step=after_reenter(delay={}ms) {}",
                        scenario.name(),
                        iteration,
                        delay,
                        snap_e2.summary()
                    );
                    if let Err(e) = send_abc(client).await {
                        notes.push(e);
                    }
                }
            }
            Scenario::AfterClick => {
                let delay = delay_override.unwrap_or(120);
                if let Err(e) = send_cursor_enter(client).await {
                    return RunResult {
                        iteration,
                        scenario: scenario.name(),
                        passed: false,
                        got: String::new(),
                        final_fg: win.snapshot(),
                        notes: e,
                    };
                }
                let _ = wait_for_cursor_pos_pkt(&mut rx, Duration::from_millis(800)).await;
                tokio::time::sleep(Duration::from_millis(30)).await;
                let snap_enter = win.snapshot();
                println!(
                    "[{}:#{}] step=after_enter {}",
                    scenario.name(),
                    iteration,
                    snap_enter.summary()
                );

                if let (Some((cx, cy)), Some((cur_x, cur_y))) =
                    (win.edit_center(), get_cursor_pos())
                {
                    client.send_mouse_delta(cx - cur_x, cy - cur_y);
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    let _ = client
                        .send_packet(KvmPacket::MouseButton {
                            button: 1,
                            is_down: true,
                        })
                        .await;
                    tokio::time::sleep(Duration::from_millis(20)).await;
                    let _ = client
                        .send_packet(KvmPacket::MouseButton {
                            button: 1,
                            is_down: false,
                        })
                        .await;
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    let snap_click = win.snapshot();
                    println!(
                        "[{}:#{}] step=after_click(target=({cx},{cy})) {}",
                        scenario.name(),
                        iteration,
                        snap_click.summary()
                    );
                } else {
                    notes.push("failed to resolve edit center or cursor pos".to_string());
                }

                if let Err(e) = send_abc(client).await {
                    notes.push(e);
                }
            }
        }

        tokio::time::sleep(Duration::from_millis(120)).await;
        let got = win.read_edit_text();
        let snap_type = win.snapshot();
        let passed = got == EXPECTED_TEXT
            && (!matches!(scenario, Scenario::Reenter) || !notes.iter().any(|n| n == "missing CursorReturn"));
        println!(
            "[{}:#{}] step=after_type result={} got={:?} {}",
            scenario.name(),
            iteration,
            if passed { "PASS" } else { "FAIL" },
            got,
            snap_type.summary()
        );

        cleanup_receiver_return(client, &mut rx).await;

        RunResult {
            iteration,
            scenario: scenario.name(),
            passed,
            got,
            final_fg: snap_type,
            notes: notes.join("; "),
        }
    }

    pub fn run() -> i32 {
        let cfg = match parse_args() {
            Ok(c) => c,
            Err(code) => return code,
        };

        let rt = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("Failed to create tokio runtime: {e}");
                return 2;
            }
        };

        let state_dir = harness_state_dir();
        let identity = match Identity::load_or_create(&state_dir) {
            Ok(id) => id,
            Err(e) => {
                eprintln!("Failed to load/create harness identity: {e}");
                return 2;
            }
        };
        let trust = TrustStore::load(Some(&state_dir));
        let monitors = get_local_monitors();

        println!(
            "Connecting to receiver at {}:{} (id={}, state_dir={})...",
            cfg.ip,
            cfg.port,
            identity.computer_id(),
            state_dir.display()
        );

        let client: Arc<KvmClientHandle> = match rt.block_on(KvmClientHandle::connect(
            &cfg.ip,
            cfg.port,
            identity.computer_id(),
            "AnyLeap-KvmHarness".to_string(),
            monitors,
            cfg.pin.clone(),
            &identity,
            &trust,
            state_dir.join("downloads"),
        )) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                eprintln!("Connection/pairing failed: {e}");
                if cfg.pin.is_none() {
                    eprintln!("Hint: pass --pin <CODE> on the first run against this receiver.");
                }
                return 2;
            }
        };

        println!(
            "Paired/connected to peer '{}' (id={}).",
            client.peer_name, client.peer_id
        );

        let win = match HarnessWindow::spawn() {
            Ok(w) => w,
            Err(e) => {
                eprintln!("Failed to create Win32 harness window: {e}");
                return 2;
            }
        };

        let mut results = Vec::new();
        for iter in 1..=cfg.repeat {
            for &scenario in &cfg.scenarios {
                let res = rt.block_on(run_one(&client, &win, scenario, iter, cfg.delay_ms));
                results.push(res);
            }
        }

        let _ = rt.block_on(client.send_packet(KvmPacket::SessionDisconnect));
        drop(win);

        println!("\n=== KVM Receiver Harness Summary ===");
        println!(
            "{:<4} | {:<11} | {:<6} | {:<8} | {:<8} | {:<28} | {}",
            "Run", "Scenario", "Result", "Expected", "Got", "Foreground Proc", "Notes"
        );
        println!("{}", "-".repeat(92));
        let mut any_fail = false;
        for r in &results {
            if !r.passed {
                any_fail = true;
            }
            println!(
                "{:<4} | {:<11} | {:<6} | {:<8} | {:<8} | {:<28} | {}",
                r.iteration,
                r.scenario,
                if r.passed { "PASS" } else { "FAIL" },
                EXPECTED_TEXT,
                format!("{:?}", r.got),
                format!("{} (0x{:X})", r.final_fg.proc_name, r.final_fg.hwnd_raw),
                r.notes
            );
        }

        let passed_count = results.iter().filter(|r| r.passed).count();
        println!(
            "\nTotal: {}/{} passed.",
            passed_count,
            results.len()
        );

        if any_fail {
            1
        } else {
            0
        }
    }
}
