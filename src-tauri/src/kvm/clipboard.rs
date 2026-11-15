use super::protocol::{KvmPacket, Outbound};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

static SUPPRESS_ECHO: AtomicBool = AtomicBool::new(false);
static SYNC_TEXT: AtomicBool = AtomicBool::new(true);
static SYNC_IMAGE: AtomicBool = AtomicBool::new(true);
static SYNC_FILES: AtomicBool = AtomicBool::new(true);

pub fn sync_text_enabled() -> bool {
    SYNC_TEXT.load(Ordering::Relaxed)
}

pub fn sync_image_enabled() -> bool {
    SYNC_IMAGE.load(Ordering::Relaxed)
}

pub fn sync_files_enabled() -> bool {
    SYNC_FILES.load(Ordering::Relaxed)
}

pub fn set_sync_settings(text: bool, image: bool, files: bool) {
    SYNC_TEXT.store(text, Ordering::SeqCst);
    SYNC_IMAGE.store(image, Ordering::SeqCst);
    SYNC_FILES.store(files, Ordering::SeqCst);
}

#[tauri::command]
pub fn kvm_set_sync_settings(text: bool, image: bool, files: bool) {
    set_sync_settings(text, image, files);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardSyncAction {
    SendText,
    SendImage,
    // --- SPEC-25a ---
    SendFiles,
    // --- end SPEC-25a ---
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboundClipboardKind {
    Text,
    Image,
}

pub fn should_apply_inbound(kind: InboundClipboardKind, sync_text: bool, sync_image: bool) -> bool {
    match kind {
        InboundClipboardKind::Text => sync_text,
        InboundClipboardKind::Image => sync_image,
    }
}

/// Prefer `CF_UNICODETEXT` over images when both formats are on the clipboard.
pub fn choose_clipboard_action(
    has_text: bool,
    has_image: bool,
    sync_text: bool,
    sync_image: bool,
) -> ClipboardSyncAction {
    if has_text {
        if sync_text {
            ClipboardSyncAction::SendText
        } else {
            ClipboardSyncAction::None
        }
    } else if has_image && sync_image {
        ClipboardSyncAction::SendImage
    } else {
        ClipboardSyncAction::None
    }
}

// --- SPEC-25a ---
/// Priority: `CF_HDROP` files (when `sync_files`) -> `CF_UNICODETEXT` -> `CF_DIB`.
pub fn choose_clipboard_action_v2(
    has_files: bool,
    has_text: bool,
    has_image: bool,
    sync_files: bool,
    sync_text: bool,
    sync_image: bool,
) -> ClipboardSyncAction {
    if has_files && sync_files {
        ClipboardSyncAction::SendFiles
    } else {
        choose_clipboard_action(has_text, has_image, sync_text, sync_image)
    }
}

/// Build a Win32 `CF_HDROP` (`DROPFILES` + UTF-16 double-null-terminated paths) payload.
pub fn build_dropfiles_bytes(paths: &[std::path::PathBuf]) -> Vec<u8> {
    let mut out = Vec::with_capacity(20 + paths.len() * 64);
    // DROPFILES { pFiles: 20, pt: (0,0), fNC: 0, fWide: 1 }
    out.extend_from_slice(&20u32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&0i32.to_le_bytes());
    out.extend_from_slice(&1i32.to_le_bytes());
    for p in paths {
        let wide: Vec<u16> = p.as_os_str().to_string_lossy().encode_utf16().collect();
        for u in wide {
            out.extend_from_slice(&u.to_le_bytes());
        }
        out.extend_from_slice(&0u16.to_le_bytes());
    }
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

#[cfg(windows)]
pub unsafe fn read_hdrop_paths(hdrop: windows::Win32::UI::Shell::HDROP) -> Vec<std::path::PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use windows::Win32::UI::Shell::DragQueryFileW;

    let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
    let mut out = Vec::with_capacity(count as usize);
    for i in 0..count {
        let len = DragQueryFileW(hdrop, i, None);
        if len == 0 {
            continue;
        }
        let mut buf = vec![0u16; (len + 1) as usize];
        let copied = DragQueryFileW(hdrop, i, Some(&mut buf));
        if copied > 0 {
            let os = OsString::from_wide(&buf[..copied as usize]);
            out.push(std::path::PathBuf::from(os));
        }
    }
    out
}

pub fn set_clipboard_files(paths: &[std::path::PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    if !super::dragdrop::file_clipboard_enabled() || !sync_files_enabled() {
        return Ok(());
    }
    SUPPRESS_ECHO.store(true, Ordering::SeqCst);

    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::DataExchange::*;
        use windows::Win32::System::Memory::*;

        let payload = build_dropfiles_bytes(paths);
        let ok = with_clipboard_retry(|| {
            let _ = EmptyClipboard();
            if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, payload.len()) {
                let ptr = GlobalLock(h);
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(payload.as_ptr(), ptr as *mut u8, payload.len());
                    let _ = GlobalUnlock(h);
                    let _ = SetClipboardData(15, Some(HANDLE(h.0))); // CF_HDROP = 15
                    return Some(());
                }
            }
            None
        });

        if ok.is_none() {
            SUPPRESS_ECHO.store(false, Ordering::SeqCst);
            return Err("Failed to open clipboard for files".to_string());
        }
    }

    tokio::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_millis(600)).await;
        SUPPRESS_ECHO.store(false, Ordering::SeqCst);
    });

    Ok(())
}

