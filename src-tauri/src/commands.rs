use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri::path::BaseDirectory;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

use crate::state::{AppState, Session};

/// Filename of the bundled adb sidecar (target-triple suffixed by Tauri).
/// Kept Windows-only for M1; extend per-platform when we add macOS/Linux.
#[cfg(windows)]
const ADB_SIDECAR: &str = "adb-x86_64-pc-windows-msvc.exe";
#[cfg(not(windows))]
const ADB_SIDECAR: &str = "adb";

#[derive(Serialize, Clone)]
pub struct DeviceInfo {
    pub serial: String,
    pub state: String,
    pub model: Option<String>,
    pub product: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub serial: String,
    pub pid: u32,
    pub started_at: i64,
}

#[derive(Serialize, Clone)]
struct SessionExited {
    id: String,
    code: Option<i32>,
    signal: Option<i32>,
    last_error: String,
    stderr: String,
}

/// Core (Tier-1) settings, mirrored from the TypeScript `CoreSettings` type.
/// Field names arrive camelCase over the IPC boundary.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CoreSettings {
    pub max_size: u32,
    pub video_bit_rate: u64,
    pub max_fps: u32,
    pub video_codec: String,
    pub stay_awake: bool,
    pub turn_screen_off: bool,
    pub fullscreen: bool,
    pub show_touches: bool,
    pub no_audio: bool,
    pub no_control: bool,
}

/// An adb mDNS service entry, as listed by `adb mdns services`.
#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct MdnsService {
    pub name: String,
    pub service_type: String,
    pub host: String,
    pub port: u16,
}

const SVC_PAIRING: &str = "_adb-tls-pairing._tcp";
const SVC_CONNECT: &str = "_adb-tls-connect._tcp";
const SVC_LEGACY: &str = "_adb._tcp";

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Locate the bundled adb across dev (`adb.exe`, sidecar staged unsuffixed) and
/// production (`adb-<triple>.exe` in the install root) layouts.
fn adb_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let candidates = [
        dir.join("adb.exe"),
        dir.join(ADB_SIDECAR),
        // dev fallback: src-tauri/target/debug -> src-tauri/binaries
        dir.join("..").join("..").join("binaries").join(ADB_SIDECAR),
    ];
    candidates.into_iter().find(|c| c.exists())
}

/// Build an `adb` sidecar command with the working mDNS backend forced on.
///
/// platform-tools r37 defaults to the Openscreen mDNS backend, which discovers
/// nothing on Windows; the legacy backend (`ADB_MDNS_OPENSCREEN=0`) works. The
/// backend is fixed when the adb *server* starts, so we set it on every adb call
/// — whichever invocation starts the server configures it correctly.
fn adb_cmd(app: &AppHandle) -> Result<tauri_plugin_shell::process::Command, String> {
    app.shell()
        .sidecar("adb")
        .map(|cmd| {
            // Working mDNS backend (Openscreen discovers nothing on Windows), and
            // disable mDNS auto-connect — we manage connections explicitly, which
            // avoids duplicate/auto-connected device entries.
            cmd.env("ADB_MDNS_OPENSCREEN", "0")
                .env("ADB_MDNS_AUTO_CONNECT", "0")
        })
        .map_err(|e| e.to_string())
}

/// Locate the scrcpy-server jar across production (bundled resource) and dev layouts.
fn resolve_server_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    // Production: bundled as a resource under scrcpy/.
    if let Ok(p) = app.path().resolve("scrcpy/scrcpy-server", BaseDirectory::Resource) {
        if p.exists() {
            return Some(p);
        }
    }
    // Dev / fallback: relative to the executable.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidates = [
                dir.join("scrcpy-server"),
                dir.join("scrcpy").join("scrcpy-server"),
                // dev: src-tauri/target/debug -> src-tauri/binaries/scrcpy-server
                dir.join("..").join("..").join("binaries").join("scrcpy-server"),
            ];
            for cand in candidates {
                if cand.exists() {
                    return Some(cand);
                }
            }
        }
    }
    None
}

