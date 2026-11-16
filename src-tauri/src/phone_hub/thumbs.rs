use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use exif::{In, Reader as ExifReader, Tag};
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::metadata::Orientation;
use image::ImageError;
use tauri::{AppHandle, Manager};
use tokio::sync::Semaphore;

use crate::phone_files::{
    evict_dir_to_cap, normalize_device_path, sanitize_pc_filename, serial_cache_hash, sh_quote,
};
use crate::phone_hub::adb;
use crate::phone_hub::media::{allowed_media_extension, MediaItem};

pub(crate) const THUMB_MAX_DIM: u32 = 256;
pub(crate) const THUMB_JPEG_QUALITY: u8 = 80;
/// Read 64 KiB + 1 byte so `< THUMB_HEAD_BYTES` proves EOF was reached in one read.
pub(crate) const THUMB_HEAD_BYTES: usize = 65_537;
pub(crate) const THUMB_CACHE_CAP_BYTES: u64 = 100 * 1024 * 1024;
pub(crate) const THUMB_MAX_CONCURRENT_READS: usize = 4;

static THUMB_READ_SEM: OnceLock<Arc<Semaphore>> = OnceLock::new();
static TMP_SEQ: AtomicU64 = AtomicU64::new(1);
static EVICT_TICK: AtomicUsize = AtomicUsize::new(0);

fn global_thumb_semaphore() -> Arc<Semaphore> {
    THUMB_READ_SEM
        .get_or_init(|| Arc::new(Semaphore::new(THUMB_MAX_CONCURRENT_READS)))
        .clone()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ThumbSource {
    CacheHit,
    ExifFastPath,
    FullPull,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ThumbOutcome {
    pub path: PathBuf,
    pub source: ThumbSource,
}

pub(crate) fn sanitize_thumb_id(id: &str) -> String {
    let out: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.is_empty() {
        "item".to_string()
    } else {
        out
    }
}

pub(crate) fn thumb_cache_path(
    cache_root: &Path,
    serial: &str,
    id: &str,
    date_modified: i64,
) -> PathBuf {
    cache_root.join(serial_cache_hash(serial)).join(format!(
        "{}-{}.jpg",
        sanitize_thumb_id(id),
        date_modified
    ))
}

pub(crate) fn build_head_exec_out_args(serial: &str, norm_path: &str) -> [String; 4] {
    [
        "-s".to_string(),
        serial.to_string(),
        "exec-out".to_string(),
        format!("head -c {THUMB_HEAD_BYTES} -- {}", sh_quote(norm_path)),
    ]
}

fn parse_exif_partial(bytes: &[u8]) -> Option<exif::Exif> {
    let mut reader = ExifReader::new();
    reader.continue_on_error(true);
    match reader.read_from_container(&mut Cursor::new(bytes)) {
        Ok(exif) => Some(exif),
        Err(exif::Error::PartialResult(partial)) => Some(partial.into_inner().0),
        Err(_) => None,
    }
}

pub(crate) fn extract_exif_orientation(bytes: &[u8]) -> u8 {
    let Some(exif) = parse_exif_partial(bytes) else {
        return 1;
    };
    exif.get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0))
        .filter(|&v| (1..=8).contains(&v))
        .unwrap_or(1) as u8
}

/// Extract an embedded JPEG thumbnail from EXIF IFD1 (`JPEGInterchangeFormat`)
/// or JFIF `APP0` (`JFXX` format `0x10`) inside the leading bytes of a file.
pub(crate) fn extract_embedded_thumbnail_and_orientation(head: &[u8]) -> Option<(Vec<u8>, u8)> {
    if let Some(exif) = parse_exif_partial(head) {
        let primary_orient = exif
            .get_field(Tag::Orientation, In::PRIMARY)
            .and_then(|f| f.value.get_uint(0))
            .filter(|&v| (1..=8).contains(&v))
            .unwrap_or(1) as u8;

        let compression_ok = exif
            .get_field(Tag::Compression, In::THUMBNAIL)
            .and_then(|f| f.value.get_uint(0))
            .is_none_or(|c| c == 6);

        if compression_ok {
            if let (Some(off_f), Some(len_f)) = (
                exif.get_field(Tag::JPEGInterchangeFormat, In::THUMBNAIL),
                exif.get_field(Tag::JPEGInterchangeFormatLength, In::THUMBNAIL),
            ) {
                if let (Some(off), Some(len)) = (off_f.value.get_uint(0), len_f.value.get_uint(0)) {
                    let start = off as usize;
                    let size = len as usize;
                    let buf = exif.buf();
                    if size > 0 {
                        if let Some(end) = start.checked_add(size) {
                            if end <= buf.len() && buf[start..].starts_with(&[0xFF, 0xD8]) {
                                let thumb_orient = exif
                                    .get_field(Tag::Orientation, In::THUMBNAIL)
                                    .and_then(|f| f.value.get_uint(0))
                                    .filter(|&v| (1..=8).contains(&v))
                                    .map(|v| v as u8)
                                    .unwrap_or(primary_orient);
                                return Some((buf[start..end].to_vec(), thumb_orient));
                            }
                        }
                    }
                }
            }
        }
    }

    extract_jfxx_jpeg_thumbnail(head).map(|bytes| (bytes, 1))
}

