//! Non-elevated host executable for SPEC-32 Task 0.2 spike.
//! Creates cross-session shared memory + DACLs, activates `MFCreateVirtualCamera`,
//! probes `MFEnumDeviceSources` + `IMFSourceReader`, and waits for Enter before removal.

#[path = "../ipc.rs"]
mod ipc;

use std::io::{self, BufRead};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::{Interface, PCWSTR, PWSTR};
use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::Media::KernelStreaming::KSCATEGORY_VIDEO_CAMERA;
use windows::Win32::Media::MediaFoundation::{
    IMFActivate, IMFMediaSource, MFCreateAttributes, MFCreateSourceReaderFromMediaSource,
    MFCreateVirtualCamera, MFEnumDeviceSources, MFShutdown, MFStartup,
    MFVirtualCameraAccess_CurrentUser, MFVirtualCameraLifetime_Session,
    MFVirtualCameraType_SoftwareCameraSource, MFSTARTUP_FULL, MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE, MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
    MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK, MF_SOURCE_READER_FIRST_VIDEO_STREAM,
    MF_VERSION,
};
use windows::Win32::System::Com::{
    CoInitializeEx, CoTaskMemFree, CoUninitialize, IClassFactory, COINIT_MULTITHREADED,
};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};

use ipc::{
    init_dll_log_file, now_unix_us, probe_global_event_creation, render_nv12_test_pattern,
    to_wide_null, ShmWriter, DLL_LOG_PATH, FRAME_FPS, FRAME_HEIGHT, FRAME_WIDTH, NV12_720P_BYTES,
    SPIKE_CLSID, SPIKE_CLSID_STR, SPIKE_DIR,
};

const FRIENDLY_NAME: &str = "AnyLeap Camera (spike)";

fn enumerate_video_cameras() -> windows::core::Result<Vec<(String, String, IMFActivate)>> {
    unsafe {
        let mut attrs = None;
        MFCreateAttributes(&mut attrs, 1)?;
        let attrs = attrs.unwrap();
        attrs.SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )?;

        let mut raw_ptr: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count = 0u32;
        MFEnumDeviceSources(&attrs, &mut raw_ptr, &mut count)?;

        let mut out = Vec::new();
        if !raw_ptr.is_null() && count > 0 {
            let slice = std::slice::from_raw_parts(raw_ptr, count as usize);
            for act in slice.iter().flatten() {
                let name = get_attr_string(act, &MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME)
                    .unwrap_or_else(|| "<unknown>".to_string());
                let link = get_attr_string(
                    act,
                    &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_SYMBOLIC_LINK,
                )
                .unwrap_or_else(|| "<none>".to_string());
                out.push((name, link, act.clone()));
            }
            CoTaskMemFree(Some(raw_ptr as *const _));
        }
        Ok(out)
    }
}

unsafe fn get_attr_string(act: &IMFActivate, key: &windows::core::GUID) -> Option<String> {
    let mut pwstr = PWSTR::null();
    let mut len = 0u32;
    if act.GetAllocatedString(key, &mut pwstr, &mut len).is_ok() && !pwstr.0.is_null() {
        let s = String::from_utf16_lossy(std::slice::from_raw_parts(pwstr.0, len as usize));
        CoTaskMemFree(Some(pwstr.0 as *const _));
        Some(s)
    } else {
        None
    }
}

fn probe_source_frames(source: &IMFMediaSource, frames_to_read: usize) {
    unsafe {
        let reader = match MFCreateSourceReaderFromMediaSource(source, None) {
            Ok(r) => r,
            Err(e) => {
                println!("[probe] MFCreateSourceReaderFromMediaSource failed: {e:?}");
                return;
            }
        };

        let mut ok_frames = 0usize;
        let mut last_ts = None;
        for i in 0..frames_to_read {
            let mut stream_idx = 0u32;
            let mut flags = 0u32;
            let mut ts = 0i64;
            let mut sample = None;
            let r_t0 = Instant::now();
            match reader.ReadSample(
                MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32,
                0,
                Some(&mut stream_idx),
                Some(&mut flags),
                Some(&mut ts),
                Some(&mut sample),
            ) {
                Ok(()) => {
                    let read_ms = r_t0.elapsed().as_secs_f64() * 1000.0;
                    if let Some(s) = sample {
                        let len = s.GetTotalLength().unwrap_or(0);
                        let delta_ms = last_ts
                            .map(|prev: i64| (ts - prev) as f64 / 10_000.0)
                            .unwrap_or(0.0);
                        last_ts = Some(ts);
                        ok_frames += 1;
                        println!(
                            "[probe] frame #{i}: bytes={len}, pts_100ns={ts}, delta_pts={delta_ms:.2} ms, ReadSample={read_ms:.2} ms, flags={flags:#x}"
                        );
                    } else {
                        println!("[probe] ReadSample #{i} returned None sample (flags={flags:#x})");
                    }
                }
                Err(e) => {
                    println!("[probe] ReadSample #{i} error: {e:?}");
                    break;
                }
            }
        }
        println!("[probe] Completed: {ok_frames}/{frames_to_read} frames flowed.");
        drop(reader);
        let _ = source.Shutdown();
    }
}

