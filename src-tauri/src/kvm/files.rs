//! Receiving files / clipboard images from unidirectional streams, and sending them.
use super::protocol::{read_frame, write_frame, StreamHeader, TransferEvent};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::sync::{broadcast, Notify};

const MAX_IMAGE: u64 = 64 * 1024 * 1024;
pub const TRANSFER_CHUNK_BYTES: usize = 256 * 1024;
pub const PROGRESS_MIN_INTERVAL: Duration = Duration::from_millis(100);
pub const QUIC_CANCEL_ERROR_CODE: u32 = 0x4341_4E43; // "CANC"
pub const TRANSFER_CANCELLED_ERR: &str = "transfer cancelled";

pub fn quic_cancel_varint() -> quinn::VarInt {
    quinn::VarInt::from_u32(QUIC_CANCEL_ERROR_CODE)
}

pub fn is_quic_cancel_read_error(err: &std::io::Error) -> bool {
    err.get_ref()
        .and_then(|e| e.downcast_ref::<quinn::ReadError>())
        .is_some_and(|re| {
            matches!(re, quinn::ReadError::Reset(code) if code.into_inner() == QUIC_CANCEL_ERROR_CODE as u64)
        })
}

pub fn is_quic_cancel_write_error(err: &quinn::WriteError) -> bool {
    matches!(
        err,
        quinn::WriteError::Stopped(code) if code.into_inner() == QUIC_CANCEL_ERROR_CODE as u64
    )
}

#[derive(Debug, Clone)]
pub struct TransferCancelMeta {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub size_bytes: u64,
}

#[derive(Clone)]
pub struct ActiveTransferHandle {
    pub meta: TransferCancelMeta,
    cancelled: Arc<AtomicBool>,
    notify: Arc<Notify>,
    emitted_cancelled: Arc<AtomicBool>,
}

impl ActiveTransferHandle {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }

    pub fn try_mark_emitted_cancelled(&self) -> bool {
        !self.emitted_cancelled.swap(true, Ordering::AcqRel)
    }
}

pub struct ActiveTransferGuard {
    conn_id: usize,
    id: String,
}

impl Drop for ActiveTransferGuard {
    fn drop(&mut self) {
        global_cancel_registry().unregister(self.conn_id, &self.id);
    }
}

#[derive(Default)]
struct CancelRegistryState {
    active: HashMap<(usize, String), ActiveTransferHandle>,
    cancelled_recent: HashSet<(usize, String)>,
    cancelled_order: VecDeque<(usize, String)>,
    emitted_ids: HashSet<(usize, String)>,
    emitted_order: VecDeque<(usize, String)>,
}

pub struct TransferCancelRegistry {
    inner: Mutex<CancelRegistryState>,
}

impl Default for TransferCancelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

const MAX_RECENT_CANCEL_IDS: usize = 256;

