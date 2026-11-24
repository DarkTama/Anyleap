//! Minimal COM `IMFMediaSourceEx` / `IMFMediaStream2` virtual camera spike DLL.

mod ipc;

use std::collections::VecDeque;
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use windows::core::{implement, ComObjectInterface, IUnknown, Interface, Result, GUID, HRESULT};
use windows::Win32::Foundation::{
    CLASS_E_CLASSNOTAVAILABLE, E_NOINTERFACE, E_POINTER, S_FALSE, S_OK,
};
use windows::Win32::Media::KernelStreaming::{
    IKsControl, IKsControl_Impl, KSIDENTIFIER, PINNAME_VIDEO_CAPTURE,
};
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFGetService, IMFGetService_Impl, IMFMediaEvent, IMFMediaEventGenerator,
    IMFMediaEventGenerator_Impl, IMFMediaEventQueue, IMFMediaSource, IMFMediaSourceEx,
    IMFMediaSourceEx_Impl, IMFMediaSource_Impl, IMFMediaStream, IMFMediaStream2,
    IMFMediaStream2_Impl, IMFMediaStream_Impl, IMFPresentationDescriptor, IMFStreamDescriptor,
    MEMediaSample, MENewStream, MESourceStarted, MESourceStopped, MEStreamStarted, MEStreamStopped,
    MFCreateAttributes, MFCreateEventQueue, MFCreateMediaType, MFCreateMemoryBuffer,
    MFCreatePresentationDescriptor, MFCreateSample, MFCreateStreamDescriptor, MFGetSystemTime,
    MFMediaType_Video, MFSampleExtension_Token, MFVideoFormat_NV12, MFVideoInterlace_Progressive,
    MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS, MFMEDIASOURCE_IS_LIVE,
    MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES, MF_DEVICESTREAM_FRAMESERVER_SHARED,
    MF_DEVICESTREAM_STREAM_CATEGORY, MF_DEVICESTREAM_STREAM_ID, MF_E_INVALIDREQUEST,
    MF_E_INVALIDSTREAMNUMBER, MF_E_SHUTDOWN, MF_E_UNSUPPORTED_SERVICE,
    MF_E_UNSUPPORTED_STATE_TRANSITION, MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_DEFAULT_STRIDE,
    MF_MT_FIXED_SIZE_SAMPLES, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE, MF_MT_INTERLACE_MODE,
    MF_MT_MAJOR_TYPE, MF_MT_PIXEL_ASPECT_RATIO, MF_MT_SAMPLE_SIZE, MF_MT_SUBTYPE, MF_STREAM_STATE,
    MF_STREAM_STATE_RUNNING, MF_STREAM_STATE_STOPPED,
};
use windows::Win32::System::Com::StructuredStorage::PROPVARIANT;
use windows::Win32::System::Com::{IClassFactory, IClassFactory_Impl};

use ipc::{
    append_dll_log, now_unix_us, render_nv12_test_pattern, ShmReader, FRAME_FPS, FRAME_HEIGHT,
    FRAME_WIDTH, NV12_720P_BYTES, SPIKE_CLSID,
};

static OBJECT_COUNT: AtomicU32 = AtomicU32::new(0);
static LOCK_COUNT: AtomicU32 = AtomicU32::new(0);

const ERROR_SET_NOT_FOUND_WIN32: u32 = 1170;

fn pack_u64(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | (lo as u64)
}

fn create_nv12_media_type() -> Result<windows::Win32::Media::MediaFoundation::IMFMediaType> {
    unsafe {
        let mt = MFCreateMediaType()?;
        mt.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        mt.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_NV12)?;
        mt.SetUINT64(&MF_MT_FRAME_SIZE, pack_u64(FRAME_WIDTH, FRAME_HEIGHT))?;
        mt.SetUINT64(&MF_MT_FRAME_RATE, pack_u64(FRAME_FPS, 1))?;
        mt.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pack_u64(1, 1))?;
        mt.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?;
        mt.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
        mt.SetUINT32(&MF_MT_FIXED_SIZE_SAMPLES, 1)?;
        mt.SetUINT32(&MF_MT_DEFAULT_STRIDE, FRAME_WIDTH)?;
        mt.SetUINT32(&MF_MT_SAMPLE_SIZE, NV12_720P_BYTES as u32)?;
        Ok(mt)
    }
}