pub fn get_clipboard_files() -> Option<Vec<std::path::PathBuf>> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::DataExchange::*;
        use windows::Win32::UI::Shell::HDROP;

        if IsClipboardFormatAvailable(15).is_ok() {
            return with_clipboard_retry(|| {
                if let Ok(handle) = GetClipboardData(15) {
                    let paths = read_hdrop_paths(HDROP(handle.0));
                    if !paths.is_empty() {
                        return Some(paths);
                    }
                }
                None
            });
        }
        None
    }

    #[cfg(not(windows))]
    None
}
// --- end SPEC-25a ---

#[cfg(windows)]
unsafe fn with_clipboard_retry<F, R>(f: F) -> Option<R>
where
    F: Fn() -> Option<R>,
{
    use windows::Win32::System::DataExchange::*;

    for _ in 0..5 {
        if OpenClipboard(None).is_ok() {
            let res = f();
            let _ = CloseClipboard();
            return res;
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    None
}

pub fn set_clipboard_text(text: &str) -> Result<(), String> {
    if !should_apply_inbound(InboundClipboardKind::Text, sync_text_enabled(), sync_image_enabled()) {
        return Ok(());
    }
    SUPPRESS_ECHO.store(true, Ordering::SeqCst);

    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::DataExchange::*;
        use windows::Win32::System::Memory::*;

        let ok = with_clipboard_retry(|| {
            let _ = EmptyClipboard();
            let utf16: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            let bytes_len = utf16.len() * 2;

            if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, bytes_len) {
                let ptr = GlobalLock(h);
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(utf16.as_ptr() as *const u8, ptr as *mut u8, bytes_len);
                    let _ = GlobalUnlock(h);
                    let _ = SetClipboardData(13, Some(HANDLE(h.0)));
                    return Some(());
                }
            }
            None
        });

        if ok.is_none() {
            SUPPRESS_ECHO.store(false, Ordering::SeqCst);
            return Err("Failed to open clipboard".to_string());
        }
    }

    tokio::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_millis(600)).await;
        SUPPRESS_ECHO.store(false, Ordering::SeqCst);
    });

    Ok(())
}

pub fn set_clipboard_image(data: &[u8]) -> Result<(), String> {
    if !should_apply_inbound(InboundClipboardKind::Image, sync_text_enabled(), sync_image_enabled()) {
        return Ok(());
    }
    if data.len() > 15_000_000 {
        return Err("Image too large".to_string());
    }

    SUPPRESS_ECHO.store(true, Ordering::SeqCst);

    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::HANDLE;
        use windows::Win32::System::DataExchange::*;
        use windows::Win32::System::Memory::*;

        let ok = with_clipboard_retry(|| {
            let _ = EmptyClipboard();
            let bytes_len = data.len();

            if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, bytes_len) {
                let ptr = GlobalLock(h);
                if !ptr.is_null() {
                    std::ptr::copy_nonoverlapping(data.as_ptr(), ptr as *mut u8, bytes_len);
                    let _ = GlobalUnlock(h);
                    let _ = SetClipboardData(8, Some(HANDLE(h.0))); // CF_DIB = 8
                    return Some(());
                }
            }
            None
        });

        if ok.is_none() {
            SUPPRESS_ECHO.store(false, Ordering::SeqCst);
            return Err("Failed to open clipboard for image".to_string());
        }
    }

    tokio::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_millis(600)).await;
        SUPPRESS_ECHO.store(false, Ordering::SeqCst);
    });

    Ok(())
}

