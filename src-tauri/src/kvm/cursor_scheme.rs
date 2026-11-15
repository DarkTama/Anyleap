//! Pointer scheme synchronization (SPEC-25a).
//! Main PC reads its `HKCU\Control Panel\Cursors` `.cur`/`.ani` files once per link.
//! Linked PC caches them in staging (never writes to Linked PC's registry), applies
//! them via `SetSystemCursor` while entered, and restores via `SPI_SETCURSORS` on
//! return, unlink, or crash recovery.

use super::files::sanitize_name;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

pub const MAX_CURSOR_FILE_BYTES: u64 = 2 * 1024 * 1024;
const APPLIED_MARKER_NAME: &str = ".applied";

/// Standard Windows cursor registry role names mapped to `OCR_*` `SYSTEM_CURSOR_ID` values.
pub const CURSOR_ROLES: &[(&str, u32)] = &[
    ("Arrow", 32512),       // OCR_NORMAL
    ("IBeam", 32513),       // OCR_IBEAM
    ("Wait", 32514),        // OCR_WAIT
    ("Crosshair", 32515),   // OCR_CROSS
    ("UpArrow", 32516),     // OCR_UP
    ("SizeNWSE", 32642),    // OCR_SIZENWSE
    ("SizeNESW", 32643),    // OCR_SIZENESW
    ("SizeWE", 32644),      // OCR_SIZEWE
    ("SizeNS", 32645),      // OCR_SIZENS
    ("SizeAll", 32646),     // OCR_SIZEALL
    ("No", 32648),          // OCR_NO
    ("Hand", 32649),        // OCR_HAND
    ("AppStarting", 32650), // OCR_APPSTARTING
    ("Help", 32651),        // OCR_HELP
    ("Pin", 32671),         // OCR_PIN
    ("Person", 32672),      // OCR_PERSON
];

/// Map a Windows cursor registry role name to its `OCR_*` system cursor ID.
pub fn cursor_role_to_ocr_id(role: &str) -> Option<u32> {
    CURSOR_ROLES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(role))
        .map(|(_, id)| *id)
}

/// Canonical role name if `role` is one of the known `CURSOR_ROLES`.
pub fn canonical_cursor_role(role: &str) -> Option<&'static str> {
    CURSOR_ROLES
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(role))
        .map(|(name, _)| *name)
}

/// True only for `.cur` or `.ani` cursor files.
pub fn is_supported_cursor_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("cur") || ext.eq_ignore_ascii_case("ani"))
}

#[derive(Debug, Clone)]
pub struct CursorRoleFile {
    pub role: String,
    pub name: String,
    pub data: Vec<u8>,
}

/// Read the local user's configured `.cur`/`.ani` cursor scheme from `HKCU\Control Panel\Cursors`.
pub fn read_local_cursor_scheme() -> Vec<CursorRoleFile> {
    #[cfg(windows)]
    {
        read_local_cursor_scheme_win()
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

#[cfg(windows)]
fn read_local_cursor_scheme_win() -> Vec<CursorRoleFile> {
    use windows::core::{PCWSTR, PWSTR};
    use windows::Win32::System::Environment::ExpandEnvironmentStringsW;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
        REG_EXPAND_SZ, REG_SZ, REG_VALUE_TYPE,
    };

    let mut out = Vec::new();
    let subkey: Vec<u16> = "Control Panel\\Cursors\0".encode_utf16().collect();
    let mut hkey = HKEY::default();
    let open_res = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(subkey.as_ptr()),
            Some(0),
            KEY_READ,
            &mut hkey,
        )
    };
    if open_res.is_err() {
        return out;
    }

    for &(role, _) in CURSOR_ROLES {
        let val_name: Vec<u16> = role.encode_utf16().chain(std::iter::once(0)).collect();
        let mut reg_type = REG_VALUE_TYPE(0);
        let mut buf_bytes = [0u8; 1024];
        let mut buf_len = buf_bytes.len() as u32;
        let q_res = unsafe {
            RegQueryValueExW(
                hkey,
                PCWSTR(val_name.as_ptr()),
                None,
                Some(&mut reg_type),
                Some(buf_bytes.as_mut_ptr()),
                Some(&mut buf_len),
            )
        };
        if q_res.is_err() || (reg_type != REG_SZ && reg_type != REG_EXPAND_SZ) || buf_len < 2 {
            continue;
        }
        let u16_len = (buf_len as usize) / 2;
        let mut raw_u16 = Vec::with_capacity(u16_len);
        for chunk in buf_bytes[..u16_len * 2].chunks_exact(2) {
            raw_u16.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        if raw_u16.last().copied() != Some(0) {
            raw_u16.push(0);
        }

        let mut expanded = vec![0u16; 1024];
        let exp_len =
            unsafe { ExpandEnvironmentStringsW(PCWSTR(raw_u16.as_ptr()), Some(&mut expanded)) };
        let path_str = if exp_len > 0 && (exp_len as usize) <= expanded.len() {
            let s = &expanded[..exp_len as usize];
            let nul = s.iter().position(|&c| c == 0).unwrap_or(s.len());
            String::from_utf16_lossy(&s[..nul])
        } else {
            let nul = raw_u16
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(raw_u16.len());
            String::from_utf16_lossy(&raw_u16[..nul])
        };
        let trimmed = path_str.trim();
        if trimmed.is_empty() {
            continue;
        }
        let p = PathBuf::from(trimmed);
        if !is_supported_cursor_file(&p) || !p.is_file() {
            continue;
        }
        let Ok(meta) = std::fs::metadata(&p) else {
            continue;
        };
        if meta.len() == 0 || meta.len() > MAX_CURSOR_FILE_BYTES {
            continue;
        }
        let Ok(data) = std::fs::read(&p) else {
            continue;
        };
        let name = p
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(sanitize_name)
            .unwrap_or_else(|| format!("{role}.cur"));
        out.push(CursorRoleFile {
            role: role.to_string(),
            name,
            data,
        });
    }

    let _ = unsafe { RegCloseKey(hkey) };
    let _ = PWSTR::null();
    out
}

