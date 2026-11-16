use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

use crate::phone_files::{
    cache_key_hash, evict_dir_to_cap, normalize_device_path, safe_serial_subdir,
    sanitize_pc_filename,
};
use crate::phone_hub::{adb, thumbs, validate_serial, PhoneHubState};

const MEDIA_CACHE_CAP_BYTES: u64 = 500 * 1024 * 1024;
const MAX_MEDIA_LIMIT: u32 = 200;

pub const IMAGES_URI: &str = "content://media/external/images/media";
pub const VIDEOS_URI: &str = "content://media/external/video/media";
pub const IMAGES_PROJECTION: &str =
    "_id:_display_name:bucket_display_name:_data:datetaken:date_modified:_size:width:height";
pub const VIDEOS_PROJECTION: &str =
    "_id:_display_name:bucket_display_name:_data:datetaken:date_modified:_size:width:height:duration";
pub const MINIMAL_MEDIA_PROJECTION: &str =
    "_id:_display_name:bucket_display_name:_data:date_modified:_size";

const KNOWN_KEYS: &[&str] = &[
    "_id",
    "_display_name",
    "bucket_display_name",
    "_data",
    "datetaken",
    "date_modified",
    "_size",
    "width",
    "height",
    "duration",
];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Album {
    pub name: String,
    pub photo_count: u32,
    pub video_count: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub album: String,
    pub path: String,
    pub date_taken_ms: i64,
    #[serde(skip, default)]
    pub date_modified: i64,
    pub size_bytes: u64,
    pub width: u32,
    pub height: u32,
    pub duration_ms: Option<u64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MediaPage {
    pub items: Vec<MediaItem>,
    pub total: u32,
}

fn strip_row_prefix(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("Row: ")?;
    let idx_end = rest.find(char::is_whitespace)?;
    if !rest[..idx_end].chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(rest[idx_end..].trim_start())
}

fn parse_row_fields(body: &str) -> Vec<(&str, &str)> {
    // Locate each known `<key>=` boundary so filenames with `, ` or quotes parse intact.
    let mut positions: Vec<(usize, &str)> = Vec::new();
    for &key in KNOWN_KEYS {
        let prefix = format!("{key}=");
        if body.starts_with(&prefix) {
            positions.push((0, key));
        }
        let mid_pat = format!(", {key}=");
        let mut search_from = 0usize;
        while let Some(rel) = body[search_from..].find(&mid_pat) {
            let abs = search_from + rel + 2;
            positions.push((abs, key));
            search_from = abs + key.len() + 1;
        }
    }
    positions.sort_by_key(|(pos, _)| *pos);

    let mut out = Vec::with_capacity(positions.len());
    for i in 0..positions.len() {
        let (start, key) = positions[i];
        let val_start = start + key.len() + 1;
        let val_end = if i + 1 < positions.len() {
            positions[i + 1].0.saturating_sub(2) // strip trailing ", "
        } else {
            body.len()
        };
        if val_start <= val_end && val_end <= body.len() {
            out.push((key, body[val_start..val_end].trim()));
        }
    }
    out
}

fn parse_nullable_u64(s: &str) -> Option<u64> {
    let t = s.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("NULL") {
        None
    } else {
        t.parse::<u64>().ok()
    }
}

fn parse_nullable_i64(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("NULL") {
        None
    } else {
        t.parse::<i64>().ok()
    }
}

fn fallback_album_from_path(norm_path: &str) -> String {
    Path::new(norm_path)
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Other")
        .to_string()
}

pub fn parse_content_rows(output: &str, kind: &str) -> Vec<MediaItem> {
    let mut out = Vec::new();
    let kind_norm = if kind == "video" { "video" } else { "photo" };
    let id_prefix = if kind_norm == "video" { "vid" } else { "img" };

    for raw in output.lines() {
        let Some(body) = strip_row_prefix(raw) else {
            continue;
        };
        let fields = parse_row_fields(body);
        let get =
            |k: &str| -> Option<&str> { fields.iter().find(|(fk, _)| *fk == k).map(|(_, v)| *v) };

        let Some(raw_id) = get("_id").filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("NULL"))
        else {
            continue;
        };
        let Some(raw_path) = get("_data") else {
            continue;
        };
        let Ok(norm_path) = normalize_device_path(raw_path) else {
            continue;
        };

        let name = get("_display_name")
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("NULL"))
            .map(|s| s.to_string())
            .unwrap_or_else(|| norm_path.rsplit('/').next().unwrap_or("media").to_string());

        let album = get("bucket_display_name")
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("NULL"))
            .map(|s| s.to_string())
            .unwrap_or_else(|| fallback_album_from_path(&norm_path));

        let date_taken = get("datetaken").and_then(parse_nullable_i64).unwrap_or(0);
        let date_modified = get("date_modified")
            .and_then(parse_nullable_i64)
            .unwrap_or(0);
        let date_taken_ms = if date_taken > 0 {
            date_taken
        } else {
            date_modified.saturating_mul(1000)
        };

        let size_bytes = get("_size").and_then(parse_nullable_u64).unwrap_or(0);
        let width = get("width").and_then(parse_nullable_u64).unwrap_or(0) as u32;
        let height = get("height").and_then(parse_nullable_u64).unwrap_or(0) as u32;
        let duration_ms = if kind_norm == "video" {
            get("duration").and_then(parse_nullable_u64)
        } else {
            None
        };

        out.push(MediaItem {
            id: format!("{id_prefix}:{raw_id}"),
            kind: kind_norm.to_string(),
            name,
            album,
            path: norm_path,
            date_taken_ms,
            date_modified,
            size_bytes,
            width,
            height,
            duration_ms,
        });
    }

    out
}

