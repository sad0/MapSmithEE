//! Camp 3D render: the Rust twin of `E:/AI/bg3d/tools/render_scene.py`. Makes the tents' canvas images
//! (`canvas.rs`) and the recoloured ground layers (`groundadj.rs`) in a work folder, writes the editor documents where
//! `render_map.py` expects them, then runs Blender (`blender -b -P render_map.py -- ...`) on the scripts shipped in
//! `resources/blender/`, reading its output for progress. Users only need Blender: no Python, numpy or PIL.
//!
//! Work folder: `scene/` (map.json, map_ground.json, map_g<n>.json, models/offsets.json), `canvas/` and `ground/`
//! (cached by parameters), `masks/` (written by render_map.py), `canvas.json` (item index -> canvas PNG).
//! The model library has the bg3d layout: `models.json`, `assets/`, `mesh/`, `proxies/`, `tex/`, `ext/`.

pub mod canvas;
pub mod groundadj;

use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct AlsoRender {
    pub out_png: PathBuf,
    pub hour: f32,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct RenderRequest {
    /// render on the CPU (slower): for graphics cards without the memory for the scene. A GPU render that runs out
    /// of memory is done again on the CPU by itself
    #[serde(default)]
    pub cpu: bool,
    /// largest texture size in the render (Cycles' simplify); 0 = automatic: 1024 per picture scale (2048 for 2x HD
    /// tiles...). At BG's scale a 2 m piece is about 84 px, so bigger textures add nothing to the picture but memory:
    /// the Camp map (1259 pieces) took 24.6 GB of RAM with the library's full textures, 5.3 GB at 1024, with the same
    /// picture (mean difference 0.2/255, 2026-11-05). `render` lowers it when memory runs out
    #[serde(default)]
    pub tex_limit: u32,
    /// render in tiles of this size (only one tile's buffers in memory); 0 = Cycles' own default
    #[serde(default)]
    pub tile: u32,
    /// the editor's `maps/<id>` document (raw data or wrapped `{id, data, version}`)
    pub map_doc: Value,
    /// `maps/<id>_ground`
    pub ground_doc: Value,
    /// `maps/<id>_g<n>` by layer index (null where a layer has none); used when the ground doc is `split`
    #[serde(default)]
    pub ground_masks: Vec<Value>,
    /// `models/offsets` (the editor's Calibrate mode), if any
    #[serde(default)]
    pub offsets: Option<Value>,
    /// output picture (.png); with `id_pass`, `<out>_alpha.png` and `<out>_id.png` are written next to it, and
    /// `<out>_light.png` next to it and to each `also` picture (the light pass, campbuild::light)
    pub out_png: PathBuf,
    #[serde(default = "default_samples")]
    pub samples: u32,
    /// hour of the day, 0..24; the map's own `time` when None
    #[serde(default)]
    pub hour: Option<f32>,
    /// also render the coverage (`_alpha`) and piece id (`_id`) passes the area build needs
    #[serde(default)]
    pub id_pass: bool,
    /// render only this pixel box of the full picture: x0, y0, x1, y1 (top-left origin)
    #[serde(default)]
    pub crop: Option<[u32; 4]>,
    /// blender.exe; found with `find_blender` when None
    #[serde(default)]
    pub blender_exe: Option<PathBuf>,
    pub library_dir: PathBuf,
    pub work_dir: PathBuf,
    /// more pictures from the same loaded scene (e.g. the night one): models are read once
    #[serde(default)]
    pub also: Vec<AlsoRender>,
    /// models already loaded and fitted, kept between renders (.blend per model); none when None
    #[serde(default)]
    pub cache_dir: Option<PathBuf>,
    /// folder holding render_map.py and its modules; the source tree's `resources/blender` when None
    #[serde(default)]
    pub scripts_dir: Option<PathBuf>,
    /// the pictures (not the passes: `_alpha`, `_id` stay 1x) at this many times the game's resolution (HD tiles)
    #[serde(default = "one")]
    pub scale: u32,
    /// save the scene instead of rendering it: `{"path": ..., "format": "blend" | "glb" | "fbx"}` (exportscene.py);
    /// `out_png` is then not written
    #[serde(default)]
    pub export: Option<Value>,
    /// the user's own models and textures (userlib), next to the library
    #[serde(default)]
    pub user_lib: Option<PathBuf>,
}

fn one() -> u32 {
    1
}

fn default_samples() -> u32 {
    128
}

#[derive(Serialize, Clone, Debug)]
pub struct RenderReport {
    pub out_png: PathBuf,
    pub alpha_png: Option<PathBuf>,
    pub id_png: Option<PathBuf>,
    pub canvases: usize,
    pub ground_layers_adjusted: usize,
    /// e.g. "OPTIX GPU", as render_map.py reported it
    pub device: String,
    pub seconds: f64,
}

#[derive(Serialize, Clone, Debug)]
pub struct Progress {
    pub pct: u32,
    pub step: String,
}

pub(crate) fn io_err(p: &Path, e: std::io::Error) -> String {
    format!("io: {}: {e}", p.display())
}

pub(crate) fn img_err(p: &Path, e: image::ImageError) -> String {
    format!("image: {}: {e}", p.display())
}

/// 12 hex digits of FNV-1a 64: names cached files after their parameters
pub(crate) fn cache_key(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    format!("{:012x}", h & 0xffff_ffff_ffff)
}

/// Python truthiness of a JSON value (`it.get('tent') or ...`)
pub(crate) fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// a document's data: `{id, data, version}` wrappers are opened, raw data is kept (Python's `d.get('data', d)`)
pub fn doc_data(v: &Value) -> &Value {
    v.get("data").unwrap_or(v)
}

// ---------------------------------------------------------------------------------------------------------------
// Blender discovery
// ---------------------------------------------------------------------------------------------------------------

/// version from a folder name like "Blender 5.2" / "blender-4.5.1-windows-x64"
fn version_of(name: &str) -> Option<(u32, u32)> {
    let i = name.find(|c: char| c.is_ascii_digit())?;
    let mut it = name[i..].split(|c: char| !c.is_ascii_digit());
    let major = it.next()?.parse().ok()?;
    let minor = it.next().and_then(|m| m.parse().ok()).unwrap_or(0);
    Some((major, minor))
}

fn exe_name() -> &'static str {
    if cfg!(windows) { "blender.exe" } else { "blender" }
}

