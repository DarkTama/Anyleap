use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::commands::adb_cmd;

pub(crate) const DEVICE_ROOT: &str = "/storage/emulated/0";
const PHOTO_CACHE_CAP_BYTES: u64 = 500 * 1024 * 1024;
const PHOTO_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp", "heic"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DeviceEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
    pub modified_unix: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct PhoneNotification {
    pub key: String,
    pub package: String,
    pub app_label: Option<String>,
    pub title: String,
    pub text: String,
    pub posted_unix: i64,
}

/// Single-quote a string for the Android `sh` shell.
pub(crate) fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn validate_serial(serial: &str) -> Result<(), String> {
    let trimmed = serial.trim();
    if trimmed.is_empty()
        || trimmed.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err("Invalid device serial".to_string());
    }
    Ok(())
}

/// Normalize a device path and ensure it stays under `/storage/emulated/0`.
pub(crate) fn normalize_device_path(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.contains('\0') || trimmed.contains('\n') || trimmed.contains('\r') {
        return Err("Invalid device path".to_string());
    }
    if !trimmed.starts_with('/') {
        return Err("Device path must be absolute".to_string());
    }

    let mut segments: Vec<&str> = Vec::new();
    for seg in trimmed.split('/') {
        match seg {
            "" | "." => continue,
            ".." => return Err("Path traversal ('..') is not allowed".to_string()),
            other => segments.push(other),
        }
    }

    let normalized = format!("/{}", segments.join("/"));
    if normalized != DEVICE_ROOT && !normalized.starts_with(&format!("{DEVICE_ROOT}/")) {
        return Err(format!("Path must stay under {DEVICE_ROOT}"));
    }
    Ok(normalized)
}

/// Extract and validate a safe PC filename from the final component of a device path.
pub(crate) fn sanitize_pc_filename(device_path: &str) -> Result<String, String> {
    let trimmed = device_path.trim().trim_end_matches('/');
    let component = trimmed.rsplit('/').next().unwrap_or("");
    if component.is_empty() || component == "." || component == ".." {
        return Err("Invalid destination filename".to_string());
    }
    if component.chars().any(|c| {
        c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
    }) {
        return Err("Filename contains invalid characters".to_string());
    }
    let stem_upper = component
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    let is_reserved = matches!(
        stem_upper.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    );
    if is_reserved {
        return Err(format!("Reserved Windows filename: {component}"));
    }
    Ok(component.to_string())
}

/// Compute a non-colliding destination path inside `dest_dir`, appending ` (n)` on collision.
pub(crate) fn unique_dest_path(dest_dir: &Path, filename: &str) -> PathBuf {
    let candidate = dest_dir.join(filename);
    if !candidate.exists() {
        return candidate;
    }
    let path = Path::new(filename);
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();

    let mut n = 1u32;
    loop {
        let next = dest_dir.join(format!("{stem} ({n}){ext}"));
        if !next.exists() {
            return next;
        }
        n += 1;
    }
}

/// Parse toybox `stat -c '%F|%s|%Y|%n'` output.
pub(crate) fn parse_stat_output(output: &str, parent_dir: &str) -> Vec<DeviceEntry> {
    let empty_glob = format!("{}/{}", parent_dir.trim_end_matches('/'), "*");
    let mut entries = Vec::new();

    for raw in output.lines() {
        let line = raw.trim_end_matches('\r');
        if line.is_empty() || line.starts_with("stat:") {
            continue;
        }
        let mut parts = line.splitn(4, '|');
        let (Some(ftype), Some(size_str), Some(mtime_str), Some(full_path)) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            continue;
        };

        if full_path == empty_glob {
            continue;
        }
        let Ok(size) = size_str.trim().parse::<u64>() else {
            continue;
        };
        let Ok(modified_unix) = mtime_str.trim().parse::<i64>() else {
            continue;
        };
        let name = full_path.rsplit('/').next().unwrap_or("").to_string();
        if name.is_empty() || name == "." || name == ".." || name == "*" {
            continue;
        }
        let is_dir = ftype.trim() == "directory";
        entries.push(DeviceEntry {
            name,
            path: full_path.to_string(),
            is_dir,
            size,
            modified_unix,
        });
    }

    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    entries
}