pub fn aggregate_albums(items: &[MediaItem]) -> Vec<Album> {
    let mut map: BTreeMap<String, (u32, u32)> = BTreeMap::new();
    for item in items {
        let entry = map.entry(item.album.clone()).or_insert((0, 0));
        if item.kind == "video" {
            entry.1 = entry.1.saturating_add(1);
        } else {
            entry.0 = entry.0.saturating_add(1);
        }
    }
    let mut albums: Vec<Album> = map
        .into_iter()
        .map(|(name, (photo_count, video_count))| Album {
            name,
            photo_count,
            video_count,
        })
        .collect();
    albums.sort_by(|a, b| {
        let ta = a.photo_count + a.video_count;
        let tb = b.photo_count + b.video_count;
        tb.cmp(&ta).then_with(|| a.name.cmp(&b.name))
    });
    albums
}

pub fn filter_and_page_media(
    items: &[MediaItem],
    kind: &str,
    album: Option<&str>,
    offset: u32,
    limit: u32,
) -> MediaPage {
    let mut filtered: Vec<MediaItem> = items
        .iter()
        .filter(|item| {
            let kind_ok = match kind {
                "photo" => item.kind == "photo",
                "video" => item.kind == "video",
                _ => true,
            };
            let album_ok = album.is_none_or(|a| a.is_empty() || item.album == a);
            kind_ok && album_ok
        })
        .cloned()
        .collect();

    filtered.sort_by(|a, b| {
        b.date_taken_ms
            .cmp(&a.date_taken_ms)
            .then_with(|| a.name.cmp(&b.name))
    });

    let total = filtered.len() as u32;
    let capped_limit = limit.min(MAX_MEDIA_LIMIT) as usize;
    let paged = filtered
        .into_iter()
        .skip(offset as usize)
        .take(capped_limit)
        .collect();

    MediaPage {
        items: paged,
        total,
    }
}

pub(crate) fn is_invalid_column_error(stdout: &str, stderr: &str) -> bool {
    stdout.contains("Invalid column") || stderr.contains("Invalid column")
}

pub(crate) fn extract_content_query_error(
    success: bool,
    stdout: &str,
    stderr: &str,
) -> Option<String> {
    let stderr_t = stderr.trim();
    let stdout_err_lines: Vec<&str> = stdout
        .lines()
        .map(str::trim)
        .filter(|l| {
            l.starts_with("Error while accessing provider")
                || l.contains("IllegalArgumentException")
                || l.contains("Invalid column")
        })
        .collect();

    let has_err_in_stderr = stderr_t.contains("Error while accessing provider")
        || stderr_t.contains("IllegalArgumentException")
        || stderr_t.contains("Invalid column");

    if !success
        || !stdout_err_lines.is_empty()
        || has_err_in_stderr
        || (!stderr_t.is_empty() && stdout.trim().is_empty())
    {
        let mut parts = Vec::new();
        if !stderr_t.is_empty() {
            parts.push(stderr_t.to_string());
        }
        if !stdout_err_lines.is_empty() {
            parts.push(stdout_err_lines.join(" | "));
        } else if !success && !stdout.trim().is_empty() {
            parts.push(stdout.trim().to_string());
        }
        if parts.is_empty() {
            Some("Media query failed".to_string())
        } else {
            Some(parts.join(": "))
        }
    } else {
        None
    }
}

