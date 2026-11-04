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
    let dir = crate::data_dir(app)?.join("camp3d");
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
    backup(root, &f);
    write_atomic(&f, &bytes)
}

/// Documents already backed up since the app started.
static BACKED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());
const BACKUPS_KEPT: usize = 5;

/// The first time a map document is written in a run, the version on disk goes to
/// `<root>/backup/<doc>.<unix seconds>.json` first; the 5 newest of each document are kept.
/// Best effort: a failed backup never stops the save.
fn backup(root: &Path, f: &Path) {
    let Ok(mut done) = BACKED.lock() else { return };
    if done.iter().any(|p| p == f) || !f.is_file() || f.parent().and_then(|p| p.file_name()) != Some("maps".as_ref()) {
        return;
    }
    done.push(f.to_path_buf());
    let Some(stem) = f.file_stem().and_then(|s| s.to_str()) else { return };
    if stem.starts_with('_') {
        return;
    }
    let dir = root.join("backup");
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if std::fs::create_dir_all(&dir).is_err() || std::fs::copy(f, dir.join(format!("{stem}.{secs}.json"))).is_err() {
        return;
    }
    let mut old: Vec<PathBuf> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_stem().and_then(|s| s.to_str()).and_then(|s| s.rsplit_once('.')).is_some_and(|(d, t)| d == stem && t.bytes().all(|b| b.is_ascii_digit()))
        })
        .collect();
    old.sort();
    for p in old.iter().rev().skip(BACKUPS_KEPT) {
        let _ = std::fs::remove_file(p);
    }
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

/// The part of a document name after the map id, when the document belongs to that map: `""` (the map),
/// `"_ground"`, `"_walk"` or `"_g<n>"` (one ground layer).
fn map_part<'a>(stem: &'a str, id: &str) -> Option<&'a str> {
    let rest = stem.strip_prefix(id)?;
    let layer = rest.strip_prefix("_g").is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    (rest.is_empty() || rest == "_ground" || rest == "_walk" || layer).then_some(rest)
}

/// "Save to file…": every document of one map in one JSON file (`.mapsmith`),
/// `{format:"mapsmithee-map", version:1, id, name, tiles, docs:{"": map, "_ground": …, "_g0": …, "_walk": …}}`.
pub fn export_map(root: &Path, id: &str, file: &Path) -> Result<(), String> {
    if !valid_segment(id) || id.starts_with('_') {
        return Err(format!("invalid_argument: bad map id '{id}'"));
    }
    let mut docs = Map::new();
    let rd = std::fs::read_dir(root.join("maps")).map_err(|e| format!("io: {e}"))?;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".json") else { continue };
        let Some(part) = map_part(stem, id) else { continue };
        if let Some(v) = get_doc(root, &format!("maps/{stem}"))? {
            docs.insert(part.to_string(), v);
        }
    }
    let Some(main) = docs.get("") else { return Err(format!("not_found: map '{id}' has nothing saved")) };
    let out = serde_json::json!({
        "format": "mapsmithee-map",
        "version": 1,
        "app": env!("CARGO_PKG_VERSION"),
        "id": id,
        "name": main.get("name").cloned().unwrap_or(Value::String(id.into())),
        "tiles": main.get("tiles").cloned().unwrap_or(Value::Null),
        "docs": docs,
    });
    let bytes = serde_json::to_vec(&out).map_err(|e| format!("internal: {e}"))?;
    std::fs::write(file, bytes).map_err(|e| format!("io: {}: {e}", file.display()))
}

/// "Open file…": a `.mapsmith` file as a new map with its own id (`new_id`); returns its `_index` entry
/// `{id, name, tiles}` for the editor to add.
pub fn import_map(root: &Path, file: &Path, new_id: &str) -> Result<Value, String> {
    let b = std::fs::read(file).map_err(|e| format!("io: {}: {e}", file.display()))?;
    let v: Value = serde_json::from_slice(&b).map_err(|_| format!("{} is not a MapSmithEE map file", file.display()))?;
    if v.get("format").and_then(Value::as_str) != Some("mapsmithee-map") {
        return Err(format!("{} is not a MapSmithEE map file", file.display()));
    }
    let Some(docs) = v.get("docs").and_then(Value::as_object).filter(|d| d.get("").is_some_and(Value::is_object)) else {
        return Err("this map file holds no map".into());
    };
    for (part, data) in docs {
        if map_part(&format!("x{part}"), "x") != Some(part.as_str()) || !data.is_object() {
            continue;
        }
        set_doc(root, &format!("maps/{new_id}{part}"), data.clone())?;
    }
    let main = &docs[""];
    let name = main.get("name").or(v.get("name")).and_then(Value::as_str).unwrap_or("Imported map");
    let tiles = main.get("tiles").or(v.get("tiles")).cloned().unwrap_or(serde_json::json!([40, 30]));
    Ok(serde_json::json!({ "id": new_id, "name": name, "tiles": tiles }))
}

