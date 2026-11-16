use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::commands::spec27_hw_serial_query_script;
use crate::device_identity::parse_hw_serial_shell_output;
use crate::phone_files::parse_dumpsys_notifications_rich;
use crate::phone_hub::apps::{
    build_list_apps_args, parse_scrcpy_app_list, resolve_label_from_cache, AppLabelCache,
};
use crate::phone_hub::audio_only::build_audio_only_args;
use crate::phone_hub::media::{
    aggregate_albums, build_media_content_query_args, filter_and_page_media, query_media_kind_with,
    IMAGES_PROJECTION, IMAGES_URI, VIDEOS_PROJECTION, VIDEOS_URI,
};
use crate::phone_hub::messages::MessageStore;
use crate::phone_hub::overview::PhoneOverview;
use crate::phone_hub::poller::{
    apply_fast_overview, apply_slow_overview, apply_static_overview, NOTIFICATIONS_DUMPSYS_SCRIPT,
    OVERVIEW_FAST_SCRIPT, OVERVIEW_SLOW_SCRIPT, OVERVIEW_STATIC_SCRIPT,
};
use crate::phone_hub::thumbs::{
    build_head_exec_out_args, resolve_media_thumbnail_with, ThumbSource,
};

fn e2e_serial() -> String {
    std::env::var("ANYLEAP_E2E_SERIAL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .expect("ANYLEAP_E2E_SERIAL must be set when running ignored real-device e2e tests")
}

fn bundled_paths() -> (PathBuf, PathBuf, PathBuf) {
    let mut adb_candidates = Vec::new();
    let mut scrcpy_candidates = Vec::new();
    let mut server_candidates = Vec::new();

    // Prefer the workspace's staged binaries so e2e tests always exercise the
    // version currently under test rather than an older installed release.
    let manifest_bin = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("binaries");
    let triple = env!("TARGET");
    adb_candidates.push(manifest_bin.join(format!("adb-{triple}.exe")));
    scrcpy_candidates.push(manifest_bin.join(format!("scrcpy-{triple}.exe")));
    server_candidates.push(manifest_bin.join("scrcpy-server"));

    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        let dir = PathBuf::from(local).join("AnyLeap");
        adb_candidates.push(dir.join("adb.exe"));
        scrcpy_candidates.push(dir.join("scrcpy.exe"));
        server_candidates.push(dir.join("scrcpy").join("scrcpy-server"));
        server_candidates.push(dir.join("scrcpy-server"));
    }

    let adb = adb_candidates
        .into_iter()
        .find(|p| p.exists())
        .expect("bundled adb binary not found");
    let scrcpy = scrcpy_candidates
        .into_iter()
        .find(|p| p.exists())
        .expect("bundled scrcpy binary not found");
    let server = server_candidates
        .into_iter()
        .find(|p| p.exists())
        .expect("bundled scrcpy-server not found");

    (adb, scrcpy, server)
}

fn prepend_dll_dir(cmd: &mut Command) {
    let dll_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join("dll");
    if dll_dir.exists() {
        let existing = std::env::var_os("PATH").unwrap_or_default();
        let mut paths = vec![dll_dir];
        paths.extend(std::env::split_paths(&existing));
        if let Ok(joined) = std::env::join_paths(paths) {
            cmd.env("PATH", joined);
        }
    }
}

fn new_adb_cmd() -> Command {
    let (adb, _, _) = bundled_paths();
    let mut cmd = Command::new(adb);
    prepend_dll_dir(&mut cmd);
    cmd.env("ADB_MDNS_OPENSCREEN", "0")
        .env("ADB_MDNS_AUTO_CONNECT", "0");
    cmd
}

fn new_scrcpy_cmd() -> Command {
    let (adb, scrcpy, server) = bundled_paths();
    let mut cmd = Command::new(scrcpy);
    prepend_dll_dir(&mut cmd);
    cmd.env("SCRCPY_SERVER_PATH", server)
        .env("ADB", adb)
        .env("ADB_MDNS_OPENSCREEN", "0")
        .env("ADB_MDNS_AUTO_CONNECT", "0");
    cmd
}

