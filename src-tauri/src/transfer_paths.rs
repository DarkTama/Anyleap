//! Shared destination path resolver for PC-side receives and phone uploads (SPEC-31).

use serde::Serialize;
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{OnceLock, RwLock};

pub const DEFAULT_PHONE_UPLOAD_DIR: &str = "/sdcard/Download";
const DEFAULT_SUBFOLDER: &str = "AnyLeap";
const INVALID_PHONE_DIR_MSG: &str =
    "Enter a folder inside /sdcard or /storage/emulated/0 (for example /sdcard/Download).";

static PROBE_SEQ: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedReceiveDir {
    pub path: PathBuf,
    pub fallback_from: Option<PathBuf>,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DefaultTransferFolders {
    pub default_receive_dir: String,
    pub default_phone_upload_dir: String,
}

#[derive(Clone, Debug)]
struct TransferConfig {
    default_receive_dir: PathBuf,
    default_os_download_dir: Option<PathBuf>,
    configured_receive_dir: Option<PathBuf>,
    phone_upload_dir: String,
}

static TRANSFER_CONFIG: OnceLock<RwLock<TransferConfig>> = OnceLock::new();

fn fallback_os_download_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(|h| PathBuf::from(h).join("Downloads"))
}

fn default_config() -> TransferConfig {
    let os_dl = fallback_os_download_dir();
    let default_receive_dir = os_dl
        .as_ref()
        .map(|d| d.join(DEFAULT_SUBFOLDER))
        .unwrap_or_else(|| std::env::temp_dir().join(DEFAULT_SUBFOLDER));
    TransferConfig {
        default_receive_dir,
        default_os_download_dir: os_dl,
        configured_receive_dir: None,
        phone_upload_dir: DEFAULT_PHONE_UPLOAD_DIR.to_string(),
    }
}

fn config_lock() -> &'static RwLock<TransferConfig> {
    TRANSFER_CONFIG.get_or_init(|| RwLock::new(default_config()))
}

/// Initialize OS-derived default receive folder on application startup.
pub fn init_defaults(os_download_dir: Option<PathBuf>) {
    let resolved_os_dl = os_download_dir.or_else(fallback_os_download_dir);
    let default_receive_dir = resolved_os_dl
        .as_ref()
        .map(|d| d.join(DEFAULT_SUBFOLDER))
        .unwrap_or_else(|| std::env::temp_dir().join(DEFAULT_SUBFOLDER));
    if let Ok(mut cfg) = config_lock().write() {
        cfg.default_receive_dir = default_receive_dir;
        cfg.default_os_download_dir = resolved_os_dl;
    }
}

/// Update live receive/upload folder settings in memory.
pub fn set_config(receive_dir: Option<String>, phone_upload_dir: &str) -> Result<(), String> {
    let norm_phone = normalize_phone_upload_dir(phone_upload_dir)?;
    let parsed_receive = receive_dir
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let mut cfg = config_lock()
        .write()
        .map_err(|_| "Transfer folder config lock poisoned".to_string())?;
    cfg.configured_receive_dir = parsed_receive;
    cfg.phone_upload_dir = norm_phone;
    Ok(())
}

pub fn get_default_folders() -> DefaultTransferFolders {
    let default_receive_dir = config_lock()
        .read()
        .map(|c| c.default_receive_dir.clone())
        .unwrap_or_else(|_| default_config().default_receive_dir);
    DefaultTransferFolders {
        default_receive_dir: default_receive_dir.display().to_string(),
        default_phone_upload_dir: DEFAULT_PHONE_UPLOAD_DIR.to_string(),
    }
}

/// Configured PC receive directory (or the OS default `<Downloads>/AnyLeap`), without probing disk.
pub fn resolve_configured_receive_dir() -> PathBuf {
    config_lock()
        .read()
        .map(|c| {
            c.configured_receive_dir
                .clone()
                .unwrap_or_else(|| c.default_receive_dir.clone())
        })
        .unwrap_or_else(|_| default_config().default_receive_dir)
}

/// Current Android destination directory for mirror drops and clipboard image pastes.
pub fn phone_upload_dir() -> String {
    config_lock()
        .read()
        .map(|c| c.phone_upload_dir.clone())
        .unwrap_or_else(|_| DEFAULT_PHONE_UPLOAD_DIR.to_string())
}

/// Format the user-facing fallback notice required by SPEC-31 §5.4.
pub fn fallback_notice_message(bad_path: &Path, actual_path: &Path) -> String {
    format!(
        "Could not save to \"{}\" — saved to \"{}\" instead.",
        bad_path.display(),
        actual_path.display()
    )
}

