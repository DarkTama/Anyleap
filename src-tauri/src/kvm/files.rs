//! Receiving files / clipboard images from unidirectional streams, and sending them.
use super::protocol::{read_frame, write_frame, StreamHeader, TransferEvent};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};

const MAX_IMAGE: u64 = 64 * 1024 * 1024;

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

async fn write_part<R: AsyncRead + Unpin>(r: &mut R, part: &Path, size: u64) -> std::io::Result<()> {
    let mut f = tokio::fs::File::create(part).await?;
    let (mut buf, mut got) = (vec![0u8; 64 * 1024], 0u64);
    loop {
        let n = r.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        got += n as u64;
        if got > size {
            return Err(std::io::Error::other("stream longer than header size"));
        }
        f.write_all(&buf[..n]).await?;
    }
    if got != size {
        return Err(std::io::Error::other("stream shorter than header size"));
    }
    f.flush().await
}

/// Handle one incoming uni stream: header, then raw bytes.
pub async fn receive<R: AsyncRead + Unpin>(r: &mut R, dir: &Path) -> Result<TransferEvent, String> {
    receive_with_policy(r, dir, super::clipboard::sync_files_enabled()).await
}

pub async fn receive_with_policy<R: AsyncRead + Unpin>(
    r: &mut R,
    dir: &Path,
    allow_files: bool,
) -> Result<TransferEvent, String> {
    let header: StreamHeader = read_frame(r).await.map_err(|e| e.to_string())?.ok_or("empty stream")?;
    match header {
        StreamHeader::File { name, size } => {
            if !allow_files {
                return Err("file transfer sync disabled".into());
            }
            let safe = sanitize_name(&name).ok_or_else(|| format!("rejected file name {name:?}"))?;
            tokio::fs::create_dir_all(dir).await.map_err(|e| e.to_string())?;
            let target = unique_path(dir, &safe);
            let part = PathBuf::from(format!("{}.part", target.display()));
            if let Err(e) = write_part(r, &part, size).await {
                let _ = tokio::fs::remove_file(&part).await;
                return Err(e.to_string());
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
}