/// Every Blender found in the usual places: (version, exe).
fn candidates() -> Vec<((u32, u32), PathBuf)> {
    let mut dirs: Vec<PathBuf> = Vec::new();
    for var in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
        if let Ok(p) = std::env::var(var) {
            dirs.push(Path::new(&p).join("Blender Foundation"));
        }
    }
    if let Ok(p) = std::env::var("LOCALAPPDATA") {
        dirs.push(Path::new(&p).join("Programs").join("Blender Foundation"));
    }
    let mut found = Vec::new();
    for d in &dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        for e in rd.flatten() {
            let exe = e.path().join(exe_name());
            let name = e.file_name().to_string_lossy().into_owned();
            if let (true, Some(v)) = (exe.is_file(), version_of(&name)) {
                found.push((v, exe));
            }
        }
    }
    // Steam's Blender, and anything on PATH (version unknown: ranked as 0.0, used only when nothing else is found)
    let mut steam = Vec::new();
    for var in ["ProgramFiles(x86)", "ProgramFiles"] {
        if let Ok(p) = std::env::var(var) {
            steam.push(Path::new(&p).join("Steam/steamapps/common/Blender").join(exe_name()));
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        steam.extend(std::env::split_paths(&path).map(|d| d.join(exe_name())));
    }
    if cfg!(target_os = "macos") {
        steam.push(PathBuf::from("/Applications/Blender.app/Contents/MacOS/Blender"));
    }
    for exe in steam {
        if exe.is_file() && !found.iter().any(|(_, f)| f == &exe) {
            found.push(((0, 0), exe));
        }
    }
    found
}

/// The newest installed Blender 5.x (else the newest 4.x, else one with an unknown version, e.g. on PATH).
/// The scripts need Blender 4.0 or later; they are tested with 5.2.
pub fn find_blender() -> Option<PathBuf> {
    let mut c = candidates();
    c.sort_by_key(|x| std::cmp::Reverse(x.0));
    let pick = |ok: &dyn Fn((u32, u32)) -> bool| c.iter().find(|(v, _)| ok(*v)).map(|(_, p)| p.clone());
    pick(&|v| v.0 >= 5).or_else(|| pick(&|v| v.0 == 4)).or_else(|| pick(&|v| v == (0, 0)))
}

// ---------------------------------------------------------------------------------------------------------------
// Preparation (render_scene.py)
// ---------------------------------------------------------------------------------------------------------------

struct Prepared {
    map_json: PathBuf,
    ground_json: PathBuf,
    canvas_json: PathBuf,
    texfiles: BTreeMap<String, String>,
    canvases: usize,
    px_per_m: String,
}

fn write_json(path: &Path, v: &Value) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
    }
    std::fs::write(path, serde_json::to_vec(v).map_err(|e| e.to_string())?).map_err(|e| io_err(path, e))
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