/// Pure eviction planner: given `(path, size, atime)`, return oldest paths to delete
/// so that remaining bytes are `<= cap_bytes`.
pub(crate) fn select_evictions(entries: &[(PathBuf, u64, u64)], cap_bytes: u64) -> Vec<PathBuf> {
    let mut total: u64 = entries.iter().map(|(_, size, _)| *size).sum();
    if total <= cap_bytes {
        return Vec::new();
    }

    let mut sorted: Vec<&(PathBuf, u64, u64)> = entries.iter().collect();
    sorted.sort_by_key(|(_, _, atime)| *atime);

    let mut to_delete = Vec::new();
    for (path, size, _) in sorted {
        if total <= cap_bytes {
            break;
        }
        total = total.saturating_sub(*size);
        to_delete.push(path.clone());
    }
    to_delete
}

fn fnv1a_hex(input: &str) -> String {
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in input.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn sanitize_serial_dir(serial: &str) -> String {
    serial
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn collect_cache_files(dir: &Path, out: &mut Vec<(PathBuf, u64, u64)>) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_cache_files(&path, out);
        } else if let Ok(meta) = entry.metadata() {
            let atime = meta
                .accessed()
                .or_else(|_| meta.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            out.push((path, meta.len(), atime));
        }
    }
}

fn evict_photo_cache_if_needed(cache_root: &Path) {
    let mut entries = Vec::new();
    collect_cache_files(cache_root, &mut entries);
    for path in select_evictions(&entries, PHOTO_CACHE_CAP_BYTES) {
        let _ = std::fs::remove_file(path);
    }
}

fn parse_flags_value(s: &str) -> Option<u32> {
    let trimmed = s.trim().trim_end_matches(|c: char| !c.is_ascii_hexdigit() && c != 'x' && c != 'X');
    if let Some(hex) = trimmed.strip_prefix("0x").or_else(|| trimmed.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()
    } else {
        trimmed.parse::<u32>().ok()
    }
}

fn unwrap_android_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed == "null" {
        return String::new();
    }
    for prefix in ["String (", "SpannableString (", "CharSequence ("] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            if let Some(inner) = rest.strip_suffix(')') {
                return inner.to_string();
            }
        }
    }
    trimmed.to_string()
}

fn extract_token_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let idx = line.find(key)?;
    let rest = &line[idx + key.len()..];
    Some(rest.split(|c: char| c.is_whitespace() || c == ')').next().unwrap_or(""))
}

fn parse_timestamp_secs(raw: &str) -> Option<i64> {
    let val = raw.trim().parse::<i64>().ok()?;
    if val > 1_000_000_000_000 {
        Some(val / 1000)
    } else if val > 0 {
        Some(val)
    } else {
        None
    }
}

/// Parse `adb shell dumpsys notification --noredact` output into read-only notifications.
/// Unknown formats return an empty list, never an error.
pub(crate) fn parse_dumpsys_notifications(output: &str) -> Vec<PhoneNotification> {
    const FLAG_ONGOING_EVENT: u32 = 0x2;
    const FLAG_GROUP_SUMMARY: u32 = 0x200;

    #[derive(Default)]
    struct RecordBuilder {
        key: String,
        package: String,
        app_label: Option<String>,
        title: String,
        text: String,
        posted_unix: i64,
        flags: u32,
        indent: usize,
    }

    let mut out = Vec::new();
    let mut seen_keys = HashSet::new();
    let mut current: Option<RecordBuilder> = None;

    let flush = |rec: RecordBuilder, out: &mut Vec<PhoneNotification>, seen: &mut HashSet<String>| {
        if (rec.flags & FLAG_ONGOING_EVENT) != 0 || (rec.flags & FLAG_GROUP_SUMMARY) != 0 {
            return;
        }
        if rec.key.is_empty() || rec.package.is_empty() {
            return;
        }
        if rec.title.is_empty() && rec.text.is_empty() {
            return;
        }
        if seen.insert(rec.key.clone()) {
            out.push(PhoneNotification {
                key: rec.key,
                package: rec.package,
                app_label: rec.app_label,
                title: rec.title,
                text: rec.text,
                posted_unix: rec.posted_unix,
            });
        }
    };

    for raw_line in output.lines() {
        let line = raw_line.trim_end_matches('\r');
        let indent = line.len() - line.trim_start().len();
        let trimmed = line.trim();

        if trimmed.contains("NotificationRecord(") {
            if let Some(prev) = current.take() {
                flush(prev, &mut out, &mut seen_keys);
            }
            let mut rec = RecordBuilder {
                indent,
                ..Default::default()
            };
            if let Some(pkg) = extract_token_value(trimmed, "pkg=") {
                rec.package = pkg.to_string();
            }
            if let Some(key) = extract_token_value(trimmed, "key=") {
                rec.key = key.to_string();
            }
            current = Some(rec);
            continue;
        }

        let Some(rec) = current.as_mut() else {
            continue;
        };

        if !trimmed.is_empty() && indent <= rec.indent {
            let finished = current.take().unwrap();
            flush(finished, &mut out, &mut seen_keys);
            continue;
        }

        if let Some(val) = trimmed.strip_prefix("key=") {
            if rec.key.is_empty() {
                rec.key = val.trim().to_string();
            }
        } else if let Some(val) = trimmed.strip_prefix("appName=") {
            let label = val.trim();
            if !label.is_empty() && label != "null" {
                rec.app_label = Some(label.to_string());
            }
        } else if let Some(val) = trimmed.strip_prefix("flags=") {
            if let Some(f) = parse_flags_value(val) {
                rec.flags |= f;
            }
        } else if let Some(f_tok) = extract_token_value(trimmed, "flags=") {
            if let Some(f) = parse_flags_value(f_tok) {
                rec.flags |= f;
            }
        }

        if let Some(val) = trimmed.strip_prefix("android.title=") {
            rec.title = unwrap_android_value(val);
        } else if let Some(val) = trimmed.strip_prefix("android.text=") {
            rec.text = unwrap_android_value(val);
        } else if let Some(val) = trimmed.strip_prefix("postTime=") {
            if let Some(ts) = parse_timestamp_secs(val) {
                rec.posted_unix = ts;
            }
        } else if let Some(val) = trimmed.strip_prefix("when=") {
            if rec.posted_unix == 0 {
                if let Some(ts) = parse_timestamp_secs(val) {
                    rec.posted_unix = ts;
                }
            }
        }
    }

    if let Some(last) = current.take() {
        flush(last, &mut out, &mut seen_keys);
    }

    out.sort_by(|a, b| b.posted_unix.cmp(&a.posted_unix));
    out
}