fn extract_jfxx_jpeg_thumbnail(head: &[u8]) -> Option<Vec<u8>> {
    if !head.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2usize;
    while i + 4 <= head.len() {
        if head[i] != 0xFF {
            i += 1;
            continue;
        }
        while i < head.len() && head[i] == 0xFF {
            i += 1;
        }
        if i >= head.len() {
            break;
        }
        let marker = head[i];
        i += 1;
        if marker == 0x00 || (0xD0..=0xD7).contains(&marker) || marker == 0x01 {
            continue;
        }
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        if i + 2 > head.len() {
            break;
        }
        let seg_len = u16::from_be_bytes([head[i], head[i + 1]]) as usize;
        if seg_len < 2 || i + seg_len > head.len() {
            break;
        }
        let payload = &head[i + 2..i + seg_len];
        if marker == 0xE0 && payload.starts_with(b"JFXX\0\x10") {
            let thumb = &payload[6..];
            if thumb.starts_with(&[0xFF, 0xD8]) {
                return Some(thumb.to_vec());
            }
        }
        i += seg_len;
    }
    None
}

pub(crate) fn render_thumbnail_jpeg(
    img_bytes: &[u8],
    orientation: u8,
) -> Result<Option<Vec<u8>>, String> {
    let mut img = match image::load_from_memory(img_bytes) {
        Ok(decoded) => decoded,
        Err(ImageError::Unsupported(_)) => return Ok(None),
        Err(e) => return Err(format!("Failed to decode image: {e}")),
    };

    if let Some(orient) = Orientation::from_exif(orientation) {
        img.apply_orientation(orient);
    }

    if img.width() > THUMB_MAX_DIM || img.height() > THUMB_MAX_DIM {
        img = img.resize(THUMB_MAX_DIM, THUMB_MAX_DIM, FilterType::Triangle);
    }

    let rgb = img.to_rgb8();
    let mut out = Vec::with_capacity(16 * 1024);
    let mut encoder = JpegEncoder::new_with_quality(&mut out, THUMB_JPEG_QUALITY);
    encoder
        .encode_image(&rgb)
        .map_err(|e| format!("Failed to encode thumbnail JPEG: {e}"))?;

    Ok(Some(out))
}

fn write_atomic_and_maybe_evict(
    cache_root: &Path,
    target: &Path,
    jpeg_bytes: &[u8],
) -> Result<(), String> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create thumbnail cache dir: {e}"))?;
    }
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp = target.with_extension(format!("jpg.tmp.{seq}"));
    std::fs::write(&tmp, jpeg_bytes)
        .map_err(|e| format!("Failed to write thumbnail cache file: {e}"))?;
    if let Err(e) = std::fs::rename(&tmp, target) {
        let _ = std::fs::remove_file(&tmp);
        // Another concurrent task may have populated `target` first.
        if !target.exists() {
            return Err(format!("Failed to finalize thumbnail cache file: {e}"));
        }
    }
    if EVICT_TICK
        .fetch_add(1, Ordering::Relaxed)
        .is_multiple_of(32)
    {
        evict_dir_to_cap(cache_root, THUMB_CACHE_CAP_BYTES);
    }
    Ok(())
}

async fn cache_hit(target: &Path) -> bool {
    tokio::fs::metadata(target)
        .await
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false)
}