fn probe_stream_read(activate: &IMFActivate, frames_to_read: usize) {
    unsafe {
        let t0 = Instant::now();
        let source: IMFMediaSource = match activate.ActivateObject() {
            Ok(s) => s,
            Err(e) => {
                println!("[probe] ActivateObject::<IMFMediaSource> failed: {e:?}");
                return;
            }
        };
        println!(
            "[probe] ActivateObject succeeded in {:.2} ms",
            t0.elapsed().as_secs_f64() * 1000.0
        );
        probe_source_frames(&source, frames_to_read);
    }
}

fn run_local_com_probe(mut shm: ShmWriter) {
    unsafe {
        let dll_w = to_wide_null(r"C:\ProgramData\AnyLeapSpike\anyleap_vcam_spike.dll");
        let hmod = LoadLibraryW(PCWSTR(dll_w.as_ptr())).expect("LoadLibraryW spike DLL failed");
        let proc = GetProcAddress(hmod, windows::core::s!("DllGetClassObject"))
            .expect("DllGetClassObject export missing");
        type DllGetClassObjectFn = unsafe extern "system" fn(
            *const windows::core::GUID,
            *const windows::core::GUID,
            *mut *mut core::ffi::c_void,
        ) -> windows::core::HRESULT;
        let get_factory: DllGetClassObjectFn = std::mem::transmute(proc);

        let mut factory_raw = std::ptr::null_mut();
        get_factory(&SPIKE_CLSID, &IClassFactory::IID, &mut factory_raw)
            .ok()
            .expect("DllGetClassObject failed");
        let factory: IClassFactory = Interface::from_raw(factory_raw);
        let source: IMFMediaSource = factory
            .CreateInstance(None)
            .expect("IClassFactory::CreateInstance(IMFMediaSource) failed");

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_flag);
        let writer_handle = std::thread::spawn(move || {
            let mut buf = vec![0u8; NV12_720P_BYTES];
            let mut seq = 2u64;
            let start = Instant::now();
            while !stop_clone.load(Ordering::Acquire) {
                let _ = shm.try_open_global_event();
                render_nv12_test_pattern(&mut buf, FRAME_WIDTH, FRAME_HEIGHT, seq, true);
                let pts = (start.elapsed().as_nanos() / 100) as i64;
                let _ = shm.write_frame(&buf, seq, pts);
                seq += 1;
                std::thread::sleep(Duration::from_millis(33));
            }
        });

        probe_source_frames(&source, 6);
        stop_flag.store(true, Ordering::Release);
        let _ = writer_handle.join();
        drop(source);
        drop(factory);
        let _ = FreeLibrary(hmod);

        if let Ok(log_txt) = std::fs::read_to_string(DLL_LOG_PATH) {
            println!("--- DLL log ---\n{log_txt}--- end DLL log ---");
        }
    }
}