/// Parse `adb devices -l` output into structured device records.
///
/// Skips the header line and any `* daemon ...` startup noise, keeps non-`device`
/// states (e.g. `unauthorized`, `offline`) so the UI can guide the user.
fn parse_adb_devices(text: &str) -> Vec<DeviceInfo> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("List of devices") || line.starts_with('*') {
            continue;
        }
        let mut it = line.split_whitespace();
        let serial = match it.next() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let mut state = match it.next() {
            Some(s) => s.to_string(),
            None => continue,
        };
        let rest: Vec<&str> = it.collect();
        // "no permissions" is the only multi-word state we expect.
        if state == "no" && rest.first() == Some(&"permissions") {
            state = "no permissions".to_string();
        }
        let mut model = None;
        let mut product = None;
        for tok in &rest {
            if let Some(v) = tok.strip_prefix("model:") {
                model = Some(v.to_string());
            }
            if let Some(v) = tok.strip_prefix("product:") {
                product = Some(v.to_string());
            }
        }
        out.push(DeviceInfo { serial, state, model, product });
    }
    out
}

/// Parse `adb mdns services` output into structured entries.
///
/// Anchors on the known service-type token (instance names can contain spaces),
/// then reads the following token as `host:port`. Skips the header and any
/// `* daemon ...` noise.
fn parse_mdns_services(text: &str) -> Vec<MdnsService> {
    let mut out = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with("List of discovered") || line.starts_with('*') {
            continue;
        }
        let toks: Vec<&str> = line.split_whitespace().collect();
        let svc_idx = match toks
            .iter()
            .position(|t| matches!(*t, SVC_PAIRING | SVC_CONNECT | SVC_LEGACY))
        {
            Some(i) if i > 0 && i + 1 < toks.len() => i,
            _ => continue,
        };
        let name = toks[..svc_idx].join(" ");
        let service_type = toks[svc_idx].to_string();
        let (host, port_str) = match toks[svc_idx + 1].rsplit_once(':') {
            Some(hp) => hp,
            None => continue,
        };
        let port = match port_str.parse::<u16>() {
            Ok(p) => p,
            Err(_) => continue,
        };
        if host.is_empty() {
            continue;
        }
        out.push(MdnsService {
            name,
            service_type,
            host: host.to_string(),
            port,
        });
    }
    out
}

/// Build the scrcpy argument vector from typed settings (single source of truth
/// for version-sensitive flag names; the frontend never injects raw flags).
fn build_scrcpy_args(serial: &str, s: &CoreSettings) -> Vec<String> {
    let mut a: Vec<String> = vec!["--serial".into(), serial.into()];
    if s.max_size > 0 {
        a.push(format!("--max-size={}", s.max_size));
    }
    if s.video_bit_rate > 0 {
        a.push(format!("--video-bit-rate={}", s.video_bit_rate));
    }
    if s.max_fps > 0 {
        a.push(format!("--max-fps={}", s.max_fps));
    }
    if matches!(s.video_codec.as_str(), "h264" | "h265" | "av1") {
        a.push(format!("--video-codec={}", s.video_codec));
    }
    if s.stay_awake {
        a.push("--stay-awake".into());
    }
    if s.turn_screen_off {
        a.push("--turn-screen-off".into());
    }
    if s.fullscreen {
        a.push("--fullscreen".into());
    }
    if s.show_touches {
        a.push("--show-touches".into());
    }
    if s.no_audio {
        a.push("--no-audio".into());
    }
    if s.no_control {
        a.push("--no-control".into());
    }
    a.push(format!("--window-title=AnyLeap — {}", serial));
    a
}

/// List USB/TCP devices known to adb.
#[tauri::command]
pub async fn list_devices(app: AppHandle) -> Result<Vec<DeviceInfo>, String> {
    let output = adb_cmd(&app)?
        .args(["devices", "-l"])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("adb devices failed: {}", err.trim()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    Ok(parse_adb_devices(&text))
}

/// scrcpy stderr patterns that suggest a (possibly transient) encoder/connection
/// failure worth retrying once on conservative flags. Seen on some OEM Android 10
/// devices where the default codec/fps combination is rejected.
fn stderr_suggests_encoder_failure(s: &str) -> bool {
    let low = s.to_lowercase();
    low.contains("could not create")
        || low.contains("encoding error")
        || low.contains("server connection failed")
        || low.contains("demuxer 'video'")
        || low.contains("connection error")
        || low.contains("failed to start")
}

/// Conservative arg set for a one-shot retry: drop --max-fps, force h264, no audio.
fn degrade_args(args: &[String]) -> Vec<String> {
    let mut out: Vec<String> = args
        .iter()
        .filter(|a| {
            !a.starts_with("--max-fps")
                && !a.starts_with("--video-codec")
                && a.as_str() != "--no-audio"
        })
        .cloned()
        .collect();
    out.push("--video-codec=h264".to_string());
    out.push("--no-audio".to_string());
    out
}

/// Pick a human-useful one-liner from scrcpy stderr. scrcpy prints the real cause
/// as an `ERROR:` line, then a terse final "killed"/"aborted" — so we surface the
/// ERROR (or a device WARN), not the last line.
fn summarize_scrcpy_error(stderr: &str, command_error: Option<&str>) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if let Some(err) = lines.iter().find(|l| l.contains("ERROR:")) {
        return err.to_string();
    }
    if let Some(w) = lines
        .iter()
        .find(|l| l.contains("WARN:") && l.to_lowercase().contains("device"))
    {
        return w.to_string();
    }
    if let Some(last) = lines.iter().rev().find(|l| {
        let low = l.to_lowercase();
        low != "killed" && low != "aborted" && low != "terminated"
    }) {
        return last.to_string();
    }
    command_error
        .map(str::to_string)
        .or_else(|| lines.last().map(|s| s.to_string()))
        .unwrap_or_else(|| "scrcpy exited without output".to_string())
}

