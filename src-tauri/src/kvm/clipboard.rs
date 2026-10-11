use super::protocol::KvmPacket;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

static SUPPRESS_ECHO: AtomicBool = AtomicBool::new(false);

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

pub fn start_clipboard_sync(
    tx: mpsc::Sender<KvmPacket>,
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
                        // Offload heavy image extraction to blocking thread pool so Tokio runtime is not stalled
                        let tx_clone = tx.clone();
                        tokio::task::spawn_blocking(move || {
                            if let Some(img_data) = get_clipboard_image() {
                                let _ = tx_clone.blocking_send(KvmPacket::ClipboardImage {
                                    format: "dib".to_string(),
                                    data: img_data,
                                });
                            } else if let Some(text) = get_clipboard_text() {
                                if !text.is_empty() {
                                    let _ = tx_clone.blocking_send(KvmPacket::ClipboardText { text });
                                }
                            }
                        });
                    }
                }
            }
        }
    });
}