fn main() {
    if let Err(e) = std::fs::create_dir_all(SPIKE_DIR) {
        eprintln!("Failed to create {SPIKE_DIR}: {e}");
        std::process::exit(1);
    }

    if let Err(e) = init_dll_log_file() {
        eprintln!("Warning: init_dll_log_file failed: {e}");
    }

    let mut shm = match ShmWriter::create(FRAME_WIDTH, FRAME_HEIGHT, FRAME_FPS) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("ShmWriter::create failed: {e}");
            std::process::exit(1);
        }
    };

    println!("=== SPEC-32 Task 0.2 Virtual Camera Spike Host ===");
    println!("User SID        : {}", shm.user_sid);
    println!("File SDDL       : {}", shm.file_sddl);
    println!("Mapping SDDL    : {}", shm.mapping_sddl);
    println!(
        "Global probe    : {}",
        probe_global_event_creation(&shm.user_sid)
    );

    // Seed initial frame #1 before virtual camera activation
    let mut frame_buf = vec![0u8; NV12_720P_BYTES];
    render_nv12_test_pattern(&mut frame_buf, FRAME_WIDTH, FRAME_HEIGHT, 1, true);
    let _ = shm.write_frame(&frame_buf, 1, 0);

    unsafe {
        let hr = CoInitializeEx(None, COINIT_MULTITHREADED);
        if hr.is_err() {
            eprintln!("CoInitializeEx failed: {hr:?}");
            std::process::exit(1);
        }
        if let Err(e) = MFStartup(MF_VERSION, MFSTARTUP_FULL) {
            eprintln!("MFStartup failed: {e:?}");
            CoUninitialize();
            std::process::exit(1);
        }

        let name_w = to_wide_null(FRIENDLY_NAME);
        let clsid_w = to_wide_null(SPIKE_CLSID_STR);

        if std::env::args().any(|a| a == "--local-probe") {
            println!("Running in-process COM + SHM probe (--local-probe)...");
            run_local_com_probe(shm);
            let _ = MFShutdown();
            CoUninitialize();
            return;
        }

        let vcam = match MFCreateVirtualCamera(
            MFVirtualCameraType_SoftwareCameraSource,
            MFVirtualCameraLifetime_Session,
            MFVirtualCameraAccess_CurrentUser,
            PCWSTR(name_w.as_ptr()),
            PCWSTR(clsid_w.as_ptr()),
            Some(&[KSCATEGORY_VIDEO_CAMERA]),
        ) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("MFCreateVirtualCamera failed: {e:?}");
                let _ = MFShutdown();
                CoUninitialize();
                std::process::exit(1);
            }
        };

        if let Err(e) = vcam.Start(None) {
            eprintln!("IMFVirtualCamera::Start failed: {e:?}");
            let _ = vcam.Remove();
            let _ = vcam.Shutdown();
            let _ = MFShutdown();
            CoUninitialize();
            std::process::exit(1);
        }
        println!("IMFVirtualCamera::Start succeeded for \"{FRIENDLY_NAME}\" ({SPIKE_CLSID_STR}).");

        // Start writer thread producing 30 fps NV12 frames into shared memory
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop_flag);
        let writer_handle = std::thread::spawn(move || {
            let mut buf = vec![0u8; NV12_720P_BYTES];
            let mut seq = 2u64;
            let start = Instant::now();
            let mut event_logged = false;
            while !stop_clone.load(Ordering::Acquire) {
                if !event_logged {
                    if let Some(msg) = shm.try_open_global_event() {
                        println!("[writer] {msg}");
                        event_logged = true;
                    }
                }
                render_nv12_test_pattern(&mut buf, FRAME_WIDTH, FRAME_HEIGHT, seq, true);
                let pts = (start.elapsed().as_nanos() / 100) as i64;
                if let Err(e) = shm.write_frame(&buf, seq, pts) {
                    eprintln!("[writer] write_frame error: {e}");
                }
                seq += 1;
                std::thread::sleep(Duration::from_millis(33));
            }
            println!(
                "[writer] Stopped after seq={} (t_us={})",
                seq - 1,
                now_unix_us()
            );
        });

        // Enumerate devices & probe frame flow
        std::thread::sleep(Duration::from_millis(250));
        match enumerate_video_cameras() {
            Ok(cams) => {
                println!(
                    "MFEnumDeviceSources found {} video capture device(s):",
                    cams.len()
                );
                let mut found_spike = None;
                for (i, (name, link, act)) in cams.iter().enumerate() {
                    println!("  [{i}] \"{name}\" | {link}");
                    if name == FRIENDLY_NAME {
                        found_spike = Some(act.clone());
                    }
                }
                if let Some(act) = found_spike {
                    probe_stream_read(&act, 6);
                } else {
                    println!("[probe] \"{FRIENDLY_NAME}\" was NOT found in MFEnumDeviceSources!");
                }
            }
            Err(e) => {
                println!("MFEnumDeviceSources failed: {e:?}");
            }
        }

        println!();
        println!(
            "Virtual camera is LIVE. Open the Windows Camera app and select \"{FRIENDLY_NAME}\"."
        );
        println!("Press ENTER to stop and remove the virtual camera...");
        let mut line = String::new();
        let _ = io::stdin().lock().read_line(&mut line);

        stop_flag.store(true, Ordering::Release);
        let _ = writer_handle.join();

        let _ = vcam.Stop();
        let _ = vcam.Remove();
        let _ = vcam.Shutdown();
        let _ = MFShutdown();
        CoUninitialize();
        println!("Virtual camera stopped and removed.");
    }
}
