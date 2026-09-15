use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri::path::BaseDirectory;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

use crate::state::{poison_error, AppState, Session};

/// Filename of the bundled adb sidecar (target-triple suffixed by Tauri).
/// Kept Windows-only for M1; extend per-platform when we add macOS/Linux.
#[cfg(target_os = "windows")]
const ADB_SIDECAR: &str = concat!("adb-", env!("TARGET"), ".exe");
#[cfg(not(target_os = "windows"))]
const ADB_SIDECAR: &str = concat!("adb-", env!("TARGET"));

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
    pub mode: String,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CameraSettings {
    pub facing: String,
    pub camera_id: Option<String>,
    pub size: Option<String>,
    pub fps: Option<u32>,
    pub high_speed: bool,
    pub torch: bool,
    pub no_audio: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CameraDeviceOption {
    pub id: String,
    pub facing: String,
    pub resolution: String,
    pub fps: Vec<u32>,
    pub zoom_range: Option<(f32, f32)>,
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
    pub no_keyboard_ime: bool,
    pub flex_display: bool,
    pub flex_display_size: String,
    pub no_window_aspect_ratio_lock: bool,
    pub render_fit: String,
    #[serde(default)]
    pub embedded: bool,
    #[serde(default)]
    pub hide_virtual_taskbar: bool,
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
pub(crate) fn adb_path() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let exe_name = if cfg!(windows) { "adb.exe" } else { "adb" };
    let candidates = [
        dir.join(exe_name),
        dir.join(ADB_SIDECAR),
        dir.join("..").join("Resources").join(ADB_SIDECAR),
        dir.join("..").join("Resources").join(exe_name),
        dir.join("..").join("..").join("binaries").join(ADB_SIDECAR),
        dir.join("..").join("..").join("binaries").join(exe_name),
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
    if s.no_keyboard_ime {
        a.push("--keyboard=uhid".into());
    }
    if s.flex_display {
        if s.flex_display_size.is_empty() {
            a.push("--new-display".into());
        } else {
            a.push(format!("--new-display={}", s.flex_display_size));
        }
        a.push("--flex-display".into());
        a.push("--no-vd-destroy-content".into());
        if s.hide_virtual_taskbar {
            a.push("--no-vd-system-decorations".into());
        }
    }
    if s.embedded {
        a.push("--window-borderless".into());
    }
    if s.no_window_aspect_ratio_lock || s.embedded {
        a.push("--no-window-aspect-ratio-lock".into());
    }
    // Always sent explicitly: with --flex-display scrcpy's default flips to
    // "unscaled", so a "letterbox" selection must not be omitted.
    if !s.render_fit.is_empty() {
        a.push(format!("--render-fit={}", s.render_fit));
    }
    a.push(format!("--window-title=AnyLeap — {}", serial));
    a
}

pub fn build_scrcpy_camera_args(serial: &str, s: &CameraSettings) -> Vec<String> {
    let mut a: Vec<String> = vec![
        "--serial".into(),
        serial.into(),
        "--video-source=camera".into(),
    ];
    if let Some(ref id) = s.camera_id {
        if !id.is_empty() {
            a.push(format!("--camera-id={}", id));
        }
    } else if !s.facing.is_empty() {
        a.push(format!("--camera-facing={}", s.facing));
    }
    if let Some(ref size) = s.size {
        if !size.is_empty() {
            a.push(format!("--camera-size={}", size));
        }
    }
    if let Some(fps) = s.fps {
        if fps > 0 {
            a.push(format!("--camera-fps={}", fps));
        }
    }
    if s.high_speed {
        a.push("--camera-high-speed".into());
    }
    if s.torch {
        a.push("--camera-torch".into());
    }
    if s.no_audio {
        a.push("--no-audio".into());
    }
    a.push(format!("--window-title=AnyLeap Camera — {}", serial));
    a
}

pub fn parse_list_cameras_output(stdout: &str) -> Vec<CameraDeviceOption> {
    let mut results = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if !line.contains("--camera-id=") {
            continue;
        }
        // e.g. "--camera-id=0    (back, 4096x3072, fps={10, 15, 20, 24, 30}, zoom-range=[1, 15])"
        let id_part = match line.split_whitespace().next() {
            Some(part) => part,
            None => continue,
        };
        let id = match id_part.strip_prefix("--camera-id=") {
            Some(id) => id.to_string(),
            None => continue,
        };

        let open_paren = match line.find('(') {
            Some(pos) => pos,
            None => continue,
        };
        let close_paren = match line.rfind(')') {
            Some(pos) => pos,
            None => continue,
        };
        if close_paren <= open_paren {
            continue;
        }
        let details = &line[open_paren + 1..close_paren];

        // Parse fields: facing, resolution, fps={...}, zoom-range=[...]
        let mut facing = String::new();
        let mut resolution = String::new();
        let mut fps = Vec::new();
        let mut zoom_range = None;

        // Extract fps={...}
        let mut details_rem = details.to_string();
        if let Some(fps_start) = details_rem.find("fps={") {
            if let Some(fps_end) = details_rem[fps_start..].find('}') {
                let fps_inner = &details_rem[fps_start + 5..fps_start + fps_end];
                fps = fps_inner
                    .split(',')
                    .filter_map(|s| s.trim().parse::<u32>().ok())
                    .collect();
                let before = &details_rem[..fps_start];
                let after = &details_rem[fps_start + fps_end + 1..];
                details_rem = format!("{},{}", before, after);
            }
        }

        // Extract zoom-range=[min, max]
        if let Some(zoom_start) = details_rem.find("zoom-range=[") {
            if let Some(zoom_end) = details_rem[zoom_start..].find(']') {
                let zoom_inner = &details_rem[zoom_start + 12..zoom_start + zoom_end];
                let parts: Vec<f32> = zoom_inner
                    .split(',')
                    .filter_map(|s| s.trim().parse::<f32>().ok())
                    .collect();
                if parts.len() == 2 {
                    zoom_range = Some((parts[0], parts[1]));
                }
                let before = &details_rem[..zoom_start];
                let after = &details_rem[zoom_start + zoom_end + 1..];
                details_rem = format!("{},{}", before, after);
            }
        }

        // The remaining comma-separated items include facing and resolution
        for item in details_rem.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            if item == "back" || item == "front" || item == "external" || item == "unknown" {
                facing = item.to_string();
            } else if item.contains('x') && item.chars().all(|c| c.is_ascii_digit() || c == 'x') {
                resolution = item.to_string();
            }
        }

        results.push(CameraDeviceOption {
            id,
            facing,
            resolution,
            fps,
            zoom_range,
        });
    }
    results
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

/// Parse scrcpy's virtual-display announcement, e.g.
/// `[server] INFO: New display: 1080x2436/440 (id=123)` → (123, (1080, 2436)).
pub(crate) fn parse_new_display_line(line: &str) -> Option<(u32, (u32, u32))> {
    let rest = line.split("New display: ").nth(1)?;
    let size_tok = rest.split_whitespace().next()?;
    let wh = size_tok.split('/').next()?;
    let (w, h) = wh.split_once('x')?;
    let w: u32 = w.trim().parse().ok()?;
    let h: u32 = h.trim().parse().ok()?;
    let id_part = rest.split("(id=").nth(1)?;
    let id: u32 = id_part.split(')').next()?.trim().parse().ok()?;
    Some((id, (w, h)))
}

/// Find the scrcpy virtual display in `dumpsys display` output: id + current
/// size (flex mode resizes the display, so the size here is live). Matches the
/// `DisplayInfo{"scrcpy", displayId N, ... real W x H, ...}` block.
pub(crate) fn parse_virtual_display(dumpsys: &str) -> Option<(u32, (u32, u32))> {
    for line in dumpsys.lines() {
        if !line.contains("DisplayInfo{\"scrcpy\"") {
            continue;
        }
        let Some(rest) = line.split("displayId ").nth(1) else { continue };
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(id) = digits.parse::<u32>() else { continue };
        let Some(real) = line.split(" real ").nth(1) else { continue };
        let Some((w_str, h_rest)) = real.split_once(" x ") else { continue };
        let h_digits: String = h_rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let Ok(w) = w_str.trim().parse::<u32>() else { continue };
        let Ok(h) = h_digits.parse::<u32>() else { continue };
        return Some((id, (w, h)));
    }
    None
}

/// Record a discovered virtual display id/size on a tracked session.
fn note_virtual_display(app: &AppHandle, session_id: &str, id: u32, size: (u32, u32)) -> Result<(), String> {
    if let Some(st) = app.try_state::<AppState>() {
        if let Some(s) = st.sessions.lock().map_err(poison_error)?.get_mut(session_id) {
            s.display_id = Some(id);
            s.virtual_size = Some(size);
        }
    }
    Ok(())
}

/// Spawn scrcpy with a prepared arg vector and register a tracked session.
/// On an early encoder/connection failure, retries once with conservative flags
/// (`retried` guards against an infinite retry loop).
/// Build a base scrcpy sidecar command with pinned environment.
fn scrcpy_cmd(app: &AppHandle) -> Result<tauri_plugin_shell::process::Command, String> {
    let server = resolve_server_path(app)
        .ok_or_else(|| "scrcpy-server not found (run scripts/fetch-binaries.ps1)".to_string())?;
    let mut cmd = app
        .shell()
        .sidecar("scrcpy")
        .map_err(|e| e.to_string())?
        .env("SCRCPY_SERVER_PATH", server.to_string_lossy().to_string());
    if let Some(adb) = adb_path() {
        cmd = cmd.env("ADB", adb.to_string_lossy().to_string());
    }
    Ok(cmd)
}

fn spawn_session(
    app: &AppHandle,
    serial: String,
    args: Vec<String>,
    retried: bool,
    mode: &str,
) -> Result<SessionInfo, String> {
    let (mut rx, child) = scrcpy_cmd(app)?
        .args(args.clone())
        .spawn()
        .map_err(|e| e.to_string())?;

    let id = uuid::Uuid::new_v4().to_string();
    let pid = child.pid();
    let started_at = now_ms();
    let info = SessionInfo {
        id: id.clone(),
        serial: serial.clone(),
        pid,
        started_at,
        mode: mode.to_string(),
    };

    app.state::<AppState>().sessions.lock().map_err(poison_error)?.insert(
        id.clone(),
        Session {
            id: id.clone(),
            serial: serial.clone(),
            pid,
            started_at,
            child,
            args: args.clone(),
            display_id: None,
            virtual_size: None,
            mode: mode.to_string(),
        },
    );

    let _ = app.emit("session-started", info.clone());

    // Drain the child's event stream; accumulate stderr, clean up + notify on exit.
    let app2 = app.clone();
    let id2 = id.clone();
    let serial2 = serial.clone();
    let args2 = args.clone();
    let mode2 = mode.to_string();
    tauri::async_runtime::spawn(async move {
        let mut stderr_buf = String::new();
        let mut command_error: Option<String> = None;
        while let Some(ev) = rx.recv().await {
            match ev {
                CommandEvent::Stdout(bytes) => {
                    // Flex mode: scrcpy announces its virtual display here.
                    let text = String::from_utf8_lossy(&bytes);
                    if let Some((did, size)) = parse_new_display_line(&text) {
                        let _ = note_virtual_display(&app2, &id2, did, size);
                    }
                }
                CommandEvent::Stderr(bytes) => {
                    let text = String::from_utf8_lossy(&bytes);
                    // scrcpy logs may land on either stream depending on build.
                    if let Some((did, size)) = parse_new_display_line(&text) {
                        let _ = note_virtual_display(&app2, &id2, did, size);
                    }
                    stderr_buf.push_str(&text);
                    // Keep the tail; scrcpy's useful ERROR line is near the end.
                    if stderr_buf.len() > 16_384 {
                        let cut = stderr_buf.len() - 16_384;
                        stderr_buf.drain(..cut);
                    }
                }
                CommandEvent::Error(e) => command_error = Some(e),
                CommandEvent::Terminated(payload) => {
                    if let Some(st) = app2.try_state::<AppState>() {
                        match st.sessions.lock() {
                            Ok(mut s) => {
                                s.remove(&id2);
                            }
                            Err(e) => eprintln!("Sessions mutex poisoned on session exit: {}", e),
                        }
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
                        let _ = spawn_session(&app2, serial2.clone(), degrade_args(&args2), true, &mode2);
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
    spawn_session(&app, serial, args, false, "display")
}

/// Query cameras available on a device via scrcpy --list-cameras.
#[tauri::command]
pub async fn list_device_cameras(
    app: AppHandle,
    serial: String,
) -> Result<Vec<CameraDeviceOption>, String> {
    let output = scrcpy_cmd(&app)?
        .args(["--serial", &serial, "--list-cameras"])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let cameras = parse_list_cameras_output(&stdout);
    if cameras.is_empty() && !output.status.success() {
        let err = summarize_scrcpy_error(&stderr, None);
        return Err(err);
    }
    Ok(cameras)
}

/// Launch scrcpy camera mode for a device and track the session.
#[tauri::command]
pub fn start_camera_mirror(
    app: AppHandle,
    serial: String,
    settings: CameraSettings,
) -> Result<SessionInfo, String> {
    let args = build_scrcpy_camera_args(&serial, &settings);
    spawn_session(&app, serial, args, false, "camera")
}

/// Send camera shortcut to the running scrcpy camera window.
#[tauri::command]
pub fn send_camera_shortcut(
    _app: AppHandle,
    serial: String,
    action: String,
) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow};

        extern "system" {
            fn keybd_event(b_vk: u8, b_scan: u8, dw_flags: u32, dw_extra_info: usize);
        }

        let titles = [
            format!("AnyLeap Camera — {}", serial),
            format!("AnyLeap — {}", serial),
        ];

        let mut found_hwnd = None;
        for t in &titles {
            let wide: Vec<u16> = t.encode_utf16().chain(std::iter::once(0)).collect();
            if let Ok(hwnd) = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(wide.as_ptr())) } {
                if !hwnd.0.is_null() {
                    found_hwnd = Some(hwnd);
                    break;
                }
            }
        }

        let hwnd = found_hwnd.ok_or_else(|| format!("scrcpy camera window for {} not found", serial))?;
        unsafe {
            let _ = SetForegroundWindow(hwnd);
            const VK_MENU: u8 = 0x12;
            const VK_SHIFT: u8 = 0x10;
            const VK_T: u8 = 0x54;
            const VK_UP: u8 = 0x26;
            const VK_DOWN: u8 = 0x28;
            const KEYEVENTF_KEYUP: u32 = 0x0002;

            match action.as_str() {
                "torch_on" => {
                    keybd_event(VK_MENU, 0, 0, 0);
                    keybd_event(VK_T, 0, 0, 0);
                    keybd_event(VK_T, 0, KEYEVENTF_KEYUP, 0);
                    keybd_event(VK_MENU, 0, KEYEVENTF_KEYUP, 0);
                }
                "torch_off" => {
                    keybd_event(VK_MENU, 0, 0, 0);
                    keybd_event(VK_SHIFT, 0, 0, 0);
                    keybd_event(VK_T, 0, 0, 0);
                    keybd_event(VK_T, 0, KEYEVENTF_KEYUP, 0);
                    keybd_event(VK_SHIFT, 0, KEYEVENTF_KEYUP, 0);
                    keybd_event(VK_MENU, 0, KEYEVENTF_KEYUP, 0);
                }
                "zoom_in" => {
                    keybd_event(VK_MENU, 0, 0, 0);
                    keybd_event(VK_UP, 0, 0, 0);
                    keybd_event(VK_UP, 0, KEYEVENTF_KEYUP, 0);
                    keybd_event(VK_MENU, 0, KEYEVENTF_KEYUP, 0);
                }
                "zoom_out" => {
                    keybd_event(VK_MENU, 0, 0, 0);
                    keybd_event(VK_DOWN, 0, 0, 0);
                    keybd_event(VK_DOWN, 0, KEYEVENTF_KEYUP, 0);
                    keybd_event(VK_MENU, 0, KEYEVENTF_KEYUP, 0);
                }
                _ => return Err(format!("unknown camera shortcut action: {}", action)),
            }
        }
    }
    Ok(())
}

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

/// Toggle scrcpy screen power mode dynamically via shortcut (MOD+o / MOD+Shift+o)
/// without killing the mirror or dropping the connection.
#[tauri::command]
pub fn restart_with_screen_off(
    app: AppHandle,
    serial: String,
    off: bool,
) -> Result<SessionInfo, String> {
    #[cfg(windows)]
    {
        use windows::core::BOOL;
        use windows::Win32::Foundation::{HWND, LPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
        };
        extern "system" {
            fn AttachThreadInput(id_attach: u32, id_attach_to: u32, f_attach: i32) -> i32;
            fn keybd_event(b_vk: u8, b_scan: u8, dw_flags: u32, dw_extra_info: usize);
            fn GetCurrentThreadId() -> u32;
        }

        struct EnumData {
            target_pid: u32,
            hwnd: Option<HWND>,
        }

        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
            if IsWindowVisible(hwnd).as_bool() {
                let mut pid = 0u32;
                GetWindowThreadProcessId(hwnd, Some(&mut pid));
                let data = &mut *(lparam.0 as *mut EnumData);
                if pid == data.target_pid {
                    data.hwnd = Some(hwnd);
                    return BOOL(0);
                }
            }
            BOOL(1)
        }

        let state = app.state::<AppState>();
        let target_pid = {
            let map = state.sessions.lock().map_err(poison_error)?;
            map.values().find(|s| s.serial == serial).map(|s| s.pid)
        };

        if let Some(pid) = target_pid {
            let mut data = EnumData {
                target_pid: pid,
                hwnd: None,
            };
            let _ = unsafe { EnumWindows(Some(enum_proc), LPARAM(&mut data as *mut EnumData as isize)) };

            let hwnd = crate::embed::get_embedded_scrcpy_hwnd(&serial)
                .map(|h| HWND(h as *mut _))
                .or(data.hwnd);

            if let Some(hwnd) = hwnd {
                unsafe {
                    let target_thread = GetWindowThreadProcessId(hwnd, None);
                    let current_thread = GetCurrentThreadId();
                    let attached = AttachThreadInput(current_thread, target_thread, 1);
                    let _ = SetForegroundWindow(hwnd);
                    std::thread::sleep(std::time::Duration::from_millis(50));

                    const VK_MENU: u8 = 0x12;
                    const VK_SHIFT: u8 = 0x10;
                    const VK_O: u8 = 0x4F;
                    const KEYEVENTF_KEYUP: u32 = 0x0002;

                    keybd_event(VK_MENU, 0, 0, 0);
                    if !off {
                        keybd_event(VK_SHIFT, 0, 0, 0);
                    }
                    keybd_event(VK_O, 0, 0, 0);
                    keybd_event(VK_O, 0, KEYEVENTF_KEYUP, 0);
                    if !off {
                        keybd_event(VK_SHIFT, 0, KEYEVENTF_KEYUP, 0);
                    }
                    keybd_event(VK_MENU, 0, KEYEVENTF_KEYUP, 0);

                    if attached != 0 {
                        let _ = AttachThreadInput(current_thread, target_thread, 0);
                    }

                    // Also post direct key message as fallback guarantee for SDL window
                    use windows::Win32::UI::WindowsAndMessaging::{PostMessageW, WM_SYSKEYDOWN, WM_SYSKEYUP};
                    const VK_O_WPARAM: windows::Win32::Foundation::WPARAM = windows::Win32::Foundation::WPARAM(0x4F);
                    const ALT_LPARAM: windows::Win32::Foundation::LPARAM = windows::Win32::Foundation::LPARAM(1 << 29);
                    let _ = PostMessageW(Some(hwnd), WM_SYSKEYDOWN, VK_O_WPARAM, ALT_LPARAM);
                    let _ = PostMessageW(Some(hwnd), WM_SYSKEYUP, VK_O_WPARAM, ALT_LPARAM);
                }

                let mut map = state.sessions.lock().map_err(poison_error)?;
                if let Some(s) = map.values_mut().find(|s| s.serial == serial) {
                    s.args = toggle_screen_off_args(&s.args, off);
                    return Ok(SessionInfo {
                        id: s.id.clone(),
                        serial: s.serial.clone(),
                        pid: s.pid,
                        started_at: s.started_at,
                        mode: s.mode.clone(),
                    });
                }
            }
        }
    }

    // Fallback if window not found or non-windows: restart scrcpy
    let old = {
        let state = app.state::<AppState>();
        let mut map = state.sessions.lock().map_err(poison_error)?;
        let key = map
            .iter()
            .find(|(_, s)| s.serial == serial)
            .map(|(k, _)| k.clone());
        key.and_then(|k| map.remove(&k))
    };
    let old = old.ok_or_else(|| "no active mirror for this device".to_string())?;
    let args = toggle_screen_off_args(&old.args, off);
    let _ = old.child.kill();
    spawn_session(&app, serial, args, false, &old.mode)
}

/// Stop a running session by killing its scrcpy child.
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

/// List currently running sessions.
#[tauri::command]
pub fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionInfo>, String> {
    Ok(state
        .sessions
        .lock()
        .map_err(poison_error)?
        .values()
        .map(|s| SessionInfo {
            id: s.id.clone(),
            serial: s.serial.clone(),
            pid: s.pid,
            started_at: s.started_at,
            mode: s.mode.clone(),
        })
        .collect())
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

/// Keycodes that act on a specific display and must be `-d`-targeted when the
/// session mirrors a virtual display: HOME(3), BACK(4), APP_SWITCH/Recents(187).
/// Volume/power/sleep/screenshot are global and stay untargeted.
const DISPLAY_TARGETED_KEYCODES: [u32; 5] = [3, 4, 187, 82, 111];

/// Send an Android key event to a device (`adb shell input keyevent`).
/// For flex (virtual display) sessions, navigation keys are injected into the
/// mirrored display via `input -d <id>` so they act on what the user sees.
#[tauri::command]
pub async fn send_keyevent(app: AppHandle, serial: String, keycode: u32) -> Result<(), String> {
    let kc = keycode.to_string();
    let display_id = if DISPLAY_TARGETED_KEYCODES.contains(&keycode) {
        app.state::<AppState>()
            .sessions
            .lock()
            .map_err(poison_error)?
            .values()
            .find(|s| s.serial == serial)
            .and_then(|s| s.display_id)
    } else {
        None
    };
    let display_id = if display_id.is_none() && DISPLAY_TARGETED_KEYCODES.contains(&keycode) {
        if let Some(adb) = adb_path() {
            query_virtual_display(&adb, &serial).map(|(d, _)| d)
        } else {
            None
        }
    } else {
        display_id
    };

    let mut args: Vec<String> = vec!["-s".into(), serial.clone(), "shell".into(), "input".into()];
    if let Some(did) = display_id {
        args.push("-d".into());
        args.push(did.to_string());
    }
    args.push("keyevent".into());
    args.push(kc);

    let output = adb_cmd(&app)?
        .args(args)
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

/// Toggle the device's main display between portrait and landscape via ADB.
/// `user_rotation` is only honored while auto-rotate is off, so auto-rotate is
/// disabled first. Has no effect on a virtual display (flex display mode).
#[tauri::command]
pub async fn toggle_device_orientation(app: AppHandle, serial: String) -> Result<String, String> {
    let auto = adb_cmd(&app)?
        .args(["-s", &serial, "shell", "settings", "put", "system", "accelerometer_rotation", "0"])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !auto.status.success() {
        return Err(String::from_utf8_lossy(&auto.stderr).trim().to_string());
    }

    let get = adb_cmd(&app)?
        .args(["-s", &serial, "shell", "settings", "get", "system", "user_rotation"])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if !get.status.success() {
        return Err(String::from_utf8_lossy(&get.stderr).trim().to_string());
    }
    let raw = String::from_utf8_lossy(&get.stdout).trim().to_string();

    // 1/3 = landscape / reverse landscape; anything else (0/2/unset) = portrait.
    let next = if raw == "1" || raw == "3" { "0" } else { "1" };
    let output = adb_cmd(&app)?
        .args(["-s", &serial, "shell", "settings", "put", "system", "user_rotation", next])
        .output()
        .await
        .map_err(|e| e.to_string())?;
    if output.status.success() {
        let label = if next == "0" { "portrait" } else { "landscape" };
        Ok(label.to_string())
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
    /// Client area in screen coords — the video region without borders and
    /// title bar, so the control strip can overlay *inside* the window edge.
    pub client_x: i32,
    pub client_y: i32,
    pub client_width: i32,
    pub client_height: i32,
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
        use windows::Win32::Foundation::POINT;
        use windows::Win32::Graphics::Gdi::ClientToScreen;
        use windows::Win32::UI::WindowsAndMessaging::{
            FindWindowW, GetClientRect, GetForegroundWindow, GetWindowRect, GetWindowTextW,
            IsIconic,
        };

        let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let hwnd = unsafe { FindWindowW(PCWSTR::null(), PCWSTR(wide.as_ptr())) }.ok()?;
        let minimized = unsafe { IsIconic(hwnd) }.as_bool();
        let mut rect = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;

        let mut client = RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) }.ok()?;
        let mut origin = POINT::default();
        let _ = unsafe { ClientToScreen(hwnd, &mut origin) };

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
            client_x: origin.x,
            client_y: origin.y,
            client_width: client.right,
            client_height: client.bottom,
            foreground,
        })
    }
    #[cfg(not(windows))]
    {
        let _ = title;
        None
    }
}
/// Drag-and-drop file/APK push. APKs are installed with `adb install -r`;
/// other files are pushed to `/sdcard/Download/`.
#[tauri::command]
pub async fn handle_dropped_files(
    app: AppHandle,
    serial: String,
    paths: Vec<String>,
) -> Result<String, String> {
    if paths.is_empty() {
        return Ok("No files provided".to_string());
    }
    let mut installed = 0;
    let mut pushed = 0;
    for path in &paths {
        if path.to_lowercase().ends_with(".apk") {
            let out = adb_cmd(&app)?
                .args(["-s", &serial, "install", "-r", path])
                .output()
                .await
                .map_err(|e| e.to_string())?;
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let combined = format!("{stdout}\n{stderr}");
            if !out.status.success() || combined.contains("Failure") {
                return Err(format!("APK install failed for {path}: {}", combined.trim()));
            }
            installed += 1;
        } else {
            let out = adb_cmd(&app)?
                .args(["-s", &serial, "push", path, "/sdcard/Download/"])
                .output()
                .await
                .map_err(|e| e.to_string())?;
            if !out.status.success() {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                let combined = format!("{stdout}\n{stderr}");
                return Err(format!("File push failed for {path}: {}", combined.trim()));
            }
            pushed += 1;
        }
    }
    Ok(format!("Installed {installed} APK(s), pushed {pushed} file(s)"))
}