fn prepare(req: &RenderRequest, progress: &dyn Fn(u32, &str), cancel: &AtomicBool) -> Result<Prepared, String> {
    let lib = &req.library_dir;
    let work = &req.work_dir;
    let models_path = lib.join("models.json");
    let models: Value = serde_json::from_slice(&std::fs::read(&models_path).map_err(|e| io_err(&models_path, e))?)
        .map_err(|e| format!("json: {}: {e}", models_path.display()))?;
    let map = doc_data(&req.map_doc);
    let ground = doc_data(&req.ground_doc);
    let items = map.get("items").and_then(Value::as_array).ok_or("the map document has no items")?;

    // the editor documents, where render_map.py looks for them: masks as <base>_g<n>.json next to <base>_ground.json
    let scene = work.join("scene");
    if scene.is_dir() {
        std::fs::remove_dir_all(&scene).map_err(|e| io_err(&scene, e))?;
    }
    let map_json = scene.join("map.json");
    let ground_json = scene.join("map_ground.json");
    write_json(&map_json, &req.map_doc)?;
    write_json(&ground_json, &req.ground_doc)?;
    for (n, m) in req.ground_masks.iter().enumerate() {
        if !m.is_null() {
            write_json(&scene.join(format!("map_g{n}.json")), m)?;
        }
    }
    if let Some(o) = &req.offsets {
        write_json(&scene.join("models").join("offsets.json"), o)?;
    }

    // tent and rug canvases: one image per look, the item's own `tent` or its model's `canvas`
    progress(1, "Tent canvases");
    let canvas_dir = work.join("canvas");
    let grain_path = lib.join("tex").join(canvas::GRAIN);
    let mut grain = None;
    let mut by_item = BTreeMap::new();
    let mut looks = BTreeSet::new();
    for (idx, it) in items.iter().enumerate() {
        let Some(m) = it.get("m").and_then(Value::as_str).and_then(|m| models.get(m)) else { continue };
        let look = it.get("tent").filter(|t| truthy(t)).or_else(|| m.get("canvas").filter(|c| truthy(c)));
        let Some(look) = look else { continue };
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        let png = canvas::make(look, &canvas_dir, &mut grain, &grain_path)?;
        looks.insert(look.to_string());
        by_item.insert(idx.to_string(), Value::String(path_str(&png)));
    }
    let canvas_json = work.join("canvas.json");
    write_json(&canvas_json, &Value::Object(by_item.into_iter().collect()))?;

    // ground layers recoloured in the editor (hue, saturation, brightness, contrast)
    progress(2, "Ground colours");
    let mut texfiles = BTreeMap::new();
    let adj = ground.get("adj");
    for (i, name) in ground.get("layers").and_then(Value::as_array).into_iter().flatten().enumerate() {
        let Some(name) = name.as_str() else { continue };
        let Some(a) = adj.and_then(|a| a.get(name)).filter(|a| truthy(a)) else { continue };
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".into());
        }
        // a variant ("dirt~2": the same texture, its own colour) reads its base layer's texture
        let base = name.split('~').next().unwrap_or(name);
        let src = req
            .user_lib
            .as_ref()
            .map(|u| u.join("tex").join(base).join("diff_2k.jpg"))
            .filter(|p| p.is_file())
            .unwrap_or_else(|| lib.join("tex").join(base).join("diff_2k.jpg"));
        let out = groundadj::make(&src, name, a, &work.join("ground"))?;
        texfiles.insert(i.to_string(), path_str(&out));
    }
    let px_per_m = map.get("px_per_m").filter(|v| v.is_number()).map(Value::to_string).unwrap_or_else(|| "42".into());
    Ok(Prepared { map_json, ground_json, canvas_json, texfiles, canvases: looks.len(), px_per_m })
}

// ---------------------------------------------------------------------------------------------------------------
// Blender run
// ---------------------------------------------------------------------------------------------------------------

/// Where the render is, read from Blender's output lines.
struct Tracker {
    id_pass: bool,
    stage: &'static str,
    pct: u32,
    step: String,
    device: String,
    /// while loading: models done / total and the one being read, pieces placed / total, when that model started
    models: (u32, u32),
    model: String,
    pieces: (u32, u32),
    since: Option<Instant>,
    /// picture k of n (one Blender session renders them all)
    job: (u32, u32),
}

/// a model id as people read it: "ph_fir_tree_01_a" -> "fir tree 01 a" (Objaverse ids stay as they are)
fn pretty(id: &str) -> String {
    id.strip_prefix("ph_").unwrap_or(id).replace('_', " ")
}

/// "Sample 12/128" (Cycles' status line) -> (12, 128)
fn sample_of(line: &str) -> Option<(u32, u32)> {
    let i = line.rfind("Sample ")?;
    let rest = &line[i + 7..];
    let (a, b) = rest.split_once('/')?;
    let b: String = b.chars().take_while(char::is_ascii_digit).collect();
    Some((a.trim().parse().ok()?, b.parse().ok()?))
}

impl Tracker {
    /// span of the progress bar for each stage of the Blender run
    /// the bar from 40 to 99 is shared by the pictures; the last one carries the coverage and id passes
    fn span(&self) -> (u32, u32) {
        if self.stage == "load" {
            return (3, 40);
        }
        let (k, n) = (self.job.0.max(1), self.job.1.max(1));
        let (lo, hi) = (40 + 59 * (k - 1) / n, 40 + 59 * k / n);
        let last = k == n && self.id_pass;
        let w = hi - lo;
        match self.stage {
            "render" if last => (lo, lo + w * 80 / 100),
            "render" => (lo, hi),
            "alpha" => (lo + w * 80 / 100, lo + w * 95 / 100),
            _ => (lo + w * 95 / 100, hi),
        }
    }