/// Spawn scrcpy with a prepared arg vector and register a tracked session.
/// On an early encoder/connection failure, retries once with conservative flags
/// (`retried` guards against an infinite retry loop).
fn spawn_session(
    app: &AppHandle,
    serial: String,
    args: Vec<String>,
    retried: bool,
) -> Result<SessionInfo, String> {
    let server = resolve_server_path(app)
        .ok_or_else(|| "scrcpy-server not found (run scripts/fetch-binaries.ps1)".to_string())?;

    let mut cmd = app
        .shell()
        .sidecar("scrcpy")
        .map_err(|e| e.to_string())?
        .env("SCRCPY_SERVER_PATH", server.to_string_lossy().to_string());
    if let Some(adb) = adb_path() {
        // Force scrcpy to use our pinned adb (avoids version conflicts).
        cmd = cmd.env("ADB", adb.to_string_lossy().to_string());
    }

    let (mut rx, child) = cmd.args(args.clone()).spawn().map_err(|e| e.to_string())?;

    let id = uuid::Uuid::new_v4().to_string();
    let pid = child.pid();
    let started_at = now_ms();
    let info = SessionInfo {
        id: id.clone(),
        serial: serial.clone(),
        pid,
        started_at,
    };

    app.state::<AppState>().sessions.lock().unwrap().insert(
        id.clone(),
        Session {
            id: id.clone(),
            serial: serial.clone(),
            pid,
            started_at,
            child,
            args: args.clone(),
        },
    );

    let _ = app.emit("session-started", info.clone());

    // Drain the child's event stream; accumulate stderr, clean up + notify on exit.
    let app2 = app.clone();
    let id2 = id.clone();
    let serial2 = serial.clone();
    let args2 = args.clone();
    tauri::async_runtime::spawn(async move {
        let mut stderr_buf = String::new();
        let mut command_error: Option<String> = None;
        while let Some(ev) = rx.recv().await {
            match ev {
                CommandEvent::Stderr(bytes) => {
                    stderr_buf.push_str(&String::from_utf8_lossy(&bytes));
                    // Keep the tail; scrcpy's useful ERROR line is near the end.
                    if stderr_buf.len() > 16_384 {
                        let cut = stderr_buf.len() - 16_384;
                        stderr_buf.drain(..cut);
                    }
                }
                CommandEvent::Error(e) => command_error = Some(e),
                CommandEvent::Terminated(payload) => {
                    if let Some(st) = app2.try_state::<AppState>() {
                        st.sessions.lock().unwrap().remove(&id2);
                    }
                    let failed = payload.code.map(|c| c != 0).unwrap_or(true);
                    let early = now_ms() - started_at < 3000;
                    if !retried && failed && early && stderr_suggests_encoder_failure(&stderr_buf)
                    {
                        // Clear the old session in the UI (no error), then retry on safe flags.
                        let _ = app2.emit(
                            "session-exited",
                            SessionExited {
                                id: id2.clone(),
                                code: payload.code,
                                signal: payload.signal,
                                last_error: String::new(),
                                stderr: stderr_buf.clone(),
                            },
                        );
                        let _ = spawn_session(&app2, serial2.clone(), degrade_args(&args2), true);
                        break;
                    }
                    let summary = summarize_scrcpy_error(&stderr_buf, command_error.as_deref());
                    let _ = app2.emit(
                        "session-exited",
                        SessionExited {
                            id: id2.clone(),
                            code: payload.code,
                            signal: payload.signal,
                            last_error: summary,
                            stderr: stderr_buf.clone(),
                        },
                    );
                    break;
                }
                _ => {}
            }
        }
    });

    Ok(info)
}