/// Parse package lines from `pm list packages -3` output.
fn parse_installed_packages(text: &str) -> Vec<String> {
    let mut packages: Vec<String> = text
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            if let Some(pkg) = line.strip_prefix("package:") {
                let p = pkg.trim();
                if !p.is_empty() {
                    Some(p.to_string())
                } else {
                    None
                }
            } else {
                Some(line.to_string())
            }
        })
        .collect();
    packages.sort();
    packages.dedup();
    packages
}

/// List third-party installed packages on the device.
#[tauri::command]
pub async fn list_installed_apps(
    app: AppHandle,
    serial: String,
) -> Result<Vec<String>, String> {
    let output = adb_cmd(&app)?
        .args(["-s", &serial, "shell", "pm", "list", "packages", "-3"])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to list packages: {}", err.trim()));
    }

    Ok(parse_installed_packages(&String::from_utf8_lossy(&output.stdout)))
}

/// Launch an app by package name via Android's monkey launcher intent.
#[tauri::command]
pub async fn launch_app(
    app: AppHandle,
    serial: String,
    package_name: String,
) -> Result<(), String> {
    let output = adb_cmd(&app)?
        .args([
            "-s",
            &serial,
            "shell",
            "monkey",
            "-p",
            &package_name,
            "-c",
            "android.intent.category.LAUNCHER",
            "1",
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{stdout}\n{stderr}");

    if !output.status.success()
        || combined.contains("** No activities found")
        || combined.contains("monkey aborted")
    {
        let msg = combined.trim();
        return Err(if msg.is_empty() {
            format!("Failed to launch {package_name}")
        } else {
            format!("Failed to launch {package_name}: {msg}")
        });
    }

    Ok(())
}

/// Direct capture of device screen into PNG bytes.
#[tauri::command]
pub async fn take_screenshot(app: AppHandle, serial: String) -> Result<Vec<u8>, String> {
    let output = adb_cmd(&app)?
        .args(["-s", &serial, "exec-out", "screencap", "-p"])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Failed to take screenshot: {}", err.trim()));
    }

    if output.stdout.is_empty() {
        return Err("Screenshot captured 0 bytes".to_string());
    }

    Ok(output.stdout)
}
/// Get primary monitor resolution (width, height)
#[tauri::command]
pub fn get_system_resolution() -> (u32, u32) {
    #[cfg(windows)]
    {
        use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};
        let w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
        let h = unsafe { GetSystemMetrics(SM_CYSCREEN) };
        if w > 0 && h > 0 {
            return (w as u32, h as u32);
        }
    }
    (1920, 1080)
}