struct SendCom<T>(T);
unsafe impl<T> Send for SendCom<T> {}
unsafe impl<T> Sync for SendCom<T> {}

struct StreamShared {
    queue: Mutex<VecDeque<Option<SendCom<IUnknown>>>>,
    cv: Condvar,
    running: AtomicBool,
    shutdown: AtomicBool,
    state: Mutex<MF_STREAM_STATE>,
}

#[implement(IMFMediaStream2, IMFMediaStream, IMFMediaEventGenerator)]
struct SpikeMediaStream {
    parent: Mutex<Option<IMFMediaSource>>,
    descriptor: IMFStreamDescriptor,
    events: IMFMediaEventQueue,
    shared: Arc<StreamShared>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl SpikeMediaStream {
    fn new(descriptor: IMFStreamDescriptor) -> Result<Self> {
        let events = unsafe { MFCreateEventQueue()? };
        OBJECT_COUNT.fetch_add(1, Ordering::SeqCst);
        Ok(Self {
            parent: Mutex::new(None),
            descriptor,
            events,
            shared: Arc::new(StreamShared {
                queue: Mutex::new(VecDeque::new()),
                cv: Condvar::new(),
                running: AtomicBool::new(false),
                shutdown: AtomicBool::new(false),
                state: Mutex::new(MF_STREAM_STATE_STOPPED),
            }),
            worker: Mutex::new(None),
        })
    }

    fn set_parent(&self, source: IMFMediaSource) {
        *self.parent.lock().unwrap() = Some(source);
    }

    fn ensure_worker(&self) {
        let mut guard = self.worker.lock().unwrap();
        if guard.is_some() {
            return;
        }
        let shared = Arc::clone(&self.shared);
        let events = SendCom(self.events.clone());
        let handle = std::thread::spawn(move || {
            let events = events;
            append_dll_log("Stream worker thread started");
            let mut shm_reader: Option<ShmReader> = None;
            let mut shm_open_logged = false;
            let mut last_seq = 0u64;
            let mut fallback_idx = 0u64;
            let mut staging = vec![0u8; NV12_720P_BYTES];
            let mut ipc_frames_logged = 0u64;

            loop {
                let token_opt = {
                    let mut q = shared.queue.lock().unwrap();
                    while q.is_empty() && !shared.shutdown.load(Ordering::Acquire) {
                        q = shared.cv.wait(q).unwrap();
                    }
                    if shared.shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    q.pop_front()
                };

                let Some(token) = token_opt else {
                    continue;
                };

                if !shared.running.load(Ordering::Acquire) {
                    continue;
                }

                // Wait for cross-session event or ~33ms frame interval so we deliver ~30 fps.
                if shm_reader.is_none() {
                    match ShmReader::open() {
                        Ok(r) => {
                            append_dll_log("ShmReader::open() succeeded inside FrameServer");
                            shm_reader = Some(r);
                        }
                        Err(e) => {
                            if !shm_open_logged {
                                append_dll_log(&format!("ShmReader::open() pending/failed: {e}"));
                                shm_open_logged = true;
                            }
                        }
                    }
                }

                if let Some(ref r) = shm_reader {
                    r.wait_frame_event(33);
                } else {
                    std::thread::sleep(std::time::Duration::from_millis(33));
                }

                if shared.shutdown.load(Ordering::Acquire)
                    || !shared.running.load(Ordering::Acquire)
                {
                    continue;
                }

                let mut used_ipc = false;
                let mut frame_w = FRAME_WIDTH;
                let mut frame_h = FRAME_HEIGHT;

                if let Some(ref mut r) = shm_reader {
                    match r.read_latest(&mut staging) {
                        Ok(snap) => {
                            used_ipc = true;
                            frame_w = snap.width;
                            frame_h = snap.height;
                            if snap.seq != last_seq {
                                last_seq = snap.seq;
                                ipc_frames_logged += 1;
                                if ipc_frames_logged <= 5 || ipc_frames_logged.is_multiple_of(150) {
                                    let now_us = now_unix_us();
                                    let lat_us = now_us.saturating_sub(snap.writer_qpc_us);
                                    append_dll_log(&format!(
                                        "IPC frame seq={} {}x{} from writer_pid={} sess={} approx_ipc_latency_us={}",
                                        snap.seq, snap.width, snap.height, snap.writer_pid, snap.writer_session_id, lat_us
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            if ipc_frames_logged == 0 {
                                append_dll_log(&format!("ShmReader::read_latest fallback ({e})"));
                            }
                        }
                    }
                }

                if !used_ipc {
                    fallback_idx = fallback_idx.wrapping_add(1);
                    staging.resize(NV12_720P_BYTES, 0);
                    render_nv12_test_pattern(
                        &mut staging,
                        FRAME_WIDTH,
                        FRAME_HEIGHT,
                        fallback_idx,
                        false,
                    );
                    frame_w = FRAME_WIDTH;
                    frame_h = FRAME_HEIGHT;
                }

                let byte_len = (frame_w as usize * frame_h as usize * 3 / 2).min(staging.len());
                unsafe {
                    if let (Ok(sample), Ok(buf)) =
                        (MFCreateSample(), MFCreateMemoryBuffer(byte_len as u32))
                    {
                        let mut ptr = std::ptr::null_mut();
                        if buf.Lock(&mut ptr, None, None).is_ok() {
                            std::ptr::copy_nonoverlapping(staging.as_ptr(), ptr, byte_len);
                            let _ = buf.Unlock();
                            let _ = buf.SetCurrentLength(byte_len as u32);
                            let _ = sample.AddBuffer(&buf);
                            let pts = MFGetSystemTime();
                            let _ = sample.SetSampleTime(pts);
                            let _ = sample.SetSampleDuration(333_333);
                            if let Some(tok) = token {
                                let _ = sample.SetUnknown(&MFSampleExtension_Token, &tok.0);
                            }
                            let sample_unk: IUnknown = sample.cast().unwrap();
                            let _ = events.0.QueueEventParamUnk(
                                MEMediaSample.0 as u32,
                                &GUID::zeroed(),
                                S_OK,
                                &sample_unk,
                            );
                        }
                    }
                }
            }
            append_dll_log("Stream worker thread exiting");
        });
        *guard = Some(handle);
    }

    fn start_stream(&self, selected_before: bool) -> Result<()> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        *self.shared.state.lock().unwrap() = MF_STREAM_STATE_RUNNING;
        self.shared.running.store(true, Ordering::Release);
        self.ensure_worker();
        let _ = selected_before;
        unsafe {
            self.events.QueueEventParamVar(
                MEStreamStarted.0 as u32,
                &GUID::zeroed(),
                S_OK,
                &PROPVARIANT::default(),
            )?;
        }
        Ok(())
    }

    fn stop_stream(&self) -> Result<()> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        self.shared.running.store(false, Ordering::Release);
        *self.shared.state.lock().unwrap() = MF_STREAM_STATE_STOPPED;
        self.shared.queue.lock().unwrap().clear();
        self.shared.cv.notify_all();
        unsafe {
            self.events.QueueEventParamVar(
                MEStreamStopped.0 as u32,
                &GUID::zeroed(),
                S_OK,
                &PROPVARIANT::default(),
            )?;
        }
        Ok(())
    }

    fn shutdown_stream(&self) {
        self.shared.running.store(false, Ordering::Release);
        self.shared.shutdown.store(true, Ordering::Release);
        self.shared.queue.lock().unwrap().clear();
        self.shared.cv.notify_all();
        if let Some(handle) = self.worker.lock().unwrap().take() {
            let _ = handle.join();
        }
        *self.parent.lock().unwrap() = None;
        unsafe {
            let _ = self.events.Shutdown();
        }
    }
}

impl Drop for SpikeMediaStream {
    fn drop(&mut self) {
        OBJECT_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

impl IMFMediaEventGenerator_Impl for SpikeMediaStream_Impl {
    fn GetEvent(&self, dwflags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS) -> Result<IMFMediaEvent> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        unsafe { self.events.GetEvent(dwflags.0) }
    }

    fn BeginGetEvent(
        &self,
        pcallback: windows::core::Ref<'_, windows::Win32::Media::MediaFoundation::IMFAsyncCallback>,
        punkstate: windows::core::Ref<'_, IUnknown>,
    ) -> Result<()> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        unsafe {
            self.events
                .BeginGetEvent(pcallback.as_ref(), punkstate.as_ref())
        }
    }

    fn EndGetEvent(
        &self,
        presult: windows::core::Ref<'_, windows::Win32::Media::MediaFoundation::IMFAsyncResult>,
    ) -> Result<IMFMediaEvent> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        unsafe { self.events.EndGetEvent(presult.as_ref()) }
    }

    fn QueueEvent(
        &self,
        met: u32,
        guidextendedtype: *const GUID,
        hrstatus: HRESULT,
        pvvalue: *const PROPVARIANT,
    ) -> Result<()> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        unsafe {
            self.events
                .QueueEventParamVar(met, guidextendedtype, hrstatus, pvvalue)
        }
    }
}

impl IMFMediaStream_Impl for SpikeMediaStream_Impl {
    fn GetMediaSource(&self) -> Result<IMFMediaSource> {
        self.parent
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| MF_E_SHUTDOWN.into())
    }