impl TransferCancelRegistry {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CancelRegistryState::default()),
        }
    }

    fn record_recent(
        set: &mut HashSet<(usize, String)>,
        order: &mut VecDeque<(usize, String)>,
        key: (usize, String),
    ) -> bool {
        if set.contains(&key) {
            return false;
        }
        set.insert(key.clone());
        order.push_back(key);
        while order.len() > MAX_RECENT_CANCEL_IDS {
            if let Some(old) = order.pop_front() {
                set.remove(&old);
            }
        }
        true
    }

    pub fn register(
        &self,
        conn_id: usize,
        meta: TransferCancelMeta,
    ) -> (ActiveTransferHandle, ActiveTransferGuard) {
        let id = meta.id.clone();
        let key = (conn_id, id.clone());
        let mut st = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let pre_cancelled = st.cancelled_recent.contains(&key);
        let handle = ActiveTransferHandle {
            meta,
            cancelled: Arc::new(AtomicBool::new(pre_cancelled)),
            notify: Arc::new(Notify::new()),
            emitted_cancelled: Arc::new(AtomicBool::new(pre_cancelled)),
        };
        st.active.insert(key, handle.clone());
        (handle, ActiveTransferGuard { conn_id, id })
    }

    pub fn unregister(&self, conn_id: usize, id: &str) {
        let mut st = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        st.active.remove(&(conn_id, id.to_string()));
    }

    /// Marks any transfer matching `(conn_id, id)` cancelled, returning its
    /// metadata if this call is the first to cancel & report it.
    pub fn cancel_for_conn(&self, conn_id: usize, id: &str) -> Option<TransferCancelMeta> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let st = &mut *guard;
        let key = (conn_id, id.to_string());
        Self::record_recent(&mut st.cancelled_recent, &mut st.cancelled_order, key.clone());
        if let Some(handle) = st.active.get(&key).cloned() {
            handle.cancelled.store(true, Ordering::Release);
            handle.notify.notify_waiters();
            if handle.try_mark_emitted_cancelled()
                && Self::record_recent(&mut st.emitted_ids, &mut st.emitted_order, key)
            {
                return Some(handle.meta);
            }
        }
        None
    }

    /// Marks all active transfers with `id` (on any connection) cancelled,
    /// returning metadata entries that have not yet emitted a cancelled event.
    pub fn cancel_any_conn(&self, id: &str) -> Vec<(usize, TransferCancelMeta)> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let st = &mut *guard;
        let matching_keys: Vec<(usize, String)> = st
            .active
            .keys()
            .filter(|(_, tid)| tid == id)
            .cloned()
            .collect();
        let mut out = Vec::new();
        for key in matching_keys {
            Self::record_recent(&mut st.cancelled_recent, &mut st.cancelled_order, key.clone());
            if let Some(handle) = st.active.get(&key).cloned() {
                handle.cancelled.store(true, Ordering::Release);
                handle.notify.notify_waiters();
                if handle.try_mark_emitted_cancelled()
                    && Self::record_recent(&mut st.emitted_ids, &mut st.emitted_order, key.clone())
                {
                    out.push((key.0, handle.meta));
                }
            }
        }
        out
    }

    /// Marks active transfers with `id` whose connection id != `exclude_conn_id` cancelled.
    pub fn cancel_except_conn(
        &self,
        exclude_conn_id: usize,
        id: &str,
    ) -> Vec<(usize, TransferCancelMeta)> {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let st = &mut *guard;
        let matching_keys: Vec<(usize, String)> = st
            .active
            .keys()
            .filter(|(cid, tid)| *cid != exclude_conn_id && tid == id)
            .cloned()
            .collect();
        let mut out = Vec::new();
        for key in matching_keys {
            Self::record_recent(&mut st.cancelled_recent, &mut st.cancelled_order, key.clone());
            if let Some(handle) = st.active.get(&key).cloned() {
                handle.cancelled.store(true, Ordering::Release);
                handle.notify.notify_waiters();
                if handle.try_mark_emitted_cancelled()
                    && Self::record_recent(&mut st.emitted_ids, &mut st.emitted_order, key.clone())
                {
                    out.push((key.0, handle.meta));
                }
            }
        }
        out
    }

    pub fn try_claim_cancelled_emission(&self, conn_id: usize, id: &str) -> bool {
        let mut guard = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let st = &mut *guard;
        let key = (conn_id, id.to_string());
        Self::record_recent(&mut st.cancelled_recent, &mut st.cancelled_order, key.clone());
        if let Some(handle) = st.active.get(&key) {
            handle.cancelled.store(true, Ordering::Release);
            handle.notify.notify_waiters();
            if !handle.try_mark_emitted_cancelled() {
                return false;
            }
        }
        Self::record_recent(&mut st.emitted_ids, &mut st.emitted_order, key)
    }

    pub fn is_recently_cancelled(&self, conn_id: usize, id: &str) -> bool {
        let st = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        st.cancelled_recent.contains(&(conn_id, id.to_string()))
    }
}

pub fn global_cancel_registry() -> &'static TransferCancelRegistry {
    static REGISTRY: OnceLock<TransferCancelRegistry> = OnceLock::new();
    REGISTRY.get_or_init(TransferCancelRegistry::new)
}

pub fn emit_cancelled_once(
    conn_id: usize,
    meta: &TransferCancelMeta,
    transfer_tx: Option<&broadcast::Sender<TransferEvent>>,
) -> bool {
    if global_cancel_registry().try_claim_cancelled_emission(conn_id, &meta.id) {
        if let Some(tx) = transfer_tx {
            let _ = tx.send(TransferEvent::Cancelled {
                id: meta.id.clone(),
                kind: meta.kind.clone(),
                name: meta.name.clone(),
                size_bytes: meta.size_bytes,
            });
        }
        return true;
    }
    false
}

/// Throttles per-transfer progress emission to at most ~10 events/s while
/// guaranteeing the initial update (`0%`) and final (`100%`) are always emitted.
pub struct ProgressThrottle {
    last_emit: Option<Instant>,
    last_pct: Option<u8>,
    min_interval: Duration,
}