/// Production thumbnail pipeline parameterized over device read closures and semaphore
/// so unit tests and real-device e2e tests exercise the exact same code path.
pub(crate) async fn resolve_media_thumbnail_with<FHead, FutHead, FPull, FutPull>(
    cache_root: PathBuf,
    serial: &str,
    item: &MediaItem,
    semaphore: Option<Arc<Semaphore>>,
    mut read_head: FHead,
    mut pull_full: FPull,
) -> Result<Option<ThumbOutcome>, String>
where
    FHead: FnMut(String, String) -> FutHead,
    FutHead: std::future::Future<Output = Result<Vec<u8>, String>>,
    FPull: FnMut(String, String, PathBuf) -> FutPull,
    FutPull: std::future::Future<Output = Result<(), String>>,
{
    if item.kind == "video" {
        return Ok(None);
    }

    let norm = normalize_device_path(&item.path)?;
    let filename = sanitize_pc_filename(&norm)?;
    let ext = allowed_media_extension(&filename, "photo")
        .ok_or_else(|| "Unsupported media type".to_string())?;

    let stamp = if item.date_modified != 0 {
        item.date_modified
    } else {
        item.date_taken_ms / 1000
    };
    let target = thumb_cache_path(&cache_root, serial, &item.id, stamp);

    if cache_hit(&target).await {
        return Ok(Some(ThumbOutcome {
            path: target,
            source: ThumbSource::CacheHit,
        }));
    }

    let sem = semaphore.unwrap_or_else(global_thumb_semaphore);
    let permit = sem
        .acquire_owned()
        .await
        .map_err(|_| "Thumbnail semaphore closed".to_string())?;

    if cache_hit(&target).await {
        return Ok(Some(ThumbOutcome {
            path: target,
            source: ThumbSource::CacheHit,
        }));
    }

    let head_bytes = read_head(serial.to_string(), norm.clone())
        .await
        .unwrap_or_default();

    // Fast path: try embedded EXIF/JFIF thumbnail inside leading ~64 KiB.
    if !head_bytes.is_empty() {
        let head_for_exif = head_bytes.clone();
        let exif_rendered = tokio::task::spawn_blocking(move || {
            let (thumb_bytes, orient) = extract_embedded_thumbnail_and_orientation(&head_for_exif)?;
            let decoded = image::load_from_memory(&thumb_bytes).ok()?;
            if decoded.width() < 32 || decoded.height() < 32 {
                return None;
            }
            render_thumbnail_jpeg(&thumb_bytes, orient).ok().flatten()
        })
        .await
        .map_err(|e| format!("Thumbnail worker failed: {e}"))?;

        if let Some(jpeg) = exif_rendered {
            drop(permit);
            let root_clone = cache_root.clone();
            let target_clone = target.clone();
            tokio::task::spawn_blocking(move || {
                write_atomic_and_maybe_evict(&root_clone, &target_clone, &jpeg)
            })
            .await
            .map_err(|e| format!("Thumbnail write worker failed: {e}"))??;
            return Ok(Some(ThumbOutcome {
                path: target,
                source: ThumbSource::ExifFastPath,
            }));
        }

        // If `head -c 65537` hit EOF, we already have the entire file in memory.
        if head_bytes.len() < THUMB_HEAD_BYTES {
            drop(permit);
            let root_clone = cache_root.clone();
            let target_clone = target.clone();
            let rendered = tokio::task::spawn_blocking(move || {
                let orient = extract_exif_orientation(&head_bytes);
                let Some(jpeg) = render_thumbnail_jpeg(&head_bytes, orient)? else {
                    return Ok(false);
                };
                write_atomic_and_maybe_evict(&root_clone, &target_clone, &jpeg)?;
                Ok::<bool, String>(true)
            })
            .await
            .map_err(|e| format!("Thumbnail worker failed: {e}"))??;

            return Ok(rendered.then_some(ThumbOutcome {
                path: target,
                source: ThumbSource::FullPull,
            }));
        }
    }

    // Fallback: pull full file to temp path under semaphore, then drop permit before CPU decode/resize.
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    let tmp_pull = cache_root
        .join(serial_cache_hash(serial))
        .join(format!(".pull-{seq}.{ext}"));
    if let Some(parent) = tmp_pull.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let pull_res = pull_full(serial.to_string(), norm, tmp_pull.clone()).await;
    drop(permit);

    if let Err(e) = pull_res {
        let _ = tokio::fs::remove_file(&tmp_pull).await;
        return Err(e);
    }

    let root_clone = cache_root.clone();
    let target_clone = target.clone();
    let rendered = tokio::task::spawn_blocking(move || {
        let bytes = std::fs::read(&tmp_pull);
        let _ = std::fs::remove_file(&tmp_pull);
        let bytes = bytes.map_err(|e| format!("Failed to read pulled photo: {e}"))?;
        let orient = extract_exif_orientation(&bytes);
        let Some(jpeg) = render_thumbnail_jpeg(&bytes, orient)? else {
            return Ok(false);
        };
        write_atomic_and_maybe_evict(&root_clone, &target_clone, &jpeg)?;
        Ok::<bool, String>(true)
    })
    .await
    .map_err(|e| format!("Thumbnail worker failed: {e}"))??;

    Ok(rendered.then_some(ThumbOutcome {
        path: target,
        source: ThumbSource::FullPull,
    }))
}