    fn GetStreamDescriptor(&self) -> Result<IMFStreamDescriptor> {
        Ok(self.descriptor.clone())
    }

    fn RequestSample(&self, ptoken: windows::core::Ref<'_, IUnknown>) -> Result<()> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        if !self.shared.running.load(Ordering::Acquire) {
            return Err(MF_E_INVALIDREQUEST.into());
        }
        let mut q = self.shared.queue.lock().unwrap();
        q.push_back(ptoken.as_ref().cloned().map(SendCom));
        self.shared.cv.notify_one();
        Ok(())
    }
}

impl IMFMediaStream2_Impl for SpikeMediaStream_Impl {
    fn SetStreamState(&self, value: MF_STREAM_STATE) -> Result<()> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        *self.shared.state.lock().unwrap() = value;
        self.shared
            .running
            .store(value == MF_STREAM_STATE_RUNNING, Ordering::Release);
        self.shared.cv.notify_all();
        Ok(())
    }

    fn GetStreamState(&self) -> Result<MF_STREAM_STATE> {
        if self.shared.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        Ok(*self.shared.state.lock().unwrap())
    }
}

#[implement(
    IMFMediaSourceEx,
    IMFMediaSource,
    IMFMediaEventGenerator,
    IMFGetService,
    IKsControl
)]
struct SpikeMediaSource {
    events: IMFMediaEventQueue,
    presentation_desc: IMFPresentationDescriptor,
    stream_attrs: IMFAttributes,
    source_attrs: IMFAttributes,
    stream: windows::core::ComObject<SpikeMediaStream>,
    started_once: AtomicBool,
    shutdown: AtomicBool,
}