impl Default for ProgressThrottle {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressThrottle {
    pub fn new() -> Self {
        Self {
            last_emit: None,
            last_pct: None,
            min_interval: PROGRESS_MIN_INTERVAL,
        }
    }

    pub fn should_emit_at(&mut self, pct: u8, now: Instant, force_final: bool) -> Option<u8> {
        let clamped = pct.min(100);
        if force_final {
            if self.last_pct == Some(100) {
                return None;
            }
            self.last_pct = Some(100);
            self.last_emit = Some(now);
            return Some(100);
        }
        if self.last_emit.is_none() {
            self.last_emit = Some(now);
            self.last_pct = Some(clamped);
            return Some(clamped);
        }
        if self.last_pct == Some(clamped) {
            return None;
        }
        let elapsed = now.saturating_duration_since(self.last_emit.unwrap());
        if elapsed >= self.min_interval || clamped == 100 {
            self.last_emit = Some(now);
            self.last_pct = Some(clamped);
            Some(clamped)
        } else {
            None
        }
    }

    pub fn should_emit(&mut self, pct: u8, force_final: bool) -> Option<u8> {
        self.should_emit_at(pct, Instant::now(), force_final)
    }
}

pub fn pct_for_bytes(done: u64, total: u64) -> u8 {
    if total == 0 {
        100
    } else {
        ((done.saturating_mul(100)) / total).min(100) as u8
    }
}

/// Reduce an untrusted name to a safe single path component, or `None`.
pub fn sanitize_name(name: &str) -> Option<String> {
    let last = name.rsplit(['/', '\\']).next()?;
    let base = last.trim_end_matches([' ', '.']);
    let bad_char = |c: char| c.is_control() || "<>:\"|?*".contains(c);
    if base.is_empty() || last.trim() == ".." || base.chars().any(bad_char) {
        return None;
    }
    let stem = base.split('.').next().unwrap_or("").to_ascii_uppercase();
    let numbered = (stem.starts_with("COM") || stem.starts_with("LPT"))
        && stem.len() == 4
        && matches!(stem.as_bytes()[3], b'1'..=b'9');
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || numbered;
    (!reserved).then(|| base.to_string())
}

/// `name`, or `stem (n).ext` if it already exists in `dir`.
pub fn unique_path(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let p = Path::new(name);
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = p.extension().and_then(|s| s.to_str());
    (1..)
        .map(|n| match ext {
            Some(e) => dir.join(format!("{stem} ({n}).{e}")),
            None => dir.join(format!("{stem} ({n})")),
        })
        .find(|c| !c.exists())
        .unwrap()
}

enum WritePartError {
    Cancelled,
    Io(std::io::Error),
}

async fn write_part_with_progress<R: AsyncRead + Unpin, F: FnMut(u64, u64)>(
    r: &mut R,
    part: &Path,
    size: u64,
    cancel: Option<&ActiveTransferHandle>,
    mut on_chunk: F,
) -> Result<(), WritePartError> {
    let mut f = tokio::fs::File::create(part).await.map_err(WritePartError::Io)?;
    let (mut buf, mut got) = (vec![0u8; TRANSFER_CHUNK_BYTES], 0u64);
    loop {
        if cancel.is_some_and(ActiveTransferHandle::is_cancelled) {
            return Err(WritePartError::Cancelled);
        }
        let n = if let Some(handle) = cancel {
            tokio::select! {
                biased;
                _ = handle.cancelled() => return Err(WritePartError::Cancelled),
                res = r.read(&mut buf) => match res {
                    Ok(n) => n,
                    Err(e) if is_quic_cancel_read_error(&e) => return Err(WritePartError::Cancelled),
                    Err(e) => return Err(WritePartError::Io(e)),
                }
            }
        } else {
            match r.read(&mut buf).await {
                Ok(n) => n,
                Err(e) if is_quic_cancel_read_error(&e) => return Err(WritePartError::Cancelled),
                Err(e) => return Err(WritePartError::Io(e)),
            }
        };
        if n == 0 {
            break;
        }
        got += n as u64;
        if got > size {
            return Err(WritePartError::Io(std::io::Error::other(
                "stream longer than header size",
            )));
        }
        f.write_all(&buf[..n]).await.map_err(WritePartError::Io)?;
        on_chunk(n as u64, got);
    }
    if cancel.is_some_and(ActiveTransferHandle::is_cancelled) {
        return Err(WritePartError::Cancelled);
    }
    if got != size {
        return Err(WritePartError::Io(std::io::Error::other(
            "stream shorter than header size",
        )));
    }
    f.flush().await.map_err(WritePartError::Io)
}

/// Handle one incoming uni stream: header, then raw bytes.
pub async fn receive<R: AsyncRead + Unpin>(r: &mut R, dir: &Path) -> Result<TransferEvent, String> {
    receive_with_events(r, dir, None).await
}

pub async fn receive_with_events<R: AsyncRead + Unpin>(
    r: &mut R,
    dir: &Path,
    transfer_tx: Option<&broadcast::Sender<TransferEvent>>,
) -> Result<TransferEvent, String> {
    receive_with_policy_and_events_for_conn(
        r,
        dir,
        super::clipboard::sync_files_enabled(),
        transfer_tx,
        0,
    )
    .await
}

pub async fn receive_with_events_for_conn<R: AsyncRead + Unpin>(
    r: &mut R,
    dir: &Path,
    transfer_tx: Option<&broadcast::Sender<TransferEvent>>,
    conn_id: usize,
) -> Result<TransferEvent, String> {
    receive_with_policy_and_events_for_conn(
        r,
        dir,
        super::clipboard::sync_files_enabled(),
        transfer_tx,
        conn_id,
    )
    .await
}

pub async fn receive_with_policy<R: AsyncRead + Unpin>(
    r: &mut R,
    dir: &Path,
    allow_files: bool,
) -> Result<TransferEvent, String> {
    receive_with_policy_and_events_for_conn(r, dir, allow_files, None, 0).await
}

pub async fn receive_with_policy_and_events<R: AsyncRead + Unpin>(
    r: &mut R,
    dir: &Path,
    allow_files: bool,
    transfer_tx: Option<&broadcast::Sender<TransferEvent>>,
) -> Result<TransferEvent, String> {
    receive_with_policy_and_events_for_conn(r, dir, allow_files, transfer_tx, 0).await
}

async fn receive_with_policy_and_events_for_conn<R: AsyncRead + Unpin>(
    r: &mut R,
    dir: &Path,
    allow_files: bool,
    transfer_tx: Option<&broadcast::Sender<TransferEvent>>,
    conn_id: usize,
) -> Result<TransferEvent, String> {
    let header: StreamHeader = read_frame(r)
        .await
        .map_err(|e| {
            if is_quic_cancel_read_error(&e) {
                TRANSFER_CANCELLED_ERR.to_string()
            } else {
                e.to_string()
            }
        })?
        .ok_or("empty stream")?;
    match header {
        StreamHeader::File { name, size } => {
            if !allow_files {
                return Err("file transfer sync disabled".into());
            }
            let safe = sanitize_name(&name).ok_or_else(|| format!("rejected file name {name:?}"))?;
            tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
            let target = unique_path(dir, &safe);
            let part = PathBuf::from(format!("{}.part", target.display()));
            if let Err(e) = write_part_with_progress(r, &part, size, None, |_, _| {}).await {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(match e {
                    WritePartError::Cancelled => TRANSFER_CANCELLED_ERR.to_string(),
                    WritePartError::Io(ioe) => ioe.to_string(),
                });
            }
            tokio::fs::rename(&part, &target).await.map_err(|e| e.to_string())?;
            let name = target.file_name().and_then(|n| n.to_str()).unwrap_or(&safe).to_string();
            Ok(TransferEvent::File { name, size })
        }
        StreamHeader::ClipboardImage { .. } => {
            let mut data = Vec::new();
            r.take(MAX_IMAGE + 1).read_to_end(&mut data).await.map_err(|e| e.to_string())?;
            if data.len() as u64 > MAX_IMAGE {
                return Err("clipboard image too large".into());
            }
            Ok(TransferEvent::ClipboardImage { data })
        }
        // --- SPEC-25c ---
        StreamHeader::OpenFile { transfer_id, name, size } => {
            let safe = sanitize_name(&name).ok_or_else(|| format!("rejected file name {name:?}"))?;
            let meta = TransferCancelMeta {
                id: transfer_id.clone(),
                kind: "open".to_string(),
                name: safe.clone(),
                size_bytes: size,
            };
            if global_cancel_registry().is_recently_cancelled(conn_id, &transfer_id) {
                emit_cancelled_once(conn_id, &meta, transfer_tx);
                return Err(TRANSFER_CANCELLED_ERR.to_string());
            }
            let (cancel_handle, _guard) = global_cancel_registry().register(conn_id, meta.clone());
            let open_dir = super::open_on_linked::open_staging_dir_for(
                &super::open_on_linked::open_staging_root(),
                &transfer_id,
            )
            .ok_or_else(|| format!("rejected transfer id {transfer_id:?}"))?;
            if let Err(e) = tokio::fs::create_dir_all(&open_dir).await {
                let msg = e.to_string();
                if let Some(tx) = transfer_tx {
                    let _ = tx.send(TransferEvent::Failed {
                        id: transfer_id,
                        kind: "open".to_string(),
                        name: safe,
                        reason: msg.clone(),
                    });
                }
                return Err(msg);
            }
            let target = unique_path(&open_dir, &safe);
            let part = PathBuf::from(format!("{}.part", target.display()));
            let mut throttle = ProgressThrottle::new();
            if let Some(tx) = transfer_tx {
                if let Some(pct) = throttle.should_emit(0, false) {
                    let _ = tx.send(TransferEvent::Progress {
                        id: transfer_id.clone(),
                        kind: "open".to_string(),
                        name: safe.clone(),
                        pct,
                        state: "active".to_string(),
                        size_bytes: size,
                    });
                }
            }
            match write_part_with_progress(r, &part, size, Some(&cancel_handle), |_, got| {
                if let Some(tx) = transfer_tx {
                    let pct = pct_for_bytes(got, size).min(99);
                    if let Some(p) = throttle.should_emit(pct, false) {
                        let _ = tx.send(TransferEvent::Progress {
                            id: transfer_id.clone(),
                            kind: "open".to_string(),
                            name: safe.clone(),
                            pct: p,
                            state: "active".to_string(),
                            size_bytes: size,
                        });
                    }
                }
            })
            .await
            {
                Ok(()) => {}
                Err(WritePartError::Cancelled) => {
                    let _ = tokio::fs::remove_file(&part).await;
                    let _ = tokio::fs::remove_dir_all(&open_dir).await;
                    super::open_on_linked::cancel_pending_open(&transfer_id);
                    emit_cancelled_once(conn_id, &meta, transfer_tx);
                    return Err(TRANSFER_CANCELLED_ERR.to_string());
                }
                Err(WritePartError::Io(e)) => {
                    let _ = tokio::fs::remove_file(&part).await;
                    if cancel_handle.is_cancelled()
                        || global_cancel_registry().is_recently_cancelled(conn_id, &transfer_id)
                    {
                        let _ = tokio::fs::remove_dir_all(&open_dir).await;
                        super::open_on_linked::cancel_pending_open(&transfer_id);
                        emit_cancelled_once(conn_id, &meta, transfer_tx);
                        return Err(TRANSFER_CANCELLED_ERR.to_string());
                    }
                    let _ = tokio::fs::remove_dir_all(&open_dir).await;
                    let msg = e.to_string();
                    if let Some(tx) = transfer_tx {
                        let _ = tx.send(TransferEvent::Failed {
                            id: transfer_id,
                            kind: "open".to_string(),
                            name: safe,
                            reason: msg.clone(),
                        });
                    }
                    return Err(msg);
                }
            }
            if cancel_handle.is_cancelled() {
                let _ = tokio::fs::remove_file(&part).await;
                let _ = tokio::fs::remove_dir_all(&open_dir).await;
                super::open_on_linked::cancel_pending_open(&transfer_id);
                emit_cancelled_once(conn_id, &meta, transfer_tx);
                return Err(TRANSFER_CANCELLED_ERR.to_string());
            }
            if let Err(e) = tokio::fs::rename(&part, &target).await {
                let _ = tokio::fs::remove_file(&part).await;
                let _ = tokio::fs::remove_dir_all(&open_dir).await;
                let msg = e.to_string();
                if let Some(tx) = transfer_tx {
                    let _ = tx.send(TransferEvent::Failed {
                        id: transfer_id,
                        kind: "open".to_string(),
                        name: safe,
                        reason: msg.clone(),
                    });
                }
                return Err(msg);
            }
            if let Some(tx) = transfer_tx {
                if let Some(p) = throttle.should_emit(100, true) {
                    let _ = tx.send(TransferEvent::Progress {
                        id: transfer_id.clone(),
                        kind: "open".to_string(),
                        name: safe.clone(),
                        pct: p,
                        state: "done".to_string(),
                        size_bytes: size,
                    });
                }
            }
            let final_name = target.file_name().and_then(|n| n.to_str()).unwrap_or(&safe).to_string();
            Ok(TransferEvent::OpenFile {
                transfer_id,
                name: final_name,
                size,
                saved_path: target,
            })
        }
        // --- end SPEC-25c ---
    }
}

pub async fn send_image(conn: &quinn::Connection, format: String, data: Vec<u8>) -> Result<(), String> {
    let mut s = conn.open_uni().await.map_err(|e| e.to_string())?;
    write_frame(&mut s, &StreamHeader::ClipboardImage { format }).await.map_err(|e| e.to_string())?;
    s.write_all(&data).await.map_err(|e| e.to_string())?;
    s.finish().map_err(|e| e.to_string())?;
    let _ = s.stopped().await;
    Ok(())
}

pub async fn send_file(conn: &quinn::Connection, path: &Path) -> Result<(), String> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("transfer.bin").to_string();
    let mut f = tokio::fs::File::open(path).await.map_err(|e| e.to_string())?;
    let size = f.metadata().await.map_err(|e| e.to_string())?.len();
    let mut s = conn.open_uni().await.map_err(|e| e.to_string())?;
    write_frame(&mut s, &StreamHeader::File { name, size }).await.map_err(|e| e.to_string())?;
    tokio::io::copy(&mut f, &mut s).await.map_err(|e| e.to_string())?;
    s.finish().map_err(|e| e.to_string())?;
    let _ = s.stopped().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitizer() {
        assert_eq!(sanitize_name("a.txt").as_deref(), Some("a.txt"));
        assert_eq!(sanitize_name("..\\..\\AppData\\Startup\\x.bat").as_deref(), Some("x.bat"));
        assert_eq!(sanitize_name("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(sanitize_name("C:\\Windows\\evil.dll").as_deref(), Some("evil.dll"));
        assert_eq!(sanitize_name("/abs/path/f.bin").as_deref(), Some("f.bin"));
        for bad in ["", ".", "..", "a/..", "dir/", "CON", "nul.txt", "COM1", "lpt9.log", "a:b", "x<y", "ads.txt:s"] {
            assert_eq!(sanitize_name(bad), None, "{bad:?}");
        }
        assert_eq!(sanitize_name("tail. .").as_deref(), Some("tail"));
        assert!(sanitize_name("COM0").is_some() && sanitize_name("CONSOLE.txt").is_some());
    }

    #[test]
    fn collisions() {
        let d = std::env::temp_dir().join(format!("anyleap-uniq-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("a.txt"), b"1").unwrap();
        assert_eq!(unique_path(&d, "a.txt"), d.join("a (1).txt"));
        std::fs::write(d.join("a (1).txt"), b"1").unwrap();
        assert_eq!(unique_path(&d, "a.txt"), d.join("a (2).txt"));
        assert_eq!(unique_path(&d, "new"), d.join("new"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn size_mismatch_leaves_no_files() {
        let d = std::env::temp_dir().join(format!("anyleap-recv-{}", std::process::id()));
        let (mut w, mut r) = tokio::io::duplex(4096);
        write_frame(&mut w, &StreamHeader::File { name: "f.bin".into(), size: 10 }).await.unwrap();
        w.write_all(b"abc").await.unwrap();
        drop(w);
        assert!(receive_with_policy(&mut r, &d, true).await.is_err());
        assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[tokio::test]
    async fn rejected_file_stream_leaves_no_files() {
        let d = std::env::temp_dir().join(format!("anyleap-rej-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (mut w, mut r) = tokio::io::duplex(4096);
        write_frame(&mut w, &StreamHeader::File { name: "f.bin".into(), size: 4 }).await.unwrap();
        w.write_all(b"abcd").await.unwrap();
        drop(w);
        assert!(receive_with_policy(&mut r, &d, false).await.is_err());
        assert!(!d.exists());
    }

    #[test]
    fn progress_throttle_limits_to_10hz_and_guarantees_endpoints() {
        let mut t = ProgressThrottle::new();
        let t0 = Instant::now();
        assert_eq!(t.should_emit_at(0, t0, false), Some(0));
        assert_eq!(t.should_emit_at(5, t0 + Duration::from_millis(20), false), None);
        assert_eq!(t.should_emit_at(25, t0 + Duration::from_millis(99), false), None);
        assert_eq!(t.should_emit_at(25, t0 + Duration::from_millis(100), false), Some(25));
        assert_eq!(t.should_emit_at(25, t0 + Duration::from_millis(250), false), None);
        assert_eq!(t.should_emit_at(100, t0 + Duration::from_millis(120), true), Some(100));
        assert_eq!(t.should_emit_at(100, t0 + Duration::from_millis(300), true), None);
    }
}
