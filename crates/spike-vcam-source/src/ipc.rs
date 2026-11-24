//! Cross-session shared-memory NV12 frame ring, header validation, and test pattern generator.
#![allow(dead_code)]

use std::ffi::c_void;
use std::fs::OpenOptions;
use std::io::Write;
use std::ptr::{copy_nonoverlapping, null_mut};
use std::sync::atomic::{fence, AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, LocalFree, ERROR_ACCESS_DENIED, GENERIC_READ, GENERIC_WRITE, HANDLE,
    HLOCAL, INVALID_HANDLE_VALUE,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, TokenUser, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY,
    TOKEN_USER,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_ALWAYS,
    OPEN_EXISTING,
};
use windows::Win32::System::Memory::{
    CreateFileMappingW, MapViewOfFile, UnmapViewOfFile, FILE_MAP_READ, FILE_MAP_WRITE,
    MEMORY_MAPPED_VIEW_ADDRESS, PAGE_READONLY, PAGE_READWRITE,
};
use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
use windows::Win32::System::Threading::{
    CreateEventW, GetCurrentProcess, GetCurrentProcessId, OpenEventW, OpenProcessToken, SetEvent,
    WaitForSingleObject, EVENT_MODIFY_STATE,
};

const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;

pub const SPIKE_CLSID_STR: &str = "{A7E4B91C-5D2A-4F8E-9C31-7B2E4D1F8A01}";
pub const SPIKE_CLSID: windows::core::GUID =
    windows::core::GUID::from_u128(0xa7e4b91c_5d2a_4f8e_9c31_7b2e4d1f8a01);

pub const SPIKE_DIR: &str = r"C:\ProgramData\AnyLeapSpike";
pub const SHM_FILE_PATH: &str = r"C:\ProgramData\AnyLeapSpike\frames.shm";
pub const DLL_LOG_PATH: &str = r"C:\ProgramData\AnyLeapSpike\vcam_dll.log";
pub const GLOBAL_EVENT_NAME: &str = r"Global\AnyLeapSpikeFrameEvent";
pub const GLOBAL_HOST_PROBE_EVENT_NAME: &str = r"Global\AnyLeapSpikeHostProbeEvent";

pub const SHM_MAGIC: u32 = 0x414C_5643; // "ALVC"
pub const SHM_VERSION: u32 = 1;
pub const SLOT_COUNT: usize = 3;

pub const FRAME_WIDTH: u32 = 1280;
pub const FRAME_HEIGHT: u32 = 720;
pub const FRAME_FPS: u32 = 30;
pub const MAX_WIDTH: u32 = 3840;
pub const MAX_HEIGHT: u32 = 2160;

pub const NV12_720P_BYTES: usize = (FRAME_WIDTH as usize) * (FRAME_HEIGHT as usize) * 3 / 2;
pub const SLOT_CAPACITY: usize = NV12_720P_BYTES;
pub const TOTAL_SHM_BYTES: usize =
    std::mem::size_of::<SharedFrameHeader>() + SLOT_COUNT * SLOT_CAPACITY;

#[repr(C, align(64))]
pub struct SharedFrameHeader {
    pub magic: u32,
    pub version: u32,
    pub writer_pid: u32,
    pub writer_session_id: u32,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub fps_num: u32,
    pub fps_den: u32,
    pub slot_capacity: u32,
    pub active_slot: AtomicU32,
    pub consumer_count: AtomicU32,
    pub frame_seq: AtomicU64,
    pub timestamp_100ns: AtomicI64,
    pub writer_qpc_us: AtomicU64,
    pub writer_sid_len: u32,
    pub writer_sid: [u8; 64],
}

#[derive(Debug, Clone)]
pub struct FrameSnapshot {
    pub seq: u64,
    pub width: u32,
    pub height: u32,
    pub stride: u32,
    pub timestamp_100ns: i64,
    pub writer_qpc_us: u64,
    pub writer_pid: u32,
    pub writer_session_id: u32,
}

pub fn now_unix_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0)
}

pub fn append_dll_log(line: &str) {
    if let Ok(mut f) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(DLL_LOG_PATH)
    {
        let pid = unsafe { GetCurrentProcessId() };
        let mut sid = 0u32;
        let _ = unsafe { ProcessIdToSessionId(pid, &mut sid) };
        let _ = writeln!(f, "[pid={pid} sess={sid} t_us={}] {line}", now_unix_us());
    }
}

