//! Document store for the Camp Map 3D editor (static/camp3d/, shim.js).
//!
//! The editor was a claude.ai artifact using a Firestore-like database (`db.doc('maps/<id>').get()/.set()`). Here
//! each document is one JSON file: `<app data>/camp3d/<collection>/<doc>.json`, holding the document's data object.
//! On first use, if `maps/_index.json` is missing, the bundled sample map (resource `samples/camp3d/maps`) is copied in.
//!
//! Documents the editor uses: `maps/_index`, `maps/<id>`, `maps/<id>_ground`, `maps/<id>_g<n>`, `maps/<id>_walk`,
//! `models/offsets` (model heights), `requests/preview` (a Blender preview render request, see `preview_request`).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde_json::{Map, Value};
use tauri::Manager;

/// Serialises file writes and the one-time seeding.
static LOCK: Mutex<bool> = Mutex::new(false); // true once seeding was checked
static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// `<app data>/camp3d`, seeded with the sample map on first call.
pub fn root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("internal: no app data dir: {e}"))?
        .join("camp3d");
    let mut seeded = LOCK.lock().map_err(|_| "internal: lock poisoned".to_string())?;
    if !*seeded {
        let res = app
            .path()
            .resolve("samples/camp3d/maps", tauri::path::BaseDirectory::Resource)
            .ok()
            .filter(|p| p.is_dir());
        let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples/camp3d/maps");
        let sample = res.unwrap_or(dev);
        seed(&dir, &sample)?;
        *seeded = true;
    }
    Ok(dir)
}

/// Copy the sample `*.json` docs into `<root>/maps/` when `maps/_index.json` is missing. Never overwrites a file.
pub fn seed(root: &Path, sample: &Path) -> Result<(), String> {
    let maps = root.join("maps");
    if maps.join("_index.json").exists() || !sample.is_dir() {
        return Ok(());
    }
    std::fs::create_dir_all(&maps).map_err(|e| format!("io: {e}"))?;
    let rd = std::fs::read_dir(sample).map_err(|e| format!("io: {}: {e}", sample.display()))?;
    for e in rd.flatten() {
        let p = e.path();
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
        if !name.ends_with(".json") || !p.is_file() || maps.join(name).exists() {
            continue;
        }
        // _index last: it marks the seed as complete
        if name == "_index.json" {
            continue;
        }
        let bytes = std::fs::read(&p).map_err(|e| format!("io: {e}"))?;
        write_atomic(&maps.join(name), &bytes)?;
    }
    let idx = sample.join("_index.json");
    if idx.is_file() {
        let bytes = std::fs::read(&idx).map_err(|e| format!("io: {e}"))?;
        write_atomic(&maps.join("_index.json"), &bytes)?;
    }
    Ok(())
}

fn valid_segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && !s.starts_with('.')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'))
}

/// `"maps/claude_camp"` -> `<root>/maps/claude_camp.json`. Exactly `collection/doc`, safe characters only.
pub fn doc_file(root: &Path, path: &str) -> Result<PathBuf, String> {
    let mut it = path.split('/');
    match (it.next(), it.next(), it.next()) {
        (Some(c), Some(d), None) if valid_segment(c) && valid_segment(d) => {
            Ok(root.join(c).join(format!("{d}.json")))
        }
        _ => Err(format!("invalid_argument: bad document path '{path}'")),
    }
}

fn write_atomic(file: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = file.parent().ok_or("internal: no parent dir")?;
    std::fs::create_dir_all(dir).map_err(|e| format!("io: {e}"))?;
    let tmp = dir.join(format!(
        ".{}.{}.{}.tmp",
        file.file_name().and_then(|n| n.to_str()).unwrap_or("doc"),
        std::process::id(),
        TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::write(&tmp, bytes).map_err(|e| format!("io: {e}"))?;
    std::fs::rename(&tmp, file).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("io: {e}")
    })
}

pub fn get_doc(root: &Path, path: &str) -> Result<Option<Value>, String> {
    let f = doc_file(root, path)?;
    match std::fs::read(&f) {
        Ok(b) => serde_json::from_slice(&b)
            .map(Some)
            .map_err(|e| format!("data_loss: {}: {e}", f.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("io: {e}")),
    }
}

pub fn set_doc(root: &Path, path: &str, data: Value) -> Result<(), String> {
    let f = doc_file(root, path)?;
    if !data.is_object() {
        return Err("invalid_argument: document data must be an object".into());
    }
    let bytes = serde_json::to_vec(&data).map_err(|e| format!("internal: {e}"))?;
    let _g = LOCK.lock().map_err(|_| "internal: lock poisoned".to_string())?;
    write_atomic(&f, &bytes)
}

fn is_delete(v: &Value) -> bool {
    v.as_object()
        .is_some_and(|o| o.len() == 1 && o.get("__delete__") == Some(&Value::Bool(true)))
}

/// Shallow merge into an existing document (Firestore `update`: fails when the document does not exist).
/// A value `{"__delete__": true}` removes that key.
pub fn update_doc(root: &Path, path: &str, data: Value) -> Result<(), String> {
    let f = doc_file(root, path)?;
    let Value::Object(patch) = data else {
        return Err("invalid_argument: update data must be an object".into());
    };
    let _g = LOCK.lock().map_err(|_| "internal: lock poisoned".to_string())?;
    let mut cur: Map<String, Value> = match std::fs::read(&f) {
        Ok(b) => match serde_json::from_slice(&b) {
            Ok(Value::Object(o)) => o,
            Ok(_) => Map::new(),
            Err(e) => return Err(format!("data_loss: {}: {e}", f.display())),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("not_found: no document '{path}'"));
        }
        Err(e) => return Err(format!("io: {e}")),
    };
    for (k, v) in patch {
        if is_delete(&v) {
            cur.remove(&k);
        } else {
            cur.insert(k, v);
        }
    }
    let bytes = serde_json::to_vec(&Value::Object(cur)).map_err(|e| format!("internal: {e}"))?;
    write_atomic(&f, &bytes)
}

