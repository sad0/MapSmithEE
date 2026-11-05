//! What every user needs around the editor: a log file, a bug report to paste in an issue, a check for a newer
//! release, the first-launch checks (Blender, disk space for the model library) and opening the project's pages.
//!
//! The log is `<app data>/logs/mapsmithee.log`: the editor's errors (window errors, failed saves, render failures) and
//! panics, one line each with a UTC time. Past 1 MB it moves to `mapsmithee.old.log` (one generation kept).

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Value};

const LOG_MAX: u64 = 1 << 20;
static LOG_LOCK: Mutex<()> = Mutex::new(());
const RELEASES: &str = "https://api.github.com/repos/sad0/MapSmithEE/releases?per_page=10";
/// pages `open_url` may open: the project's own and its Discord
const URL_OK: [&str; 2] = ["https://github.com/sad0/", "https://discord.gg/"];

pub fn log_dir(app: &tauri::AppHandle) -> PathBuf {
    crate::data_dir(app).unwrap_or_default().join("logs")
}

fn utc_now() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (days, rem) = (s / 86400, s % 86400);
    // civil date from days since 1970-01-01 (Howard Hinnant's algorithm)
    let z = days as i64 + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

/// One line in `<dir>/mapsmithee.log`. Best effort: logging never fails the caller.
pub fn log_to(dir: &Path, level: &str, msg: &str) {
    let Ok(_g) = LOG_LOCK.lock() else { return };
    let _ = std::fs::create_dir_all(dir);
    let f = dir.join("mapsmithee.log");
    if std::fs::metadata(&f).is_ok_and(|m| m.len() > LOG_MAX) {
        let _ = std::fs::rename(&f, dir.join("mapsmithee.old.log"));
    }
    if let Ok(mut out) = std::fs::OpenOptions::new().create(true).append(true).open(&f) {
        let one_line = msg.replace(['\r', '\n'], " ⏎ ");
        let _ = writeln!(out, "{} [{level}] {one_line}", utc_now());
    }
}

pub fn log(app: &tauri::AppHandle, level: &str, msg: &str) {
    log_to(&log_dir(app), level, msg);
}

/// Panics go to the log too (a release build aborts on panic, so this is the only trace left).
pub fn log_panics(app: &tauri::AppHandle) {
    let dir = log_dir(app);
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log_to(&dir, "panic", &info.to_string());
        prev(info);
    }));
}

/// The last `n` lines of the log.
fn log_tail(dir: &Path, n: usize) -> Vec<String> {
    let t = std::fs::read_to_string(dir.join("mapsmithee.log")).unwrap_or_default();
    let lines: Vec<&str> = t.lines().collect();
    lines[lines.len().saturating_sub(n)..].iter().map(|s| s.to_string()).collect()
}

#[tauri::command]
pub fn app_log(app: tauri::AppHandle, level: String, msg: String) {
    let level: String = level.chars().filter(|c| c.is_ascii_alphanumeric()).take(10).collect();
    log(&app, &level, &msg.chars().take(4000).collect::<String>());
}

/// Free bytes on the drive holding `p` (or its nearest existing parent). None where unknown.
pub fn free_bytes(p: &Path) -> Option<u64> {
    let mut at = p.to_path_buf();
    while !at.exists() {
        at = at.parent()?.to_path_buf();
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
        }
        let w: Vec<u16> = at.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
        // SAFETY: a NUL-terminated wide path and three valid out pointers
        let ok = unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, &mut total, &mut free) };
        (ok != 0).then_some(avail)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// First-launch checks: `{blender, library_ready, library_dir, free, need}`. `need` is what the model library
/// download still takes (bytes, 0 once installed), `free` what the library's drive has (null when unknown).
#[tauri::command]
pub fn system_check(app: tauri::AppHandle) -> Value {
    let st = crate::library_status(app.clone());
    let dir = crate::library_dir(&app);
    let need = if st.ready { 0 } else { crate::library::FULL_SIZE };
    json!({
        "blender": crate::blender_of(&app).ok().map(|p| p.display().to_string()),
        "library_ready": st.ready,
        "library_dir": dir.display().to_string(),
        "free": free_bytes(&dir),
        "need": need,
    })
}