pub fn get_clipboard_text() -> Option<String> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::HGLOBAL;
        use windows::Win32::System::DataExchange::*;
        use windows::Win32::System::Memory::*;

        if IsClipboardFormatAvailable(13).is_ok() {
            return with_clipboard_retry(|| {
                if let Ok(handle) = GetClipboardData(13) {
                    let h_global = HGLOBAL(handle.0);
                    let ptr = GlobalLock(h_global) as *const u16;
                    if !ptr.is_null() {
                        let mut len = 0;
                        while *ptr.add(len) != 0 {
                            len += 1;
                        }
                        let slice = std::slice::from_raw_parts(ptr, len);
                        let text = String::from_utf16_lossy(slice);
                        let _ = GlobalUnlock(h_global);
                        return Some(text);
                    }
                }
                None
            });
        }
        None
    }

    #[cfg(not(windows))]
    None
}

pub fn get_clipboard_image() -> Option<Vec<u8>> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::HGLOBAL;
        use windows::Win32::System::DataExchange::*;
        use windows::Win32::System::Memory::*;

        if IsClipboardFormatAvailable(8).is_ok() {
            return with_clipboard_retry(|| {
                if let Ok(handle) = GetClipboardData(8) {
                    let h_global = HGLOBAL(handle.0);
                    let size = GlobalSize(h_global);
                    if size > 0 && size < 15_000_000 {
                        let ptr = GlobalLock(h_global) as *const u8;
                        if !ptr.is_null() {
                            let bytes = std::slice::from_raw_parts(ptr, size).to_vec();
                            let _ = GlobalUnlock(h_global);
                            return Some(bytes);
                        }
                    }
                }
                None
            });
        }
        None
    }

    #[cfg(not(windows))]
    None
}

pub fn should_suppress_echo() -> bool {
    SUPPRESS_ECHO.load(Ordering::Relaxed)
}

#[cfg(windows)]
fn has_clipboard_text() -> bool {
    unsafe { windows::Win32::System::DataExchange::IsClipboardFormatAvailable(13).is_ok() }
}

#[cfg(windows)]
fn has_clipboard_image() -> bool {
    unsafe { windows::Win32::System::DataExchange::IsClipboardFormatAvailable(8).is_ok() }
}

// --- SPEC-25a ---
#[cfg(windows)]
fn has_clipboard_files() -> bool {
    unsafe { windows::Win32::System::DataExchange::IsClipboardFormatAvailable(15).is_ok() }
}
// --- end SPEC-25a ---