pub fn init_dll_log_file() -> Result<(), String> {
    let user_sid = current_process_sid_string()?;
    let sddl = format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{user_sid})(A;;GA;;;S-1-5-19)");
    let sd = security_descriptor_from_sddl(&sddl)?;
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0 .0,
        bInheritHandle: false.into(),
    };
    let _ = std::fs::remove_file(DLL_LOG_PATH);
    let path_w = to_wide_null(DLL_LOG_PATH);
    unsafe {
        let h = CreateFileW(
            PCWSTR(path_w.as_ptr()),
            GENERIC_READ.0 | GENERIC_WRITE.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            Some(&sa),
            OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL,
            None,
        )
        .map_err(|e| format!("CreateFileW({DLL_LOG_PATH}) failed: {e:?}"))?;
        let _ = CloseHandle(h);
    }
    Ok(())
}

pub fn to_wide_null(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn current_process_sid_string() -> Result<String, String> {
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|e| format!("OpenProcessToken failed: {e:?}"))?;

        let mut needed = 0u32;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut needed);
        if needed == 0 {
            let _ = CloseHandle(token);
            return Err("GetTokenInformation returned needed=0".to_string());
        }

        let mut buf = vec![0u8; needed as usize];
        let res = GetTokenInformation(
            token,
            TokenUser,
            Some(buf.as_mut_ptr() as *mut c_void),
            needed,
            &mut needed,
        );
        let _ = CloseHandle(token);
        res.map_err(|e| format!("GetTokenInformation(TokenUser) failed: {e:?}"))?;

        let token_user = &*(buf.as_ptr() as *const TOKEN_USER);
        let mut sid_pwstr = PWSTR::null();
        ConvertSidToStringSidW(token_user.User.Sid, &mut sid_pwstr)
            .map_err(|e| format!("ConvertSidToStringSidW failed: {e:?}"))?;

        let mut len = 0usize;
        while *sid_pwstr.0.add(len) != 0 {
            len += 1;
        }
        let sid_str = String::from_utf16_lossy(std::slice::from_raw_parts(sid_pwstr.0, len));
        let _ = LocalFree(Some(HLOCAL(sid_pwstr.0 as *mut c_void)));
        Ok(sid_str)
    }
}

pub fn validate_sid_ascii(sid: &str) -> bool {
    sid.starts_with("S-1-")
        && sid.len() <= 64
        && sid
            .bytes()
            .all(|b| b.is_ascii_digit() || b == b'-' || b == b'S')
}

struct OwnedSecurityDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for OwnedSecurityDescriptor {
    fn drop(&mut self) {
        if !self.0 .0.is_null() {
            unsafe {
                let _ = LocalFree(Some(HLOCAL(self.0 .0)));
            }
        }
    }
}

fn security_descriptor_from_sddl(sddl: &str) -> Result<OwnedSecurityDescriptor, String> {
    let wide = to_wide_null(sddl);
    let mut sd = PSECURITY_DESCRIPTOR::default();
    unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            PCWSTR(wide.as_ptr()),
            SDDL_REVISION_1,
            &mut sd,
            None,
        )
        .map_err(|e| {
            format!("ConvertStringSecurityDescriptorToSecurityDescriptorW({sddl}) failed: {e:?}")
        })?;
    }
    Ok(OwnedSecurityDescriptor(sd))
}

pub fn build_file_sddl(user_sid: &str) -> String {
    // Protected DACL: SYSTEM full, Builtin Admins full, Current User full, LOCAL SERVICE read-only (FR)
    format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{user_sid})(A;;FR;;;S-1-5-19)")
}

pub fn build_mapping_sddl(user_sid: &str) -> String {
    // Protected DACL: SYSTEM full, Builtin Admins full, Current User full, LOCAL SERVICE read-only (FR / FILE_MAP_READ)
    format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;{user_sid})(A;;FR;;;S-1-5-19)")
}

pub fn build_event_sddl(user_sid: &str) -> String {
    // Protected DACL: SYSTEM full, Builtin Admins full, LOCAL SERVICE full, Current User EVENT_ALL_ACCESS (0x001F0003)
    format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;S-1-5-19)(A;;0x001F0003;;;{user_sid})")
}

