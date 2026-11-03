//! MapSmithEE backend: game reading, area documents, saving to the project folder, installing into a test copy.

pub mod are;
pub mod bmpmap;
pub mod campbuild;
pub mod camp3d;
pub mod render;
pub mod res;
pub mod tis;
pub mod wed;

use res::{Game, Res};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{Manager, State};

#[derive(Default)]
pub struct App {
    game: Mutex<Option<Arc<Game>>>,
    /// full WEDs of the open areas (tile maps are not sent to the UI): area -> (day, night)
    weds: Mutex<HashMap<String, (wed::Wed, Option<wed::Wed>)>>,
    /// pictures served at http://msimg.localhost/<key>
    pics: Arc<Mutex<HashMap<String, Vec<u8>>>>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Settings {
    pub project_dir: String,
    pub recent_games: Vec<String>,
    pub install_targets: Vec<String>,
    /// folders MapSmithEE never writes into
    pub blocked: Vec<String>,
}

fn settings_path(app: &tauri::AppHandle) -> Res<PathBuf> {
    let d = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    Ok(d.join("settings.json"))
}

fn load_settings(app: &tauri::AppHandle) -> Settings {
    let mut s: Settings = settings_path(app)
        .ok()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if s.project_dir.is_empty() {
        let docs = app.path().document_dir().unwrap_or_else(|_| PathBuf::from("."));
        s.project_dir = docs.join("MapSmithEE").to_string_lossy().into_owned();
    }
    if s.blocked.is_empty() {
        s.blocked = vec!["E:/Enhanced Edition/Environnement de test".into(), "steamapps/common".into()];
    }
    s
}

#[tauri::command]
fn settings_get(app: tauri::AppHandle) -> Settings {
    load_settings(&app)
}

#[tauri::command]
fn settings_set(app: tauri::AppHandle, settings: Settings) -> Res<()> {
    let t = serde_json::to_string_pretty(&settings).map_err(|e| e.to_string())?;
    std::fs::write(settings_path(&app)?, t).map_err(|e| e.to_string())
}

fn norm(p: &str) -> String {
    p.replace('\\', "/").trim_end_matches('/').to_lowercase()
}

/// true when `dir` is inside one of the blocked folders
fn is_blocked(s: &Settings, dir: &Path) -> bool {
    let d = norm(&dir.to_string_lossy());
    s.blocked.iter().map(|b| norm(b)).any(|b| !b.is_empty() && (d.starts_with(&b) || d.contains(&format!("/{b}/")) || d.ends_with(&format!("/{b}"))))
}

#[derive(Serialize)]
struct GameInfo {
    root: String,
    areas: Vec<String>,
    /// e.g. "BGEE", "BG2EE", "EET": guessed from the resources
    kind: String,
}

#[tauri::command]
async fn game_open(app: tauri::AppHandle, state: State<'_, App>, root: String) -> Res<GameInfo> {
    let path = PathBuf::from(&root);
    let g = tauri::async_runtime::spawn_blocking(move || Game::open(&path)).await.map_err(|e| e.to_string())??;
    let kind = if g.exists("BG0100", "ARE") && g.exists("AR0100", "ARE") {
        "EET"
    } else if g.exists("AR0602", "ARE") && !g.exists("AR2600", "ARE") {
        "BG2EE"
    } else if g.exists("AR2600", "ARE") {
        "BGEE"
    } else {
        "IE"
    }
    .to_string();
    let info = GameInfo { root: root.clone(), areas: g.names("ARE"), kind };
    *state.game.lock().unwrap() = Some(Arc::new(g));
    let mut s = load_settings(&app);
    s.recent_games.retain(|r| norm(r) != norm(&root));
    s.recent_games.insert(0, root);
    s.recent_games.truncate(8);
    settings_set(app, s)?;
    Ok(info)
}

fn game(state: &App) -> Res<Arc<Game>> {
    state.game.lock().unwrap().clone().ok_or_else(|| "no game open".into())
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Maps {
    pub sr: Option<bmpmap::BmpMap>,
    pub ht: Option<bmpmap::BmpMap>,
    pub lm: Option<bmpmap::BmpMap>,
    pub ln: Option<bmpmap::BmpMap>,
}

#[derive(Serialize)]
struct Pic {
    url: String,
    w: u32,
    h: u32,
    doors: Vec<render::DoorPatch>,
}

#[derive(Serialize)]
struct AreaDoc {
    name: String,
    /// "game" or "project": where the ARE/WED/maps were read from
    source: String,
    are: are::Are,
    wed_day: wed::WedEdit,
    wed_night: Option<wed::WedEdit>,
    /// the night WED had the same walls and doors: edits go to both
    night_linked: bool,
    maps: Maps,
    day: Pic,
    night: Option<Pic>,
}

/// a file of the area: the project folder first, then the game
fn read(g: &Game, project: &Path, name: &str, ext: &str) -> Res<Option<(Vec<u8>, bool)>> {
    let p = project.join(format!("{name}.{ext}"));
    if p.exists() {
        return std::fs::read(&p).map(|b| Some((b, true))).map_err(|e| e.to_string());
    }
    Ok(g.get(name, ext)?.map(|b| (b, false)))
}

fn area_dir(s: &Settings, name: &str) -> PathBuf {
    PathBuf::from(&s.project_dir).join(name.to_ascii_uppercase())
}

#[tauri::command]
async fn area_open(app: tauri::AppHandle, state: State<'_, App>, name: String) -> Res<AreaDoc> {
    let g = game(&state)?;
    let s = load_settings(&app);
    let dir = area_dir(&s, &name);
    let pics = state.pics.clone();
    let n = name.to_ascii_uppercase();
    let (doc, weds) = tauri::async_runtime::spawn_blocking(move || -> Res<(AreaDoc, (wed::Wed, Option<wed::Wed>))> {
        let (ab, from_project) = read(&g, &dir, &n, "ARE")?.ok_or(format!("{n}.ARE not found"))?;
        let a = are::parse(&ab)?;
        let wname = if a.wed.is_empty() { n.clone() } else { a.wed.clone() };
        let (wb, _) = read(&g, &dir, &wname, "WED")?.ok_or(format!("{wname}.WED not found"))?;
        let wd = wed::parse(&wb)?;
        // extended night (0x40): the engine swaps to <wed>N.WED and its own tileset
        let wn = if a.area_type & 0x40 != 0 {
            read(&g, &dir, &format!("{wname}N"), "WED")?.map(|(b, _)| wed::parse(&b)).transpose()?
        } else {
            None
        };
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let put = |key: String, png: Vec<u8>| -> String {
            pics.lock().unwrap().insert(key.clone(), png);
            format!("http://msimg.localhost/{key}?t={stamp}")
        };
        let pic = |w: &wed::Wed, key: &str| -> Res<Pic> {
            let p = render::draw(&g, w, key)?;
            let mut doors = Vec::new();
            for (mut d, png) in p.doors {
                d.img = put(d.img.clone(), png);
                doors.push(d);
            }
            Ok(Pic { url: put(key.to_string(), p.png), w: p.w, h: p.h, doors })
        };
        let day = pic(&wd, &format!("{n}-day"))?;
        let night = wn.as_ref().map(|w| pic(w, &format!("{n}-night"))).transpose()?;
        let bmp = |suffix: &str| -> Option<bmpmap::BmpMap> {
            read(&g, &dir, &format!("{wname}{suffix}"), "BMP").ok().flatten().and_then(|(b, _)| bmpmap::parse(&b).ok())
        };
        let maps = Maps { sr: bmp("SR"), ht: bmp("HT"), lm: bmp("LM"), ln: bmp("LN") };
        let edit = |w: &wed::Wed| wed::WedEdit { walls: w.walls.clone(), doors: w.doors.clone() };
        let wed_night = wn.as_ref().map(edit);
        let night_linked = wed_night.as_ref().is_some_and(|e| *e == edit(&wd));
        let doc = AreaDoc {
            name: n.clone(),
            source: if from_project { "project" } else { "game" }.into(),
            are: a,
            wed_day: edit(&wd),
            wed_night,
            night_linked,
            maps,
            day,
            night,
        };
        Ok((doc, (wd, wn)))
    })
    .await
    .map_err(|e| e.to_string())??;
    state.weds.lock().unwrap().insert(name.to_ascii_uppercase(), weds);
    Ok(doc)
}

#[tauri::command]
fn tlk_string(state: State<'_, App>, strref: u32) -> Res<String> {
    Ok(game(&state)?.string(strref))
}

#[derive(Deserialize)]
struct SaveReq {
    name: String,
    are: are::Are,
    wed_day: wed::WedEdit,
    wed_night: Option<wed::WedEdit>,
    maps: Maps,
}

fn write_atomic(p: &Path, b: &[u8]) -> Res<()> {
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, b).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, p).map_err(|e| format!("{}: {e}", p.display()))
}