impl SpikeMediaSource {
    fn create() -> Result<windows::core::ComObject<Self>> {
        unsafe {
            let events = MFCreateEventQueue()?;
            let mt = create_nv12_media_type()?;
            let sd = MFCreateStreamDescriptor(0, &[Some(mt.clone())])?;
            let handler = sd.GetMediaTypeHandler()?;
            handler.SetCurrentMediaType(&mt)?;

            sd.SetGUID(&MF_DEVICESTREAM_STREAM_CATEGORY, &PINNAME_VIDEO_CAPTURE)?;
            sd.SetUINT32(&MF_DEVICESTREAM_STREAM_ID, 0)?;
            sd.SetUINT32(&MF_DEVICESTREAM_FRAMESERVER_SHARED, 1)?;
            sd.SetUINT32(&MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES, 1)?;

            let mut stream_attrs: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut stream_attrs, 4)?;
            let stream_attrs = stream_attrs.unwrap();
            stream_attrs.SetGUID(&MF_DEVICESTREAM_STREAM_CATEGORY, &PINNAME_VIDEO_CAPTURE)?;
            stream_attrs.SetUINT32(&MF_DEVICESTREAM_STREAM_ID, 0)?;
            stream_attrs.SetUINT32(&MF_DEVICESTREAM_FRAMESERVER_SHARED, 1)?;
            stream_attrs.SetUINT32(&MF_DEVICESTREAM_ATTRIBUTE_FRAMESOURCE_TYPES, 1)?;

            let mut source_attrs: Option<IMFAttributes> = None;
            MFCreateAttributes(&mut source_attrs, 2)?;
            let source_attrs = source_attrs.unwrap();

            let pd = MFCreatePresentationDescriptor(Some(&[Some(sd.clone())]))?;
            pd.SelectStream(0)?;

            let stream_obj = windows::core::ComObject::new(SpikeMediaStream::new(sd)?);
            OBJECT_COUNT.fetch_add(1, Ordering::SeqCst);

            let src = windows::core::ComObject::new(Self {
                events,
                presentation_desc: pd,
                stream_attrs,
                source_attrs,
                stream: stream_obj,
                started_once: AtomicBool::new(false),
                shutdown: AtomicBool::new(false),
            });

            let as_media_source: IMFMediaSource = src.to_interface();
            src.stream.set_parent(as_media_source);
            append_dll_log("SpikeMediaSource created");
            Ok(src)
        }
    }
}