    /// true when the progress changed
    fn line(&mut self, line: &str) -> bool {
        let (pct, step) = (self.pct, self.step.clone());
        if let Some(st) = line.strip_prefix("MAPSMITH stats ").or_else(|| line.strip_prefix("Fra:")) {
            // "Mem: 3M | Sample 12/128", "Mem: 1M | Updating Images": the last part says what Blender does
            if let Some((a, b)) = sample_of(st) {
                let (lo, hi) = self.span();
                // Cycles ends a pass with "Sample 0/n": the bar never goes back within a stage
                if self.stage != "load" && b > 0 {
                    self.pct = self.pct.max(lo + (hi - lo) * a.min(b) / b);
                }
                self.step = format!("{} (sample {a}/{b})", self.stage_name());
            } else if let Some(last) = st.rsplit('|').next().map(str::trim).filter(|s| !s.is_empty()) {
                self.step = format!("{}: {last}", self.stage_name());
            }
        } else if let Some(rest) = line.strip_prefix("MAPSMITH ") {
            let mut w = rest.split_whitespace();
            match (w.next(), w.next()) {
                (Some("models"), Some(f)) => {
                    if let Some((i, n)) = f.split_once('/').and_then(|(i, n)| Some((i.parse::<u32>().ok()?, n.parse::<u32>().ok()?))) {
                        self.pct = 5 + 35 * i / n.max(1);
                        let id = w.next().unwrap_or("");
                        let label = w.collect::<Vec<_>>().join(" ");
                        self.models = (i + 1, n);
                        // several models share a label (fir and pine are both "conifer"): show which one
                        self.model = if label.is_empty() { id.to_string() } else { format!("{label} · {}", pretty(id)) };
                        self.since = Some(Instant::now());
                        self.step = self.load_step();
                    }
                }
                (Some("job"), Some(f)) => {
                    if let Some((k, n)) = f.split_once('/').and_then(|(k, n)| Some((k.parse::<u32>().ok()?, n.parse::<u32>().ok()?))) {
                        self.job = (k, n);
                    }
                }
                (Some("item"), Some(f)) => {
                    if let Some((i, n)) = f.split_once('/').and_then(|(i, n)| Some((i.parse::<u32>().ok()?, n.parse::<u32>().ok()?))) {
                        self.pieces = (i, n);
                        self.step = self.load_step();
                    }
                }
                (Some("stage"), Some(s)) => {
                    self.stage = match s {
                        "render" => "render",
                        "alpha" => "alpha",
                        "id" => "id",
                        _ => "load",
                    };
                    self.pct = self.span().0;
                    self.step = match self.stage {
                        "render" => "Rendering".into(),
                        "alpha" => "Coverage pass".into(),
                        _ => "Piece id pass".into(),
                    };
                }
                (Some("device"), _) => self.device = rest["device".len()..].trim().to_string(),
                _ => {}
            }
        } else if line.starts_with("placed ") {
            self.pct = 40;
            self.step = "Lights and camera".into();
        }
        self.pct != pct || self.step != step
    }

    /// "Loading models 23/82 · pieces 412/1259 · fir tree (35 s)": the seconds keep counting while a big model imports
    fn load_step(&self) -> String {
        let mut s = format!("Loading models {}/{}", self.models.0, self.models.1);
        if self.pieces.1 > 0 {
            s += &format!(" · pieces {}/{}", self.pieces.0, self.pieces.1);
        }
        if !self.model.is_empty() {
            s += &format!(" · {}", self.model);
        }
        if let Some(t) = self.since.filter(|t| t.elapsed() >= Duration::from_secs(2)) {
            s += &format!(" ({} s)", t.elapsed().as_secs());
        }
        s
    }

    /// once a second while a model loads: refresh the seconds; true when the text changed
    fn tick(&mut self) -> bool {
        if self.stage != "load" || self.since.is_none() {
            return false;
        }
        let s = self.load_step();
        let changed = s != self.step;
        self.step = s;
        changed
    }

    fn stage_name(&self) -> &'static str {
        match self.stage {
            "render" => "Rendering",
            "alpha" => "Coverage pass",
            "id" => "Piece id pass",
            _ => "Loading",
        }
    }
}

/// Blender's words when the graphics card has not the memory for the scene (OptiX, CUDA, HIP, Metal)
pub fn gpu_out_of_memory(text: &str) -> bool {
    let l = text.to_ascii_lowercase();
    l.contains("out of gpu memory")
        || l.contains("failed to build optix acceleration structure")
        || (l.contains("out of memory") && ["cuda", "optix", "hip", "metal", "gpu", "device"].iter().any(|k| l.contains(k)))
}

/// Blender's words when the computer's own memory (RAM) runs out
pub fn out_of_memory(text: &str) -> bool {
    let l = text.to_ascii_lowercase();
    ["out of memory", "memoryerror", "bad_alloc", "failed to allocate", "not enough memory", "cannot allocate"].iter().any(|k| l.contains(k))
}

/// Blender's words when the disk is full
pub fn disk_full(text: &str) -> bool {
    let l = text.to_ascii_lowercase();
    ["no space left", "not enough space on the disk", "os error 112", "disk full"].iter().any(|k| l.contains(k))
}

/// Why a Blender run failed, for `render`'s fallbacks and its message
#[derive(Debug, Clone, PartialEq)]
pub enum Fail {
    GpuMemory,
    Memory,
    /// Blender stopped by itself (a crash): its exit code
    Crash(String),
    Disk,
    /// render_map.py raised an error: its last line
    Script(String),
    /// anything else, with Blender's exit status
    Other(String),
    /// the user stopped it
    Cancelled,
    /// before Blender ran (no Blender, no library, bad request): the message as is
    Setup(String),
}