/// write the area into <project>/<AREA>/: ARE, WED (+N), and the bitmaps that exist
#[tauri::command]
fn area_save(app: tauri::AppHandle, state: State<'_, App>, req: SaveReq) -> Res<Vec<String>> {
    let s = load_settings(&app);
    let dir = area_dir(&s, &req.name);
    if is_blocked(&s, &dir) {
        return Err(format!("{} is a protected folder", dir.display()));
    }
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let n = req.name.to_ascii_uppercase();
    let weds = state.weds.lock().unwrap();
    let (wd, wn) = weds.get(&n).ok_or("area not open")?;
    let wname = if req.are.wed.is_empty() { n.clone() } else { req.are.wed.clone() };
    let mut files = Vec::new();
    let mut out = |file: String, b: Vec<u8>| -> Res<()> {
        write_atomic(&dir.join(&file), &b)?;
        files.push(file);
        Ok(())
    };
    out(format!("{n}.ARE"), are::write(&req.are))?;
    let mut d = wd.clone();
    d.walls = req.wed_day.walls.clone();
    d.doors = req.wed_day.doors.clone();
    out(format!("{wname}.WED"), wed::write(&d))?;
    if let (Some(w), Some(e)) = (wn, &req.wed_night) {
        let mut w = w.clone();
        w.walls = e.walls.clone();
        w.doors = e.doors.clone();
        out(format!("{wname}N.WED"), wed::write(&w))?;
    }
    for (suffix, m) in [("SR", &req.maps.sr), ("HT", &req.maps.ht), ("LM", &req.maps.lm), ("LN", &req.maps.ln)] {
        if let Some(m) = m {
            out(format!("{wname}{suffix}.BMP"), bmpmap::write(m)?)?;
        }
    }
    Ok(files)
}