#[tauri::command]
pub async fn camp3d_map_export(app: tauri::AppHandle, map_id: String, path: String) -> Result<(), String> {
    export_map(&root(&app)?, &map_id, Path::new(&path))
}

/// "Delete this map…": every document of the map (its backups stay in `<root>/backup`).
pub fn delete_map(root: &Path, id: &str) -> Result<(), String> {
    if !valid_segment(id) || id.starts_with('_') {
        return Err(format!("invalid_argument: bad map id '{id}'"));
    }
    let rd = std::fs::read_dir(root.join("maps")).map_err(|e| format!("io: {e}"))?;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if let Some(stem) = name.strip_suffix(".json").filter(|s| map_part(s, id).is_some()) {
            delete_doc(root, &format!("maps/{stem}"))?;
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn camp3d_map_delete(app: tauri::AppHandle, map_id: String) -> Result<(), String> {
    delete_map(&root(&app)?, &map_id)
}

#[tauri::command]
pub async fn camp3d_map_import(app: tauri::AppHandle, path: String) -> Result<Value, String> {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let mut n = ms as u64;
    let mut id = String::new();
    while n > 0 {
        let d = (n % 36) as u32;
        id.insert(0, char::from_digit(d, 36).unwrap_or('0'));
        n /= 36;
    }
    import_map(&root(&app)?, Path::new(&path), &format!("m{id}"))
}

/// Opens the folder where the maps are kept (`<app data>/camp3d/maps`) in the file manager.
#[tauri::command]
pub async fn camp3d_maps_folder(app: tauri::AppHandle) -> Result<String, String> {
    let dir = root(&app)?.join("maps");
    std::fs::create_dir_all(&dir).map_err(|e| format!("io: {e}"))?;
    let prog = if cfg!(windows) { "explorer" } else if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(prog).arg(&dir).spawn().map_err(|e| format!("{prog}: {e}"))?;
    Ok(dir.display().to_string())
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
    fn map_file() {
        let root = tmp("mapfile");
        for (d, v) in [("a", json!({"name": "A", "tiles": [10, 8]})), ("a_ground", json!({"split": true})), ("a_g3", json!({"mask": "x"})),
                       ("a_walk", json!({"fix": "y"})), ("a_b", json!({"name": "other"})), ("a_gx", json!({})), ("_index", json!({"maps": []}))] {
            set_doc(&root, &format!("maps/{d}"), v).unwrap();
        }
        let f = root.join("a.mapsmith");
        export_map(&root, "a", &f).unwrap();
        let v: Value = serde_json::from_slice(&std::fs::read(&f).unwrap()).unwrap();
        let mut parts: Vec<_> = v["docs"].as_object().unwrap().keys().cloned().collect();
        parts.sort();
        assert_eq!(parts, ["", "_g3", "_ground", "_walk"]);
        assert_eq!(import_map(&root, &f, "m2").unwrap(), json!({"id": "m2", "name": "A", "tiles": [10, 8]}));
        assert_eq!(get_doc(&root, "maps/m2_g3").unwrap(), Some(json!({"mask": "x"})));
        assert_eq!(get_doc(&root, "maps/m2_walk").unwrap(), Some(json!({"fix": "y"})));
        assert!(export_map(&root, "zz", &f).is_err());
        delete_map(&root, "a").unwrap();
        assert_eq!(get_doc(&root, "maps/a").unwrap(), None);
        assert_eq!(get_doc(&root, "maps/a_g3").unwrap(), None);
        assert!(get_doc(&root, "maps/a_b").unwrap().is_some() && get_doc(&root, "maps/m2").unwrap().is_some());
        std::fs::write(&f, b"{}").unwrap();
        assert!(import_map(&root, &f, "m3").is_err());
        // a second write of a document backs up the first version, once per run
        set_doc(&root, "maps/m2", json!({"v": 2})).unwrap();
        set_doc(&root, "maps/m2", json!({"v": 3})).unwrap();
        let bk: Vec<_> = std::fs::read_dir(root.join("backup")).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("m2.")).collect();
        assert_eq!(bk.len(), 1, "{bk:?}");
        let _ = std::fs::remove_dir_all(&root);
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