/// A failed run: why, and Blender's last lines (for the log and the bug report)
#[derive(Debug, Clone)]
pub struct Failed {
    pub why: Fail,
    pub tail: String,
}

impl From<String> for Failed {
    fn from(e: String) -> Failed {
        Failed { why: if e == "cancelled" { Fail::Cancelled } else { Fail::Setup(e) }, tail: String::new() }
    }
}

impl From<&str> for Failed {
    fn from(e: &str) -> Failed {
        e.to_string().into()
    }
}

/// What a run's output says went wrong (`code`: Blender's exit code, None when it was killed or crashed hard)
pub fn classify(tail: &str, code: Option<i32>, traceback: bool) -> Fail {
    if gpu_out_of_memory(tail) {
        return Fail::GpuMemory;
    }
    if out_of_memory(tail) || code.is_some_and(|c| c as u32 == 0xC000_0017) {
        return Fail::Memory;
    }
    if disk_full(tail) {
        return Fail::Disk;
    }
    if traceback {
        let last = tail.lines().rev().map(|l| l.trim_start_matches("! ").trim()).find(|l| !l.is_empty() && !l.starts_with("Error: script failed") && !l.starts_with("Blender quit"));
        return Fail::Script(last.unwrap_or("").to_string());
    }
    match code {
        // Windows' crash codes (access violation, stack overflow, heap corruption, fail fast) or no code at all
        Some(c) if [0xC000_0005u32, 0xC000_00FD, 0xC000_0374, 0xC000_0409].contains(&(c as u32)) => Fail::Crash(format!("0x{:08X}", c as u32)),
        None => Fail::Crash("no exit code".into()),
        Some(c) => Fail::Other(format!("exit code {c}")),
    }
}

/// The steps `render` takes down when memory runs out: (largest texture per picture scale, tile size)
const LIGHTER: [(u32, u32); 3] = [(1024, 0), (512, 1024), (256, 512)];

/// The message for a render that failed for good: what happened, why, what to do. `pieces`: the map's piece count,
/// `ram`: this PC's memory in GB
pub fn explain(f: &Failed, pieces: usize, ram: Option<f64>) -> String {
    let ram = ram.map(|g| format!(" (this PC has {g:.0} GB)")).unwrap_or_default();
    let tail = if f.tail.is_empty() { String::new() } else { format!("\n\nBlender's last lines:\n{}", f.tail) };
    match &f.why {
        Fail::Cancelled => "cancelled".into(),
        Fail::Setup(e) => e.clone(),
        Fail::GpuMemory | Fail::Memory => format!(
            "Not enough memory to render this map, even with lighter textures{ram}.\n\
             What helps: close the other programs (browsers, games); untick HD tiles; use fewer pieces or a smaller \
             map (this one has {pieces} pieces); then Render again.{tail}"
        ),
        Fail::Crash(code) => format!(
            "Blender stopped unexpectedly ({code}), even with lighter settings.\n\
             It often comes from the graphics driver: update it, or tick \"Render on the CPU\". If it happens again, \
             ? > Report a bug (the report holds the details).{tail}"
        ),
        Fail::Disk => format!(
            "The disk is full: Blender could not write the pictures.\n\
             Free some space on the drive of MapSmithEE's data (a render needs a few hundred MB, the model library \
             about 6 GB the first time), then Render again.{tail}"
        ),
        Fail::Script(line) => format!(
            "MapSmithEE's render script failed: {line}\n\
             This is a MapSmithEE bug, not your map: ? > Report a bug (the report holds the details).{tail}"
        ),
        Fail::Other(status) => format!(
            "Blender failed ({status}).\n\
             Try again; if it fails again: ? > Report a bug (the report holds the details).{tail}"
        ),
    }
}

/// Run the whole render: preparation, then Blender. `progress(pct 0..100, step)`; setting `cancel` kills Blender.
/// When memory runs out it tries again by itself, a step lighter each time: a graphics card's memory -> the CPU
/// (the computer's RAM), then smaller textures and rendering in tiles (`LIGHTER`); a crash is taken as one too (on
/// Windows a lack of memory often ends that way). The error says what happened and what to do (`explain`).
pub fn render(req: &RenderRequest, progress: &dyn Fn(u32, &str), cancel: &AtomicBool) -> Result<RenderReport, String> {
    let pieces = doc_data(&req.map_doc).get("items").and_then(Value::as_array).map_or(0, Vec::len);
    let sc = req.scale.max(1);
    let mut r = RenderRequest { tex_limit: if req.tex_limit == 0 { 1024 * sc } else { req.tex_limit }, ..req.clone() };
    let mut step = 0;
    let mut notes: Vec<&str> = Vec::new();
    loop {
        match render_on(&r, progress, cancel) {
            Ok(mut rep) => {
                if !notes.is_empty() {
                    rep.device = format!("{} ({})", rep.device, notes.join(", "));
                }
                return Ok(rep);
            }
            Err(f) => {
                let memory = matches!(f.why, Fail::GpuMemory | Fail::Memory | Fail::Crash(_));
                if f.why == Fail::GpuMemory && !r.cpu {
                    progress(3, "Not enough graphics memory: rendering on the CPU (slower)");
                    notes.push("the graphics card ran out of memory");
                    r.cpu = true;
                    continue;
                }
                if memory && step + 1 < LIGHTER.len() {
                    step += 1;
                    let (t, tile) = LIGHTER[step];
                    r.tex_limit = (t * sc).min(r.tex_limit.saturating_sub(1).max(128));
                    r.tile = tile;
                    progress(3, &format!("Not enough memory: trying again lighter ({} of {})", step, LIGHTER.len() - 1));
                    notes.push(if step == 1 { "lighter textures" } else { "lightest textures" });
                    continue;
                }
                return Err(explain(&f, pieces, crate::support::ram_gb()));
            }
        }
    }
}