pub fn probe_global_event_creation(user_sid: &str) -> String {
    let sddl = build_event_sddl(user_sid);
    let Ok(sd) = security_descriptor_from_sddl(&sddl) else {
        return "failed to build SDDL".to_string();
    };
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0 .0,
        bInheritHandle: false.into(),
    };
    let ev_name_w = to_wide_null(GLOBAL_HOST_PROBE_EVENT_NAME);
    let ev_res = unsafe {
        match CreateEventW(Some(&sa), false, false, PCWSTR(ev_name_w.as_ptr())) {
            Ok(h) => {
                let _ = CloseHandle(h);
                "CreateEventW(Global\\)=OK".to_string()
            }
            Err(e) => {
                let last = GetLastError();
                format!("CreateEventW(Global\\)=ERR({last:?}, {e:?})")
            }
        }
    };

    let map_name_w = to_wide_null(r"Global\AnyLeapSpikeHostProbeSection");
    let map_res = unsafe {
        match CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            Some(&sa),
            PAGE_READWRITE,
            0,
            4096,
            PCWSTR(map_name_w.as_ptr()),
        ) {
            Ok(h) => {
                let _ = CloseHandle(h);
                "CreateFileMappingW(Global\\)=OK".to_string()
            }
            Err(e) => {
                let last = GetLastError();
                if last == ERROR_ACCESS_DENIED || e.code() == ERROR_ACCESS_DENIED.to_hresult() {
                    "CreateFileMappingW(Global\\)=ERROR_ACCESS_DENIED(5)".to_string()
                } else {
                    format!("CreateFileMappingW(Global\\)=ERR({last:?}, {e:?})")
                }
            }
        }
    };
    format!("{ev_res}; {map_res}")
}

pub struct ShmWriter {
    file: HANDLE,
    mapping: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    event: Option<HANDLE>,
    pub file_sddl: String,
    pub mapping_sddl: String,
    pub user_sid: String,
}

unsafe impl Send for ShmWriter {}

impl ShmWriter {
    pub fn create(width: u32, height: u32, fps: u32) -> Result<Self, String> {
        let user_sid = current_process_sid_string()?;
        let file_sddl = build_file_sddl(&user_sid);
        let mapping_sddl = build_mapping_sddl(&user_sid);

        let file_sd = security_descriptor_from_sddl(&file_sddl)?;
        let file_sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: file_sd.0 .0,
            bInheritHandle: false.into(),
        };

        // Remove stale file if present so DACL is freshly applied on creation.
        let _ = std::fs::remove_file(SHM_FILE_PATH);

        let path_w = to_wide_null(SHM_FILE_PATH);
        let file = unsafe {
            CreateFileW(
                PCWSTR(path_w.as_ptr()),
                GENERIC_READ.0 | GENERIC_WRITE.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                Some(&file_sa),
                OPEN_ALWAYS,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
            .map_err(|e| format!("CreateFileW({SHM_FILE_PATH}) failed: {e:?}"))?
        };
        if file == INVALID_HANDLE_VALUE {
            return Err("CreateFileW returned INVALID_HANDLE_VALUE".to_string());
        }

        let map_sd = security_descriptor_from_sddl(&mapping_sddl)?;
        let map_sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: map_sd.0 .0,
            bInheritHandle: false.into(),
        };

