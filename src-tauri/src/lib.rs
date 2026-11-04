//! MapSmithEE backend: game reading, area documents, saving to the project folder, installing into a test copy.

pub mod are;
pub mod bmpmap;
pub mod campbuild;
pub mod camp3d;
pub mod library;
pub mod camprender;
pub mod render;
pub mod res;
pub mod support;
pub mod tis;
pub mod userlib;
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
    /// the full model library for renders; empty = <app data>/library
    #[serde(default)]
    pub library_dir: String,
    /// blender.exe; empty = found automatically
    #[serde(default)]
    pub blender_exe: String,
}

/// where the render library lives: the setting, else <app data>/library (a dev build also falls back on the
/// author's working copy when it exists)
pub fn library_dir(app: &tauri::AppHandle) -> PathBuf {
    let s = load_settings(app);
    if !s.library_dir.is_empty() {
        return PathBuf::from(s.library_dir);
    }
    let own = app.path().app_data_dir().unwrap_or_default().join("library");
    let dev = PathBuf::from("E:/AI/bg3d");
    if cfg!(debug_assertions) && !library::is_library(&own) && library::is_library(&dev) { dev } else { own }
}

/// the release manifest of the model library
const LIBRARY_MANIFEST: &str = "https://github.com/sad0/MapSmithEE/releases/download/library-v1/library-manifest.json";
static LIBRARY_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
fn library_status(app: tauri::AppHandle) -> library::Status {
    library::status(&library_dir(&app))
}

#[derive(Serialize, Clone)]
struct LibraryProgress {
    done: u64,
    total: u64,
    step: String,
}

#[tauri::command]
async fn library_install(app: tauri::AppHandle) -> Res<library::Status> {
    use tauri::Emitter;
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?.join("library");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    LIBRARY_CANCEL.store(false, std::sync::atomic::Ordering::Relaxed);
    let a2 = app.clone();
    let progress = move |done: u64, total: u64, step: &str| {
        a2.emit("library-progress", LibraryProgress { done, total, step: step.to_string() }).ok();
    };
    let st = library::install(LIBRARY_MANIFEST, &dir, &progress, &LIBRARY_CANCEL).await?;
    let mut s = load_settings(&app);
    s.library_dir = dir.to_string_lossy().into_owned();
    settings_set(app, s)?;
    Ok(st)
}

#[tauri::command]
fn library_cancel() {
    LIBRARY_CANCEL.store(true, std::sync::atomic::Ordering::Relaxed);
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
        s.blocked = vec!["steamapps/common".into()];
        if cfg!(debug_assertions) {
            s.blocked.insert(0, "E:/Enhanced Edition/Environnement de test".into());
        }
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

#[derive(Deserialize)]
struct RenderAreaReq {
    map_id: String,
    /// 6 characters, e.g. AR9900
    name: String,
    night: bool,
    samples: u32,
    /// "generic" (a self-contained area named `name`) or "sadoqol" (SadoQoL's camp SQCAMP, from its template)
    #[serde(default)]
    target: String,
    /// 1 (the game's resolution), 2 or 4: HD tiles for IEX next to the 1x area
    #[serde(default)]
    scale: u32,
}

/// SadoQoL's camp files the SQCAMP build starts from (read only)
const SADOQOL_CAMP_TEMPLATE: &str = "E:/Enhanced Edition/Environnement de test/EET 2.7.3 (dev)/SadoQoL/camp";

/// the "SadoQoL camp" render target needs the SadoQoL sources' camp folder (read only); it is offered only where it exists
#[tauri::command]
fn camp3d_render_targets() -> Vec<&'static str> {
    let mut t = vec!["generic"];
    if Path::new(SADOQOL_CAMP_TEMPLATE).join("SQCAMP.ARE").is_file() {
        t.push("sadoqol");
    }
    t
}

#[derive(Serialize)]
struct RenderAreaReport {
    out_dir: String,
    files: Vec<String>,
    console: Option<String>,
    /// HD tiles folder in out_dir (scale > 1)
    hd_dir: Option<String>,
    walls: usize,
    seconds: u64,
}

#[derive(Serialize, Clone)]
struct RenderAreaProgress {
    pct: u32,
    step: String,
}