fn try_prepare_writable_dir(dir: &Path) -> Result<(), String> {
    if dir.exists() && !dir.is_dir() {
        return Err("Destination path is not a directory".to_string());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let seq = PROBE_SEQ.fetch_add(1, Ordering::Relaxed);
    let probe = dir.join(format!(".anyleap-write-test-{}-{seq}", std::process::id()));
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&probe)
        .map_err(|e| e.to_string())?;
    drop(file);
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

fn prepare_receive_dir_with(
    primary: PathBuf,
    default_receive_dir: PathBuf,
    os_download_dir: Option<PathBuf>,
    temp_fallback: PathBuf,
) -> Result<ResolvedReceiveDir, String> {
    if try_prepare_writable_dir(&primary).is_ok() {
        return Ok(ResolvedReceiveDir {
            path: primary,
            fallback_from: None,
        });
    }

    let mut candidates = Vec::with_capacity(3);
    if default_receive_dir != primary {
        candidates.push(default_receive_dir);
    }
    if let Some(os_dl) = os_download_dir {
        if os_dl != primary && !candidates.contains(&os_dl) {
            candidates.push(os_dl);
        }
    }
    if temp_fallback != primary && !candidates.contains(&temp_fallback) {
        candidates.push(temp_fallback);
    }

    for cand in candidates {
        if try_prepare_writable_dir(&cand).is_ok() {
            return Ok(ResolvedReceiveDir {
                path: cand,
                fallback_from: Some(primary),
            });
        }
    }

    Err(format!(
        "Could not prepare any writable receive folder (tried {})",
        primary.display()
    ))
}

/// Ensure the configured PC receive folder exists and is writable, falling back through
/// `<Downloads>/AnyLeap` -> OS `download_dir` -> `temp_dir()/AnyLeap` if needed.
pub fn prepare_receive_dir() -> Result<ResolvedReceiveDir, String> {
    let (primary, default_receive_dir, os_download_dir) = {
        let cfg = config_lock()
            .read()
            .map_err(|_| "Transfer folder config lock poisoned".to_string())?;
        let primary = cfg
            .configured_receive_dir
            .clone()
            .unwrap_or_else(|| cfg.default_receive_dir.clone());
        (
            primary,
            cfg.default_receive_dir.clone(),
            cfg.default_os_download_dir.clone(),
        )
    };
    prepare_receive_dir_with(
        primary,
        default_receive_dir,
        os_download_dir,
        std::env::temp_dir().join(DEFAULT_SUBFOLDER),
    )
}

/// Ensure an explicit user-picked PC receive folder exists and is writable, falling back cleanly
/// if the chosen folder is unavailable.
pub fn prepare_receive_dir_at(explicit_dir: &Path) -> Result<ResolvedReceiveDir, String> {
    let (default_receive_dir, os_download_dir) = {
        let cfg = config_lock()
            .read()
            .map_err(|_| "Transfer folder config lock poisoned".to_string())?;
        (
            cfg.default_receive_dir.clone(),
            cfg.default_os_download_dir.clone(),
        )
    };
    prepare_receive_dir_with(
        explicit_dir.to_path_buf(),
        default_receive_dir,
        os_download_dir,
        std::env::temp_dir().join(DEFAULT_SUBFOLDER),
    )
}

/// Compute a non-colliding destination path inside `dest_dir`, appending ` (n)` on collision.
pub fn unique_dest_path(dest_dir: &Path, filename: &str) -> PathBuf {
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

fn is_forbidden_phone_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\'' | '"' | '`' | '$' | ';' | '&' | '|' | '<' | '>' | '\\' | '*' | '?'
        )
}