pub(crate) async fn fetch_media_thumbnail(
    app: &AppHandle,
    serial: &str,
    item: &MediaItem,
) -> Result<Option<String>, String> {
    let cache_root = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("Failed to resolve app cache dir: {e}"))?
        .join("phone_hub_thumbs");

    let app_head = app.clone();
    let app_pull = app.clone();
    let outcome = resolve_media_thumbnail_with(
        cache_root,
        serial,
        item,
        None,
        move |s, path| {
            let app = app_head.clone();
            async move {
                let cmd = adb(&app).await?;
                let args = build_head_exec_out_args(&s, &path);
                let out = cmd
                    .args(args)
                    .output()
                    .await
                    .map_err(|e| format!("Failed to read photo header: {e}"))?;
                if !out.status.success() {
                    return Err("adb exec-out head failed".to_string());
                }
                Ok(out.stdout)
            }
        },
        move |s, path, dest| {
            let app = app_pull.clone();
            async move {
                let dest_str = dest.to_string_lossy().to_string();
                let out = adb(&app)
                    .await?
                    .args(["-s", &s, "pull", &path, &dest_str])
                    .output()
                    .await
                    .map_err(|e| format!("Failed to pull photo: {e}"))?;
                if !out.status.success() {
                    let stderr = String::from_utf8_lossy(&out.stderr);
                    return Err(format!("Photo pull failed: {}", stderr.trim()));
                }
                Ok(())
            }
        },
    )
    .await?;

    Ok(outcome.map(|o| o.path.to_string_lossy().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, GenericImageView, Rgb, RgbImage};
    use std::sync::atomic::AtomicUsize;
    use std::time::Duration;

    fn encode_test_jpeg(w: u32, h: u32, top_left: [u8; 3], bottom_right: [u8; 3]) -> Vec<u8> {
        let mut img = RgbImage::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let px = if x < w / 2 && y < h / 2 {
                    Rgb(top_left)
                } else {
                    Rgb(bottom_right)
                };
                img.put_pixel(x, y, px);
            }
        }
        let mut out = Vec::new();
        JpegEncoder::new_with_quality(&mut out, 85)
            .encode_image(&img)
            .unwrap();
        out
    }

    /// Build a minimal valid JPEG with an APP1 EXIF segment containing IFD0 Orientation
    /// and IFD1 embedded JPEG thumbnail (little-endian TIFF).
    fn build_jpeg_with_exif_thumb(
        main_w: u32,
        main_h: u32,
        orientation: u16,
        thumb_jpeg: &[u8],
    ) -> Vec<u8> {
        let main_jpeg = encode_test_jpeg(main_w, main_h, [200, 30, 30], [30, 30, 200]);

        // Build little-endian TIFF payload:
        // Header (8 bytes): "II" 0x2A00, IFD0 offset = 8
        // IFD0 (at 8): 1 entry (Orientation=0x0112, SHORT=3, count=1, val=orientation) + next_ifd=26
        // IFD1 (at 26): 3 entries (Compression=0x0103, JPEGInterchangeFormat=0x0201, JPEGInterchangeFormatLength=0x0202) + next_ifd=0
        // Thumbnail JPEG starts at offset 26 + 2 + 3*12 + 4 = 68.
        let mut tiff = Vec::new();
        tiff.extend_from_slice(b"II");
        tiff.extend_from_slice(&42u16.to_le_bytes());
        tiff.extend_from_slice(&8u32.to_le_bytes());

        // IFD0: 1 entry (2 + 12 + 4 = 18 bytes -> ends at 26)
        tiff.extend_from_slice(&1u16.to_le_bytes());
        tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation
        tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&orientation.to_le_bytes());
        tiff.extend_from_slice(&0u16.to_le_bytes());
        tiff.extend_from_slice(&26u32.to_le_bytes()); // next IFD = IFD1

        // IFD1: 3 entries (2 + 36 + 4 = 42 bytes -> ends at 68)
        let thumb_offset = 68u32;
        let thumb_len = thumb_jpeg.len() as u32;
        tiff.extend_from_slice(&3u16.to_le_bytes());
        // Tag 0x0103 Compression = 6 (JPEG)
        tiff.extend_from_slice(&0x0103u16.to_le_bytes());
        tiff.extend_from_slice(&3u16.to_le_bytes());
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&6u16.to_le_bytes());
        tiff.extend_from_slice(&0u16.to_le_bytes());
        // Tag 0x0201 JPEGInterchangeFormat
        tiff.extend_from_slice(&0x0201u16.to_le_bytes());
        tiff.extend_from_slice(&4u16.to_le_bytes()); // LONG
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&thumb_offset.to_le_bytes());
        // Tag 0x0202 JPEGInterchangeFormatLength
        tiff.extend_from_slice(&0x0202u16.to_le_bytes());
        tiff.extend_from_slice(&4u16.to_le_bytes()); // LONG
        tiff.extend_from_slice(&1u32.to_le_bytes());
        tiff.extend_from_slice(&thumb_len.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());

        assert_eq!(tiff.len(), thumb_offset as usize);
        tiff.extend_from_slice(thumb_jpeg);

        // Wrap in APP1 marker after SOI (first 2 bytes of main_jpeg).
        let app1_len = (2 + 6 + tiff.len()) as u16;
        let mut out = Vec::new();
        out.extend_from_slice(&main_jpeg[..2]); // FF D8
        out.extend_from_slice(&[0xFF, 0xE1]);
        out.extend_from_slice(&app1_len.to_be_bytes());
        out.extend_from_slice(b"Exif\0\0");
        out.extend_from_slice(&tiff);
        out.extend_from_slice(&main_jpeg[2..]);
        out
    }

    fn test_media_item(id: &str, path: &str, kind: &str, date_modified: i64) -> MediaItem {
        MediaItem {
            id: id.to_string(),
            kind: kind.to_string(),
            name: path.rsplit('/').next().unwrap_or("photo.jpg").to_string(),
            album: "Camera".to_string(),
            path: path.to_string(),
            date_taken_ms: date_modified * 1000,
            date_modified,
            size_bytes: 500_000,
            width: 640,
            height: 320,
            duration_ms: None,
        }
    }

    #[test]
    fn extracts_exif_thumbnail_and_orientation_from_jpeg_prefix() {
        let embedded = encode_test_jpeg(160, 120, [255, 0, 0], [0, 0, 255]);
        let full = build_jpeg_with_exif_thumb(800, 600, 6, &embedded);
        // Truncate right after APP1 + a bit, simulating a partial head read.
        let prefix = &full[..full.len().min(8192)];

        let (extracted, orient) =
            extract_embedded_thumbnail_and_orientation(prefix).expect("must extract EXIF thumb");
        assert_eq!(orient, 6);
        assert_eq!(extracted, embedded);

        let rendered = render_thumbnail_jpeg(&extracted, orient)
            .unwrap()
            .expect("must render JPEG");
        let decoded = image::load_from_memory(&rendered).unwrap();
        // Orientation 6 rotates 160x120 -> 120x160.
        assert_eq!(decoded.dimensions(), (120, 160));
    }

    #[test]
    fn extracts_jfxx_thumbnail_from_app0() {
        let embedded = encode_test_jpeg(96, 64, [10, 200, 10], [20, 20, 20]);
        let main = encode_test_jpeg(400, 300, [0, 0, 0], [255, 255, 255]);
        let seg_len = (2 + 6 + embedded.len()) as u16;
        let mut with_jfxx = Vec::new();
        with_jfxx.extend_from_slice(&main[..2]);
        with_jfxx.extend_from_slice(&[0xFF, 0xE0]);
        with_jfxx.extend_from_slice(&seg_len.to_be_bytes());
        with_jfxx.extend_from_slice(b"JFXX\0\x10");
        with_jfxx.extend_from_slice(&embedded);
        with_jfxx.extend_from_slice(&main[2..]);

        let (extracted, orient) =
            extract_embedded_thumbnail_and_orientation(&with_jfxx).expect("must extract JFXX");
        assert_eq!(orient, 1);
        assert_eq!(extracted, embedded);
    }

    #[test]
    fn resize_preserves_aspect_ratio_orientation_and_max_256() {
        // 640x320 (2:1) with Orientation 6 (90 CW) -> 320x640 -> resized to 128x256.
        let raw = encode_test_jpeg(640, 320, [255, 0, 0], [0, 255, 0]);
        let rendered = render_thumbnail_jpeg(&raw, 6)
            .unwrap()
            .expect("must render");
        let img = image::load_from_memory(&rendered).unwrap();
        assert_eq!(img.dimensions(), (128, 256));

        // After 90 deg CW rotation, original top-left (red) moves to top-right.
        let tr = img.get_pixel(110, 10);
        assert!(
            tr[0] > 180 && tr[1] < 80,
            "expected rotated red pixel at top-right, got {tr:?}"
        );

        // Small image (100x50) is not upscaled.
        let small = encode_test_jpeg(100, 50, [0, 0, 255], [0, 0, 255]);
        let rendered_small = render_thumbnail_jpeg(&small, 1).unwrap().unwrap();
        let small_img = image::load_from_memory(&rendered_small).unwrap();
        assert_eq!(small_img.dimensions(), (100, 50));
    }

    #[test]
    fn cache_key_layout_and_eviction() {
        let root = PathBuf::from("/tmp/phone_hub_thumbs");
        let p1 = thumb_cache_path(&root, "192.168.0.175:44565", "img:101", 1700000100);
        let p2 = thumb_cache_path(&root, "192.168.0.175:44565", "img:101", 1700000200);
        let p3 = thumb_cache_path(&root, "USB123", "img:101", 1700000100);
        assert_ne!(p1, p2);
        assert_ne!(p1.parent(), p3.parent());
        let name = p1.file_name().unwrap().to_str().unwrap();
        assert_eq!(name, "img_101-1700000100.jpg");
        assert!(!name.contains(':'));

        let tmp = std::env::temp_dir().join(format!("al_thumb_evict_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let sub = tmp.join("abc");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("1.jpg"), vec![0u8; 400]).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(sub.join("2.jpg"), vec![0u8; 400]).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        std::fs::write(sub.join("3.jpg"), vec![0u8; 400]).unwrap();

        evict_dir_to_cap(&tmp, 800);
        let remaining: Vec<_> = std::fs::read_dir(&sub)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(remaining.len() <= 2);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn semaphore_limits_concurrent_device_reads_to_four_and_caches() {
        let tmp = std::env::temp_dir().join(format!("al_thumb_sem_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);

        let embedded = encode_test_jpeg(160, 120, [50, 150, 250], [10, 10, 10]);
        let full_jpeg = Arc::new(build_jpeg_with_exif_thumb(1024, 768, 1, &embedded));

        let in_flight = Arc::new(AtomicUsize::new(0));
        let max_in_flight = Arc::new(AtomicUsize::new(0));
        let head_calls = Arc::new(AtomicUsize::new(0));
        let pull_calls = Arc::new(AtomicUsize::new(0));
        let sem = Arc::new(Semaphore::new(THUMB_MAX_CONCURRENT_READS));

        let mut handles = Vec::new();
        for i in 0..12 {
            let root = tmp.clone();
            let item = test_media_item(
                &format!("img:{i}"),
                &format!("/storage/emulated/0/DCIM/Camera/IMG_{i}.jpg"),
                "photo",
                1700000000 + i,
            );
            let jpeg = full_jpeg.clone();
            let cur = in_flight.clone();
            let peak = max_in_flight.clone();
            let hc = head_calls.clone();
            let pc = pull_calls.clone();
            let s = sem.clone();

            handles.push(tokio::spawn(async move {
                resolve_media_thumbnail_with(
                    root,
                    "192.168.0.175:44565",
                    &item,
                    Some(s),
                    move |_serial, _path| {
                        let cur = cur.clone();
                        let peak = peak.clone();
                        let hc = hc.clone();
                        let jpeg = jpeg.clone();
                        async move {
                            hc.fetch_add(1, Ordering::SeqCst);
                            let now = cur.fetch_add(1, Ordering::SeqCst) + 1;
                            peak.fetch_max(now, Ordering::SeqCst);
                            tokio::time::sleep(Duration::from_millis(30)).await;
                            cur.fetch_sub(1, Ordering::SeqCst);
                            Ok(jpeg[..jpeg.len().min(THUMB_HEAD_BYTES)].to_vec())
                        }
                    },
                    move |_serial, _path, _dest| {
                        let pc = pc.clone();
                        async move {
                            pc.fetch_add(1, Ordering::SeqCst);
                            Ok(())
                        }
                    },
                )
                .await
            }));
        }

        for h in handles {
            let outcome = h.await.unwrap().unwrap().unwrap();
            assert_eq!(outcome.source, ThumbSource::ExifFastPath);
            assert!(outcome.path.starts_with(&tmp));
            assert_eq!(
                outcome.path.extension().and_then(|e| e.to_str()),
                Some("jpg")
            );
        }

        assert!(max_in_flight.load(Ordering::SeqCst) <= 4);
        assert!(max_in_flight.load(Ordering::SeqCst) >= 2);
        assert_eq!(head_calls.load(Ordering::SeqCst), 12);
        assert_eq!(pull_calls.load(Ordering::SeqCst), 0);

        // Second fetch must hit disk cache with zero device reads.
        let item0 = test_media_item(
            "img:0",
            "/storage/emulated/0/DCIM/Camera/IMG_0.jpg",
            "photo",
            1700000000,
        );
        let outcome = resolve_media_thumbnail_with(
            tmp.clone(),
            "192.168.0.175:44565",
            &item0,
            Some(sem),
            |_s, _p| async { panic!("cache hit should not call read_head") },
            |_s, _p, _d| async { panic!("cache hit should not call pull_full") },
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(outcome.source, ThumbSource::CacheHit);

        // Videos return None and disallowed extensions are rejected.
        let vid = test_media_item(
            "vid:1",
            "/storage/emulated/0/DCIM/Camera/VID_1.mp4",
            "video",
            1,
        );
        assert!(resolve_media_thumbnail_with(
            tmp.clone(),
            "s",
            &vid,
            None,
            |_, _| async { Ok(vec![]) },
            |_, _, _| async { Ok(()) },
        )
        .await
        .unwrap()
        .is_none());

        let bad = test_media_item(
            "img:bad",
            "/storage/emulated/0/Download/payload.jpg.exe",
            "photo",
            1,
        );
        assert!(resolve_media_thumbnail_with(
            tmp.clone(),
            "s",
            &bad,
            None,
            |_, _| async { Ok(vec![]) },
            |_, _, _| async { Ok(()) },
        )
        .await
        .is_err());

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[tokio::test]
    async fn falls_back_to_full_pull_when_no_exif_thumb_in_large_file() {
        let tmp = std::env::temp_dir().join(format!("al_thumb_pull_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);

        let full_png_bytes = {
            let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(512, 256, Rgb([20, 120, 220])));
            let mut buf = Vec::new();
            img.write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
                .unwrap();
            buf
        };

        let item = test_media_item(
            "img:77",
            "/storage/emulated/0/Pictures/Screenshots/shot.png",
            "photo",
            1700000077,
        );

        let outcome = resolve_media_thumbnail_with(
            tmp.clone(),
            "serial-x",
            &item,
            None,
            |_s, _p| async {
                // Simulate a truncated >64KiB non-EXIF prefix so fallback pull is triggered.
                Ok(vec![0x89; THUMB_HEAD_BYTES])
            },
            move |_s, _p, dest| {
                let bytes = full_png_bytes.clone();
                async move {
                    std::fs::write(&dest, bytes).map_err(|e| e.to_string())?;
                    Ok(())
                }
            },
        )
        .await
        .unwrap()
        .unwrap();

        assert_eq!(outcome.source, ThumbSource::FullPull);
        let decoded = image::open(&outcome.path).unwrap();
        assert_eq!(decoded.dimensions(), (256, 128));

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