/// This PC's memory (RAM) in GB, None where unknown
pub fn ram_gb() -> Option<f64> {
    #[cfg(windows)]
    {
        #[repr(C)]
        struct MemoryStatusEx {
            len: u32,
            load: u32,
            total: u64,
            avail: u64,
            page_total: u64,
            page_avail: u64,
            virt_total: u64,
            virt_avail: u64,
            ext: u64,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GlobalMemoryStatusEx(m: *mut MemoryStatusEx) -> i32;
        }
        let mut m = MemoryStatusEx { len: std::mem::size_of::<MemoryStatusEx>() as u32, load: 0, total: 0, avail: 0, page_total: 0, page_avail: 0, virt_total: 0, virt_avail: 0, ext: 0 };
        // SAFETY: a zeroed struct with its length set, as the API asks
        (unsafe { GlobalMemoryStatusEx(&mut m) } != 0).then(|| m.total as f64 / (1u64 << 30) as f64)
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// "Copy bug report": version, system, Blender, model library, folders and the last lines of the log, as text.
#[tauri::command]
pub fn app_report(app: tauri::AppHandle) -> String {
    let st = crate::library_status(app.clone());
    let data = crate::data_dir(&app).unwrap_or_default();
    let mut r = format!(
        "MapSmithEE {}{} ({} {}, {} GB RAM)\nBlender: {}\nModel library: {} ({})\nApp data: {}\n",
        env!("CARGO_PKG_VERSION"),
        if crate::portable_dir().is_some() { " portable" } else { "" },
        std::env::consts::OS,
        std::env::consts::ARCH,
        ram_gb().map_or("?".into(), |g| format!("{g:.0}")),
        crate::blender_of(&app).map(|p| p.display().to_string()).unwrap_or_else(|e| format!("not found ({e})")),
        if st.ready { "installed" } else { "not installed" },
        crate::library_dir(&app).display(),
        data.display(),
    );
    r.push_str("\nLast log lines:\n");
    let tail = log_tail(&log_dir(&app), 60);
    if tail.is_empty() {
        r.push_str("(none)\n");
    }
    for l in tail {
        r.push_str(&l);
        r.push('\n');
    }
    r
}

/// Is `b` a newer version than `a`? Plain `x.y.z` numbers, a leading `v` ignored.
pub fn newer(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> { s.trim_start_matches('v').split('.').map(|p| p.parse().unwrap_or(0)).collect() };
    parse(b) > parse(a)
}

/// The newest release on GitHub when it is newer than this one: `{version, url}`, else null. Asked once a day at
/// most by the editor; a failure (offline) is just null.
#[tauri::command]
pub async fn update_check() -> Option<Value> {
    let client = reqwest::Client::builder()
        .user_agent("MapSmithEE")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .ok()?;
    let list: Value = client.get(RELEASES).send().await.ok()?.json().await.ok()?;
    let cur = env!("CARGO_PKG_VERSION");
    list.as_array()?
        .iter()
        .filter(|r| r["draft"] != true)
        .filter_map(|r| Some((r["tag_name"].as_str()?, r["html_url"].as_str()?)))
        .filter(|(t, _)| t.starts_with('v') && newer(cur, t))
        .max_by(|a, b| if newer(a.0, b.0) { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater })
        .map(|(t, u)| json!({ "version": t.trim_start_matches('v'), "url": u }))
}

/// Opens one of the project's pages (tutorial, issues, releases, Discord) in the default browser.
#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !URL_OK.iter().any(|p| url.starts_with(p)) || url.chars().any(|c| c.is_whitespace() || c == '"') {
        return Err(format!("not a MapSmithEE page: {url}"));
    }
    let r = if cfg!(windows) {
        std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", &url]).spawn()
    } else if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(&url).spawn()
    } else {
        std::process::Command::new("xdg-open").arg(&url).spawn()
    };
    r.map(|_| ()).map_err(|e| e.to_string())
}

/// Opens the log folder.
#[tauri::command]
pub fn open_logs(app: tauri::AppHandle) -> Result<String, String> {
    let dir = log_dir(&app);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let prog = if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(prog).arg(&dir).spawn().map_err(|e| e.to_string())?;
    Ok(dir.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(newer("0.2.1", "v0.2.2") && newer("0.2.9", "0.10.0") && newer("0.2", "0.2.1"));
        assert!(!newer("0.2.1", "v0.2.1") && !newer("0.3.0", "0.2.9"));
    }

    #[test]
    fn log_rotates() {
        let d = std::env::temp_dir().join(format!("ms_log_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        log_to(&d, "error", "one\ntwo");
        assert!(log_tail(&d, 5)[0].ends_with("[error] one ⏎ two"));
        std::fs::write(d.join("mapsmithee.log"), vec![b'x'; LOG_MAX as usize + 1]).unwrap();
        log_to(&d, "info", "fresh");
        assert_eq!(log_tail(&d, 5).len(), 1);
        assert!(d.join("mapsmithee.old.log").is_file());
        let _ = std::fs::remove_dir_all(&d);
        assert!(utc_now().len() == 20);
    }
}