        let mapping = unsafe {
            CreateFileMappingW(
                file,
                Some(&map_sa),
                PAGE_READWRITE,
                0,
                TOTAL_SHM_BYTES as u32,
                PCWSTR::null(),
            )
            .map_err(|e| {
                let _ = CloseHandle(file);
                format!("CreateFileMappingW(file-backed, unnamed) failed: {e:?}")
            })?
        };

        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_WRITE, 0, 0, TOTAL_SHM_BYTES) };
        if view.Value.is_null() {
            let err = unsafe { GetLastError() };
            unsafe {
                let _ = CloseHandle(mapping);
                let _ = CloseHandle(file);
            }
            return Err(format!("MapViewOfFile(WRITE) failed: {err:?}"));
        }

        unsafe {
            let hdr = &mut *(view.Value as *mut SharedFrameHeader);
            let pid = GetCurrentProcessId();
            let mut session_id = 0u32;
            let _ = ProcessIdToSessionId(pid, &mut session_id);

            hdr.magic = SHM_MAGIC;
            hdr.version = SHM_VERSION;
            hdr.writer_pid = pid;
            hdr.writer_session_id = session_id;
            hdr.width = width;
            hdr.height = height;
            hdr.stride = width;
            hdr.fps_num = fps;
            hdr.fps_den = 1;
            hdr.slot_capacity = SLOT_CAPACITY as u32;
            hdr.active_slot = AtomicU32::new(0);
            hdr.consumer_count = AtomicU32::new(0);
            hdr.frame_seq = AtomicU64::new(0);
            hdr.timestamp_100ns = AtomicI64::new(0);
            hdr.writer_qpc_us = AtomicU64::new(0);

            let sid_bytes = user_sid.as_bytes();
            let copy_len = sid_bytes.len().min(hdr.writer_sid.len());
            hdr.writer_sid.fill(0);
            hdr.writer_sid[..copy_len].copy_from_slice(&sid_bytes[..copy_len]);
            hdr.writer_sid_len = copy_len as u32;
        }

        Ok(Self {
            file,
            mapping,
            view,
            event: None,
            file_sddl,
            mapping_sddl,
            user_sid,
        })
    }

    pub fn try_open_global_event(&mut self) -> Option<String> {
        if self.event.is_some() {
            return None;
        }
        let name_w = to_wide_null(GLOBAL_EVENT_NAME);
        unsafe {
            match OpenEventW(
                windows::Win32::System::Threading::SYNCHRONIZATION_ACCESS_RIGHTS(
                    EVENT_MODIFY_STATE.0 | SYNCHRONIZE_ACCESS,
                ),
                false,
                PCWSTR(name_w.as_ptr()),
            ) {
                Ok(h) => {
                    self.event = Some(h);
                    Some(format!(
                        "OpenEventW({GLOBAL_EVENT_NAME}) succeeded from Session 1"
                    ))
                }
                Err(e) => {
                    let code = GetLastError();
                    if code == ERROR_ACCESS_DENIED || e.code() == ERROR_ACCESS_DENIED.to_hresult() {
                        Some(format!("OpenEventW({GLOBAL_EVENT_NAME}) returned ERROR_ACCESS_DENIED (5): {e:?}"))
                    } else {
                        None
                    }
                }
            }
        }
    }

    pub fn write_frame(&mut self, nv12: &[u8], seq: u64, pts_100ns: i64) -> Result<(), String> {
        if nv12.len() > SLOT_CAPACITY {
            return Err(format!(
                "frame size {} exceeds slot capacity {}",
                nv12.len(),
                SLOT_CAPACITY
            ));
        }
        unsafe {
            let hdr = &*(self.view.Value as *const SharedFrameHeader);
            let cur_slot = hdr.active_slot.load(Ordering::Relaxed) as usize;
            let next_slot = (cur_slot + 1) % SLOT_COUNT;
            let base = (self.view.Value as *mut u8).add(std::mem::size_of::<SharedFrameHeader>());
            let dst = base.add(next_slot * SLOT_CAPACITY);
            copy_nonoverlapping(nv12.as_ptr(), dst, nv12.len());
            fence(Ordering::Release);
            hdr.timestamp_100ns.store(pts_100ns, Ordering::Release);
            hdr.writer_qpc_us.store(now_unix_us(), Ordering::Release);
            hdr.active_slot.store(next_slot as u32, Ordering::Release);
            hdr.frame_seq.store(seq, Ordering::Release);
        }
        if let Some(ev) = self.event {
            unsafe {
                let _ = SetEvent(ev);
            }
        }
        Ok(())
    }

    pub fn header(&self) -> &SharedFrameHeader {
        unsafe { &*(self.view.Value as *const SharedFrameHeader) }
    }
}

impl Drop for ShmWriter {
    fn drop(&mut self) {
        unsafe {
            if let Some(ev) = self.event.take() {
                let _ = CloseHandle(ev);
            }
            if !self.view.Value.is_null() {
                let _ = UnmapViewOfFile(self.view);
                self.view.Value = null_mut();
            }
            if !self.mapping.is_invalid() && !self.mapping.0.is_null() {
                let _ = CloseHandle(self.mapping);
            }
            if !self.file.is_invalid() && !self.file.0.is_null() {
                let _ = CloseHandle(self.file);
            }
        }
    }
}

pub struct ShmReader {
    file: HANDLE,
    mapping: HANDLE,
    view: MEMORY_MAPPED_VIEW_ADDRESS,
    event: Option<HANDLE>,
}

unsafe impl Send for ShmReader {}