pub(crate) fn query_virtual_display(adb: &std::path::Path, serial: &str) -> Option<(u32, (u32, u32))> {
    let out = std::process::Command::new(adb)
        .args(["-s", serial, "shell", "dumpsys", "display"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_virtual_display(&String::from_utf8_lossy(&out.stdout))
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

    fn base_settings() -> CoreSettings {
        CoreSettings {
            max_size: 0,
            video_bit_rate: 0,
            max_fps: 0,
            video_codec: String::new(),
            stay_awake: false,
            turn_screen_off: false,
            fullscreen: false,
            show_touches: false,
            no_audio: false,
            no_control: false,
            no_keyboard_ime: false,
            flex_display: false,
            flex_display_size: String::new(),
            no_window_aspect_ratio_lock: false,
            render_fit: String::new(),
            embedded: false,
            hide_virtual_taskbar: false,
        }
    }

    #[test]
    fn parses_new_display_log_line() {
        // Captured from scrcpy 4.0 against a real device (Android 16).
        let line = "[server] INFO: New display: 1080x2436/440 (id=123)";
        assert_eq!(parse_new_display_line(line), Some((123, (1080, 2436))));
        // Without an explicit dpi suffix.
        assert_eq!(
            parse_new_display_line("[server] INFO: New display: 1920x1080 (id=5)"),
            Some((5, (1920, 1080)))
        );
        assert!(parse_new_display_line("INFO: Texture: 956x2160").is_none());
        assert!(parse_new_display_line("New display: garbage (id=)").is_none());
    }

    #[test]
    fn parses_virtual_display_from_dumpsys() {
        // Trimmed from a real Android 16 `dumpsys display` line.
        let dump = "    mBaseDisplayInfo=DisplayInfo{\"scrcpy\", displayId 123, displayGroupId 1, \
FLAG_PRESENTATION, FLAG_TRUSTED, real 1080 x 2436, largest app 1080 x 2436, density 440}";
        assert_eq!(parse_virtual_display(dump), Some((123, (1080, 2436))));
        // Non-scrcpy displays are ignored.
        let other = "DisplayInfo{\"Built-in Screen\", displayId 0, real 1080 x 2436}";
        assert!(parse_virtual_display(other).is_none());
        assert!(parse_virtual_display("").is_none());
    }

    #[test]
    fn flex_display_emits_new_display_and_flex_flags() {
        let mut s = base_settings();
        s.flex_display = true;
        let args = build_scrcpy_args("SER", &s);
        assert!(args.contains(&"--new-display".to_string()));
        assert!(args.contains(&"--flex-display".to_string()));

        s.flex_display_size = "1920x1080/240".into();
        let args = build_scrcpy_args("SER", &s);
        assert!(args.contains(&"--new-display=1920x1080/240".to_string()));
        assert!(!args.contains(&"--new-display".to_string()));
    }

    #[test]
    fn render_fit_is_explicit_even_for_letterbox() {
        let mut s = base_settings();
        s.render_fit = "letterbox".into();
        let args = build_scrcpy_args("SER", &s);
        assert!(args.contains(&"--render-fit=letterbox".to_string()));

        s.render_fit = String::new();
        let args = build_scrcpy_args("SER", &s);
        assert!(!args.iter().any(|a| a.starts_with("--render-fit")));
    }

    #[test]
    fn tolerates_spaces_in_name() {
        let text = "my phone _adb-tls-connect._tcp 10.0.0.7:5555\n";
        let svcs = parse_mdns_services(text);
        assert_eq!(svcs.len(), 1);
        assert_eq!(svcs[0].name, "my phone");
        assert_eq!(svcs[0].port, 5555);
    }

    #[test]
    fn test_parse_list_cameras_output() {
        let output = r#"
[server] INFO: List of cameras:
    --camera-id=0    (back, 4096x3072, fps={10, 15, 20, 24, 30}, zoom-range=[1, 15])
    --camera-id=1    (front, 4080x3072, fps={10, 15, 20, 24, 30}, zoom-range=[1, 4])
"#;
        let cameras = parse_list_cameras_output(output);
        assert_eq!(cameras.len(), 2);
        assert_eq!(cameras[0].id, "0");
        assert_eq!(cameras[0].facing, "back");
        assert_eq!(cameras[0].resolution, "4096x3072");
        assert_eq!(cameras[0].fps, vec![10, 15, 20, 24, 30]);
        assert_eq!(cameras[0].zoom_range, Some((1.0, 15.0)));

        assert_eq!(cameras[1].id, "1");
        assert_eq!(cameras[1].facing, "front");
    }

    #[test]
    fn test_build_scrcpy_camera_args() {
        let s = CameraSettings {
            facing: "back".into(),
            camera_id: None,
            size: Some("1920x1080".into()),
            fps: Some(30),
            high_speed: false,
            torch: true,
            no_audio: false,
        };
        let args = build_scrcpy_camera_args("SER123", &s);
        assert!(args.contains(&"--video-source=camera".to_string()));
        assert!(args.contains(&"--camera-facing=back".to_string()));
        assert!(args.contains(&"--camera-size=1920x1080".to_string()));
        assert!(args.contains(&"--camera-fps=30".to_string()));
        assert!(args.contains(&"--camera-torch".to_string()));
        assert!(!args.contains(&"--no-audio".to_string()));
        assert!(args.contains(&"--window-title=AnyLeap Camera — SER123".to_string()));
    }

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
    #[test]
    fn embedded_mode_emits_borderless_and_no_aspect_ratio_lock() {
        let mut s = base_settings();
        s.embedded = true;
        let args = build_scrcpy_args("SER", &s);
        assert!(args.contains(&"--window-borderless".to_string()));
        assert!(args.contains(&"--no-window-aspect-ratio-lock".to_string()));
    }
    #[test]
    fn parses_installed_packages_output() {
        let text = "package:com.android.chrome\r\npackage:org.mozilla.firefox\npackage:com.example.app\n";
        let pkgs = parse_installed_packages(text);
        assert_eq!(pkgs, vec!["com.android.chrome", "com.example.app", "org.mozilla.firefox"]);
    }
    #[test]
    fn parses_battery_info() {
        let text = "Current Battery Service state:\n\
  AC powered: false\n\
  USB powered: true\n\
  level: 82\n\
  scale: 100\n";
        let info = parse_battery_info(text);
        assert_eq!(info.level, Some(82));
        assert!(info.charging);
    }

}

/// Save and push an image from clipboard directly to device's /sdcard/Download folder
#[tauri::command]
pub async fn push_clipboard_image(
    app: AppHandle,
    serial: String,
    image_bytes: Vec<u8>,
    filename: Option<String>,
) -> Result<String, String> {
    if image_bytes.is_empty() {
        return Err("No image data provided".into());
    }
    let fname = filename.unwrap_or_else(|| {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        format!("paste_{}.png", ts)
    });

    let temp_dir = std::env::temp_dir();
    let temp_path = temp_dir.join(&fname);
    std::fs::write(&temp_path, &image_bytes)
        .map_err(|e| format!("Failed to write temp file: {e}"))?;

    let device_dest = format!("/sdcard/Download/{}", fname);
    let push_res = adb_cmd(&app)?
        .args([
            "-s",
            &serial,
            "push",
            temp_path.to_str().unwrap_or_default(),
            &device_dest,
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    let _ = std::fs::remove_file(&temp_path);

    if !push_res.status.success() {
        return Err(format!(
            "adb push image failed: {}",
            String::from_utf8_lossy(&push_res.stderr)
        ));
    }

    // Trigger media scanner so Android Gallery picks it up immediately
    let _ = adb_cmd(&app)?
        .args([
            "-s",
            &serial,
            "shell",
            "am",
            "broadcast",
            "-a",
            "android.intent.action.MEDIA_SCANNER_SCAN_FILE",
            "-d",
            &format!("file://{}", device_dest),
        ])
        .output()
        .await;

    Ok(device_dest)
}
#[derive(serde::Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct BatteryInfo {
    pub level: Option<u32>,
    pub charging: bool,
}

pub(crate) fn parse_battery_info(output: &str) -> BatteryInfo {
    let mut level = None;
    let mut charging = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("level:") {
            if let Ok(lvl) = rest.trim().parse::<u32>() {
                level = Some(lvl);
            }
        } else if trimmed.starts_with("AC powered: true")
            || trimmed.starts_with("USB powered: true")
            || trimmed.starts_with("Wireless powered: true")
        {
            charging = true;
        }
    }
    BatteryInfo { level, charging }
}

#[tauri::command]
pub async fn get_battery_info(
    app: AppHandle,
    serial: String,
) -> Result<BatteryInfo, String> {
    let output = adb_cmd(&app)?
        .args(["-s", &serial, "shell", "dumpsys", "battery"])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Ok(BatteryInfo::default());
    }

    Ok(parse_battery_info(&String::from_utf8_lossy(&output.stdout)))
}