fn render_on(req: &RenderRequest, progress: &dyn Fn(u32, &str), cancel: &AtomicBool) -> Result<RenderReport, Failed> {
    let t0 = Instant::now();
    let out = std::path::absolute(&req.out_png).map_err(|e| io_err(&req.out_png, e))?;
    let out_s = path_str(&out);
    if !out_s.to_lowercase().ends_with(".png") || out_s.contains('#') {
        return Err(format!("the output must be a .png file without '#': {out_s}").into());
    }
    let blender = req.blender_exe.clone().or_else(find_blender).ok_or(
        "Blender 5.x was not found. Install it from blender.org, or pick its blender.exe with Choose… in the Render dialog \
         (a portable Blender or one on another drive works too).",
    )?;
    if !blender.is_file() {
        return Err(format!(
            "Blender is not at {} any more (moved or uninstalled?). Pick it again with Choose… in the Render dialog, or \
             Automatic to search the usual folders.",
            blender.display()
        )
        .into());
    }
    let scripts = req.scripts_dir.clone().unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/blender"));
    let script = scripts.join("render_map.py");
    if !script.is_file() {
        return Err(format!(
            "MapSmithEE's render script is missing ({}): the install is damaged. Install MapSmithEE again (your maps stay).",
            script.display()
        )
        .into());
    }
    if !req.library_dir.join("models.json").is_file() {
        return Err(format!(
            "The model library is not at {} (models.json missing): click Download models in the Render dialog.",
            req.library_dir.display()
        )
        .into());
    }
    let work = std::path::absolute(&req.work_dir).map_err(|e| io_err(&req.work_dir, e))?;
    std::fs::create_dir_all(&work).map_err(|e| io_err(&work, e))?;
    if let Some(d) = out.parent() {
        std::fs::create_dir_all(d).map_err(|e| io_err(d, e))?;
    }
    let req = RenderRequest { work_dir: work.clone(), ..req.clone() };

    progress(0, "Preparing textures");
    let prep = prepare(&req, progress, cancel)?;
    let ground_adjusted = prep.texfiles.len();

    // blender -b --python-exit-code 1 -P render_map.py -- <map> <ground> <out> <samples> <hour|-> [x0 y0 x1 y1]
    let mut cmd = Command::new(&blender);
    cmd.arg("-b").args(["--python-exit-code", "1"]).arg("-P").arg(&script).arg("--");
    cmd.arg(&prep.map_json).arg(&prep.ground_json).arg(&out_s).arg(req.samples.to_string());
    cmd.arg(req.hour.map(|h| h.to_string()).unwrap_or_else(|| "-".into()));
    if let Some(c) = req.crop {
        cmd.args(c.map(|v| v.to_string()));
    }
    cmd.env("MAPSMITH_LIBRARY", path_str(&req.library_dir))
        .env("MAPSMITH_WORK", path_str(&work))
        .env("MAPSMITH_CANVAS", path_str(&prep.canvas_json))
        .env("BG3D_IDPASS", if req.id_pass { "1" } else { "" })
        .env("MAPSMITH_DEVICE", if req.cpu { "cpu" } else { "" })
        .env("MAPSMITH_TEXLIMIT", if req.tex_limit == 0 { String::new() } else { req.tex_limit.clamp(128, 8192).next_power_of_two().min(8192).to_string() })
        .env("MAPSMITH_TILE", if req.tile == 0 { String::new() } else { req.tile.to_string() })
        .env("MAPSMITH_JOBS", {
            // area builds (id_pass) also get each picture's light pass, for the game's light maps
            let mut jobs = vec![json!({"out": path_str(&out), "hour": req.hour, "id": req.id_pass, "light": req.id_pass})];
            jobs.extend(req.also.iter().map(|a| json!({"out": path_str(&a.out_png), "hour": a.hour, "id": false, "light": req.id_pass})));
            json!(jobs).to_string()
        })
        .env("MAPSMITH_CACHE", req.cache_dir.as_deref().map(path_str).unwrap_or_default())
        .env("BG3D_PXM", &prep.px_per_m)
        .env("MAPSMITH_SCALE", req.scale.max(1).to_string())
        .env("MAPSMITH_EXPORT", req.export.as_ref().map(|e| e.to_string()).unwrap_or_default())
        .env("MAPSMITH_USERLIB", req.user_lib.as_deref().map(path_str).unwrap_or_default())
        .env("BG3D_TEXFILES", json!(prep.texfiles).to_string())
        .current_dir(&work)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    progress(3, "Starting Blender");
    let mut child = cmd.spawn().map_err(|e| format!("cannot start {}: {e}", blender.display()))?;

    // both pipes read on their own threads, lines sent here: (is_stderr, line)
    let (tx, rx) = mpsc::channel::<(bool, String)>();
    let mut readers = Vec::new();
    for (is_err, pipe) in [(false, child.stdout.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>)), (true, child.stderr.take().map(|p| Box::new(p) as Box<dyn std::io::Read + Send>))] {
        let Some(pipe) = pipe else { continue };
        let tx = tx.clone();
        readers.push(std::thread::spawn(move || {
            for chunk in BufReader::new(pipe).split(b'\n').map_while(Result::ok) {
                let line = String::from_utf8_lossy(&chunk).trim_end_matches('\r').to_string();
                if tx.send((is_err, line)).is_err() {
                    break;
                }
            }
        }));
    }
    drop(tx);

    let mut tr = Tracker { id_pass: req.id_pass, stage: "load", pct: 3, step: String::new(), device: String::new(), models: (0, 0), model: String::new(), pieces: (0, 0), since: None, job: (1, 1) };
    let mut tail: Vec<String> = Vec::new();
    let mut traceback = false;
    // Cycles may go on after a memory error and save a black picture: that render failed
    let mut oom = false;
    let status = loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("cancelled".into());
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok((is_err, line)) => {
                traceback |= line.starts_with("Traceback");
                if gpu_out_of_memory(&line) || out_of_memory(&line) {
                    oom = true;
                    let _ = child.kill();
                }
                if !line.starts_with("MAPSMITH stats ") {
                    tail.push(if is_err { format!("! {line}") } else { line.clone() });
                    if tail.len() > 40 {
                        tail.remove(0);
                    }
                }
                if !is_err && tr.line(&line) {
                    progress(tr.pct, &tr.step);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if tr.tick() {
                    progress(tr.pct, &tr.step);
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break child.wait().map_err(|e| Failed::from(e.to_string()))?,
        }
    };
    for r in readers {
        let _ = r.join();
    }
    let made = match req.export.as_ref().and_then(|e| e.get("path")).and_then(Value::as_str) {
        Some(p) => PathBuf::from(p),
        None => out.clone(),
    };
    if oom || !status.success() || traceback || !made.is_file() {
        let tail = tail.join("\n");
        let why = match classify(&tail, status.code(), traceback) {
            Fail::Other(_) if status.success() && !made.is_file() => Fail::Other(format!("no picture written: {}", made.display())),
            w => w,
        };
        return Err(Failed { why, tail });
    }
    let stem = out.file_stem().unwrap_or_default().to_string_lossy().into_owned();
    let side = |suffix: &str| out.with_file_name(format!("{stem}{suffix}"));
    let (alpha, id) = if req.id_pass { (Some(side("_alpha.png")), Some(side("_id.png"))) } else { (None, None) };
    progress(100, "Done");
    Ok(RenderReport {
        out_png: out,
        alpha_png: alpha,
        id_png: id,
        canvases: prep.canvases,
        ground_layers_adjusted: ground_adjusted,
        device: tr.device,
        seconds: t0.elapsed().as_secs_f64(),
    })
}