/// Run a MediaStore `content query` via `run_query`, retrying once with
/// [`MINIMAL_MEDIA_PROJECTION`] if the device rejects a column as invalid.
pub(crate) async fn query_media_kind_with<F, Fut>(
    uri: &'static str,
    kind: &str,
    primary_projection: &'static str,
    mut run_query: F,
) -> Result<Vec<MediaItem>, String>
where
    F: FnMut(&'static str, &'static str) -> Fut,
    Fut: std::future::Future<Output = Result<(bool, String, String), String>>,
{
    let (ok1, out1, err1) = run_query(uri, primary_projection).await?;
    if let Some(msg1) = extract_content_query_error(ok1, &out1, &err1) {
        if is_invalid_column_error(&out1, &err1) {
            let (ok2, out2, err2) = run_query(uri, MINIMAL_MEDIA_PROJECTION).await?;
            if let Some(msg2) = extract_content_query_error(ok2, &out2, &err2) {
                return Err(format!("Media query ({kind}) failed: {msg2}"));
            }
            return Ok(parse_content_rows(&out2, kind));
        }
        return Err(format!("Media query ({kind}) failed: {msg1}"));
    }
    Ok(parse_content_rows(&out1, kind))
}

pub(crate) fn build_media_content_query_args<'a>(
    serial: &'a str,
    uri: &'a str,
    projection: &'a str,
) -> [&'a str; 9] {
    [
        "-s",
        serial,
        "shell",
        "content",
        "query",
        "--uri",
        uri,
        "--projection",
        projection,
    ]
}

async fn run_adb_content_query(
    app: &AppHandle,
    serial: &str,
    uri: &str,
    projection: &str,
) -> Result<(bool, String, String), String> {
    let cmd = adb(app).await?;
    let args = build_media_content_query_args(serial, uri, projection);
    let out = cmd
        .args(args)
        .output()
        .await
        .map_err(|e| format!("Failed to query media: {e}"))?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    ))
}

async fn query_all_media(app: &AppHandle, serial: &str) -> Result<Vec<MediaItem>, String> {
    let (img_res, vid_res) = tokio::join!(
        query_media_kind_with(IMAGES_URI, "photo", IMAGES_PROJECTION, |uri, proj| {
            run_adb_content_query(app, serial, uri, proj)
        }),
        query_media_kind_with(VIDEOS_URI, "video", VIDEOS_PROJECTION, |uri, proj| {
            run_adb_content_query(app, serial, uri, proj)
        }),
    );

    match (img_res, vid_res) {
        (Ok(mut imgs), Ok(vids)) => {
            imgs.extend(vids);
            Ok(imgs)
        }
        (Err(e1), Err(e2)) => Err(format!("{e1}; {e2}")),
        (Err(e), Ok(_)) | (Ok(_), Err(e)) => Err(e),
    }
}

async fn resolve_media_item(
    app: &AppHandle,
    state: &PhoneHubState,
    serial: &str,
    id: &str,
) -> Result<MediaItem, String> {
    if let Ok(guard) = state.media_cache.lock() {
        if let Some(items) = guard.get(serial) {
            if let Some(found) = items.iter().find(|m| m.id == id) {
                return Ok(found.clone());
            }
        }
    }
    let fresh = query_all_media(app, serial).await?;
    let found = fresh.iter().find(|m| m.id == id).cloned();
    if let Ok(mut guard) = state.media_cache.lock() {
        guard.insert(serial.to_string(), fresh);
    }
    found.ok_or_else(|| format!("Media item not found: {id}"))
}

#[tauri::command]
pub async fn phone_list_albums(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    serial: String,
) -> Result<Vec<Album>, String> {
    validate_serial(&serial)?;
    let all = query_all_media(&app, &serial).await?;
    let albums = aggregate_albums(&all);
    if let Ok(mut guard) = state.media_cache.lock() {
        guard.insert(serial, all);
    }
    Ok(albums)
}

