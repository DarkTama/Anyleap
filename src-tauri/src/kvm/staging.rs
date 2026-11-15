//! Pre-staging directories, sanitized relative paths, transfer limits, and cleanup (SPEC-25a).

use super::files::{sanitize_name, unique_path};
use super::protocol::FileMeta;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};
use tokio::sync::Notify;

pub const DEFAULT_STAGING_LIMIT_MB: u32 = 2048;
pub const DEFAULT_CLIP_PREFETCH_LIMIT_MB: u32 = 100;
pub const TOTAL_STAGING_CAP_MB: u32 = 4096;
pub const AWAITING_FETCH_TTL: Duration = Duration::from_secs(600);
pub const STAGING_MAX_AGE: Duration = Duration::from_secs(24 * 3600);
const MAX_REL_PATH_COMPONENTS: usize = 32;
const MAX_COLLECTION_ITEMS: usize = 10_000;

static STAGING_LIMIT_MB: AtomicU32 = AtomicU32::new(DEFAULT_STAGING_LIMIT_MB);
static CLIP_PREFETCH_LIMIT_MB: AtomicU32 = AtomicU32::new(DEFAULT_CLIP_PREFETCH_LIMIT_MB);

pub fn staging_limit_mb() -> u32 {
    STAGING_LIMIT_MB.load(Ordering::Relaxed)
}

pub fn set_staging_limit_mb(limit_mb: u32) {
    STAGING_LIMIT_MB.store(limit_mb, Ordering::SeqCst);
}

pub fn clip_prefetch_limit_mb() -> u32 {
    CLIP_PREFETCH_LIMIT_MB.load(Ordering::Relaxed)
}

pub fn set_clip_prefetch_limit_mb(limit_mb: u32) {
    CLIP_PREFETCH_LIMIT_MB.store(limit_mb, Ordering::SeqCst);
}

pub fn staging_limit_bytes(limit_mb: u32) -> u64 {
    (limit_mb as u64).saturating_mul(1024 * 1024)
}