pub fn delete_doc(root: &Path, path: &str) -> Result<(), String> {
    let f = doc_file(root, path)?;
    let _g = LOCK.lock().map_err(|_| "internal: lock poisoned".to_string())?;
    match std::fs::remove_file(&f) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("io: {e}")),
    }
}

/// Where the editor's "Preview render" request lands: `<app data>/camp3d/requests/preview.json`,
/// `{kind:"preview", time, view:{x,y,z,zoom}|null, asked, done:false}`. The map it refers to is the one last saved
/// (`maps/_index` + the editor's current map doc). A render tool sets `done: true` when it has answered.
pub fn preview_request(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(root(app)?.join("requests").join("preview.json"))
}

#[tauri::command]
pub async fn camp3d_get(app: tauri::AppHandle, path: String) -> Result<Option<Value>, String> {
    get_doc(&root(&app)?, &path)
}

#[tauri::command]
pub async fn camp3d_set(app: tauri::AppHandle, path: String, data: Value) -> Result<(), String> {
    set_doc(&root(&app)?, &path, data)
}

#[tauri::command]
pub async fn camp3d_update(app: tauri::AppHandle, path: String, data: Value) -> Result<(), String> {
    update_doc(&root(&app)?, &path, data)
}

#[tauri::command]
pub async fn camp3d_delete(app: tauri::AppHandle, path: String) -> Result<(), String> {
    delete_doc(&root(&app)?, &path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("camp3d_test_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn paths() {
        let r = Path::new("R");
        assert_eq!(doc_file(r, "maps/claude_camp").unwrap(), r.join("maps").join("claude_camp.json"));
        assert!(doc_file(r, "maps/_index").is_ok());
        assert!(doc_file(r, "maps/a.b-c_1").is_ok());
        for bad in ["maps", "maps/", "/x", "a/b/c", "../x", "maps/..", "maps/.hidden", "maps/a b", "maps\\x", "m/x:y", ""] {
            assert!(doc_file(r, bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn crud() {
        let root = tmp("crud");
        assert_eq!(get_doc(&root, "maps/x").unwrap(), None);
        assert!(update_doc(&root, "maps/x", json!({"a": 1})).unwrap_err().starts_with("not_found"));
        set_doc(&root, "maps/x", json!({"a": 1, "b": [1, 2], "c": {"d": 1}})).unwrap();
        assert_eq!(get_doc(&root, "maps/x").unwrap(), Some(json!({"a": 1, "b": [1, 2], "c": {"d": 1}})));
        update_doc(&root, "maps/x", json!({"a": 2, "b": {"__delete__": true}, "c": {"e": 2}})).unwrap();
        assert_eq!(get_doc(&root, "maps/x").unwrap(), Some(json!({"a": 2, "c": {"e": 2}})));
        set_doc(&root, "maps/x", json!({"z": true})).unwrap();
        assert_eq!(get_doc(&root, "maps/x").unwrap(), Some(json!({"z": true})));
        assert!(set_doc(&root, "maps/x", json!([1])).is_err());
        delete_doc(&root, "maps/x").unwrap();
        delete_doc(&root, "maps/x").unwrap();
        assert_eq!(get_doc(&root, "maps/x").unwrap(), None);
        // no temp files left behind
        let left: Vec<_> = std::fs::read_dir(root.join("maps")).unwrap().flatten().collect();
        assert!(left.is_empty(), "{left:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn seeding() {
        let root = tmp("seed");
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../samples/camp3d/maps");
        seed(&root, &sample).unwrap();
        let idx = get_doc(&root, "maps/_index").unwrap().unwrap();
        assert_eq!(idx["maps"][0]["id"], "claude_camp");
        let map = get_doc(&root, "maps/claude_camp").unwrap().unwrap();
        assert!(map["items"].as_array().unwrap().len() > 1000);
        assert!(get_doc(&root, "maps/claude_camp_g28").unwrap().is_some());
        // a second seed never overwrites
        set_doc(&root, "maps/claude_camp", json!({"edited": 1})).unwrap();
        seed(&root, &sample).unwrap();
        assert_eq!(get_doc(&root, "maps/claude_camp").unwrap(), Some(json!({"edited": 1})));
        let _ = std::fs::remove_dir_all(&root);
    }
}