fn run_adb_shell(serial: &str, script: &str) -> String {
    let out = new_adb_cmd()
        .args(["-s", serial, "shell", script])
        .output()
        .expect("failed to execute adb shell");
    assert!(
        out.status.success() || !out.stdout.is_empty(),
        "adb shell failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn resolve_e2e_identity(serial: &str) -> String {
    let script = spec27_hw_serial_query_script();
    let stdout = run_adb_shell(serial, &script);
    parse_hw_serial_shell_output(&stdout).unwrap_or_else(|| serial.to_string())
}

fn fetch_e2e_apps_cache(serial: &str) -> (String, AppLabelCache, usize, usize, usize) {
    let identity = resolve_e2e_identity(serial);
    let out = new_scrcpy_cmd()
        .args(build_list_apps_args(serial))
        .output()
        .expect("failed to execute scrcpy --list-apps");
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let apps = parse_scrcpy_app_list(&combined);
    let total = apps.len();
    let system_count = apps.iter().filter(|a| a.system).count();
    let user_count = total.saturating_sub(system_count);
    let mut cache = AppLabelCache::default();
    cache.set_apps(&identity, apps);
    (identity, cache, total, user_count, system_count)
}

#[test]
#[ignore]
fn e2e_overview_parses_real_device() {
    let serial = e2e_serial();
    let static_out = run_adb_shell(&serial, OVERVIEW_STATIC_SCRIPT);
    let fast_out = run_adb_shell(&serial, OVERVIEW_FAST_SCRIPT);
    let slow_out = run_adb_shell(&serial, OVERVIEW_SLOW_SCRIPT);

    let mut ov = PhoneOverview::default();
    apply_static_overview(&static_out, &mut ov);
    apply_fast_overview(&fast_out, &mut ov);
    apply_slow_overview(&slow_out, &mut ov);

    assert!(ov.model.is_some(), "expected non-null model");
    let battery = ov.battery.as_ref().expect("expected non-null battery");
    assert!(battery.level.is_some(), "expected battery level");
    let storage = ov.storage.as_ref().expect("expected non-null storage");
    assert!(storage.total_bytes > 0, "expected storage total_bytes > 0");
    let ram = ov.ram.as_ref().expect("expected non-null ram");
    assert!(ram.total_bytes > 0, "expected ram total_bytes > 0");

    eprintln!(
        "[e2e:overview] model_ok={} android_ok={} battery_level={:?} storage_total_bytes={} ram_total_bytes={}",
        ov.model.is_some(),
        ov.android_version.is_some(),
        battery.level,
        storage.total_bytes,
        ram.total_bytes
    );
}

#[tokio::test]
#[ignore]
async fn e2e_media_albums_and_pages_real_device() {
    let serial = e2e_serial();
    let serial_img = serial.clone();
    let serial_vid = serial.clone();

    let (img_res, vid_res) = tokio::join!(
        query_media_kind_with(IMAGES_URI, "photo", IMAGES_PROJECTION, move |uri, proj| {
            let s = serial_img.clone();
            async move {
                tokio::task::spawn_blocking(move || {
                    let args = build_media_content_query_args(&s, uri, proj);
                    let out = new_adb_cmd()
                        .args(args)
                        .output()
                        .map_err(|e| format!("Failed to query media: {e}"))?;
                    Ok((
                        out.status.success(),
                        String::from_utf8_lossy(&out.stdout).to_string(),
                        String::from_utf8_lossy(&out.stderr).to_string(),
                    ))
                })
                .await
                .map_err(|e| e.to_string())?
            }
        }),
        query_media_kind_with(VIDEOS_URI, "video", VIDEOS_PROJECTION, move |uri, proj| {
            let s = serial_vid.clone();
            async move {
                tokio::task::spawn_blocking(move || {
                    let args = build_media_content_query_args(&s, uri, proj);
                    let out = new_adb_cmd()
                        .args(args)
                        .output()
                        .map_err(|e| format!("Failed to query media: {e}"))?;
                    Ok((
                        out.status.success(),
                        String::from_utf8_lossy(&out.stdout).to_string(),
                        String::from_utf8_lossy(&out.stderr).to_string(),
                    ))
                })
                .await
                .map_err(|e| e.to_string())?
            }
        }),
    );

    let mut all = img_res.expect("photo query should succeed");
    let vids = vid_res.expect("video query should succeed");
    all.extend(vids);

    let albums = aggregate_albums(&all);
    let photos = filter_and_page_media(&all, "photo", None, 0, 200);
    let videos = filter_and_page_media(&all, "video", None, 0, 200);

    assert!(!albums.is_empty(), "expected >0 albums");
    assert!(photos.total > 0, "expected >0 photos");
    assert!(videos.total > 0, "expected >0 videos");

    let photo_nonzero_dates = photos.items.iter().filter(|m| m.date_taken_ms > 0).count();
    let video_nonzero_dates = videos.items.iter().filter(|m| m.date_taken_ms > 0).count();
    assert!(
        photo_nonzero_dates > 0 && photo_nonzero_dates == photos.items.len(),
        "expected non-zero dateTakenMs on returned photos"
    );
    assert!(
        video_nonzero_dates > 0 && video_nonzero_dates == videos.items.len(),
        "expected non-zero dateTakenMs on returned videos"
    );

    eprintln!(
        "[e2e:media] albums={} photos_total={} videos_total={} photo_page_nonzero_dates={}/{} video_page_nonzero_dates={}/{}",
        albums.len(),
        photos.total,
        videos.total,
        photo_nonzero_dates,
        photos.items.len(),
        video_nonzero_dates,
        videos.items.len()
    );
}

#[test]
#[ignore]
fn e2e_apps_list_and_system_label_real_device() {
    let serial = e2e_serial();
    let (identity, cache, total, user_count, system_count) = fetch_e2e_apps_cache(&serial);
    assert!(total > 0, "expected >0 apps from scrcpy --list-apps");

    let apps = cache.get_apps(&identity).expect("apps cached");
    assert!(
        apps.iter().all(|a| !a.label.trim().is_empty()),
        "all parsed apps must have non-empty labels"
    );

    let sys_pkg = apps
        .iter()
        .find(|a| a.package == "com.android.settings")
        .or_else(|| apps.iter().find(|a| a.system))
        .expect("expected at least one system package")
        .package
        .clone();
    let resolved = resolve_label_from_cache(&cache, &identity, &sys_pkg, None);
    assert_ne!(resolved, sys_pkg, "system package should resolve to label");

    eprintln!(
        "[e2e:apps] total={} user={} system={} system_pkg_resolved=true",
        total, user_count, system_count
    );
}

#[test]
#[ignore]
fn e2e_notifications_and_messages_real_device() {
    let serial = e2e_serial();
    let (identity, cache, _total_apps, _, _) = fetch_e2e_apps_cache(&serial);
    let stdout = run_adb_shell(&serial, NOTIFICATIONS_DUMPSYS_SCRIPT);
    let records = parse_dumpsys_notifications_rich(&stdout);

    let mut labels_map = HashMap::new();
    let mut resolved_count = 0usize;
    for rec in &records {
        let label = resolve_label_from_cache(
            &cache,
            &identity,
            &rec.base.package,
            rec.base.app_label.as_deref(),
        );
        if label != rec.base.package {
            resolved_count += 1;
        }
        labels_map.insert(rec.base.package.clone(), label);
    }

    if !records.is_empty() {
        assert!(
            resolved_count > 0,
            "expected at least one notification with resolved appLabel != package"
        );
    }

    let mut store = MessageStore::default();
    store.ingest(&serial, &records, &labels_map);
    let convs = store.conversations(&serial);
    let messaging_records = records.iter().filter(|r| r.is_messaging).count();

    eprintln!(
        "[e2e:notifications] notifications={} resolved_labels={} messaging_records={} conversations={}",
        records.len(),
        resolved_count,
        messaging_records,
        convs.len()
    );
}

#[test]
#[ignore]
fn e2e_audio_only_start_stop_real_device() {
    let serial = e2e_serial();
    let args = build_audio_only_args(&serial);
    let mut child = new_scrcpy_cmd()
        .args(&args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn scrcpy audio-only");

    let start = Instant::now();
    let mut exited_early = None;
    while start.elapsed() < Duration::from_secs(2) {
        if let Some(status) = child.try_wait().expect("try_wait failed") {
            exited_early = Some(status);
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    if let Some(status) = exited_early {
        let out = child.wait_with_output().expect("wait_with_output failed");
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        eprintln!(
            "[e2e:audio_only] status=exited_early code={:?} stderr={}",
            status.code(),
            stderr
        );
    } else {
        child.kill().expect("failed to kill scrcpy audio-only");
        let _ = child.wait();
        eprintln!("[e2e:audio_only] status=started_and_stopped_ok");
    }
}

#[tokio::test]
#[ignore]
async fn e2e_media_thumbnails_real_device() {
    let serial = e2e_serial();
    let serial_q = serial.clone();
    let all_photos =
        query_media_kind_with(IMAGES_URI, "photo", IMAGES_PROJECTION, move |uri, proj| {
            let s = serial_q.clone();
            async move {
                tokio::task::spawn_blocking(move || {
                    let args = build_media_content_query_args(&s, uri, proj);
                    let out = new_adb_cmd()
                        .args(args)
                        .output()
                        .map_err(|e| format!("Failed to query media: {e}"))?;
                    Ok((
                        out.status.success(),
                        String::from_utf8_lossy(&out.stdout).to_string(),
                        String::from_utf8_lossy(&out.stderr).to_string(),
                    ))
                })
                .await
                .map_err(|e| e.to_string())?
            }
        })
        .await
        .expect("photo query should succeed");

    let page = filter_and_page_media(&all_photos, "photo", None, 0, 60);
    assert!(
        !page.items.is_empty(),
        "expected at least 1 photo on device"
    );

    let cache_root = std::env::temp_dir().join(format!("al_e2e_thumbs_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache_root);

    let item_count = page.items.len();
    let t0 = Instant::now();
    let mut set = tokio::task::JoinSet::new();
    for item in page.items {
        let root = cache_root.clone();
        let ser = serial.clone();
        set.spawn(async move {
            resolve_media_thumbnail_with(
                root,
                &ser,
                &item,
                None,
                move |s, path| async move {
                    tokio::task::spawn_blocking(move || {
                        let args = build_head_exec_out_args(&s, &path);
                        let out = new_adb_cmd()
                            .args(args)
                            .output()
                            .map_err(|e| format!("exec-out head failed: {e}"))?;
                        if !out.status.success() {
                            return Err("adb exec-out head failed".to_string());
                        }
                        Ok(out.stdout)
                    })
                    .await
                    .map_err(|e| e.to_string())?
                },
                move |s, path, dest| async move {
                    tokio::task::spawn_blocking(move || {
                        let dest_str = dest.to_string_lossy().to_string();
                        let out = new_adb_cmd()
                            .args(["-s", &s, "pull", &path, &dest_str])
                            .output()
                            .map_err(|e| format!("adb pull failed: {e}"))?;
                        if !out.status.success() {
                            return Err(format!(
                                "adb pull failed: {}",
                                String::from_utf8_lossy(&out.stderr).trim()
                            ));
                        }
                        Ok(())
                    })
                    .await
                    .map_err(|e| e.to_string())?
                },
            )
            .await
        });
    }

    let mut exif_fast = 0usize;
    let mut full_pull = 0usize;
    let mut cache_hits = 0usize;
    let mut unsupported_none = 0usize;
    let mut max_bytes = 0u64;
    let mut max_dim = 0u32;

    while let Some(joined) = set.join_next().await {
        let outcome_opt = joined
            .expect("thumbnail task panicked")
            .expect("thumbnail pipeline returned error");
        let Some(outcome) = outcome_opt else {
            unsupported_none += 1;
            continue;
        };
        match outcome.source {
            ThumbSource::ExifFastPath => exif_fast += 1,
            ThumbSource::FullPull => full_pull += 1,
            ThumbSource::CacheHit => cache_hits += 1,
        }
        let meta = std::fs::metadata(&outcome.path).expect("thumb file exists");
        max_bytes = max_bytes.max(meta.len());
        let (w, h) = image::image_dimensions(&outcome.path).expect("valid thumb image");
        let long_side = w.max(h);
        assert!(
            long_side > 0 && long_side <= 256,
            "thumb long side must be <= 256 px, got {w}x{h}"
        );
        max_dim = max_dim.max(long_side);
    }

    let elapsed = t0.elapsed();
    let _ = std::fs::remove_dir_all(&cache_root);

    assert!(exif_fast + full_pull + cache_hits > 0);
    eprintln!(
        "[e2e:thumbs] count={} elapsed_ms={} exif_fast_path={} full_pull={} cache_hits={} unsupported_none={} max_thumb_bytes={} max_long_side_px={}",
        item_count,
        elapsed.as_millis(),
        exif_fast,
        full_pull,
        cache_hits,
        unsupported_none,
        max_bytes,
        max_dim
    );
}