pub fn total_staging_cap_bytes() -> u64 {
    staging_limit_bytes(TOTAL_STAGING_CAP_MB)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipOfferMode {
    Prefetch,
    AwaitFetch,
}

/// Pure decision: whether a clip offer prefetches immediately (`<= limit`) or
/// waits in `awaiting_fetch` until `kvm_fetch_clip` is requested.
pub fn decide_clip_offer_mode(total_bytes: u64, prefetch_limit_mb: u32) -> ClipOfferMode {
    if total_bytes <= staging_limit_bytes(prefetch_limit_mb) {
        ClipOfferMode::Prefetch
    } else {
        ClipOfferMode::AwaitFetch
    }
}

/// Pure decision: when a new clip offer arrives or is sent, return the previous
/// clip offer ID to delete/cancel immediately if different from `new_offer_id`.
pub fn decide_replace_previous_clip(
    previous_offer_id: Option<&str>,
    new_offer_id: &str,
) -> Option<String> {
    previous_offer_id
        .filter(|prev| !prev.is_empty() && *prev != new_offer_id)
        .map(str::to_string)
}

/// Entries protected from total-staging-cap eviction: `_cursors` and `open-*`.
pub fn is_staging_cap_protected(name: &str) -> bool {
    name == "_cursors" || name.starts_with("open-")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagingDirEntry {
    pub name: String,
    pub size_bytes: u64,
    pub modified: SystemTime,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StagingEvictionOutcome {
    Fits {
        evict_names: Vec<String>,
        freed_bytes: u64,
        final_total_bytes: u64,
    },
    Refused {
        current_bytes: u64,
        needed_bytes: u64,
        cap_bytes: u64,
    },
}

/// Pure LRU eviction planner for the 4096 MB total staging cap.
/// `_cursors` is excluded from total byte accounting and never evicted; `open-*`
/// and active entries count toward the cap but are never evicted. Finished
/// drag/clip entries are evicted oldest-first (`modified` ascending).
pub fn plan_staging_eviction(
    entries: &[StagingDirEntry],
    incoming_bytes: u64,
    cap_bytes: u64,
) -> StagingEvictionOutcome {
    let total_bytes: u64 = entries
        .iter()
        .filter(|e| e.name != "_cursors")
        .fold(0u64, |acc, e| acc.saturating_add(e.size_bytes));

    if total_bytes.saturating_add(incoming_bytes) <= cap_bytes {
        return StagingEvictionOutcome::Fits {
            evict_names: Vec::new(),
            freed_bytes: 0,
            final_total_bytes: total_bytes.saturating_add(incoming_bytes),
        };
    }

    let mut evictable: Vec<&StagingDirEntry> = entries
        .iter()
        .filter(|e| !is_staging_cap_protected(&e.name) && !e.is_active)
        .collect();
    evictable.sort_by(|a, b| a.modified.cmp(&b.modified).then_with(|| a.name.cmp(&b.name)));

    let mut remaining = total_bytes;
    let mut freed_bytes = 0u64;
    let mut evict_names = Vec::new();
    for candidate in evictable {
        if remaining.saturating_add(incoming_bytes) <= cap_bytes {
            break;
        }
        remaining = remaining.saturating_sub(candidate.size_bytes);
        freed_bytes = freed_bytes.saturating_add(candidate.size_bytes);
        evict_names.push(candidate.name.clone());
    }

    if remaining.saturating_add(incoming_bytes) <= cap_bytes {
        StagingEvictionOutcome::Fits {
            evict_names,
            freed_bytes,
            final_total_bytes: remaining.saturating_add(incoming_bytes),
        }
    } else {
        StagingEvictionOutcome::Refused {
            current_bytes: total_bytes,
            needed_bytes: total_bytes.saturating_add(incoming_bytes),
            cap_bytes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferDestination {
    Staging,
    DownloadsFallback,
}

/// Pure decision: transfers whose total byte size is within `staging_limit_mb`
/// go to staging; larger transfers fall back to `Downloads\AnyLeap_Transfers`.
pub fn choose_transfer_destination(total_bytes: u64, staging_limit_mb: u32) -> TransferDestination {
    if total_bytes <= staging_limit_bytes(staging_limit_mb) {
        TransferDestination::Staging
    } else {
        TransferDestination::DownloadsFallback
    }
}

/// Default staging root under `%LOCALAPPDATA%\AnyLeap\staging`.
pub fn default_staging_root() -> PathBuf {
    std::env::var("LOCALAPPDATA")
        .map(|p| PathBuf::from(p).join("AnyLeap").join("staging"))
        .unwrap_or_else(|_| std::env::temp_dir().join("AnyLeap").join("staging"))
}

/// Validate a transfer/drag ID so it can never escape `root`.
pub fn sanitize_transfer_id(id: &str) -> Option<String> {
    if id.contains('/') || id.contains('\\') {
        return None;
    }
    sanitize_name(id)
}

/// Return `<root>/<sanitized_id>` or `None` if `id` is unsafe.
pub fn staging_dir_for(root: &Path, id: &str) -> Option<PathBuf> {
    let safe_id = sanitize_transfer_id(id)?;
    Some(root.join(safe_id))
}

/// Sanitize an untrusted relative path (preserving safe subdirectory structure)
/// while blocking traversal (`..`), drive prefixes, UNC paths, reserved Windows
/// device names, control characters, and illegal filename characters.
pub fn sanitize_rel_path(rel_path: &str) -> Option<PathBuf> {
    if rel_path.is_empty() || rel_path.starts_with('/') || rel_path.starts_with('\\') {
        return None;
    }
    let bytes = rel_path.as_bytes();
    if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
        return None;
    }

    let mut out = PathBuf::new();
    let mut count = 0usize;
    for raw_seg in rel_path.split(['/', '\\']) {
        if raw_seg.is_empty() || raw_seg == "." || raw_seg == ".." {
            return None;
        }
        if raw_seg.contains('/') || raw_seg.contains('\\') {
            return None;
        }
        let safe_seg = sanitize_name(raw_seg)?;
        if safe_seg != raw_seg.trim_end_matches([' ', '.']) || safe_seg != raw_seg {
            // Reject trailing spaces/dots on components as well as mismatched rewrites.
            return None;
        }
        count += 1;
        if count > MAX_REL_PATH_COMPONENTS {
            return None;
        }
        out.push(safe_seg);
    }
    if count == 0 {
        None
    } else {
        Some(out)
    }
}

/// Extract unique top-level paths inside `base_dir` from `files` for `CF_HDROP`.
pub fn top_level_staged_paths(base_dir: &Path, files: &[FileMeta]) -> Vec<PathBuf> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for meta in files {
        let Some(rel) = sanitize_rel_path(&meta.rel_path) else {
            continue;
        };
        let Some(first) = rel.components().next() else {
            continue;
        };
        let top = first.as_os_str().to_os_string();
        if seen.insert(top.clone()) {
            out.push(base_dir.join(top));
        }
    }
    out
}

/// Walk local filesystem paths (real files/directories only; symlinks skipped)
/// into `FileMeta` entries and `(rel_path, disk_path)` pairs for streaming.
pub fn collect_paths_to_file_meta(
    paths: &[PathBuf],
) -> Result<(Vec<FileMeta>, Vec<(String, PathBuf)>), String> {
    let mut metas = Vec::new();
    let mut sources = Vec::new();
    let mut used_top_names = HashSet::new();

    for path in paths {
        let meta = std::fs::symlink_metadata(path)
            .map_err(|e| format!("Cannot inspect {}: {e}", path.display()))?;
        if meta.file_type().is_symlink() {
            continue;
        }
        let raw_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| format!("Invalid file name: {}", path.display()))?;
        let Some(safe_top) = sanitize_name(raw_name) else {
            continue;
        };
        if !used_top_names.insert(safe_top.clone()) {
            continue;
        }

        if meta.is_file() {
            let size = meta.len();
            metas.push(FileMeta {
                rel_path: safe_top.clone(),
                size,
                is_dir: false,
            });
            sources.push((safe_top, path.clone()));
        } else if meta.is_dir() {
            collect_dir_recursive(path, &safe_top, &mut metas, &mut sources)?;
        }
        if metas.len() >= MAX_COLLECTION_ITEMS {
            return Err("Too many items in file transfer".into());
        }
    }

    Ok((metas, sources))
}

fn collect_dir_recursive(
    dir: &Path,
    rel_prefix: &str,
    metas: &mut Vec<FileMeta>,
    sources: &mut Vec<(String, PathBuf)>,
) -> Result<(), String> {
    if metas.len() >= MAX_COLLECTION_ITEMS {
        return Err("Too many items in file transfer".into());
    }
    metas.push(FileMeta {
        rel_path: rel_prefix.to_string(),
        size: 0,
        is_dir: true,
    });

    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let child_path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&child_path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        }
        let Some(name) = child_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(safe) = sanitize_name(name) else {
            continue;
        };
        let child_rel = format!("{rel_prefix}/{safe}");
        if meta.is_dir() {
            collect_dir_recursive(&child_path, &child_rel, metas, sources)?;
        } else if meta.is_file() {
            let size = meta.len();
            metas.push(FileMeta {
                rel_path: child_rel.clone(),
                size,
                is_dir: false,
            });
            sources.push((child_rel, child_path));
        }
        if metas.len() >= MAX_COLLECTION_ITEMS {
            return Err("Too many items in file transfer".into());
        }
    }
    Ok(())
}

/// Remove `<root>/<id>` recursively if present.
pub fn cleanup_staging_entry(root: &Path, id: &str) {
    if let Some(dir) = staging_dir_for(root, id) {
        if dir.exists() {
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
}

/// Entries preserved across peer disconnects: `_cursors` (cleared by cursor_scheme)
/// and `open-<transfer_id>` (Open-on-Linked temporary copies, swept only after 24 h).
fn is_preserved_on_disconnect(path: &Path) -> bool {
    path.file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|name| name == "_cursors" || name.starts_with("open-"))
}

/// Remove all transfer staging subdirectories inside `root`, preserving `_cursors` and `open-*`.
pub fn cleanup_all_staging(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if is_preserved_on_disconnect(&path) {
            continue;
        }
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(&path);
        } else {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// Pure expiry check: returns `true` when `modified` is strictly older than `max_age` relative to `now`.
pub fn is_expired(modified: SystemTime, now: SystemTime, max_age: Duration) -> bool {
    now.duration_since(modified).is_ok_and(|age| age > max_age)
}

/// Delete staging entries in `root` older than `max_age` relative to `now`.
pub fn cleanup_expired_staging(root: &Path, max_age: Duration, now: SystemTime) -> usize {
    let Ok(entries) = std::fs::read_dir(root) else {
        return 0;
    };
    let mut removed = 0usize;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().and_then(|n| n.to_str()) == Some("_cursors") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = meta.modified() else {
            continue;
        };
        if is_expired(modified, now, max_age) {
            let ok = if meta.is_dir() {
                std::fs::remove_dir_all(&path).is_ok()
            } else {
                std::fs::remove_file(&path).is_ok()
            };
            if ok {
                removed += 1;
            }
        }
    }
    removed
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferKind {
    Drag,
    Clip,
}

#[derive(Debug, Clone)]
pub struct TransferOfferPlan {
    pub destination: TransferDestination,
    pub base_dir: PathBuf,
    pub total_bytes: u64,
    pub expected_files: usize,
    pub top_level_paths: Vec<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct TransferCompletion {
    pub id: String,
    pub kind: TransferKind,
    pub destination: TransferDestination,
    pub base_dir: PathBuf,
    pub top_level_paths: Vec<PathBuf>,
    pub total_bytes: u64,
}

struct ActiveTransfer {
    kind: TransferKind,
    destination: TransferDestination,
    base_dir: PathBuf,
    top_name_map: HashMap<String, String>,
    expected_files: usize,
    received_files: usize,
    total_bytes: u64,
    received_bytes: u64,
    streamed_bytes: u64,
    top_level_paths: Vec<PathBuf>,
}

static TRANSFER_REGISTRY: OnceLock<Mutex<HashMap<String, ActiveTransfer>>> = OnceLock::new();
static TRANSFER_NOTIFY: OnceLock<Arc<Notify>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<String, ActiveTransfer>> {
    TRANSFER_REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn transfer_notify() -> &'static Arc<Notify> {
    TRANSFER_NOTIFY.get_or_init(|| Arc::new(Notify::new()))
}

fn remap_rel_path(rel: &Path, top_name_map: &HashMap<String, String>) -> PathBuf {
    let mut comps = rel.components();
    let Some(first) = comps.next() else {
        return rel.to_path_buf();
    };
    let first_str = first.as_os_str().to_string_lossy();
    let mapped_first = top_name_map
        .get(first_str.as_ref())
        .map(String::as_str)
        .unwrap_or(first_str.as_ref());
    let mut out = PathBuf::from(mapped_first);
    for c in comps {
        out.push(c.as_os_str());
    }
    out
}

/// Register an incoming `DragOffer` or `FileClipOffer` and prepare directories.
pub fn register_transfer_offer(
    id: &str,
    kind: TransferKind,
    files: &[FileMeta],
    staging_root: &Path,
    download_dir: &Path,
    limit_mb: u32,
) -> Result<TransferOfferPlan, String> {
    let total_bytes: u64 = files
        .iter()
        .filter(|f| !f.is_dir)
        .fold(0u64, |acc, f| acc.saturating_add(f.size));
    let expected_files = files.iter().filter(|f| !f.is_dir).count();
    let destination = choose_transfer_destination(total_bytes, limit_mb);

    let base_dir = match destination {
        TransferDestination::Staging => {
            enforce_staging_cap(staging_root, total_bytes, total_staging_cap_bytes())?;
            staging_dir_for(staging_root, id).ok_or_else(|| format!("unsafe transfer id {id:?}"))?
        }
        TransferDestination::DownloadsFallback => download_dir.to_path_buf(),
    };

    std::fs::create_dir_all(&base_dir).map_err(|e| e.to_string())?;

    let mut top_name_map = HashMap::new();
    if destination == TransferDestination::DownloadsFallback {
        for f in files {
            let Some(rel) = sanitize_rel_path(&f.rel_path) else {
                continue;
            };
            let Some(first) = rel.components().next() else {
                continue;
            };
            let top = first.as_os_str().to_string_lossy().into_owned();
            if !top_name_map.contains_key(&top) {
                let unique = unique_path(&base_dir, &top);
                if let Some(u_name) = unique.file_name().and_then(|n| n.to_str()) {
                    top_name_map.insert(top, u_name.to_string());
                }
            }
        }
    }

    for f in files.iter().filter(|f| f.is_dir) {
        if let Some(rel) = sanitize_rel_path(&f.rel_path) {
            let mapped = remap_rel_path(&rel, &top_name_map);
            let _ = std::fs::create_dir_all(base_dir.join(mapped));
        }
    }

    let mut seen = HashSet::new();
    let mut top_level_paths = Vec::new();
    for f in files {
        let Some(rel) = sanitize_rel_path(&f.rel_path) else {
            continue;
        };
        let Some(first) = rel.components().next() else {
            continue;
        };
        let top = first.as_os_str().to_string_lossy().into_owned();
        let mapped = top_name_map.get(&top).cloned().unwrap_or(top);
        if seen.insert(mapped.clone()) {
            top_level_paths.push(base_dir.join(mapped));
        }
    }

    let plan = TransferOfferPlan {
        destination,
        base_dir: base_dir.clone(),
        total_bytes,
        expected_files,
        top_level_paths: top_level_paths.clone(),
    };

    if expected_files > 0 {
        let mut reg = registry().lock().unwrap();
        reg.insert(
            id.to_string(),
            ActiveTransfer {
                kind,
                destination,
                base_dir,
                top_name_map,
                expected_files,
                received_files: 0,
                total_bytes,
                received_bytes: 0,
                streamed_bytes: 0,
                top_level_paths,
            },
        );
        transfer_notify().notify_waiters();
    }

    Ok(plan)
}

/// Resolve the target file path for an incoming `DragFile`/`ClipFile` stream.
/// Waits briefly if the corresponding control-stream offer packet hasn't been
/// processed yet, and falls back to `<fallback_root>/<id>/<rel_path>` for
/// standalone stream tests that don't send an offer packet.
pub async fn resolve_transfer_target(
    kind: TransferKind,
    id: &str,
    rel_path: &str,
    fallback_root: &Path,
) -> Result<PathBuf, String> {
    let rel = sanitize_rel_path(rel_path)
        .ok_or_else(|| format!("rejected relative path {rel_path:?}"))?;

    if kind == TransferKind::Clip && has_pending_inbound_clip(id) {
        return Err("clip offer is awaiting user fetch".into());
    }

    for attempt in 0..2 {
        if let Some(target) = {
            let reg = registry().lock().unwrap();
            reg.get(id)
                .filter(|entry| entry.kind == kind)
                .map(|entry| entry.base_dir.join(remap_rel_path(&rel, &entry.top_name_map)))
        } {
            return Ok(target);
        }
        if attempt == 0 {
            let notified = transfer_notify().notified();
            let _ = tokio::time::timeout(Duration::from_millis(2000), notified).await;
        }
    }

    let base = staging_dir_for(fallback_root, id)
        .ok_or_else(|| format!("rejected transfer id {id:?}"))?;
    Ok(base.join(rel))
}

/// Increment live received-chunk bytes for an in-flight transfer and return
/// `(streamed_bytes, total_bytes)` if registered.
pub fn record_transfer_bytes_progress(id: &str, delta_bytes: u64) -> Option<(u64, u64)> {
    let mut reg = registry().lock().unwrap();
    let entry = reg.get_mut(id)?;
    entry.streamed_bytes = entry
        .streamed_bytes
        .saturating_add(delta_bytes)
        .min(entry.total_bytes.max(1));
    Some((entry.streamed_bytes, entry.total_bytes))
}

/// Record one completed file for `id`; returns `Some(TransferCompletion)` when
/// every expected file in the offer has arrived. Also returns current `(received_bytes, total_bytes)`.
pub fn record_file_received(id: &str, size: u64) -> (Option<TransferCompletion>, Option<(u64, u64)>) {
    let mut reg = registry().lock().unwrap();
    let Some(entry) = reg.get_mut(id) else {
        return (None, None);
    };
    entry.received_files = entry.received_files.saturating_add(1);
    entry.received_bytes = entry.received_bytes.saturating_add(size);
    let progress = Some((entry.received_bytes, entry.total_bytes));
    if entry.received_files >= entry.expected_files {
        let done = reg.remove(id).unwrap();
        (
            Some(TransferCompletion {
                id: id.to_string(),
                kind: done.kind,
                destination: done.destination,
                base_dir: done.base_dir,
                top_level_paths: done.top_level_paths,
                total_bytes: done.total_bytes,
            }),
            progress,
        )
    } else {
        (None, progress)
    }
}

pub fn cancel_transfer(id: &str) {
    let _ = cancel_transfer_with_info(id);
}

pub fn cancel_transfer_with_info(id: &str) -> Option<(TransferKind, u64)> {
    let removed = {
        let mut reg = registry().lock().unwrap();
        reg.remove(id)
    };
    if let Some(entry) = removed {
        let info = (entry.kind, entry.total_bytes);
        if entry.destination == TransferDestination::Staging {
            let _ = std::fs::remove_dir_all(&entry.base_dir);
        }
        return Some(info);
    }
    None
}

pub fn active_transfer_ids() -> HashSet<String> {
    let mut ids = HashSet::new();
    if let Ok(reg) = registry().lock() {
        ids.extend(reg.keys().cloned());
    }
    if let Ok(pending) = pending_inbound_clips().lock() {
        ids.extend(pending.keys().cloned());
    }
    ids
}

/// Recursive non-symlink disk size helper for staging directory accounting.
pub fn dir_size_bytes(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.file_type().is_symlink() {
        return 0;
    }
    if meta.is_file() {
        return meta.len();
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in entries.flatten() {
        total = total.saturating_add(dir_size_bytes(&entry.path()));
    }
    total
}

/// Compute total staging bytes across `staging_root` (`drag`, `clip`, `open-*`),
/// excluding `_cursors`, and list immediate entries with metadata for LRU eviction.
pub fn scan_staging_entries(
    staging_root: &Path,
    active_ids: &HashSet<String>,
) -> Vec<StagingDirEntry> {
    let Ok(entries) = std::fs::read_dir(staging_root) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
            continue;
        };
        if name == "_cursors" {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        let size_bytes = dir_size_bytes(&path);
        let is_active = active_ids.contains(&name);
        out.push(StagingDirEntry {
            name,
            size_bytes,
            modified,
            is_active,
        });
    }
    out
}

/// Enforce `cap_bytes` (default 4096 MB) on `staging_root` by LRU-evicting the
/// oldest finished drag/clip entries. Protected entries (`open-*`, `_cursors`,
/// active transfers) are never evicted; if cap is still exceeded, refuses.
pub fn enforce_staging_cap(
    staging_root: &Path,
    incoming_bytes: u64,
    cap_bytes: u64,
) -> Result<usize, String> {
    if !staging_root.exists() {
        if incoming_bytes <= cap_bytes {
            return Ok(0);
        }
        return Err(format!(
            "Transfer ({} MB) exceeds {} MB staging capacity",
            incoming_bytes / (1024 * 1024),
            cap_bytes / (1024 * 1024)
        ));
    }
    let active_ids = active_transfer_ids();
    let entries = scan_staging_entries(staging_root, &active_ids);
    match plan_staging_eviction(&entries, incoming_bytes, cap_bytes) {
        StagingEvictionOutcome::Fits {
            evict_names,
            freed_bytes,
            final_total_bytes,
        } => {
            let mut evicted = 0usize;
            for name in &evict_names {
                let target = staging_root.join(name);
                let ok = if target.is_dir() {
                    std::fs::remove_dir_all(&target).is_ok()
                } else {
                    std::fs::remove_file(&target).is_ok()
                };
                if ok {
                    evicted += 1;
                }
            }
            if evicted > 0 {
                super::debug_log::note(|| {
                    format!(
                        "STAGING_EVICT count={evicted} freed_bytes={freed_bytes} final_total_bytes={final_total_bytes}"
                    )
                });
            }
            Ok(evicted)
        }
        StagingEvictionOutcome::Refused {
            current_bytes,
            needed_bytes,
            cap_bytes,
        } => Err(format!(
            "Staging storage full ({} MB used, need {} MB / {} MB cap)",
            current_bytes / (1024 * 1024),
            needed_bytes / (1024 * 1024),
            cap_bytes / (1024 * 1024),
        )),
    }
}

static PREVIOUS_CLIP_OFFER: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn previous_clip_offer() -> &'static Mutex<Option<String>> {
    PREVIOUS_CLIP_OFFER.get_or_init(|| Mutex::new(None))
}

/// Record `new_offer_id` as the latest clip offer and return any distinct prior
/// clip offer ID so its staging directory and active transfer can be replaced.
pub fn swap_previous_clip_offer(new_offer_id: &str) -> Option<String> {
    let mut g = previous_clip_offer().lock().unwrap();
    let prev = decide_replace_previous_clip(g.as_deref(), new_offer_id);
    *g = Some(new_offer_id.to_string());
    prev
}

pub fn take_previous_clip_offer() -> Option<String> {
    previous_clip_offer().lock().unwrap().take()
}

#[derive(Debug, Clone)]
pub struct PendingInboundClip {
    pub offer_id: String,
    pub files: Vec<FileMeta>,
    pub summary: String,
    pub total_bytes: u64,
    pub created_at: Instant,
}

#[derive(Debug, Clone)]
pub struct PendingOutboundClip {
    pub offer_id: String,
    pub files: Vec<FileMeta>,
    pub sources: Vec<(String, PathBuf)>,
    pub summary: String,
    pub total_bytes: u64,
    pub created_at: Instant,
}

static PENDING_INBOUND_CLIPS: OnceLock<Mutex<HashMap<String, PendingInboundClip>>> = OnceLock::new();
static PENDING_OUTBOUND_CLIPS: OnceLock<Mutex<HashMap<String, PendingOutboundClip>>> =
    OnceLock::new();

fn pending_inbound_clips() -> &'static Mutex<HashMap<String, PendingInboundClip>> {
    PENDING_INBOUND_CLIPS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn pending_outbound_clips() -> &'static Mutex<HashMap<String, PendingOutboundClip>> {
    PENDING_OUTBOUND_CLIPS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub fn insert_pending_inbound_clip(item: PendingInboundClip) {
    pending_inbound_clips()
        .lock()
        .unwrap()
        .insert(item.offer_id.clone(), item);
}

pub fn take_pending_inbound_clip(offer_id: &str) -> Option<PendingInboundClip> {
    pending_inbound_clips().lock().unwrap().remove(offer_id)
}

pub fn has_pending_inbound_clip(offer_id: &str) -> bool {
    pending_inbound_clips().lock().unwrap().contains_key(offer_id)
}

pub fn sweep_expired_pending_inbound_clips(ttl: Duration) -> Vec<PendingInboundClip> {
    let now = Instant::now();
    let mut g = pending_inbound_clips().lock().unwrap();
    let expired_ids: Vec<String> = g
        .iter()
        .filter(|(_, v)| now.duration_since(v.created_at) >= ttl)
        .map(|(k, _)| k.clone())
        .collect();
    expired_ids
        .into_iter()
        .filter_map(|id| g.remove(&id))
        .collect()
}

pub fn drain_pending_inbound_clips() -> Vec<PendingInboundClip> {
    pending_inbound_clips()
        .lock()
        .unwrap()
        .drain()
        .map(|(_, v)| v)
        .collect()
}

pub fn insert_pending_outbound_clip(item: PendingOutboundClip) {
    pending_outbound_clips()
        .lock()
        .unwrap()
        .insert(item.offer_id.clone(), item);
}

pub fn take_pending_outbound_clip(offer_id: &str) -> Option<PendingOutboundClip> {
    pending_outbound_clips().lock().unwrap().remove(offer_id)
}

pub fn sweep_expired_pending_outbound_clips(ttl: Duration) -> usize {
    let now = Instant::now();
    let mut g = pending_outbound_clips().lock().unwrap();
    let before = g.len();
    g.retain(|_, v| now.duration_since(v.created_at) < ttl);
    before.saturating_sub(g.len())
}

pub fn clear_pending_outbound_clips() {
    pending_outbound_clips().lock().unwrap().clear();
}

pub fn cancel_all_transfers() {
    let removed: Vec<ActiveTransfer> = {
        let mut reg = registry().lock().unwrap();
        reg.drain().map(|(_, v)| v).collect()
    };
    for entry in removed {
        if entry.destination == TransferDestination::Staging {
            let _ = std::fs::remove_dir_all(&entry.base_dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staging_limit_decides_staging_vs_downloads_fallback() {
        assert_eq!(choose_transfer_destination(0, 2048), TransferDestination::Staging);
        assert_eq!(
            choose_transfer_destination(2048 * 1024 * 1024, 2048),
            TransferDestination::Staging
        );
        assert_eq!(
            choose_transfer_destination(2048 * 1024 * 1024 + 1, 2048),
            TransferDestination::DownloadsFallback
        );
        assert_eq!(
            choose_transfer_destination(1024, 0),
            TransferDestination::DownloadsFallback
        );
    }

    #[test]
    fn sanitized_rel_path_blocks_traversal_and_reserved_names() {
        assert_eq!(
            sanitize_rel_path("folder/sub/file.txt"),
            Some(PathBuf::from("folder").join("sub").join("file.txt"))
        );
        assert_eq!(
            sanitize_rel_path("a\\b.png"),
            Some(PathBuf::from("a").join("b.png"))
        );
        for bad in [
            "",
            "/abs/x",
            "\\abs\\x",
            "C:foo.txt",
            "C:\\Windows\\system32\\cmd.exe",
            "../evil.txt",
            "a/../b.txt",
            "a/./b.txt",
            ".",
            "..",
            "a//b.txt",
            "a/CON/b.txt",
            "a/nul.txt",
            "COM1",
            "a/bad:stream.txt",
            "a/bad<name>.txt",
            "trailing.",
            "trailing ",
        ] {
            assert_eq!(sanitize_rel_path(bad), None, "expected None for {bad:?}");
        }
        assert_eq!(staging_dir_for(Path::new("/tmp"), "../escape"), None);
        assert_eq!(staging_dir_for(Path::new("/tmp"), "a/b"), None);
        assert!(staging_dir_for(Path::new("/tmp"), "drag-123").is_some());
    }

    #[test]
    fn top_level_staged_paths_deduplicates_nested_folder_entries() {
        let base = Path::new("staging/d1");
        let files = vec![
            FileMeta { rel_path: "MyDir".into(), size: 0, is_dir: true },
            FileMeta { rel_path: "MyDir/a.txt".into(), size: 10, is_dir: false },
            FileMeta { rel_path: "MyDir/sub/b.txt".into(), size: 20, is_dir: false },
            FileMeta { rel_path: "single.pdf".into(), size: 30, is_dir: false },
            FileMeta { rel_path: "../bad.exe".into(), size: 40, is_dir: false },
        ];
        assert_eq!(
            top_level_staged_paths(base, &files),
            vec![base.join("MyDir"), base.join("single.pdf")]
        );
    }

    #[test]
    fn staging_cleanup_removes_entry_all_and_expired_over_24h() {
        let root = std::env::temp_dir().join(format!("anyleap-stg-clean-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("drag-1")).unwrap();
        std::fs::write(root.join("drag-1").join("a.txt"), b"hi").unwrap();
        std::fs::create_dir_all(root.join("drag-2")).unwrap();
        std::fs::create_dir_all(root.join("_cursors")).unwrap();

        cleanup_staging_entry(&root, "drag-1");
        assert!(!root.join("drag-1").exists());
        assert!(root.join("drag-2").exists());

        cleanup_all_staging(&root);
        assert!(!root.join("drag-2").exists());
        assert!(root.join("_cursors").exists());

        // Expiry decision & real cleanup with synthetic `now`
        let now = SystemTime::now();
        assert!(!is_expired(now - Duration::from_secs(23 * 3600), now, STAGING_MAX_AGE));
        assert!(!is_expired(now - Duration::from_secs(24 * 3600), now, STAGING_MAX_AGE));
        assert!(is_expired(now - Duration::from_secs(24 * 3600 + 1), now, STAGING_MAX_AGE));

        std::fs::create_dir_all(root.join("drag-old")).unwrap();
        let removed = cleanup_expired_staging(
            &root,
            STAGING_MAX_AGE,
            now + Duration::from_secs(25 * 3600),
        );
        assert_eq!(removed, 1);
        assert!(!root.join("drag-old").exists());
        assert!(root.join("_cursors").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn cleanup_all_staging_preserves_open_on_linked_and_cursors_until_24h_expiry() {
        let root = std::env::temp_dir().join(format!("anyleap-stg-open-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("drag-x")).unwrap();
        std::fs::write(root.join("drag-x").join("a.txt"), b"drag").unwrap();
        std::fs::create_dir_all(root.join("open-y")).unwrap();
        std::fs::write(root.join("open-y").join("doc.pdf"), b"open").unwrap();
        std::fs::create_dir_all(root.join("_cursors")).unwrap();

        cleanup_all_staging(&root);
        assert!(!root.join("drag-x").exists());
        assert!(root.join("open-y").exists());
        assert!(root.join("_cursors").exists());

        let now = SystemTime::now();
        let removed = cleanup_expired_staging(
            &root,
            STAGING_MAX_AGE,
            now + Duration::from_secs(25 * 3600),
        );
        assert_eq!(removed, 1);
        assert!(!root.join("open-y").exists());
        assert!(root.join("_cursors").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn clip_replace_previous_decision() {
        assert_eq!(decide_replace_previous_clip(None, "offer-1"), None);
        assert_eq!(decide_replace_previous_clip(Some(""), "offer-1"), None);
        assert_eq!(decide_replace_previous_clip(Some("offer-1"), "offer-1"), None);
        assert_eq!(
            decide_replace_previous_clip(Some("offer-1"), "offer-2"),
            Some("offer-1".to_string())
        );
    }

    #[test]
    fn clip_prefetch_limit_decision() {
        let limit_mb = DEFAULT_CLIP_PREFETCH_LIMIT_MB;
        let limit_bytes = (limit_mb as u64) * 1024 * 1024;
        assert_eq!(decide_clip_offer_mode(0, limit_mb), ClipOfferMode::Prefetch);
        assert_eq!(
            decide_clip_offer_mode(limit_bytes, limit_mb),
            ClipOfferMode::Prefetch
        );
        assert_eq!(
            decide_clip_offer_mode(limit_bytes + 1, limit_mb),
            ClipOfferMode::AwaitFetch
        );
        assert_eq!(decide_clip_offer_mode(1, 0), ClipOfferMode::AwaitFetch);
    }

    #[test]
    fn staging_cap_eviction_order_never_evicts_open_cursors_or_active() {
        let base = SystemTime::UNIX_EPOCH + Duration::from_secs(10_000);
        let mb = 1024 * 1024;
        let entries = vec![
            StagingDirEntry {
                name: "_cursors".into(),
                size_bytes: 500 * mb,
                modified: base,
                is_active: false,
            },
            StagingDirEntry {
                name: "open-1".into(),
                size_bytes: 2000 * mb,
                modified: base + Duration::from_secs(1),
                is_active: false,
            },
            StagingDirEntry {
                name: "drag-active".into(),
                size_bytes: 500 * mb,
                modified: base + Duration::from_secs(2),
                is_active: true,
            },
            StagingDirEntry {
                name: "clip-oldest".into(),
                size_bytes: 900 * mb,
                modified: base + Duration::from_secs(3),
                is_active: false,
            },
            StagingDirEntry {
                name: "drag-middle".into(),
                size_bytes: 800 * mb,
                modified: base + Duration::from_secs(4),
                is_active: false,
            },
            StagingDirEntry {
                name: "clip-newest".into(),
                size_bytes: 400 * mb,
                modified: base + Duration::from_secs(5),
                is_active: false,
            },
        ];

        // `_cursors` is excluded; active+open+finished = 2000+500+900+800+400 = 4600 MB.
        // Wait: test with finished <= cap + incoming = 4096 cap. Wait: adjust sizes so initial <= 4096:
        // Total non-_cursors = 2000 + 500 + 700 + 600 + 200 = 4000 MB, incoming = 1000 MB -> need to free 904 MB.
        let mut entries = entries;
        entries[3].size_bytes = 700 * mb;
        entries[4].size_bytes = 600 * mb;
        entries[5].size_bytes = 200 * mb;

        let cap = 4096 * mb;
        let plan = plan_staging_eviction(&entries, 1000 * mb, cap);
        match plan {
            StagingEvictionOutcome::Fits {
                evict_names,
                freed_bytes,
                ..
            } => {
                // Evicts oldest finished first (`clip-oldest` 700 MB + `drag-middle` 600 MB = 1300 MB),
                // leaving `clip-newest`, `drag-active`, `open-1`, `_cursors`.
                assert_eq!(evict_names, vec!["clip-oldest", "drag-middle"]);
                assert_eq!(freed_bytes, 1300 * mb);
            }
            other => panic!("expected Fits, got {other:?}"),
        }

        // If `open-*` + active alone exceed cap for incoming transfer, refuse without deleting protected entries.
        let refused = plan_staging_eviction(&entries, 2000 * mb, cap);
        assert!(matches!(refused, StagingEvictionOutcome::Refused { .. }));
    }

    #[test]
    fn enforce_staging_cap_disk_evicts_oldest_and_preserves_open_and_cursors() {
        let root = std::env::temp_dir().join(format!("anyleap-stg-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("_cursors")).unwrap();
        std::fs::write(root.join("_cursors").join("arrow.cur"), vec![1u8; 4096]).unwrap();
        std::fs::create_dir_all(root.join("open-keep")).unwrap();
        std::fs::write(root.join("open-keep").join("a.bin"), vec![2u8; 400]).unwrap();
        std::fs::create_dir_all(root.join("clip-old")).unwrap();
        std::fs::write(root.join("clip-old").join("b.bin"), vec![3u8; 500]).unwrap();
        // Ensure newer mtime on second finished entry.
        std::thread::sleep(Duration::from_millis(25));
        std::fs::create_dir_all(root.join("drag-new")).unwrap();
        std::fs::write(root.join("drag-new").join("c.bin"), vec![4u8; 300]).unwrap();

        // Non-_cursors total = 400 + 500 + 300 = 1200. Cap = 1200. Incoming = 400 -> must evict clip-old (500).
        let evicted = enforce_staging_cap(&root, 400, 1200).unwrap();
        assert_eq!(evicted, 1);
        assert!(!root.join("clip-old").exists());
        assert!(root.join("drag-new").exists());
        assert!(root.join("open-keep").exists());
        assert!(root.join("_cursors").exists());

        // Now total = 400 (open) + 300 (drag-new) = 700. Incoming 900 with cap 1200 -> 700 - 300 + 900 = 1300 > 1200 -> refused.
        assert!(enforce_staging_cap(&root, 900, 1200).is_err());

        let _ = std::fs::remove_dir_all(&root);
    }
}