impl Drop for SpikeMediaSource {
    fn drop(&mut self) {
        OBJECT_COUNT.fetch_sub(1, Ordering::SeqCst);
        append_dll_log("SpikeMediaSource dropped");
    }
}

impl IMFMediaEventGenerator_Impl for SpikeMediaSource_Impl {
    fn GetEvent(&self, dwflags: MEDIA_EVENT_GENERATOR_GET_EVENT_FLAGS) -> Result<IMFMediaEvent> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        unsafe { self.events.GetEvent(dwflags.0) }
    }

    fn BeginGetEvent(
        &self,
        pcallback: windows::core::Ref<'_, windows::Win32::Media::MediaFoundation::IMFAsyncCallback>,
        punkstate: windows::core::Ref<'_, IUnknown>,
    ) -> Result<()> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        unsafe {
            self.events
                .BeginGetEvent(pcallback.as_ref(), punkstate.as_ref())
        }
    }

    fn EndGetEvent(
        &self,
        presult: windows::core::Ref<'_, windows::Win32::Media::MediaFoundation::IMFAsyncResult>,
    ) -> Result<IMFMediaEvent> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        unsafe { self.events.EndGetEvent(presult.as_ref()) }
    }

    fn QueueEvent(
        &self,
        met: u32,
        guidextendedtype: *const GUID,
        hrstatus: HRESULT,
        pvvalue: *const PROPVARIANT,
    ) -> Result<()> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        unsafe {
            self.events
                .QueueEventParamVar(met, guidextendedtype, hrstatus, pvvalue)
        }
    }
}