/// Validate and normalize an Android shared-storage destination folder for mirror uploads.
pub fn normalize_phone_upload_dir(raw: &str) -> Result<String, String> {
    if raw.chars().any(is_forbidden_phone_char) {
        return Err(INVALID_PHONE_DIR_MSG.to_string());
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() || !trimmed.starts_with('/') {
        return Err(INVALID_PHONE_DIR_MSG.to_string());
    }

    let mut segments: Vec<&str> = Vec::new();
    for seg in trimmed.split('/') {
        match seg {
            "" | "." => continue,
            ".." => return Err(INVALID_PHONE_DIR_MSG.to_string()),
            other => segments.push(other),
        }
    }

    if segments.is_empty() {
        return Err(INVALID_PHONE_DIR_MSG.to_string());
    }

    let normalized = format!("/{}", segments.join("/"));
    let under_sdcard = normalized == "/sdcard" || normalized.starts_with("/sdcard/");
    let under_emulated =
        normalized == "/storage/emulated/0" || normalized.starts_with("/storage/emulated/0/");

    if !under_sdcard && !under_emulated {
        return Err(INVALID_PHONE_DIR_MSG.to_string());
    }

    Ok(normalized)
}

#[tauri::command]
pub fn set_transfer_folders(
    receive_dir: Option<String>,
    phone_upload_dir: String,
) -> Result<(), String> {
    set_config(receive_dir, &phone_upload_dir)
}

#[tauri::command]
pub fn get_default_transfer_folders() -> DefaultTransferFolders {
    get_default_folders()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static CONFIG_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn temp_test_dir(name: &str) -> PathBuf {
        let seq = PROBE_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "anyleap-transfer-paths-{name}-{}-{seq}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn unique_dest_path_handles_collisions_extensions_and_dotfiles() {
        let dir = temp_test_dir("unique");

        assert_eq!(unique_dest_path(&dir, "report.pdf"), dir.join("report.pdf"));
        std::fs::write(dir.join("report.pdf"), b"1").unwrap();
        assert_eq!(
            unique_dest_path(&dir, "report.pdf"),
            dir.join("report (1).pdf")
        );
        std::fs::write(dir.join("report (1).pdf"), b"2").unwrap();
        assert_eq!(
            unique_dest_path(&dir, "report.pdf"),
            dir.join("report (2).pdf")
        );

        std::fs::write(dir.join("README"), b"r").unwrap();
        assert_eq!(unique_dest_path(&dir, "README"), dir.join("README (1)"));

        std::fs::write(dir.join("archive.tar.gz"), b"a").unwrap();
        assert_eq!(
            unique_dest_path(&dir, "archive.tar.gz"),
            dir.join("archive.tar (1).gz")
        );

        std::fs::write(dir.join(".env"), b"e").unwrap();
        assert_eq!(unique_dest_path(&dir, ".env"), dir.join(".env (1)"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn prepare_receive_dir_creates_writable_subdirs_and_falls_back_on_file_or_bad_path() {
        let root = temp_test_dir("prepare");
        let default_dl = root.join("Downloads").join("AnyLeap");
        let os_dl = root.join("Downloads");
        let temp_fb = root.join("TempFallback");

        let custom = root.join("Custom").join("Nested");
        let ok_res = prepare_receive_dir_with(
            custom.clone(),
            default_dl.clone(),
            Some(os_dl.clone()),
            temp_fb.clone(),
        )
        .unwrap();
        assert_eq!(ok_res.path, custom);
        assert_eq!(ok_res.fallback_from, None);
        assert!(custom.is_dir());

        // Configured path points to an existing regular file -> falls back to default_dl.
        let regular_file = root.join("not-a-dir.txt");
        std::fs::write(&regular_file, b"hello").unwrap();
        let fb_res = prepare_receive_dir_with(
            regular_file.clone(),
            default_dl.clone(),
            Some(os_dl.clone()),
            temp_fb.clone(),
        )
        .unwrap();
        assert_eq!(fb_res.path, default_dl);
        assert_eq!(fb_res.fallback_from, Some(regular_file.clone()));

        // Nested under a regular file also falls back cleanly.
        let nested_under_file = regular_file.join("child");
        let fb_nested = prepare_receive_dir_with(
            nested_under_file.clone(),
            default_dl.clone(),
            Some(os_dl),
            temp_fb,
        )
        .unwrap();
        assert_eq!(fb_nested.path, default_dl);
        assert_eq!(fb_nested.fallback_from, Some(nested_under_file));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn normalize_phone_upload_dir_accepts_shared_storage_and_rejects_unsafe_paths() {
        assert_eq!(
            normalize_phone_upload_dir("/sdcard/Download").unwrap(),
            "/sdcard/Download"
        );
        assert_eq!(
            normalize_phone_upload_dir("  /sdcard/Documents/Work/  ").unwrap(),
            "/sdcard/Documents/Work"
        );
        assert_eq!(
            normalize_phone_upload_dir("/storage/emulated/0/Download").unwrap(),
            "/storage/emulated/0/Download"
        );
        assert_eq!(
            normalize_phone_upload_dir("/sdcard/My Folder").unwrap(),
            "/sdcard/My Folder"
        );

        for bad in [
            "",
            "   ",
            "sdcard/Download",
            "/sdcard/../Download",
            "/sdcard/Download/..",
            "/data/local/tmp",
            "/storage/emulated/1/Download",
            "/sdcard/Download\0",
            "/sdcard/Download\n",
            "/sdcard/Download\r",
            "/sdcard/\"Download\"",
            "/sdcard/'Download'",
            "/sdcard/Download;rm",
            "/sdcard/$HOME",
        ] {
            assert!(
                normalize_phone_upload_dir(bad).is_err(),
                "expected error for {bad:?}"
            );
        }
    }

    #[test]
    fn set_config_updates_live_state_and_preserves_on_error() {
        let _guard = CONFIG_TEST_LOCK.lock().unwrap();
        let root = temp_test_dir("live-cfg");
        let dl_root = root.join("Downloads");
        init_defaults(Some(dl_root.clone()));

        let custom = root.join("CustomReceive");
        set_config(
            Some(custom.to_string_lossy().into_owned()),
            "/sdcard/Documents/",
        )
        .unwrap();
        assert_eq!(resolve_configured_receive_dir(), custom);
        assert_eq!(phone_upload_dir(), "/sdcard/Documents");

        // Invalid phone path leaves prior valid config intact.
        assert!(set_config(None, "/data/local/tmp").is_err());
        assert_eq!(resolve_configured_receive_dir(), custom);
        assert_eq!(phone_upload_dir(), "/sdcard/Documents");

        // Reset receive_dir to None restores OS default.
        set_config(None, DEFAULT_PHONE_UPLOAD_DIR).unwrap();
        assert_eq!(resolve_configured_receive_dir(), dl_root.join("AnyLeap"));
        assert_eq!(phone_upload_dir(), "/sdcard/Download");

        let _ = std::fs::remove_dir_all(&root);
    }
}