/// copy the area's project files into <target>/override. Refuses protected folders and anything without chitin.key.
#[tauri::command]
fn area_install(app: tauri::AppHandle, name: String, target: String) -> Res<Vec<String>> {
    let s = load_settings(&app);
    let root = PathBuf::from(&target);
    if is_blocked(&s, &root) {
        return Err(format!("{target} is a protected folder: install into a test copy"));
    }
    if !root.join("chitin.key").exists() {
        return Err(format!("{target} is not a game folder (no chitin.key)"));
    }
    let src = area_dir(&s, &name);
    let ov = root.join("override");
    std::fs::create_dir_all(&ov).map_err(|e| e.to_string())?;
    let mut done = Vec::new();
    for f in std::fs::read_dir(&src).map_err(|e| format!("nothing saved for {name}: {e}"))?.flatten() {
        let p = f.path();
        if p.extension().is_some_and(|e| e != "tmp") {
            std::fs::copy(&p, ov.join(f.file_name())).map_err(|e| e.to_string())?;
            done.push(f.file_name().to_string_lossy().into_owned());
        }
    }
    let mut s = s;
    s.install_targets.retain(|t| norm(t) != norm(&target));
    s.install_targets.insert(0, target);
    settings_set(app, s)?;
    Ok(done)
}

#[derive(Serialize, Clone)]
struct CampbuildProgress {
    pct: u32,
    step: String,
}

/// build a Camp 3D map into a playable area in `opts.out_dir` (refuses protected folders); progress events
/// "campbuild-progress" {pct, step}
#[tauri::command]
async fn campbuild_run(app: tauri::AppHandle, input: campbuild::BuildInput, opts: campbuild::BuildOptions) -> Res<campbuild::BuildReport> {
    use tauri::Emitter;
    let s = load_settings(&app);
    if is_blocked(&s, &opts.out_dir) {
        return Err(format!("{} is a protected folder", opts.out_dir.display()));
    }
    tauri::async_runtime::spawn_blocking(move || {
        campbuild::build(&input, &opts, &|pct, step| {
            let _ = app.emit("campbuild-progress", CampbuildProgress { pct, step: step.to_string() });
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = App::default();
    let pics = app.pics.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(app)
        .register_uri_scheme_protocol("msimg", move |_ctx, req| {
            let key = req.uri().path().trim_start_matches('/').to_string();
            let key = key.split('?').next().unwrap_or("").to_string();
            match pics.lock().unwrap().get(&key) {
                Some(b) => tauri::http::Response::builder()
                    .header("Content-Type", "image/png")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(b.clone())
                    .unwrap(),
                None => tauri::http::Response::builder().status(404).body(Vec::new()).unwrap(),
            }
        })
        .invoke_handler(tauri::generate_handler![
            settings_get,
            settings_set,
            game_open,
            area_open,
            area_save,
            area_install,
            tlk_string,
            camp3d::camp3d_get,
            camp3d::camp3d_set,
            camp3d::camp3d_update,
            camp3d::camp3d_delete,
            campbuild_run
        ])
        .run(tauri::generate_context!())
        .expect("error while running MapSmithEE");
}