impl IMFMediaSource_Impl for SpikeMediaSource_Impl {
    fn GetCharacteristics(&self) -> Result<u32> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        Ok(MFMEDIASOURCE_IS_LIVE.0 as u32)
    }

    fn CreatePresentationDescriptor(&self) -> Result<IMFPresentationDescriptor> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        unsafe { self.presentation_desc.Clone() }
    }

    fn Start(
        &self,
        ppresentationdescriptor: windows::core::Ref<'_, IMFPresentationDescriptor>,
        _pguidtimeformat: *const GUID,
        _pvarstartposition: *const PROPVARIANT,
    ) -> Result<()> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        let pd = ppresentationdescriptor.ok()?;
        let mut selected = windows::core::BOOL(0);
        let mut _sd = None;
        unsafe {
            pd.GetStreamDescriptorByIndex(0, &mut selected, &mut _sd)?;
        }

        append_dll_log(&format!(
            "IMFMediaSource::Start(selected={})",
            selected.as_bool()
        ));

        if selected.as_bool() {
            let first = !self.started_once.swap(true, Ordering::SeqCst);
            if first {
                let stream_unk: IUnknown = self.stream.to_interface::<IMFMediaStream>().cast()?;
                unsafe {
                    self.events.QueueEventParamUnk(
                        MENewStream.0 as u32,
                        &GUID::zeroed(),
                        S_OK,
                        &stream_unk,
                    )?;
                }
            }
            self.stream.start_stream(!first)?;
        }

        unsafe {
            self.events.QueueEventParamVar(
                MESourceStarted.0 as u32,
                &GUID::zeroed(),
                S_OK,
                &PROPVARIANT::default(),
            )?;
        }
        Ok(())
    }

    fn Stop(&self) -> Result<()> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        append_dll_log("IMFMediaSource::Stop()");
        self.stream.stop_stream()?;
        unsafe {
            self.events.QueueEventParamVar(
                MESourceStopped.0 as u32,
                &GUID::zeroed(),
                S_OK,
                &PROPVARIANT::default(),
            )?;
        }
        Ok(())
    }

    fn Pause(&self) -> Result<()> {
        Err(MF_E_UNSUPPORTED_STATE_TRANSITION.into())
    }

    fn Shutdown(&self) -> Result<()> {
        if self.shutdown.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        append_dll_log("IMFMediaSource::Shutdown()");
        self.stream.shutdown_stream();
        unsafe {
            let _ = self.events.Shutdown();
        }
        Ok(())
    }
}

impl IMFMediaSourceEx_Impl for SpikeMediaSource_Impl {
    fn GetSourceAttributes(&self) -> Result<IMFAttributes> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        Ok(self.source_attrs.clone())
    }

    fn GetStreamAttributes(&self, dwstreamidentifier: u32) -> Result<IMFAttributes> {
        if self.shutdown.load(Ordering::Acquire) {
            return Err(MF_E_SHUTDOWN.into());
        }
        if dwstreamidentifier != 0 {
            return Err(MF_E_INVALIDSTREAMNUMBER.into());
        }
        Ok(self.stream_attrs.clone())
    }

    fn SetD3DManager(&self, _pmanager: windows::core::Ref<'_, IUnknown>) -> Result<()> {
        Ok(())
    }
}

impl IMFGetService_Impl for SpikeMediaSource_Impl {
    fn GetService(
        &self,
        guidservice: *const GUID,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> Result<()> {
        if riid.is_null() || ppvobject.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *ppvobject = std::ptr::null_mut();
            let svc = if guidservice.is_null() {
                GUID::zeroed()
            } else {
                *guidservice
            };
            append_dll_log(&format!(
                "IMFGetService::GetService(svc={svc:?}, riid={:?})",
                *riid
            ));
            if *riid == IKsControl::IID {
                let iface: IKsControl =
                    (*ComObjectInterface::<IKsControl>::as_interface_ref(self)).clone();
                *ppvobject = iface.into_raw();
                return Ok(());
            }
        }
        Err(MF_E_UNSUPPORTED_SERVICE.into())
    }
}