#[derive(Default)]
struct CursorSchemeCache {
    offered_roles: Vec<String>,
    files: HashMap<String, PathBuf>,
    applied: bool,
}

static SCHEME_CACHE: OnceLock<Mutex<CursorSchemeCache>> = OnceLock::new();

fn cache() -> &'static Mutex<CursorSchemeCache> {
    SCHEME_CACHE.get_or_init(|| Mutex::new(CursorSchemeCache::default()))
}

fn cursors_cache_dir(staging_root: &Path) -> PathBuf {
    staging_root.join("_cursors")
}

pub fn set_offered_roles(staging_root: &Path, roles: Vec<String>) {
    if roles.is_empty() {
        clear_remote_cursors(staging_root);
        return;
    }
    let valid_roles: Vec<String> = roles
        .into_iter()
        .filter_map(|r| canonical_cursor_role(&r).map(str::to_string))
        .collect();
    if let Ok(mut g) = cache().lock() {
        g.offered_roles = valid_roles;
    }
}

pub fn store_remote_cursor_file(
    staging_root: &Path,
    role: &str,
    name: &str,
    data: &[u8],
) -> Result<(), String> {
    let canonical =
        canonical_cursor_role(role).ok_or_else(|| format!("unsupported cursor role {role:?}"))?;
    let safe_name = sanitize_name(name).ok_or_else(|| format!("unsafe cursor name {name:?}"))?;
    let path_check = Path::new(&safe_name);
    if !is_supported_cursor_file(path_check) {
        return Err(format!("unsupported cursor extension {safe_name:?}"));
    }
    if data.is_empty() || data.len() as u64 > MAX_CURSOR_FILE_BYTES {
        return Err("invalid cursor file size".into());
    }

    let dir = cursors_cache_dir(staging_root);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let ext = path_check
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("cur")
        .to_ascii_lowercase();
    let dest = dir.join(format!("{canonical}.{ext}"));
    std::fs::write(&dest, data).map_err(|e| e.to_string())?;

    if let Ok(mut g) = cache().lock() {
        g.files.insert(canonical.to_string(), dest);
    }
    Ok(())
}

/// Apply cached remote cursor files using `SetSystemCursor` (never touches registry).
pub fn apply_remote_cursors(staging_root: &Path) {
    let entries: Vec<(u32, PathBuf)> = {
        let Ok(g) = cache().lock() else {
            return;
        };
        if g.files.is_empty() {
            return;
        }
        g.files
            .iter()
            .filter_map(|(role, path)| cursor_role_to_ocr_id(role).map(|id| (id, path.clone())))
            .collect()
    };
    if entries.is_empty() {
        return;
    }

    let dir = cursors_cache_dir(staging_root);
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(dir.join(APPLIED_MARKER_NAME), b"1");

    #[cfg(windows)]
    unsafe {
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::{
            LoadCursorFromFileW, SetSystemCursor, SYSTEM_CURSOR_ID,
        };
        for (ocr_id, path) in entries {
            let wide: Vec<u16> = path
                .to_string_lossy()
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            if let Ok(hcur) = LoadCursorFromFileW(PCWSTR(wide.as_ptr())) {
                let _ = SetSystemCursor(hcur, SYSTEM_CURSOR_ID(ocr_id));
            }
        }
    }

    if let Ok(mut g) = cache().lock() {
        g.applied = true;
    }
}