async fn run_adb_shell(app: &AppHandle, serial: &str, shell_cmd: &str) -> Result<String, String> {
    let out = adb_cmd(app)?
        .args(["-s", serial, "shell", shell_cmd])
        .output()
        .await
        .map_err(|e| format!("Failed to run adb shell: {e}"))?;
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

#[tauri::command]
pub async fn list_device_dir(
    app: AppHandle,
    serial: String,
    path: String,
) -> Result<Vec<DeviceEntry>, String> {
    validate_serial(&serial)?;
    let norm = normalize_device_path(&path)?;
    let cmd = format!("stat -c '%F|%s|%Y|%n' {}/*", sh_quote(&norm));
    let stdout = run_adb_shell(&app, &serial, &cmd).await?;
    Ok(parse_stat_output(&stdout, &norm))
}

#[tauri::command]
pub async fn pull_device_file(
    app: AppHandle,
    serial: String,
    device_path: String,
    dest_dir: String,
) -> Result<String, String> {
    validate_serial(&serial)?;
    let norm = normalize_device_path(&device_path)?;
    let filename = sanitize_pc_filename(&norm)?;
    let dest_dir_path = Path::new(&dest_dir);
    if !dest_dir_path.is_dir() {
        return Err("Destination folder does not exist".to_string());
    }
    let final_path = unique_dest_path(dest_dir_path, &filename);
    let final_str = final_path.to_string_lossy().to_string();

    let out = adb_cmd(&app)?
        .args(["-s", &serial, "pull", &norm, &final_str])
        .output()
        .await
        .map_err(|e| format!("Failed to run adb pull: {e}"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let stdout = String::from_utf8_lossy(&out.stdout);
        return Err(format!("adb pull failed: {stderr} {stdout}").trim().to_string());
    }
    Ok(final_str)
}

#[tauri::command]
pub async fn push_to_device(
    app: AppHandle,
    serial: String,
    local_paths: Vec<String>,
    device_dir: String,
) -> Result<(), String> {
    validate_serial(&serial)?;
    let norm_dir = normalize_device_path(&device_dir)?;
    for local in local_paths {
        if !Path::new(&local).exists() {
            return Err(format!("Local path does not exist: {local}"));
        }
        let target = format!("{norm_dir}/");
        let out = adb_cmd(&app)?
            .args(["-s", &serial, "push", &local, &target])
            .output()
            .await
            .map_err(|e| format!("Failed to run adb push: {e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            return Err(format!("adb push failed: {stderr} {stdout}").trim().to_string());
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_device_path(
    app: AppHandle,
    serial: String,
    device_path: String,
) -> Result<(), String> {
    validate_serial(&serial)?;
    let norm = normalize_device_path(&device_path)?;
    if norm == DEVICE_ROOT {
        return Err("Cannot delete internal storage root".to_string());
    }
    let cmd = format!("rm -r {}", sh_quote(&norm));
    let out = adb_cmd(&app)?
        .args(["-s", &serial, "shell", &cmd])
        .output()
        .await
        .map_err(|e| format!("Failed to run rm: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Delete failed: {}", stderr.trim()));
    }
    Ok(())
}

#[tauri::command]
pub async fn list_device_photos(
    app: AppHandle,
    serial: String,
    offset: usize,
    limit: usize,
) -> Result<Vec<DeviceEntry>, String> {
    validate_serial(&serial)?;
    let camera_dir = "/storage/emulated/0/DCIM/Camera";
    let screenshots_dir = "/storage/emulated/0/Pictures/Screenshots";

    let cmd_cam = format!("stat -c '%F|%s|%Y|%n' {}/*", sh_quote(camera_dir));
    let cmd_scr = format!("stat -c '%F|%s|%Y|%n' {}/*", sh_quote(screenshots_dir));

    let (cam_out, scr_out) = tokio::join!(
        run_adb_shell(&app, &serial, &cmd_cam),
        run_adb_shell(&app, &serial, &cmd_scr),
    );

    let mut photos = Vec::new();
    if let Ok(out) = cam_out {
        photos.extend(parse_stat_output(&out, camera_dir));
    }
    if let Ok(out) = scr_out {
        photos.extend(parse_stat_output(&out, screenshots_dir));
    }

    photos.retain(|entry| {
        if entry.is_dir {
            return false;
        }
        let ext = entry
            .name
            .rsplit('.')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        PHOTO_EXTENSIONS.contains(&ext.as_str())
    });

    photos.sort_by(|a, b| {
        b.modified_unix
            .cmp(&a.modified_unix)
            .then_with(|| a.name.cmp(&b.name))
    });

    Ok(photos.into_iter().skip(offset).take(limit).collect())
}

#[tauri::command]
pub async fn cache_device_photo(
    app: AppHandle,
    serial: String,
    device_path: String,
    modified_unix: Option<i64>,
) -> Result<String, String> {
    validate_serial(&serial)?;
    let norm = normalize_device_path(&device_path)?;
    let filename = sanitize_pc_filename(&norm)?;
    let ext = Path::new(&filename)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .filter(|e| PHOTO_EXTENSIONS.contains(&e.as_str()))
        .ok_or_else(|| "Unsupported image extension".to_string())?;

    let cache_root = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("Failed to resolve app cache dir: {e}"))?
        .join("photos");
    let serial_dir = cache_root.join(sanitize_serial_dir(&serial));
    std::fs::create_dir_all(&serial_dir)
        .map_err(|e| format!("Failed to create photo cache dir: {e}"))?;

    let hash_input = format!("{norm}:{}", modified_unix.unwrap_or(0));
    let target = serial_dir.join(format!("{}.{ext}", fnv1a_hex(&hash_input)));

    if target.exists() && target.metadata().map(|m| m.len() > 0).unwrap_or(false) {
        return Ok(target.to_string_lossy().to_string());
    }

    let target_str = target.to_string_lossy().to_string();
    let out = adb_cmd(&app)?
        .args(["-s", &serial, "pull", &norm, &target_str])
        .output()
        .await
        .map_err(|e| format!("Failed to pull photo: {e}"))?;

    if !out.status.success() {
        let _ = std::fs::remove_file(&target);
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Photo pull failed: {}", stderr.trim()));
    }

    evict_photo_cache_if_needed(&cache_root);
    Ok(target_str)
}

#[tauri::command]
pub async fn get_device_notifications(
    app: AppHandle,
    serial: String,
) -> Result<Vec<PhoneNotification>, String> {
    validate_serial(&serial)?;
    let out = adb_cmd(&app)?
        .args(["-s", &serial, "shell", "dumpsys", "notification", "--noredact"])
        .output()
        .await
        .map_err(|e| format!("Failed to query notifications: {e}"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("dumpsys notification failed: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&out.stdout);
    Ok(parse_dumpsys_notifications(&stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sh_quote_escapes_single_quotes() {
        assert_eq!(sh_quote("/storage/emulated/0/a b"), "'/storage/emulated/0/a b'");
        assert_eq!(sh_quote("it's"), "'it'\\''s'");
    }

    #[test]
    fn stat_parser_handles_files_dirs_pipes_spaces_empty_and_perm_denied() {
        let sample = "\
regular file|1234|1700000100|/storage/emulated/0/Download/my file | v2.txt\n\
directory|4096|1700000200|/storage/emulated/0/Download/Work Docs\n\
stat: cannot stat '/storage/emulated/0/Download/secret': Permission denied\n\
regular empty file|0|1700000000|/storage/emulated/0/Download/*\n\
";
        let entries = parse_stat_output(sample, "/storage/emulated/0/Download");
        assert_eq!(entries.len(), 2);
        assert!(entries[0].is_dir);
        assert_eq!(entries[0].name, "Work Docs");
        assert!(!entries[1].is_dir);
        assert_eq!(entries[1].name, "my file | v2.txt");
        assert_eq!(entries[1].size, 1234);

        let empty = "stat: '/storage/emulated/0/Empty/*': No such file or directory\n";
        assert!(parse_stat_output(empty, "/storage/emulated/0/Empty").is_empty());
    }

    #[test]
    fn path_normaliser_rejects_traversal_and_other_roots() {
        assert_eq!(
            normalize_device_path("/storage/emulated/0/Download/").unwrap(),
            "/storage/emulated/0/Download"
        );
        assert_eq!(
            normalize_device_path("/storage/emulated/0//DCIM/./Camera").unwrap(),
            "/storage/emulated/0/DCIM/Camera"
        );
        assert!(normalize_device_path("/storage/emulated/0/../other").is_err());
        assert!(normalize_device_path("/data/local/tmp").is_err());
        assert!(normalize_device_path("Download/file.txt").is_err());
    }

    #[test]
    fn pc_filename_sanitiser_rejects_reserved_and_invalid() {
        assert_eq!(
            sanitize_pc_filename("/storage/emulated/0/Download/photo.jpg").unwrap(),
            "photo.jpg"
        );
        assert!(sanitize_pc_filename("/storage/emulated/0/Download/.").is_err());
        assert!(sanitize_pc_filename("/storage/emulated/0/Download/..").is_err());
        assert!(sanitize_pc_filename("/storage/emulated/0/Download/CON").is_err());
        assert!(sanitize_pc_filename("/storage/emulated/0/Download/nul.txt").is_err());
        assert!(sanitize_pc_filename("/storage/emulated/0/Download/com1").is_err());
        assert!(sanitize_pc_filename("").is_err());
    }

    #[test]
    fn dumpsys_notification_parser_handles_android12_and_android14_fixtures() {
        let a12 = include_str!("../tests/fixtures/dumpsys_notification_android12_synthetic.txt");
        let parsed12 = parse_dumpsys_notifications(a12);
        assert_eq!(parsed12.len(), 2);
        assert_eq!(parsed12[0].package, "com.example.auth");
        assert_eq!(parsed12[0].title, "Verification Code");
        assert_eq!(parsed12[0].text, "Your code is 482910");
        assert_eq!(parsed12[0].posted_unix, 1700000500);
        assert_eq!(parsed12[1].package, "com.example.messaging");
        assert_eq!(parsed12[1].title, "Alex");

        let a14 = include_str!("../tests/fixtures/dumpsys_notification_android14_synthetic.txt");
        let parsed14 = parse_dumpsys_notifications(a14);
        assert_eq!(parsed14.len(), 2);
        assert_eq!(parsed14[0].package, "com.example.calendar");
        assert_eq!(parsed14[0].title, "Team Standup");
        assert_eq!(parsed14[1].package, "com.example.mail");
        assert_eq!(parsed14[1].app_label.as_deref(), Some("MailBox"));
        assert_eq!(parsed14[1].text, "main branch checks succeeded in 2m 14s");

        assert!(parse_dumpsys_notifications("random unrecognized output").is_empty());
    }

    #[test]
    fn photo_cache_eviction_deletes_oldest_until_under_cap() {
        let entries = vec![
            (PathBuf::from("b.jpg"), 300, 20),
            (PathBuf::from("a.jpg"), 200, 10),
            (PathBuf::from("c.jpg"), 400, 30),
        ];
        // Total = 900, cap = 500 -> must evict a.jpg (atime 10, 200B -> 700) and b.jpg (atime 20, 300B -> 400)
        let evict = select_evictions(&entries, 500);
        assert_eq!(evict, vec![PathBuf::from("a.jpg"), PathBuf::from("b.jpg")]);
        assert!(select_evictions(&entries, 900).is_empty());
    }
}
