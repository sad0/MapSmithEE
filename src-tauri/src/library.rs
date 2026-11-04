//! The full-quality model library used by the Blender render (models.json, assets/, mesh/, proxies/, tex/, ext/).
//! It ships as split zip archives in a GitHub release, listed by a manifest; this downloads them (resumable, checked
//! with sha256) and unpacks them into `<app data>/library`. A local library (E:/AI/bg3d on John's PC) can be set instead.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub type Res<T> = Result<T, String>;

/// release manifest: `library-manifest.json` next to the parts
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Manifest {
    pub version: String,
    pub parts: Vec<Part>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Part {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: String,
}

/// disk space the first download takes at its peak: the zips (about 2.6 GB) and what they unpack to, both on disk
/// until the end
pub const FULL_SIZE: u64 = 6_000_000_000;

#[derive(Serialize, Clone, Debug)]
pub struct Status {
    pub dir: String,
    /// models.json is there
    pub ready: bool,
    /// version written after a complete download, or "local" for a folder set by hand
    pub version: Option<String>,
}

pub fn is_library(dir: &Path) -> bool {
    dir.join("models.json").exists() && dir.join("assets").is_dir()
}

pub fn status(dir: &Path) -> Status {
    let version = std::fs::read_to_string(dir.join(".version")).ok().map(|s| s.trim().to_string());
    Status { dir: dir.to_string_lossy().into_owned(), ready: is_library(dir), version }
}

fn sha256_file(p: &Path) -> Res<String> {
    let mut f = std::fs::File::open(p).map_err(|e| e.to_string())?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// download every part (resuming partial files), check them, unpack, write `.version`.
/// `progress(done_bytes, total_bytes, step)`
pub async fn install(
    manifest_url: &str,
    dir: &Path,
    progress: &(dyn Fn(u64, u64, &str) + Send + Sync),
    cancel: &AtomicBool,
) -> Res<Status> {
    let client = reqwest::Client::builder().user_agent("MapSmithEE").build().map_err(|e| e.to_string())?;
    let m: Manifest = client
        .get(manifest_url)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| format!("manifest: {e}"))?
        .json()
        .await
        .map_err(|e| format!("manifest: {e}"))?;
    let dl = dir.join("_download");
    std::fs::create_dir_all(&dl).map_err(|e| e.to_string())?;
    let total: u64 = m.parts.iter().map(|p| p.size).sum();
    let mut done: u64 = 0;
    for part in &m.parts {
        let path = dl.join(&part.name);
        let mut have = std::fs::metadata(&path).map(|md| md.len()).unwrap_or(0);
        if have > part.size {
            std::fs::remove_file(&path).ok();
            have = 0;
        }
        if have < part.size {
            let mut req = client.get(&part.url);
            if have > 0 {
                req = req.header("Range", format!("bytes={have}-"));
            }
            let mut resp = req.send().await.and_then(|r| r.error_for_status()).map_err(|e| format!("{}: {e}", part.name))?;
            // a server that ignores Range sends the whole file again
            if have > 0 && resp.status() != reqwest::StatusCode::PARTIAL_CONTENT {
                have = 0;
            }
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(have > 0)
                .write(true)
                .truncate(have == 0)
                .open(&path)
                .map_err(|e| e.to_string())?;
            let mut got = have;
            while let Some(chunk) = resp.chunk().await.map_err(|e| format!("{}: {e}", part.name))? {
                if cancel.load(Ordering::Relaxed) {
                    return Err("cancelled".into());
                }
                f.write_all(&chunk).map_err(|e| e.to_string())?;
                got += chunk.len() as u64;
                progress(done + got, total, &format!("Downloading {}", part.name));
            }
        }
        progress(done + part.size, total, &format!("Checking {}", part.name));
        let p2 = path.clone();
        let sum = tokio::task::spawn_blocking(move || sha256_file(&p2)).await.map_err(|e| e.to_string())??;
        if !sum.eq_ignore_ascii_case(&part.sha256) {
            std::fs::remove_file(&path).ok();
            return Err(format!("{} is damaged (checksum mismatch): run the download again", part.name));
        }
        done += part.size;
    }
    for part in &m.parts {
        progress(total, total, &format!("Unpacking {}", part.name));
        let (zip, out) = (dl.join(&part.name), dir.to_path_buf());
        tokio::task::spawn_blocking(move || unzip(&zip, &out)).await.map_err(|e| e.to_string())??;
    }
    std::fs::write(dir.join(".version"), &m.version).map_err(|e| e.to_string())?;
    std::fs::remove_dir_all(&dl).ok();
    Ok(status(dir))
}

fn unzip(zip: &Path, out: &Path) -> Res<()> {
    let f = std::fs::File::open(zip).map_err(|e| e.to_string())?;
    let mut a = zip::ZipArchive::new(f).map_err(|e| e.to_string())?;
    for i in 0..a.len() {
        let mut e = a.by_index(i).map_err(|e| e.to_string())?;
        // enclosed_name refuses absolute paths and `..`
        let Some(rel) = e.enclosed_name() else { continue };
        let target: PathBuf = out.join(rel);
        if e.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut w = std::fs::File::create(&target).map_err(|e| format!("{}: {e}", target.display()))?;
        std::io::copy(&mut e, &mut w).map_err(|e| e.to_string())?;
    }
    Ok(())
}