impl IKsControl_Impl for SpikeMediaSource_Impl {
    fn KsProperty(
        &self,
        property: *const KSIDENTIFIER,
        _propertylength: u32,
        _propertydata: *mut c_void,
        _datalength: u32,
        bytesreturned: *mut u32,
    ) -> Result<()> {
        unsafe {
            if !bytesreturned.is_null() {
                *bytesreturned = 0;
            }
            if !property.is_null() {
                let p = &*property;
                append_dll_log(&format!(
                    "IKsControl::KsProperty(set={:?}, id={}, flags={:#x})",
                    p.Anonymous.Anonymous.Set,
                    p.Anonymous.Anonymous.Id,
                    p.Anonymous.Anonymous.Flags
                ));
            }
        }
        Err(HRESULT::from_win32(ERROR_SET_NOT_FOUND_WIN32).into())
    }

    fn KsMethod(
        &self,
        _method: *const KSIDENTIFIER,
        _methodlength: u32,
        _methoddata: *mut c_void,
        _datalength: u32,
        bytesreturned: *mut u32,
    ) -> Result<()> {
        unsafe {
            if !bytesreturned.is_null() {
                *bytesreturned = 0;
            }
        }
        Err(HRESULT::from_win32(ERROR_SET_NOT_FOUND_WIN32).into())
    }

    fn KsEvent(
        &self,
        _event: *const KSIDENTIFIER,
        _eventlength: u32,
        _eventdata: *mut c_void,
        _datalength: u32,
        bytesreturned: *mut u32,
    ) -> Result<()> {
        unsafe {
            if !bytesreturned.is_null() {
                *bytesreturned = 0;
            }
        }
        Err(HRESULT::from_win32(ERROR_SET_NOT_FOUND_WIN32).into())
    }
}

#[implement(IClassFactory)]
struct SpikeClassFactory;

impl SpikeClassFactory {
    fn new() -> Self {
        OBJECT_COUNT.fetch_add(1, Ordering::SeqCst);
        Self
    }
}

impl Drop for SpikeClassFactory {
    fn drop(&mut self) {
        OBJECT_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

impl IClassFactory_Impl for SpikeClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: windows::core::Ref<'_, IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut c_void,
    ) -> Result<()> {
        if ppvobject.is_null() || riid.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe {
            *ppvobject = std::ptr::null_mut();
        }
        if !punkouter.is_null() {
            return Err(windows::Win32::Foundation::CLASS_E_NOAGGREGATION.into());
        }
        let source = SpikeMediaSource::create()?;
        let unk: IUnknown = source.to_interface::<IMFMediaSourceEx>().cast()?;
        let hr = unsafe { unk.query(riid, ppvobject) };
        if hr.is_err() {
            return Err(hr.into());
        }
        Ok(())
    }

    fn LockServer(&self, flock: windows::core::BOOL) -> Result<()> {
        if flock.as_bool() {
            LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
        } else {
            LOCK_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

/// COM class-factory entry point called by the COM runtime / FrameServer.
///
/// # Safety
/// Caller must pass valid COM `rclsid`, `riid`, and `ppv` pointers.
#[no_mangle]
pub unsafe extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    *ppv = std::ptr::null_mut();
    if *rclsid != SPIKE_CLSID {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    append_dll_log("DllGetClassObject called for SPIKE_CLSID");
    let factory = windows::core::ComObject::new(SpikeClassFactory::new());
    let unk: IUnknown = factory.to_interface();
    let hr = unk.query(riid, ppv);
    if hr.is_err() && hr != E_NOINTERFACE {
        append_dll_log(&format!("DllGetClassObject QueryInterface failed: {hr:?}"));
    }
    hr
}

/// COM DLL unload query.
///
/// # Safety
/// Must be called according to COM in-proc server unload rules.
#[no_mangle]
pub unsafe extern "system" fn DllCanUnloadNow() -> HRESULT {
    if OBJECT_COUNT.load(Ordering::SeqCst) == 0 && LOCK_COUNT.load(Ordering::SeqCst) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}