#[tauri::command]
pub async fn phone_list_media(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    serial: String,
    kind: String,
    album: Option<String>,
    offset: u32,
    limit: u32,
) -> Result<MediaPage, String> {
    validate_serial(&serial)?;
    let use_cached = offset > 0;
    let all = if use_cached {
        let cached = state
            .media_cache
            .lock()
            .ok()
            .and_then(|g| g.get(&serial).cloned());
        match cached {
            Some(items) => items,
            None => {
                let fresh = query_all_media(&app, &serial).await?;
                if let Ok(mut guard) = state.media_cache.lock() {
                    guard.insert(serial.clone(), fresh.clone());
                }
                fresh
            }
        }
    } else {
        let fresh = query_all_media(&app, &serial).await?;
        if let Ok(mut guard) = state.media_cache.lock() {
            guard.insert(serial.clone(), fresh.clone());
        }
        fresh
    };

    Ok(filter_and_page_media(
        &all,
        &kind,
        album.as_deref(),
        offset,
        limit,
    ))
}

#[tauri::command]
pub async fn phone_media_thumbnail(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    serial: String,
    id: String,
) -> Result<Option<String>, String> {
    validate_serial(&serial)?;
    if id.starts_with("vid:") {
        return Ok(None);
    }
    let item = resolve_media_item(&app, &state, &serial, &id).await?;
    if item.kind == "video" {
        return Ok(None);
    }
    thumbs::fetch_media_thumbnail(&app, &serial, &item).await
}

pub fn allowed_media_extension(name: &str, kind: &str) -> Option<&'static str> {
    let trimmed = name.trim_end_matches(['.', ' ']);
    let ext = Path::new(trimmed)
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();

    match kind {
        "photo" => match ext.as_str() {
            "jpg" => Some("jpg"),
            "jpeg" => Some("jpeg"),
            "png" => Some("png"),
            "gif" => Some("gif"),
            "webp" => Some("webp"),
            "heic" => Some("heic"),
            "heif" => Some("heif"),
            "bmp" => Some("bmp"),
            "avif" => Some("avif"),
            _ => None,
        },
        "video" => match ext.as_str() {
            "mp4" => Some("mp4"),
            "m4v" => Some("m4v"),
            "mov" => Some("mov"),
            "3gp" => Some("3gp"),
            "mkv" => Some("mkv"),
            "webm" => Some("webm"),
            _ => None,
        },
        _ => None,
    }
}