/// Launch scrcpy for a device and track the session.
#[tauri::command]
pub fn start_mirror(
    app: AppHandle,
    serial: String,
    settings: CoreSettings,
) -> Result<SessionInfo, String> {
    let args = build_scrcpy_args(&serial, &settings);
    spawn_session(&app, serial, args, false)
}

/// Restart a device's mirror with scrcpy's screen-off toggled (true scrcpy
/// "dark screen, still mirroring"). Reuses the original args, flipping the flag.
#[tauri::command]
pub fn restart_with_screen_off(
    app: AppHandle,
    serial: String,
    off: bool,
) -> Result<SessionInfo, String> {
    let old = {
        let state = app.state::<AppState>();
        let mut map = state.sessions.lock().unwrap();
        let key = map
            .iter()
            .find(|(_, s)| s.serial == serial)
            .map(|(k, _)| k.clone());
        key.and_then(|k| map.remove(&k))
    };
    let old = old.ok_or_else(|| "no active mirror for this device".to_string())?;
    let mut args = old.args.clone();
    let _ = old.child.kill();
    args.retain(|a| a != "--turn-screen-off");
    if off {
        args.push("--turn-screen-off".to_string());
    }
    spawn_session(&app, serial, args, false)
}

/// Stop a running session by killing its scrcpy child.
#[tauri::command]
pub fn stop_mirror(state: State<'_, AppState>, session_id: String) -> Result<(), String> {
    // Take ownership out of the map, then kill outside the lock.
    let session = state.sessions.lock().unwrap().remove(&session_id);
    match session {
        Some(s) => s.child.kill().map_err(|e| e.to_string()),
        None => Err("no such session".into()),
    }
}

/// List currently running sessions.
#[tauri::command]
pub fn list_sessions(state: State<'_, AppState>) -> Vec<SessionInfo> {
    state
        .sessions
        .lock()
        .unwrap()
        .values()
        .map(|s| SessionInfo {
            id: s.id.clone(),
            serial: s.serial.clone(),
            pid: s.pid,
            started_at: s.started_at,
        })
        .collect()
}

/// Discover wireless adb services on the LAN via `adb mdns services`.
#[tauri::command]
pub async fn discover_wireless(app: AppHandle) -> Result<Vec<MdnsService>, String> {
    let output = adb_cmd(&app)?
        .args(["mdns", "services"])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    // Empty discovery is normal (not an error); only surface a real failure.
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        let err = err.trim();
        if !err.is_empty() {
            return Err(format!("adb mdns services failed: {err}"));
        }
    }
    Ok(parse_mdns_services(&String::from_utf8_lossy(&output.stdout)))
}

/// Pair with a device using a 6-digit pairing code (Android 11+ wireless debugging).
#[tauri::command]
pub async fn pair_device(
    app: AppHandle,
    host: String,
    port: u16,
    code: String,
) -> Result<String, String> {
    let target = format!("{host}:{port}");
    let output = adb_cmd(&app)?
        .args(["pair", &target, &code])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout}\n{}", String::from_utf8_lossy(&output.stderr));
    if combined.to_lowercase().contains("successfully paired") {
        Ok(stdout.trim().to_string())
    } else {
        let msg = combined.trim();
        Err(if msg.is_empty() {
            "adb pair failed (no output)".to_string()
        } else {
            msg.to_string()
        })
    }
}

/// Connect to a wireless device (`adb connect host:port`).
#[tauri::command]
pub async fn connect_device(app: AppHandle, host: String, port: u16) -> Result<String, String> {
    let target = format!("{host}:{port}");
    let output = adb_cmd(&app)?
        .args(["connect", &target])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{stdout}\n{}", String::from_utf8_lossy(&output.stderr));
    let low = combined.to_lowercase();
    if low.contains("connected to") && !low.contains("failed") && !low.contains("cannot") {
        Ok(stdout.trim().to_string())
    } else {
        let msg = combined.trim();
        Err(if msg.is_empty() {
            "adb connect failed (no output)".to_string()
        } else {
            msg.to_string()
        })
    }
}