impl ShmReader {
    pub fn open() -> Result<Self, String> {
        let path_w = to_wide_null(SHM_FILE_PATH);
        let file = unsafe {
            CreateFileW(
                PCWSTR(path_w.as_ptr()),
                GENERIC_READ.0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
            .map_err(|e| format!("Reader CreateFileW({SHM_FILE_PATH}) failed: {e:?}"))?
        };
        if file == INVALID_HANDLE_VALUE {
            return Err("Reader CreateFileW returned INVALID_HANDLE_VALUE".to_string());
        }

        let mapping = unsafe {
            CreateFileMappingW(file, None, PAGE_READONLY, 0, 0, PCWSTR::null()).map_err(|e| {
                let _ = CloseHandle(file);
                format!("Reader CreateFileMappingW(PAGE_READONLY) failed: {e:?}")
            })?
        };

        let view = unsafe { MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, TOTAL_SHM_BYTES) };
        if view.Value.is_null() {
            let err = unsafe { GetLastError() };
            unsafe {
                let _ = CloseHandle(mapping);
                let _ = CloseHandle(file);
            }
            return Err(format!(
                "Reader MapViewOfFile(FILE_MAP_READ) failed: {err:?}"
            ));
        }

        let mut reader = Self {
            file,
            mapping,
            view,
            event: None,
        };
        reader.ensure_global_event();
        Ok(reader)
    }

    fn ensure_global_event(&mut self) {
        if self.event.is_some() {
            return;
        }
        let Ok(user_sid) = self.read_writer_sid() else {
            return;
        };
        let sddl = build_event_sddl(&user_sid);
        let Ok(sd) = security_descriptor_from_sddl(&sddl) else {
            append_dll_log(&format!("Failed to build event SDDL: {sddl}"));
            return;
        };
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0 .0,
            bInheritHandle: false.into(),
        };
        let name_w = to_wide_null(GLOBAL_EVENT_NAME);
        unsafe {
            match CreateEventW(Some(&sa), false, false, PCWSTR(name_w.as_ptr())) {
                Ok(h) => {
                    append_dll_log(&format!("CreateEventW({GLOBAL_EVENT_NAME}) succeeded inside FrameServer with DACL: {sddl}"));
                    self.event = Some(h);
                }
                Err(e) => {
                    let err = GetLastError();
                    append_dll_log(&format!("CreateEventW({GLOBAL_EVENT_NAME}) failed inside FrameServer ({err:?}): {e:?}"));
                }
            }
        }
    }

    pub fn read_writer_sid(&self) -> Result<String, ()> {
        let hdr = unsafe { &*(self.view.Value as *const SharedFrameHeader) };
        if hdr.magic != SHM_MAGIC || hdr.version != SHM_VERSION {
            return Err(());
        }
        let len = (hdr.writer_sid_len as usize).min(hdr.writer_sid.len());
        if len == 0 {
            return Err(());
        }
        let s = std::str::from_utf8(&hdr.writer_sid[..len]).map_err(|_| ())?;
        if validate_sid_ascii(s) {
            Ok(s.to_string())
        } else {
            Err(())
        }
    }

    pub fn wait_frame_event(&self, timeout_ms: u32) {
        if let Some(ev) = self.event {
            unsafe {
                let _ = WaitForSingleObject(ev, timeout_ms);
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(timeout_ms as u64));
        }
    }

    pub fn read_latest(&mut self, out: &mut Vec<u8>) -> Result<FrameSnapshot, String> {
        self.ensure_global_event();
        for _attempt in 0..4 {
            let hdr = unsafe { &*(self.view.Value as *const SharedFrameHeader) };
            let magic = hdr.magic;
            let version = hdr.version;
            if magic != SHM_MAGIC || version != SHM_VERSION {
                return Err(format!("invalid header magic={magic:#x} version={version}"));
            }
            let width = hdr.width;
            let height = hdr.height;
            let stride = hdr.stride;
            let slot_capacity = hdr.slot_capacity as usize;
            if width == 0
                || height == 0
                || (width & 1) != 0
                || (height & 1) != 0
                || width > MAX_WIDTH
                || height > MAX_HEIGHT
                || stride < width
                || slot_capacity > SLOT_CAPACITY
            {
                return Err(format!(
                    "header bounds check failed: {width}x{height} stride={stride} cap={slot_capacity}"
                ));
            }
            let needed = (stride as usize)
                .checked_mul(height as usize)
                .and_then(|y| y.checked_mul(3))
                .map(|t| t / 2)
                .ok_or_else(|| "frame byte size overflow".to_string())?;
            if needed > slot_capacity {
                return Err(format!("needed {needed} > slot_capacity {slot_capacity}"));
            }

            let seq1 = hdr.frame_seq.load(Ordering::Acquire);
            if seq1 == 0 {
                return Err("no frame written yet (seq=0)".to_string());
            }
            let slot = hdr.active_slot.load(Ordering::Acquire) as usize;
            if slot >= SLOT_COUNT {
                return Err(format!("invalid active_slot={slot}"));
            }
            let ts = hdr.timestamp_100ns.load(Ordering::Acquire);
            let qpc_us = hdr.writer_qpc_us.load(Ordering::Acquire);
            let writer_pid = hdr.writer_pid;
            let writer_session_id = hdr.writer_session_id;

            out.resize(needed, 0);
            unsafe {
                let base =
                    (self.view.Value as *const u8).add(std::mem::size_of::<SharedFrameHeader>());
                let src = base.add(slot * SLOT_CAPACITY);
                copy_nonoverlapping(src, out.as_mut_ptr(), needed);
            }
            fence(Ordering::Acquire);
            let seq2 = hdr.frame_seq.load(Ordering::Acquire);
            if seq1 == seq2 {
                return Ok(FrameSnapshot {
                    seq: seq1,
                    width,
                    height,
                    stride,
                    timestamp_100ns: ts,
                    writer_qpc_us: qpc_us,
                    writer_pid,
                    writer_session_id,
                });
            }
        }
        Err("seqlock contention exceeded retries".to_string())
    }
}