// ---------------------------------------------------------------------------------------------------------------
// Tauri commands
// ---------------------------------------------------------------------------------------------------------------

pub static CANCEL: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);

/// the bundled scripts (resource `blender/`), else the source tree's
pub fn scripts_dir(app: &tauri::AppHandle) -> PathBuf {
    use tauri::Manager;
    app.path()
        .resolve("blender", tauri::path::BaseDirectory::Resource)
        .ok()
        .filter(|p| p.join("render_map.py").is_file())
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/blender"))
}

#[tauri::command]
pub fn camprender_find_blender() -> Option<String> {
    find_blender().map(|p| p.to_string_lossy().into_owned())
}

/// Runs one render (one at a time); emits "camprender-progress" `{pct, step}`.
#[tauri::command]
pub async fn camprender_run(app: tauri::AppHandle, req: RenderRequest) -> Result<RenderReport, String> {
    use tauri::Emitter;
    if RUNNING.swap(true, Ordering::SeqCst) {
        return Err("a render is already running".into());
    }
    CANCEL.store(false, Ordering::SeqCst);
    let mut req = req;
    if req.scripts_dir.is_none() {
        req.scripts_dir = Some(scripts_dir(&app));
    }
    let r = tauri::async_runtime::spawn_blocking(move || {
        // Blender reports dozens of steps a second while it prepares a render: at most ~8 events a second, except
        // when the percentage moves
        let last = std::sync::Mutex::new((u32::MAX, Instant::now()));
        let progress = |pct: u32, step: &str| {
            let mut l = last.lock().unwrap();
            if pct != l.0 || pct == 100 || l.1.elapsed() >= Duration::from_millis(120) {
                *l = (pct, Instant::now());
                let _ = app.emit("camprender-progress", Progress { pct, step: step.to_string() });
            }
        };
        render(&req, &progress, &CANCEL)
    })
    .await
    .map_err(|e| e.to_string());
    RUNNING.store(false, Ordering::SeqCst);
    r?
}