#[tauri::command]
pub async fn phone_open_media(
    app: AppHandle,
    state: State<'_, PhoneHubState>,
    serial: String,
    id: String,
) -> Result<(), String> {
    validate_serial(&serial)?;
    let item = resolve_media_item(&app, &state, &serial, &id).await?;
    let norm = normalize_device_path(&item.path)?;
    let filename = sanitize_pc_filename(&norm)?;
    let ext = allowed_media_extension(&filename, &item.kind)
        .ok_or_else(|| "Unsupported media type".to_string())?;

    let cache_root = app
        .path()
        .app_cache_dir()
        .map_err(|e| format!("Failed to resolve app cache dir: {e}"))?
        .join("phone_hub_media");
    let serial_dir = cache_root.join(safe_serial_subdir(&serial));
    std::fs::create_dir_all(&serial_dir)
        .map_err(|e| format!("Failed to create media cache dir: {e}"))?;

    let target = serial_dir.join(format!(
        "{}.{ext}",
        cache_key_hash(&norm, item.date_taken_ms / 1000)
    ));

    if !target.exists() || target.metadata().map(|m| m.len() == 0).unwrap_or(true) {
        let target_str = target.to_string_lossy().to_string();
        let out = adb(&app)
            .await?
            .args(["-s", &serial, "pull", &norm, &target_str])
            .output()
            .await
            .map_err(|e| format!("Failed to pull media: {e}"))?;
        if !out.status.success() {
            let _ = std::fs::remove_file(&target);
            let stderr = String::from_utf8_lossy(&out.stderr);
            return Err(format!("Media pull failed: {}", stderr.trim()));
        }
        evict_dir_to_cap(&cache_root, MEDIA_CACHE_CAP_BYTES);
    }

    app.opener()
        .open_path(target.to_string_lossy().as_ref(), None::<&str>)
        .map_err(|e| format!("Failed to open media: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_rows_with_quotes_and_spaces() {
        let img_raw = include_str!("../../tests/fixtures/phone_hub/content_images.txt");
        let vid_raw = include_str!("../../tests/fixtures/phone_hub/content_videos.txt");

        let images = parse_content_rows(img_raw, "photo");
        // Row 3 has path traversal and must be rejected.
        assert_eq!(images.len(), 3);
        assert_eq!(images[0].name, "IMG-20260808-WA0071.jpg");
        assert_eq!(
            images[0].path,
            "/storage/emulated/0/Android/media/com.example.chat/ChatApp/accounts/1001/Media/Chat Images/IMG-20260808-WA0071.jpg"
        );
        assert_eq!(images[0].album, "Chat Images");
        // datetaken=NULL falls back to date_modified (seconds -> ms).
        assert_eq!(images[0].date_taken_ms, 1787287360 * 1000);
        assert_eq!(images[0].duration_ms, None);

        assert_eq!(images[1].name, "Beach Sunset '26.jpg");
        assert_eq!(images[1].album, "Camera");
        assert_eq!(images[1].date_taken_ms, 1787288000000);

        // Comma + double quotes inside display name and path + NULL datetaken fallback.
        assert_eq!(images[2].name, "Notes, Draft \"v2\".png");
        assert_eq!(
            images[2].path,
            "/storage/emulated/0/Pictures/Screenshots/Notes, Draft \"v2\".png"
        );
        assert_eq!(images[2].date_taken_ms, 1787287000 * 1000);

        let videos = parse_content_rows(vid_raw, "video");
        assert_eq!(videos.len(), 2);
        assert_eq!(videos[0].name, "VID-20260808-WA0012.mp4");
        assert_eq!(videos[0].album, "Chat Video");
        assert_eq!(videos[0].date_taken_ms, 1787287500 * 1000);
        assert_eq!(videos[0].duration_ms, Some(42559));
        assert_eq!(videos[1].name, "Demo Clip 'Final'.mp4");
        assert_eq!(videos[1].album, "Screen Recordings");
        assert_eq!(videos[1].duration_ms, Some(15420));
    }

    #[test]
    fn album_aggregation_counts() {
        let mut all = parse_content_rows(
            include_str!("../../tests/fixtures/phone_hub/content_images.txt"),
            "photo",
        );
        all.extend(parse_content_rows(
            include_str!("../../tests/fixtures/phone_hub/content_videos.txt"),
            "video",
        ));

        let albums = aggregate_albums(&all);
        let chat_imgs = albums.iter().find(|a| a.name == "Chat Images").unwrap();
        assert_eq!(chat_imgs.photo_count, 1);
        assert_eq!(chat_imgs.video_count, 0);

        let camera = albums.iter().find(|a| a.name == "Camera").unwrap();
        assert_eq!(camera.photo_count, 1);
        assert_eq!(camera.video_count, 0);

        let shots = albums.iter().find(|a| a.name == "Screenshots").unwrap();
        assert_eq!(shots.photo_count, 1);
        assert_eq!(shots.video_count, 0);

        let chat_vids = albums.iter().find(|a| a.name == "Chat Video").unwrap();
        assert_eq!(chat_vids.photo_count, 0);
        assert_eq!(chat_vids.video_count, 1);

        let recs = albums
            .iter()
            .find(|a| a.name == "Screen Recordings")
            .unwrap();
        assert_eq!(recs.photo_count, 0);
        assert_eq!(recs.video_count, 1);
    }

    #[test]
    fn paging_offset_limit_and_kind_filter() {
        let mut all = parse_content_rows(
            include_str!("../../tests/fixtures/phone_hub/content_images.txt"),
            "photo",
        );
        all.extend(parse_content_rows(
            include_str!("../../tests/fixtures/phone_hub/content_videos.txt"),
            "video",
        ));

        let page1 = filter_and_page_media(&all, "all", None, 0, 2);
        assert_eq!(page1.total, 5);
        assert_eq!(page1.items.len(), 2);
        // Newest first: 1787288500000 (Demo Clip), 1787288000000 (Beach Sunset)
        assert_eq!(page1.items[0].name, "Demo Clip 'Final'.mp4");
        assert_eq!(page1.items[1].name, "Beach Sunset '26.jpg");

        let page2 = filter_and_page_media(&all, "all", None, 2, 2);
        assert_eq!(page2.total, 5);
        assert_eq!(page2.items.len(), 2);
        // Next newest (using date_modified * 1000 fallback for NULL datetaken):
        // 1787287500000 (VID-20260808-WA0012.mp4), 1787287360000 (IMG-20260808-WA0071.jpg)
        assert_eq!(page2.items[0].name, "VID-20260808-WA0012.mp4");
        assert_eq!(page2.items[1].name, "IMG-20260808-WA0071.jpg");

        let videos_only = filter_and_page_media(&all, "video", None, 0, 500);
        assert_eq!(videos_only.total, 2);
        assert!(videos_only.items.iter().all(|m| m.kind == "video"));

        let chat_photos = filter_and_page_media(&all, "photo", Some("Chat Images"), 0, 50);
        assert_eq!(chat_photos.total, 1);
    }

    #[test]
    fn media_projection_uses_datetaken_and_minimal_fallback() {
        assert_eq!(
            IMAGES_PROJECTION,
            "_id:_display_name:bucket_display_name:_data:datetaken:date_modified:_size:width:height"
        );
        assert_eq!(
            VIDEOS_PROJECTION,
            "_id:_display_name:bucket_display_name:_data:datetaken:date_modified:_size:width:height:duration"
        );
        assert_eq!(
            MINIMAL_MEDIA_PROJECTION,
            "_id:_display_name:bucket_display_name:_data:date_modified:_size"
        );
        assert!(!IMAGES_PROJECTION.contains("date_taken"));
        assert!(!VIDEOS_PROJECTION.contains("date_taken"));
    }

    #[test]
    fn null_datetaken_falls_back_to_date_modified_ms_and_ordering() {
        let raw = "\
Row: 0 _id=1, _display_name=older_null.jpg, bucket_display_name=Chat Images, _data=/storage/emulated/0/Android/media/com.example.chat/Chat Images/older_null.jpg, datetaken=NULL, date_modified=1700000100, _size=1000, width=100, height=100\n\
Row: 1 _id=2, _display_name=newer_null.jpg, bucket_display_name=Chat Images, _data=/storage/emulated/0/Android/media/com.example.chat/Chat Images/newer_null.jpg, datetaken=NULL, date_modified=1700000300, _size=1000, width=100, height=100\n\
Row: 2 _id=3, _display_name=mid_taken.jpg, bucket_display_name=Chat Images, _data=/storage/emulated/0/Android/media/com.example.chat/Chat Images/mid_taken.jpg, datetaken=1700000200000, date_modified=1700000050, _size=1000, width=100, height=100\n";
        let items = parse_content_rows(raw, "photo");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].date_taken_ms, 1700000100 * 1000);
        assert_eq!(items[1].date_taken_ms, 1700000300 * 1000);
        assert_eq!(items[2].date_taken_ms, 1700000200000);

        let page = filter_and_page_media(&items, "photo", None, 0, 10);
        let names: Vec<&str> = page.items.iter().map(|i| i.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["newer_null.jpg", "mid_taken.jpg", "older_null.jpg"]
        );
    }

    #[tokio::test]
    async fn invalid_column_retries_once_with_minimal_projection() {
        let mut seen_projections: Vec<String> = Vec::new();
        let res = query_media_kind_with(
            IMAGES_URI,
            "photo",
            IMAGES_PROJECTION,
            |_uri, proj| {
                seen_projections.push(proj.to_string());
                let call_idx = seen_projections.len();
                async move {
                    if call_idx == 1 {
                        Ok((
                            true,
                            "Error while accessing provider:media\njava.lang.IllegalArgumentException: Invalid column width\n".to_string(),
                            String::new(),
                        ))
                    } else {
                        Ok((
                            true,
                            "Row: 0 _id=9001, _display_name=Fallback.jpg, bucket_display_name=Chat Images, _data=/storage/emulated/0/Android/media/com.example.chat/Chat Images/Fallback.jpg, date_modified=1787287360, _size=102400\n".to_string(),
                            String::new(),
                        ))
                    }
                }
            },
        )
        .await
        .expect("minimal projection retry should succeed");

        assert_eq!(
            seen_projections,
            vec![
                IMAGES_PROJECTION.to_string(),
                MINIMAL_MEDIA_PROJECTION.to_string(),
            ]
        );
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].name, "Fallback.jpg");
        assert_eq!(res[0].album, "Chat Images");
        assert_eq!(res[0].date_taken_ms, 1787287360 * 1000);
        assert_eq!(res[0].width, 0);
    }

    #[tokio::test]
    async fn media_query_surfaces_error_when_both_fail_or_non_column_error() {
        let err_both = query_media_kind_with(
            IMAGES_URI,
            "photo",
            IMAGES_PROJECTION,
            |_uri, _proj| async {
                Ok((
                    false,
                    "Error while accessing provider:media\njava.lang.IllegalArgumentException: Invalid column _data\n".to_string(),
                    String::new(),
                ))
            },
        )
        .await
        .expect_err("both failing must surface error");
        assert!(err_both.contains("Invalid column _data"));

        let mut attempts = 0usize;
        let err_perm =
            query_media_kind_with(VIDEOS_URI, "video", VIDEOS_PROJECTION, |_uri, _proj| {
                attempts += 1;
                async {
                    Ok((
                        false,
                        String::new(),
                        "SecurityException: Permission Denial".to_string(),
                    ))
                }
            })
            .await
            .expect_err("non-column error must fail without retry");
        assert_eq!(attempts, 1);
        assert!(err_perm.contains("Permission Denial"));
    }

    #[test]
    fn allowed_media_extension_accepts_allow_listed_case_insensitively() {
        let photo_exts = [
            "jpg", "jpeg", "png", "gif", "webp", "heic", "heif", "bmp", "avif",
        ];
        for ext in photo_exts {
            assert_eq!(
                allowed_media_extension(&format!("image.{ext}"), "photo"),
                Some(ext)
            );
            assert_eq!(
                allowed_media_extension(&format!("IMAGE.{}", ext.to_ascii_uppercase()), "photo"),
                Some(ext)
            );
        }

        let video_exts = ["mp4", "m4v", "mov", "3gp", "mkv", "webm"];
        for ext in video_exts {
            assert_eq!(
                allowed_media_extension(&format!("clip.{ext}"), "video"),
                Some(ext)
            );
            assert_eq!(
                allowed_media_extension(&format!("CLIP.{}", ext.to_ascii_uppercase()), "video"),
                Some(ext)
            );
        }
    }

    #[test]
    fn allowed_media_extension_rejects_dangerous_and_invalid_names() {
        let dangerous = [
            "exe", "hta", "lnk", "ps1", "bat", "cmd", "js", "scr", "msi", "com", "vbs",
        ];
        for ext in dangerous {
            assert_eq!(
                allowed_media_extension(&format!("payload.{ext}"), "photo"),
                None
            );
            assert_eq!(
                allowed_media_extension(&format!("payload.{}", ext.to_ascii_uppercase()), "video"),
                None
            );
        }

        // Double extension judged by the last extension.
        assert_eq!(allowed_media_extension("photo.jpg.exe", "photo"), None);
        assert_eq!(
            allowed_media_extension("archive.exe.jpg", "photo"),
            Some("jpg")
        );

        // Trailing dots and spaces are trimmed before checking.
        assert_eq!(allowed_media_extension("video.mp4.", "video"), Some("mp4"));
        assert_eq!(
            allowed_media_extension("video.mp4.  ", "video"),
            Some("mp4")
        );
        assert_eq!(allowed_media_extension("payload.hta.", "photo"), None);

        // Missing extension.
        assert_eq!(allowed_media_extension("photo", "photo"), None);
        assert_eq!(allowed_media_extension("", "photo"), None);
        assert_eq!(allowed_media_extension("...", "video"), None);
        assert_eq!(allowed_media_extension(".jpg", "photo"), None);

        // Kind mismatch: only allow extensions from the requested kind's list.
        assert_eq!(allowed_media_extension("clip.mp4", "photo"), None);
        assert_eq!(allowed_media_extension("pic.jpg", "video"), None);
        assert_eq!(allowed_media_extension("pic.jpg", "other"), None);
    }
}