pub fn start_clipboard_sync(
    tx: mpsc::Sender<Outbound>,
    is_running: Arc<AtomicBool>,
) {
    tokio::spawn(async move {
        #[cfg(windows)]
        unsafe {
            use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;

            let mut last_seq = GetClipboardSequenceNumber();

            while is_running.load(Ordering::Relaxed) {
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

                let cur_seq = GetClipboardSequenceNumber();
                if cur_seq != last_seq {
                    last_seq = cur_seq;

                    if !should_suppress_echo() {
                        let tx_clone = tx.clone();
                        tokio::task::spawn_blocking(move || {
                            match choose_clipboard_action_v2(
                                has_clipboard_files(),
                                has_clipboard_text(),
                                has_clipboard_image(),
                                super::dragdrop::file_clipboard_enabled() && sync_files_enabled(),
                                sync_text_enabled(),
                                sync_image_enabled(),
                            ) {
                                ClipboardSyncAction::SendFiles => {
                                    if let Some(paths) = get_clipboard_files() {
                                        if let Ok((files, sources)) =
                                            super::staging::collect_paths_to_file_meta(&paths)
                                        {
                                            if !files.is_empty() {
                                                let offer_id = uuid::Uuid::new_v4().to_string();
                                                let total_bytes: u64 = files
                                                    .iter()
                                                    .filter(|f| !f.is_dir)
                                                    .map(|f| f.size)
                                                    .sum();
                                                let prefetch = super::staging::decide_clip_offer_mode(
                                                    total_bytes,
                                                    super::staging::clip_prefetch_limit_mb(),
                                                ) == super::staging::ClipOfferMode::Prefetch;
                                                let sources_for_stream = if prefetch {
                                                    Some(sources)
                                                } else {
                                                    let summary = files
                                                        .first()
                                                        .map(|f| f.rel_path.clone())
                                                        .unwrap_or_else(|| "Files".to_string());
                                                    super::staging::insert_pending_outbound_clip(
                                                        super::staging::PendingOutboundClip {
                                                            offer_id: offer_id.clone(),
                                                            files: files.clone(),
                                                            sources,
                                                            summary,
                                                            total_bytes,
                                                            created_at: std::time::Instant::now(),
                                                        },
                                                    );
                                                    None
                                                };
                                                let _ = tx_clone.blocking_send(Outbound::Packet(
                                                    KvmPacket::FileClipOffer {
                                                        offer_id: offer_id.clone(),
                                                        files,
                                                    },
                                                ));
                                                if let Some(sources) = sources_for_stream {
                                                    for (rel_path, disk_path) in sources {
                                                        let size = std::fs::metadata(&disk_path)
                                                            .map(|m| m.len())
                                                            .unwrap_or(0);
                                                        let _ = tx_clone.blocking_send(Outbound::ClipFile {
                                                            offer_id: offer_id.clone(),
                                                            rel_path,
                                                            path: disk_path,
                                                            size,
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                ClipboardSyncAction::SendText => {
                                    if let Some(text) = get_clipboard_text() {
                                        if !text.is_empty() {
                                            let _ = tx_clone.blocking_send(Outbound::Packet(
                                                KvmPacket::ClipboardText { text },
                                            ));
                                        }
                                    }
                                }
                                ClipboardSyncAction::SendImage => {
                                    if let Some(img_data) = get_clipboard_image() {
                                        let _ = tx_clone.blocking_send(Outbound::Image {
                                            format: "dib".to_string(),
                                            data: img_data,
                                        });
                                    }
                                }
                                ClipboardSyncAction::None => {}
                            }
                        });
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_priority_prefers_text_over_image() {
        assert_eq!(
            choose_clipboard_action(true, true, true, true),
            ClipboardSyncAction::SendText
        );
        assert_eq!(
            choose_clipboard_action(true, false, true, true),
            ClipboardSyncAction::SendText
        );
        assert_eq!(
            choose_clipboard_action(false, true, true, true),
            ClipboardSyncAction::SendImage
        );
        assert_eq!(
            choose_clipboard_action(false, false, true, true),
            ClipboardSyncAction::None
        );
    }

    #[test]
    fn sync_settings_gate_outbound_and_inbound_clipboard() {
        // Outbound gating via choose_clipboard_action
        assert_eq!(
            choose_clipboard_action(true, true, false, true),
            ClipboardSyncAction::None
        );
        assert_eq!(
            choose_clipboard_action(false, true, true, false),
            ClipboardSyncAction::None
        );

        // Inbound gating via should_apply_inbound (pure, no global state mutation)
        assert!(should_apply_inbound(InboundClipboardKind::Text, true, false));
        assert!(!should_apply_inbound(InboundClipboardKind::Text, false, true));
        assert!(should_apply_inbound(InboundClipboardKind::Image, false, true));
        assert!(!should_apply_inbound(InboundClipboardKind::Image, true, false));
    }

    // --- SPEC-25a ---
    #[test]
    fn clipboard_v2_prioritizes_files_when_enabled_and_builds_dropfiles() {
        assert_eq!(
            choose_clipboard_action_v2(true, true, true, true, true, true),
            ClipboardSyncAction::SendFiles
        );
        assert_eq!(
            choose_clipboard_action_v2(true, true, false, false, true, true),
            ClipboardSyncAction::SendText
        );
        assert_eq!(
            choose_clipboard_action_v2(true, false, false, false, true, true),
            ClipboardSyncAction::None
        );

        let bytes = build_dropfiles_bytes(&[
            std::path::PathBuf::from("C:\\tmp\\a.txt"),
            std::path::PathBuf::from("C:\\tmp\\b.txt"),
        ]);
        assert_eq!(u32::from_le_bytes(bytes[0..4].try_into().unwrap()), 20);
        assert_eq!(i32::from_le_bytes(bytes[16..20].try_into().unwrap()), 1); // fWide = 1
        assert!(bytes.ends_with(&[0, 0, 0, 0])); // double-null UTF-16 terminator
    }
    // --- end SPEC-25a ---
}