/// Restore the Linked PC's own system cursors via `SystemParametersInfoW(SPI_SETCURSORS)`.
pub fn restore_system_cursors(staging_root: &Path) {
    let was_applied = cache().lock().map(|g| g.applied).unwrap_or(false);
    let marker = cursors_cache_dir(staging_root).join(APPLIED_MARKER_NAME);
    if !was_applied && !marker.exists() {
        return;
    }

    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            SystemParametersInfoW, SPIF_SENDCHANGE, SPI_SETCURSORS,
        };
        let _ = SystemParametersInfoW(SPI_SETCURSORS, 0, None, SPIF_SENDCHANGE);
    }

    let _ = std::fs::remove_file(&marker);
    if let Ok(mut g) = cache().lock() {
        g.applied = false;
    }
}

/// Restore cursors if an earlier crashed session left `.applied` on disk.
pub fn restore_if_crash_marker(staging_root: &Path) {
    let marker = cursors_cache_dir(staging_root).join(APPLIED_MARKER_NAME);
    if marker.exists() {
        restore_system_cursors(staging_root);
    }
}

/// Restore system cursors (if applied) and remove cached remote cursor files.
pub fn clear_remote_cursors(staging_root: &Path) {
    restore_system_cursors(staging_root);
    if let Ok(mut g) = cache().lock() {
        g.offered_roles.clear();
        g.files.clear();
        g.applied = false;
    }
    let dir = cursors_cache_dir(staging_root);
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_role_mapping_covers_standard_roles_and_rejects_unknown() {
        assert_eq!(cursor_role_to_ocr_id("Arrow"), Some(32512));
        assert_eq!(cursor_role_to_ocr_id("arrow"), Some(32512));
        assert_eq!(cursor_role_to_ocr_id("IBeam"), Some(32513));
        assert_eq!(cursor_role_to_ocr_id("Wait"), Some(32514));
        assert_eq!(cursor_role_to_ocr_id("Crosshair"), Some(32515));
        assert_eq!(cursor_role_to_ocr_id("UpArrow"), Some(32516));
        assert_eq!(cursor_role_to_ocr_id("SizeNWSE"), Some(32642));
        assert_eq!(cursor_role_to_ocr_id("SizeNESW"), Some(32643));
        assert_eq!(cursor_role_to_ocr_id("SizeWE"), Some(32644));
        assert_eq!(cursor_role_to_ocr_id("SizeNS"), Some(32645));
        assert_eq!(cursor_role_to_ocr_id("SizeAll"), Some(32646));
        assert_eq!(cursor_role_to_ocr_id("No"), Some(32648));
        assert_eq!(cursor_role_to_ocr_id("Hand"), Some(32649));
        assert_eq!(cursor_role_to_ocr_id("AppStarting"), Some(32650));
        assert_eq!(cursor_role_to_ocr_id("Help"), Some(32651));
        assert_eq!(cursor_role_to_ocr_id("UnknownRole"), None);
        assert_eq!(canonical_cursor_role("hand"), Some("Hand"));
        assert_eq!(canonical_cursor_role("../evil"), None);
    }

    #[test]
    fn cursor_file_extension_filter_accepts_cur_and_ani_only() {
        assert!(is_supported_cursor_file(Path::new(
            "C:\\Windows\\Cursors\\arrow.cur"
        )));
        assert!(is_supported_cursor_file(Path::new("busy.ANI")));
        assert!(!is_supported_cursor_file(Path::new("evil.exe")));
        assert!(!is_supported_cursor_file(Path::new("noext")));
    }

    #[test]
    fn store_remote_cursor_file_validates_role_extension_and_writes_cache() {
        let root = std::env::temp_dir().join(format!("anyleap-cur-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);

        assert!(store_remote_cursor_file(&root, "InvalidRole", "a.cur", b"CUR").is_err());
        assert!(store_remote_cursor_file(&root, "Arrow", "a.exe", b"CUR").is_err());
        assert!(store_remote_cursor_file(&root, "Arrow", "../a.cur", b"CUR").is_ok()); // sanitize_name strips path to a.cur
        assert!(root.join("_cursors").join("Arrow.cur").exists());

        clear_remote_cursors(&root);
        assert!(!root.join("_cursors").exists());
        let _ = std::fs::remove_dir_all(&root);
    }
}