impl Drop for ShmReader {
    fn drop(&mut self) {
        unsafe {
            if let Some(ev) = self.event.take() {
                let _ = CloseHandle(ev);
            }
            if !self.view.Value.is_null() {
                let _ = UnmapViewOfFile(self.view);
                self.view.Value = null_mut();
            }
            if !self.mapping.is_invalid() && !self.mapping.0.is_null() {
                let _ = CloseHandle(self.mapping);
            }
            if !self.file.is_invalid() && !self.file.0.is_null() {
                let _ = CloseHandle(self.file);
            }
        }
    }
}

/// 5x7 bitmap digits '0'-'9' for visible frame counter overlay in NV12.
const DIGITS_5X7: [[u8; 7]; 10] = [
    [
        0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
    ], // 0
    [
        0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
    ], // 1
    [
        0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
    ], // 2
    [
        0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
    ], // 3
    [
        0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
    ], // 4
    [
        0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
    ], // 5
    [
        0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
    ], // 6
    [
        0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
    ], // 7
    [
        0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
    ], // 8
    [
        0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
    ], // 9
];

/// Writes a moving SMPTE-style color bar NV12 frame with a 6-digit frame counter.
/// `from_host`: true renders gold/cyan host bar tint; false renders fallback magenta/blue tint.
pub fn render_nv12_test_pattern(
    buf: &mut [u8],
    width: u32,
    height: u32,
    frame_idx: u64,
    from_host: bool,
) {
    let w = width as usize;
    let h = height as usize;
    let y_size = w * h;
    let uv_size = y_size / 2;
    if buf.len() < y_size + uv_size {
        return;
    }

    // Y, U, V (BT.601 studio swing) for 8 color bars
    let bars: [(u8, u8, u8); 8] = if from_host {
        [
            (235, 128, 128), // White
            (210, 16, 146),  // Yellow / Gold
            (170, 166, 16),  // Cyan
            (145, 54, 34),   // Green
            (106, 202, 222), // Magenta
            (81, 90, 240),   // Red
            (41, 240, 110),  // Blue
            (24, 128, 128),  // Dark slate
        ]
    } else {
        [
            (180, 128, 128), // Grey
            (140, 200, 200), // Lavender
            (120, 220, 100), // Sky
            (100, 80, 180),  // Rose
            (80, 160, 60),   // Teal
            (60, 100, 200),  // Coral
            (45, 220, 120),  // Cobalt
            (20, 128, 128),  // Slate
        ]
    };

    let shift = ((frame_idx * 8) as usize) % w;
    let bar_w = w / 8;

    let (y_plane, uv_plane) = buf.split_at_mut(y_size);
    for row in 0..h {
        let y_row = &mut y_plane[row * w..(row + 1) * w];
        // Moving horizontal sweep line near bottom
        let is_sweep =
            (row >= h - 40 && row < h - 24) && (((frame_idx * 14) as usize) % w).abs_diff(0) < w;
        for (col, px) in y_row.iter_mut().enumerate().take(w) {
            let sx = (col + shift) % w;
            let bar_idx = (sx / bar_w).min(7);
            *px = bars[bar_idx].0;
        }
        if is_sweep {
            let marker = ((frame_idx * 16) as usize) % w;
            let end = (marker + 80).min(w);
            y_row[marker..end].fill(235);
        }
    }

    for uv_row in 0..(h / 2) {
        let row_slice = &mut uv_plane[uv_row * w..(uv_row + 1) * w];
        for pair in 0..(w / 2) {
            let col = pair * 2;
            let sx = (col + shift) % w;
            let bar_idx = (sx / bar_w).min(7);
            row_slice[col] = bars[bar_idx].1;
            row_slice[col + 1] = bars[bar_idx].2;
        }
    }

    // Draw dark badge + bright 6-digit counter in top-left (even coordinates)
    let scale = 6usize;
    let badge_x = 40usize;
    let badge_y = 40usize;
    let badge_w = 6 * (5 * scale + scale) + scale * 2;
    let badge_h = 7 * scale + scale * 4;
    for row in badge_y..(badge_y + badge_h).min(h) {
        for col in badge_x..(badge_x + badge_w).min(w) {
            y_plane[row * w + col] = 16;
            if (row & 1) == 0 && (col & 1) == 0 {
                let uv_idx = (row / 2) * w + col;
                uv_plane[uv_idx] = 128;
                uv_plane[uv_idx + 1] = 128;
            }
        }
    }

    let mut val = frame_idx % 1_000_000;
    let mut digits = [0usize; 6];
    for i in (0..6).rev() {
        digits[i] = (val % 10) as usize;
        val /= 10;
    }

    for (di, &digit) in digits.iter().enumerate() {
        let gx = badge_x + scale + di * (5 * scale + scale);
        let gy = badge_y + scale * 2;
        let glyph = DIGITS_5X7[digit];
        for (gr, &bits) in glyph.iter().enumerate() {
            for gc in 0..5usize {
                if (bits & (1 << (4 - gc))) != 0 {
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let py = gy + gr * scale + dy;
                            let px = gx + gc * scale + dx;
                            if py < h && px < w {
                                y_plane[py * w + px] = 235;
                            }
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sid_validation_checks_format() {
        assert!(validate_sid_ascii(
            "S-1-5-21-1122164896-1719761939-243207128-1001"
        ));
        assert!(!validate_sid_ascii("bad-sid"));
        assert!(!validate_sid_ascii("S-1-5-19;bad"));
    }

    #[test]
    fn nv12_pattern_has_expected_size_and_motion() {
        let mut a = vec![0u8; NV12_720P_BYTES];
        let mut b = vec![0u8; NV12_720P_BYTES];
        render_nv12_test_pattern(&mut a, FRAME_WIDTH, FRAME_HEIGHT, 1, true);
        render_nv12_test_pattern(&mut b, FRAME_WIDTH, FRAME_HEIGHT, 2, true);
        assert_ne!(a, b);
    }

    #[test]
    fn shm_writer_reader_roundtrip_and_bounds_checks() {
        std::fs::create_dir_all(SPIKE_DIR).unwrap();
        let mut writer = ShmWriter::create(FRAME_WIDTH, FRAME_HEIGHT, FRAME_FPS).unwrap();
        let mut reader = ShmReader::open().unwrap();

        let mut frame = vec![0u8; NV12_720P_BYTES];
        render_nv12_test_pattern(&mut frame, FRAME_WIDTH, FRAME_HEIGHT, 42, true);
        writer.write_frame(&frame, 42, 12345).unwrap();

        let mut out = Vec::new();
        let snap = reader.read_latest(&mut out).unwrap();
        assert_eq!(snap.seq, 42);
        assert_eq!(snap.width, FRAME_WIDTH);
        assert_eq!(snap.height, FRAME_HEIGHT);
        assert_eq!(out.len(), NV12_720P_BYTES);
        assert_eq!(out, frame);

        // Bounds check: invalid active_slot >= SLOT_COUNT must be rejected
        writer.header().active_slot.store(9, Ordering::Release);
        let err = reader.read_latest(&mut out).unwrap_err();
        assert!(err.contains("invalid active_slot"), "unexpected err: {err}");
    }
}