/// Disconnect a wireless device (idempotent).
#[tauri::command]
pub async fn disconnect_device(app: AppHandle, host: String, port: u16) -> Result<(), String> {
    let target = format!("{host}:{port}");
    let output = adb_cmd(&app)?
        .args(["disconnect", &target])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout).to_lowercase(),
        String::from_utf8_lossy(&output.stderr).to_lowercase()
    );
    if output.status.success()
        || combined.contains("disconnected")
        || combined.contains("no such device")
    {
        Ok(())
    } else {
        Err(combined.trim().to_string())
    }
}

/// Send an Android key event to a device (`adb shell input keyevent`).
#[tauri::command]
pub async fn send_keyevent(app: AppHandle, serial: String, keycode: u32) -> Result<(), String> {
    let kc = keycode.to_string();
    let output = adb_cmd(&app)?
        .args(["-s", &serial, "shell", "input", "keyevent", &kc])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Expand the notification shade on a device.
#[tauri::command]
pub async fn open_notifications(app: AppHandle, serial: String) -> Result<(), String> {
    let output = adb_cmd(&app)?
        .args(["-s", &serial, "shell", "cmd", "statusbar", "expand-notifications"])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

/// Geometry of a scrcpy mirror window + its monitor's work area (for docking).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MirrorRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub minimized: bool,
    pub work_left: i32,
    pub work_top: i32,
    pub work_right: i32,
    pub work_bottom: i32,
    /// Title of the current foreground window (so the strip can follow focus).
    pub foreground: String,
}

/// Find a scrcpy mirror window by exact title and return its screen rect so the
/// floating control strip can dock to it. Windows-only; `None` elsewhere.
#[tauri::command]
pub fn mirror_rect(title: String) -> Option<MirrorRect> {
    #[cfg(windows)]
    {
        use std::mem::size_of;
        use windows::core::PCWSTR;
        use windows::Win32::Foundation::RECT;
        use windows::Win32::Graphics::Gdi::{
            GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            FindWindowW, GetForegroundWindow, GetWindowRect, GetWindowTextW, IsIconic,
        };

        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let hwnd = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(wide.as_ptr())) }.ok()?;
        let minimized = unsafe { IsIconic(hwnd) }.as_bool();
        let mut rect = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;

        let hmon = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        let work = if unsafe { GetMonitorInfoW(hmon, &mut mi) }.as_bool() {
            mi.rcWork
        } else {
            rect
        };

        let mut buf = [0u16; 512];
        let n = unsafe { GetWindowTextW(GetForegroundWindow(), &mut buf) };
        let foreground = String::from_utf16_lossy(&buf[..n.max(0) as usize]);

        Some(MirrorRect {
            x: rect.left,
            y: rect.top,
            width: rect.right - rect.left,
            height: rect.bottom - rect.top,
            minimized,
            work_left: work.left,
            work_top: work.top,
            work_right: work.right,
            work_bottom: work.bottom,
            foreground,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = title;
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_canonical_mdns_output() {
        let text = "List of discovered mdns services\n\
adb-ABC123\t_adb._tcp\t192.168.1.10:5555\n\
adb-ABC123-QXjCrW\t_adb-tls-pairing._tcp\t192.168.1.10:33861\n\
adb-ABC123-TnSdi9\t_adb-tls-connect._tcp\t192.168.1.10:42135\n";
        let svcs = parse_mdns_services(text);
        assert_eq!(svcs.len(), 3);
        let pairing = svcs.iter().find(|s| s.service_type == SVC_PAIRING).unwrap();
        assert_eq!(pairing.host, "192.168.1.10");
        assert_eq!(pairing.port, 33861);
    }

    #[test]
    fn handles_empty_and_header_only() {
        assert!(parse_mdns_services("").is_empty());
        assert!(parse_mdns_services("List of discovered mdns services\n").is_empty());
    }

    #[test]
    fn skips_bad_port_and_unknown_service() {
        let text = "name _adb-tls-connect._tcp 10.0.0.5:notaport\n\
other _weird._tcp 10.0.0.6:1234\n";
        assert!(parse_mdns_services(text).is_empty());
    }

    #[test]
    fn tolerates_spaces_in_name() {
        let text = "my phone _adb-tls-connect._tcp 10.0.0.7:5555\n";
        let svcs = parse_mdns_services(text);
        assert_eq!(svcs.len(), 1);
        assert_eq!(svcs[0].name, "my phone");
        assert_eq!(svcs[0].port, 5555);
    }
}