/// "Render area" in the Camp 3D page: the day picture with its coverage and id passes, the night one, then the area
/// build, into <project>/<NAME>. One event stream "render-progress" covers the three steps.
#[tauri::command]
async fn camp3d_render_area(app: tauri::AppHandle, req: RenderAreaReq) -> Res<RenderAreaReport> {
    use tauri::Emitter;
    let sadoqol = req.target == "sadoqol";
    let name = if sadoqol { "SQCAMP".to_string() } else { req.name.trim().to_ascii_uppercase() };
    if name.len() != 6 || !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_') {
        return Err("the area name needs exactly 6 letters or digits".into());
    }
    let s = load_settings(&app);
    let out_dir = PathBuf::from(&s.project_dir).join(&name);
    if is_blocked(&s, &out_dir) {
        return Err(format!("{} is a protected folder", out_dir.display()));
    }
    let lib = library_dir(&app);
    if !library::is_library(&lib) {
        return Err("the model library is not downloaded yet".into());
    }
    let docs = camp3d::root(&app)?;
    let read = |doc: &str| -> Res<serde_json::Value> {
        let p = docs.join("maps").join(format!("{doc}.json"));
        let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        serde_json::from_str(&t).map_err(|e| e.to_string())
    };
    let map_doc = read(&req.map_id)?;
    let ground_doc = read(&format!("{}_ground", req.map_id))?;
    let layers = ground_doc.get("layers").and_then(|l| l.as_array()).map(|l| l.len()).unwrap_or(0);
    let ground_masks: Vec<serde_json::Value> = (0..layers).map(|i| read(&format!("{}_g{i}", req.map_id)).unwrap_or(serde_json::Value::Null)).collect();
    let offsets = std::fs::read_to_string(docs.join("models").join("offsets.json")).ok().and_then(|t| serde_json::from_str(&t).ok());
    let work = app.path().app_data_dir().map_err(|e| e.to_string())?.join("render").join(&req.map_id);
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let blender = if s.blender_exe.is_empty() { None } else { Some(PathBuf::from(&s.blender_exe)) };
    let scripts = camprender::scripts_dir(&app);
    let cache = app.path().app_data_dir().map_err(|e| e.to_string())?.join("render").join("model-cache");
    let (night, samples, map_id) = (req.night, req.samples.clamp(4, 1024), req.map_id.clone());
    let scale = match req.scale {
        2 | 4 => req.scale,
        _ => 1,
    };
    let started = std::time::Instant::now();
    camprender::CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    tauri::async_runtime::spawn_blocking(move || -> Res<RenderAreaReport> {
        let emit = |pct: u32, step: String| {
            let _ = app.emit("render-progress", RenderAreaProgress { pct, step });
        };
        // one Blender session renders the day picture with its passes and the night one: the models load once, and
        // from the cache of earlier renders when they are there
        let render_end = 90;
        let r = camprender::RenderRequest {
            map_doc: map_doc.clone(),
            ground_doc: ground_doc.clone(),
            ground_masks: ground_masks.clone(),
            offsets: offsets.clone(),
            out_png: work.join("day.png"),
            samples,
            hour: Some(14.0),
            id_pass: true,
            crop: None,
            blender_exe: blender.clone(),
            library_dir: lib.clone(),
            work_dir: work.join("prep"),
            also: if night { vec![camprender::AlsoRender { out_png: work.join("night.png"), hour: 22.0 }] } else { vec![] },
            cache_dir: Some(cache.clone()),
            scripts_dir: Some(scripts.clone()),
            scale,
            export: None,
            user_lib: Some(userlib_dir(&app)?),
        };
        camprender::render(&r, &|p, step| emit(p * render_end / 100, format!("Render: {step}")), &camprender::CANCEL)?;
        let last = LastRender { name, target: req.target.clone(), night, scale: scale as usize };
        let rep = build_area(&docs, &work, &lib, &map_id, &last, &out_dir, &|p, step| emit(render_end + p * (100 - render_end) / 100, step))?;
        emit(100, "Done".into());
        Ok(RenderAreaReport { out_dir: out_dir.to_string_lossy().into_owned(), files: rep.files, console: rep.console, hd_dir: rep.hd_dir, walls: rep.walls, seconds: started.elapsed().as_secs() })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// what the last render of a map made (kept next to its pictures): the Walls tool rebuilds the area from them
#[derive(Serialize, serde::Deserialize, Clone)]
struct LastRender {
    name: String,
    target: String,
    night: bool,
    /// the pictures' scale (HD tiles); 1 for renders made before it existed
    #[serde(default = "one")]
    scale: usize,
}

fn one() -> usize {
    1
}

/// the area from the render pictures in `work` (day.png + passes, night.png), and the walls file for the editor
fn build_area(
    docs: &Path,
    work: &Path,
    lib: &Path,
    map_id: &str,
    last: &LastRender,
    out_dir: &Path,
    progress: &(dyn Fn(u32, String) + Sync),
) -> Res<campbuild::BuildReport> {
    let input = campbuild::BuildInput {
        map: docs.join("maps").join(format!("{map_id}.json")),
        walk: Some(docs.join("maps").join(format!("{map_id}_walk.json"))).filter(|p| p.exists()),
        render: work.join("day.png"),
        alpha: Some(work.join("day_alpha.png")),
        id: Some(work.join("day_id.png")),
        night: last.night.then(|| work.join("night.png")),
        models: campbuild::Models::Dirs(vec![lib.join("proxies"), userlib_dir_of(docs).join("proxies")]),
    };
    let mode = if last.target == "sadoqol" {
        campbuild::Mode::SadoQolCamp { template_dir: PathBuf::from(SADOQOL_CAMP_TEMPLATE) }
    } else {
        campbuild::Mode::Generic
    };
    let opts = campbuild::BuildOptions { name: last.name.clone(), mode, out_dir: out_dir.to_path_buf(), scale: last.scale };
    let rep = campbuild::build(&input, &opts, &|p, step| progress(p, format!("Area: {step}")))?;
    let walls = serde_json::json!({ "last": last, "auto": rep.auto_walls, "made": rep.walls });
    std::fs::write(work.join("walls.json"), walls.to_string()).map_err(|e| e.to_string())?;
    Ok(rep)
}

/// the Walls tool: each piece's traced walls from the map's last render (null before any render)
#[tauri::command]
fn camp3d_walls(app: tauri::AppHandle, map_id: String) -> Res<Option<serde_json::Value>> {
    let p = app.path().app_data_dir().map_err(|e| e.to_string())?.join("render").join(&map_id).join("walls.json");
    match std::fs::read_to_string(&p) {
        Ok(t) => serde_json::from_str(&t).map(Some).map_err(|e| e.to_string()),
        Err(_) => Ok(None),
    }
}

/// the Walls tool's "Rebuild area": the last render's pictures, the map's current walls and spots, no Blender (seconds)
#[tauri::command]
async fn camp3d_rebuild_area(app: tauri::AppHandle, map_id: String) -> Res<RenderAreaReport> {
    let work = app.path().app_data_dir().map_err(|e| e.to_string())?.join("render").join(&map_id);
    let t = std::fs::read_to_string(work.join("walls.json")).map_err(|_| "render this map once first".to_string())?;
    let v: serde_json::Value = serde_json::from_str(&t).map_err(|e| e.to_string())?;
    let last: LastRender = serde_json::from_value(v["last"].clone()).map_err(|e| e.to_string())?;
    let s = load_settings(&app);
    let out_dir = PathBuf::from(&s.project_dir).join(&last.name);
    if is_blocked(&s, &out_dir) {
        return Err(format!("{} is a protected folder", out_dir.display()));
    }
    let lib = library_dir(&app);
    let docs = camp3d::root(&app)?;
    let started = std::time::Instant::now();
    tauri::async_runtime::spawn_blocking(move || -> Res<RenderAreaReport> {
        let rep = build_area(&docs, &work, &lib, &map_id, &last, &out_dir, &|_, _| {})?;
        Ok(RenderAreaReport { out_dir: out_dir.to_string_lossy().into_owned(), files: rep.files, console: rep.console, hd_dir: rep.hd_dir, walls: rep.walls, seconds: started.elapsed().as_secs() })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn camp3d_render_cancel() {
    camprender::CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// the user's own pieces and ground textures ("Import…"), next to the camp3d documents: <app data>/userlib
fn userlib_dir_of(docs: &Path) -> PathBuf {
    docs.parent().unwrap_or(docs).join("userlib")
}

fn userlib_dir(app: &tauri::AppHandle) -> Res<PathBuf> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("userlib"))
}

fn blender_of(app: &tauri::AppHandle) -> Res<PathBuf> {
    let s = load_settings(app);
    let b = if s.blender_exe.is_empty() { camprender::find_blender() } else { Some(PathBuf::from(&s.blender_exe)) };
    b.filter(|p| p.is_file()).ok_or_else(|| "Blender not found: set its path in the settings".into())
}

/// the Blender renders use: the one picked in the Render dialog, else the newest found in the usual places
#[tauri::command]
fn blender_get(app: tauri::AppHandle) -> serde_json::Value {
    let s = load_settings(&app);
    let manual = !s.blender_exe.is_empty();
    let path = blender_of(&app).ok().map(|p| p.to_string_lossy().into_owned());
    serde_json::json!({ "path": path, "manual": manual, "set": s.blender_exe })
}

/// picks blender.exe by hand (a portable Blender, another drive); empty = back to automatic
#[tauri::command]
fn blender_set(app: tauri::AppHandle, path: String) -> Res<()> {
    let p = PathBuf::from(&path);
    if !path.is_empty() {
        let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if !p.is_file() || !name.starts_with("blender") {
            return Err(format!("{path} is not blender.exe"));
        }
    }
    let mut s = load_settings(&app);
    s.blender_exe = path;
    settings_set(app, s)
}

/// the editor's start: the user's pieces (editor entries) and ground textures
#[tauri::command]
fn userlib_list(app: tauri::AppHandle) -> Res<serde_json::Value> {
    Ok(userlib::list(&userlib_dir(&app)?))
}

#[derive(serde::Deserialize)]
struct ImportReq {
    /// "model" (any format Blender reads), "sprite" (PNG with an optional _shadow sibling, or BAM) or "texture"
    kind: String,
    path: String,
    name: String,
    /// the piece's type (models.json label: "bush", "barrel"...), which sets its tray category
    #[serde(default)]
    label: String,
    /// a model's height in metres (else its own size)
    #[serde(default)]
    height_m: Option<f64>,
    /// a BAM's cycle and frame
    #[serde(default)]
    cycle: usize,
    #[serde(default)]
    frame: usize,
}

#[tauri::command]
async fn userlib_import(app: tauri::AppHandle, req: ImportReq) -> Res<serde_json::Value> {
    let dir = userlib_dir(&app)?;
    let src = PathBuf::from(&req.path);
    if !src.is_file() {
        return Err(format!("{} not found", src.display()));
    }
    let name = if req.name.trim().is_empty() { src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default() } else { req.name.clone() };
    let label = if req.label.trim().is_empty() { "statue".to_string() } else { req.label.trim().to_string() };
    let (blender, scripts) = if req.kind == "model" { (Some(blender_of(&app)?), camprender::scripts_dir(&app)) } else { (None, PathBuf::new()) };
    tauri::async_runtime::spawn_blocking(move || match req.kind.as_str() {
        "texture" => userlib::import_texture(&dir, &src, &name),
        "sprite" => userlib::import_sprite(&dir, &src, &name, &label, req.cycle, req.frame),
        "model" => userlib::import_model(&dir, blender.as_deref().unwrap(), &scripts, &src, &name, &label, req.height_m),
        k => Err(format!("unknown import kind {k}")),
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
fn userlib_delete(app: tauri::AppHandle, id: String) -> Res<()> {
    userlib::delete(&userlib_dir(&app)?, &id)
}

#[derive(serde::Deserialize)]
struct ExportReq {
    map_id: String,
    /// "blend", "glb" or "fbx"
    format: String,
    path: String,
    #[serde(default)]
    night: bool,
}

/// "Export scene…": the scene Blender would render (camera, sun and fires of the hour, ground, pieces), saved for
/// another 3D program; progress on "render-progress" like a render
#[tauri::command]
async fn camp3d_export_scene(app: tauri::AppHandle, req: ExportReq) -> Res<String> {
    use tauri::Emitter;
    if !["blend", "glb", "fbx"].contains(&req.format.as_str()) {
        return Err(format!("unknown format {}", req.format));
    }
    let lib = library_dir(&app);
    if !library::is_library(&lib) {
        return Err("the model library is not downloaded yet".into());
    }
    let docs = camp3d::root(&app)?;
    let read = |doc: &str| -> Res<serde_json::Value> {
        let p = docs.join("maps").join(format!("{doc}.json"));
        let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
        serde_json::from_str(&t).map_err(|e| e.to_string())
    };
    let map_doc = read(&req.map_id)?;
    let ground_doc = read(&format!("{}_ground", req.map_id))?;
    let layers = ground_doc.get("layers").and_then(|l| l.as_array()).map(|l| l.len()).unwrap_or(0);
    let ground_masks = (0..layers).map(|i| read(&format!("{}_g{i}", req.map_id)).unwrap_or(serde_json::Value::Null)).collect();
    let offsets = std::fs::read_to_string(docs.join("models").join("offsets.json")).ok().and_then(|t| serde_json::from_str(&t).ok());
    let data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let work = data.join("render").join(&req.map_id).join("export");
    let s = load_settings(&app);
    let r = camprender::RenderRequest {
        map_doc,
        ground_doc,
        ground_masks,
        offsets,
        out_png: work.join("unused.png"),
        samples: 128,
        hour: Some(if req.night { 22.0 } else { 14.0 }),
        id_pass: false,
        crop: None,
        blender_exe: if s.blender_exe.is_empty() { None } else { Some(PathBuf::from(&s.blender_exe)) },
        library_dir: lib,
        work_dir: work.join("prep"),
        also: vec![],
        cache_dir: Some(data.join("render").join("model-cache")),
        scripts_dir: Some(camprender::scripts_dir(&app)),
        scale: 1,
        export: Some(serde_json::json!({ "path": req.path.replace('\\', "/"), "format": req.format })),
        user_lib: Some(data.join("userlib")),
    };
    camprender::CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    let path = req.path.clone();
    tauri::async_runtime::spawn_blocking(move || {
        camprender::render(&r, &|p, step| {
            let _ = app.emit("render-progress", RenderAreaProgress { pct: p, step: format!("Export: {step}") });
        }, &camprender::CANCEL)
        .map(|_| path)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = App::default();
    let pics = app.pics.clone();
    // the user's own pieces and textures for the editor (http://msuser.localhost/<path> on Windows)
    let user_dir2: std::sync::Mutex<Option<PathBuf>> = Default::default();
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
        .register_uri_scheme_protocol("msuser", move |ctx, req| {
            let mut d = user_dir2.lock().unwrap();
            if d.is_none() {
                *d = ctx.app_handle().path().app_data_dir().ok().map(|p| p.join("userlib"));
            }
            let path = req.uri().path().to_string();
            match d.as_ref().and_then(|d| userlib::serve(d, &path)) {
                Some((b, mime)) => tauri::http::Response::builder()
                    .header("Content-Type", mime)
                    .header("Access-Control-Allow-Origin", "*")
                    .header("Cache-Control", "no-store")
                    .body(b)
                    .unwrap(),
                None => tauri::http::Response::builder().status(404).body(Vec::new()).unwrap(),
            }
        })
        .setup(|app| {
            support::log_panics(app.handle());
            support::log(app.handle(), "info", &format!("MapSmithEE {} started", env!("CARGO_PKG_VERSION")));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            userlib_list,
            userlib_import,
            userlib_delete,
            camp3d_export_scene,
            settings_get,
            settings_set,
            game_open,
            area_open,
            area_save,
            area_install,
            tlk_string,
            library_status,
            library_install,
            library_cancel,
            camp3d_render_targets,
            camp3d::camp3d_get,
            camp3d::camp3d_set,
            camp3d::camp3d_update,
            camp3d::camp3d_delete,
            camp3d::camp3d_map_export,
            camp3d::camp3d_map_import,
            camp3d::camp3d_map_delete,
            support::app_log,
            support::app_report,
            support::system_check,
            support::update_check,
            support::open_url,
            support::open_logs,
            camp3d::camp3d_maps_folder,
            campbuild_run,
            camprender::camprender_find_blender,
            blender_get,
            blender_set,
            camprender::camprender_run,
            camprender::camprender_cancel,
            camp3d_render_area,
            camp3d_walls,
            camp3d_rebuild_area,
            camp3d_render_cancel
        ])
        .run(tauri::generate_context!())
        .expect("error while running MapSmithEE");
}