/// Stops the running render (kills Blender); `camprender_run` then returns Err("cancelled").
#[tauri::command]
pub fn camprender_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_and_messages() {
        assert_eq!(classify("Error: Out of memory", Some(1), false), Fail::Memory);
        assert_eq!(classify("Error: System is out of GPU memory", Some(1), false), Fail::GpuMemory);
        assert_eq!(classify("Error: Failed to build OptiX acceleration structure", Some(1), false), Fail::GpuMemory);
        assert_eq!(classify("OSError: [Errno 28] No space left on device", Some(1), true), Fail::Disk);
        assert_eq!(classify("Traceback ...
! KeyError: 'items'
! Error: script failed, file: x
Blender quit", Some(1), true), Fail::Script("KeyError: 'items'".into()));
        assert_eq!(classify("", Some(0xC000_0005u32 as i32), false), Fail::Crash("0xC0000005".into()));
        assert_eq!(classify("", Some(0xC000_0017u32 as i32), false), Fail::Memory);
        assert_eq!(classify("", None, false), Fail::Crash("no exit code".into()));
        let m = explain(&Failed { why: Fail::Memory, tail: "Error: Out of memory".into() }, 1259, Some(12.0));
        assert!(m.starts_with("Not enough memory") && m.contains("this PC has 12 GB") && m.contains("1259 pieces") && m.contains("Error: Out of memory"), "{m}");
        assert!(explain(&Failed { why: Fail::Crash("0xC0000005".into()), tail: String::new() }, 3, None).contains("Render on the CPU"));
        assert_eq!(explain(&"cancelled".into(), 0, None), "cancelled");
    }

    #[test]
    fn gpu_memory_words() {
        assert!(gpu_out_of_memory("Error: Failed to build OptiX acceleration structure"));
        assert!(gpu_out_of_memory("Error: System is out of GPU memory"));
        assert!(gpu_out_of_memory("CUDA error: Out of memory in cuMemAlloc"));
        assert!(!gpu_out_of_memory("MAPSMITH device OPTIX GPU"));
        assert!(!gpu_out_of_memory("Python: MemoryError: out of memory"));
    }

    #[test]
    fn samples_and_stages() {
        let mut t = Tracker { id_pass: true, stage: "load", pct: 3, step: String::new(), device: String::new(), models: (0, 0), model: String::new(), pieces: (0, 0), since: None, job: (1, 1) };
        assert!(t.line("MAPSMITH models 41/82 tent_pup"));
        assert_eq!(t.pct, 22);
        assert_eq!(t.step, "Loading models 42/82 · tent_pup");
        assert!(t.line("MAPSMITH models 42/82 ph_fir_tree_01_a conifer"));
        assert!(t.line("MAPSMITH item 412/1259"));
        assert_eq!(t.step, "Loading models 43/82 · pieces 412/1259 · conifer · fir tree 01 a");
        t.line("MAPSMITH device OPTIX GPU");
        assert_eq!(t.device, "OPTIX GPU");
        t.line("MAPSMITH stage render");
        assert_eq!(t.pct, 40);
        t.line("MAPSMITH stats Remaining: 00:00.30 | Mem: 3M | Sample 4/8");
        assert_eq!(t.pct, 63);
        t.line("MAPSMITH stats Mem: 1M | Loading render kernels (may take a few minutes the first time)");
        assert_eq!(t.step, "Rendering: Loading render kernels (may take a few minutes the first time)");
        t.line("MAPSMITH stage alpha");
        t.line("Fra:1 Mem:1.2G | Scene, ViewLayer | Sample 16/16");
        assert_eq!(t.pct, 96);
        // two pictures from one session: the first gets the first half of the bar
        let mut t2 = Tracker { id_pass: false, stage: "load", pct: 3, step: String::new(), device: String::new(), models: (0, 0), model: String::new(), pieces: (0, 0), since: None, job: (1, 1) };
        t2.line("MAPSMITH job 1/2");
        t2.line("MAPSMITH stage render");
        t2.line("MAPSMITH stats Sample 8/8");
        assert_eq!(t2.pct, 69);
        assert_eq!(sample_of("x | Sample 12/128"), Some((12, 128)));
    }

    #[test]
    fn helpers() {
        assert_eq!(version_of("Blender 5.2"), Some((5, 2)));
        assert_eq!(version_of("blender-4.5.1-windows-x64"), Some((4, 5)));
        assert_eq!(doc_data(&json!({"id": "a", "data": {"x": 1}})), &json!({"x": 1}));
        assert_eq!(doc_data(&json!({"x": 1})), &json!({"x": 1}));
        assert!(!truthy(&json!({})) && truthy(&json!({"pat": "plain"})) && !truthy(&json!(0)));
        assert_eq!(cache_key("abc").len(), 12);
    }
}
