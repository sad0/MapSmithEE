//! Camp 3D map -> playable area (port of E:/AI/bg3d/tools/build_camp3d.py + pvrz.py).
//!
//! Inputs: the editor's map document (items, spots, exits), its walk corrections, the Blender render with its id and
//! coverage passes, an optional night render, and the model table (dims, label). Outputs in `out_dir`: the TIS and its
//! PVRZ pages, the WED (walls cut from the pieces' silhouettes), the night TIS/WED, SR/HT/LM/LN bitmaps, the ARE and a
//! preview. `Generic` makes a self-contained area; `SadoQolCamp` rebuilds SadoQoL's SQCAMP from its template ARE.

mod contour;
mod files;
mod palettes;
mod preview;
pub mod pvrz;
mod raster;

use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::f64::consts::PI;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// BG's own flames per light-source label: (BAM, share of the piece's height where the flame sits)
const FLAME_BAM: [(&str, &str, f64); 5] = [
    ("brazier", "BDFIRET", 0.9),
    ("forge", "BDFIRET", 0.8),
    ("torch", "FIRE_4", 0.85),
    ("lamp post", "FIRE_4", 0.9),
    ("lantern", "FLAME2S", 0.7),
];
/// the SoD large fire pit, for the camp fire
const CAMPFIRE_BAM: &str = "FPIT1L";
/// default px per metre; each map carries its own (px_per_m, 60 = the old default)
const PXM_DEFAULT: f64 = 42.0;
const ELEV_DEG: f64 = 38.0;
const NOBLOCK: [&str; 5] = ["grass", "fern", "moss", "roots", "firewood"];
const TREES: [&str; 4] = ["tree", "conifer", "dead tree", "bg tree"];
/// search map values: 15 grass (walkable), 8 obstacle that does not block sight
pub const WALK: u8 = 15;
pub const BLOCK: u8 = 8;
/// wall pieces: lower ones don't hide actors
const MIN_HEIGHT: f64 = 0.9;
const MIN_PIXELS: usize = 60;
const MIN_AREA: f64 = 80.0;
const DP_EPS: f64 = 2.0;
/// travel cursor (world map wheel), as the BD5000 camp's exit
const REGION_CURSOR: u32 = 34;

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Model {
    pub dims: [f64; 3],
    pub label: String,
}

/// the model table: a folder of `index*.json` (merged in name order) or the table itself
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(untagged)]
pub enum Models {
    Dir(PathBuf),
    Table(HashMap<String, Model>),
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct BuildInput {
    /// map document (raw data or `{id, data, version}`)
    pub map: PathBuf,
    /// walk document (cells, fix PNG)
    pub walk: Option<PathBuf>,
    /// day render; its passes default to `<render>_alpha.png` and `<render>_id.png`
    pub render: PathBuf,
    pub alpha: Option<PathBuf>,
    pub id: Option<PathBuf>,
    pub night: Option<PathBuf>,
    pub models: Models,
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind")]
pub enum Mode {
    /// a self-contained area, nothing read from a mod
    Generic,
    /// SadoQoL's camp SQCAMP: template ARE (priest, merchant), M_SQ0CMP.lua; `name` is ignored
    SadoQolCamp { template_dir: PathBuf },
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct BuildOptions {
    /// 6 characters, e.g. "AR9900"
    pub name: String,
    pub mode: Mode,
    pub out_dir: PathBuf,
    /// the render pictures (day, night) are at this scale (1, 2, 4): the area stays 1x (tiles made from the
    /// pictures shrunk), and HD tiles for IEX go to `<out_dir>/HD<scale>X` (TIS + PVRZ, same names)
    #[serde(default = "one")]
    pub scale: usize,
}

fn one() -> usize {
    1
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct BuildReport {
    pub name: String,
    pub files: Vec<String>,
    pub tiles: [usize; 2],
    pub pages: usize,
    pub walls: usize,
    pub blocked_cells: usize,
    pub flames: usize,
    /// HD tiles folder (scale > 1), relative to out_dir
    pub hd_dir: Option<String>,
    /// set when the ENTRY spot stood in an exit and was moved out
    pub entry_moved: Option<[i32; 2]>,
    /// a generic area: the game console line that takes the party to its arrival point (also in HOW_TO_TEST.txt)
    pub console: Option<String>,
    /// the traced walls of each piece, before edits: for the editor's Walls tool
    #[serde(skip)]
    pub auto_walls: Vec<AutoWall>,
    pub ms: u64,
}

pub type Pt = (i32, i32);

/// one piece's traced walls (area pixels), with its ground point at build time
#[derive(Serialize, Clone, Debug)]
pub struct AutoWall {
    pub i: usize,
    pub id: Option<String>,
    pub g: Pt,
    pub pts: Vec<Vec<Pt>>,
}

/// a spot in area pixels; facing for the ones that have one
#[derive(Clone, Copy, Debug)]
pub struct Spot {
    pub x: i32,
    pub y: i32,
    pub f: Option<i64>,
}

/// what the writers need, computed from the map document
pub struct Area {
    pub name: String,
    pub tw: usize,
    pub th: usize,
    pub pxm: f64,
    /// spot name -> spot, in the Python's order
    pub spots: Vec<(String, Spot)>,
    pub exits: Vec<Vec<Pt>>,
    pub walls: Vec<Vec<Pt>>,
    pub search: Vec<u8>,
    /// (name, BAM, x, y)
    pub flames: Vec<(String, String, i32, i32)>,
    pub night: bool,
}

impl Area {
    pub fn spot(&self, k: &str) -> Option<Spot> {
        self.spots.iter().find(|(n, _)| n == k).map(|(_, s)| *s)
    }
}

fn sin_e() -> f64 {
    (ELEV_DEG * PI / 180.0).sin()
}

fn num(v: &Value) -> f64 {
    v.as_f64().unwrap_or(0.0)
}

/// Python's round() to an int
fn pround(v: f64) -> i32 {
    v.round_ties_even() as i32
}

fn read_doc(p: &Path) -> Result<Value, String> {
    let t = std::fs::read_to_string(p).map_err(|e| format!("{}: {e}", p.display()))?;
    let v: Value = serde_json::from_str(&t).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(match v.get("data") {
        Some(d) if d.is_object() => d.clone(),
        _ => v,
    })
}

fn load_models(m: &Models) -> Result<HashMap<String, Model>, String> {
    match m {
        Models::Table(t) => Ok(t.clone()),
        Models::Dir(d) => {
            let mut names: Vec<String> = std::fs::read_dir(d)
                .map_err(|e| format!("{}: {e}", d.display()))?
                .flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|f| f.starts_with("index") && f.ends_with(".json"))
                .collect();
            names.sort();
            let mut out = HashMap::new();
            for n in names {
                let t = std::fs::read_to_string(d.join(&n)).map_err(|e| format!("{n}: {e}"))?;
                let v: HashMap<String, Value> = serde_json::from_str(&t).map_err(|e| format!("{n}: {e}"))?;
                for (k, e) in v {
                    if let Ok(m) = serde_json::from_value::<Model>(e) {
                        out.insert(k, m);
                    }
                }
            }
            if out.is_empty() {
                return Err(format!("{}: no models (index*.json)", d.display()));
            }
            Ok(out)
        }
    }
}

pub fn validate_name(n: &str) -> Result<String, String> {
    let u = n.trim().to_ascii_uppercase();
    if u.len() != 6 || !u.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_') {
        return Err(format!("area name '{n}': 6 letters or digits (e.g. AR9900)"));
    }
    Ok(u)
}

fn sibling(p: &Path, suffix: &str) -> PathBuf {
    let s = p.to_string_lossy();
    let stem = s.strip_suffix(".png").or_else(|| s.strip_suffix(".PNG")).unwrap_or(&s);
    PathBuf::from(format!("{stem}{suffix}.png"))
}

fn load_rgb(p: &Path, w: usize, h: usize) -> Result<Vec<u8>, String> {
    let img = image::open(p).map_err(|e| format!("{}: {e}", p.display()))?.to_rgb8();
    if (img.width() as usize, img.height() as usize) != (w, h) {
        return Err(format!("{}: {}x{}, the map needs {w}x{h}", p.display(), img.width(), img.height()));
    }
    Ok(img.into_raw())
}

struct Item<'a> {
    m: &'a str,
    p: [f64; 3],
    q: [f64; 4],
    s: f64,
    sz: [f64; 3],
    bam: Option<&'a str>,
    /// the editor's piece id (wall edits follow the piece, not its place in the list)
    id: Option<&'a str>,
    /// the editor's wall edit for this piece: {"off": true}, or {"pts": [[[dx, dy], ...], ...]} in area pixels from
    /// the piece's ground point
    wall: Option<&'a Value>,
}

fn items(doc: &Value) -> Vec<Item<'_>> {
    let arr = |v: Option<&Value>, d: &[f64]| -> Vec<f64> {
        v.and_then(|v| v.as_array()).map(|a| a.iter().map(num).collect()).unwrap_or_else(|| d.to_vec())
    };
    doc.get("items")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .map(|it| {
                    let p = arr(it.get("p"), &[0.0, 0.0, 0.0]);
                    let q = arr(it.get("q"), &[0.0, 0.0, 0.0, 1.0]);
                    let sz = arr(it.get("sz"), &[1.0, 1.0, 1.0]);
                    Item {
                        m: it.get("m").and_then(|v| v.as_str()).unwrap_or(""),
                        p: [
                            p.first().copied().unwrap_or(0.0),
                            p.get(1).copied().unwrap_or(0.0),
                            p.get(2).copied().unwrap_or(0.0),
                        ],
                        q: [q[0], q[1], q[2], q[3]],
                        s: it.get("s").map(num).unwrap_or(1.0),
                        sz: [sz[0], sz[1], sz[2]],
                        bam: it.get("bam").and_then(|v| v.as_str()),
                        id: it.get("id").and_then(|v| v.as_str()),
                        wall: it.get("wall").filter(|w| w.is_object()),
                    }
                })
                .collect()
        })
        .unwrap_or_default()
}

/// build the area. `progress(pct, step)` is called between steps.
pub fn build(input: &BuildInput, opts: &BuildOptions, progress: &dyn Fn(u32, &str)) -> Result<BuildReport, String> {
    let t0 = Instant::now();
    let sado = match &opts.mode {
        Mode::SadoQolCamp { template_dir } => Some(template_dir.clone()),
        Mode::Generic => None,
    };
    let name = if sado.is_some() { "SQCAMP".to_string() } else { validate_name(&opts.name)? };
    progress(0, "Reading the map");
    let doc = read_doc(&input.map)?;
    let models = load_models(&input.models)?;
    let its = items(&doc);
    let pxm = match doc.get("px_per_m").and_then(|v| v.as_f64()) {
        Some(v) if v != 60.0 => v,
        _ => PXM_DEFAULT,
    };
    let se = sin_e();
    let size_m = doc.get("size_m").and_then(|v| v.as_array()).ok_or("map: no size_m")?;
    let (sw, sh) = (num(&size_m[0]), num(size_m.get(1).unwrap_or(&Value::Null)));
    let tw = (sw * pxm / 64.0).round_ties_even() as usize;
    let th = (sh * pxm * se / 64.0).round_ties_even() as usize;
    if tw == 0 || th == 0 {
        return Err("map: empty size".into());
    }
    let (mw, mh) = (tw * 64, th * 64);
    let px = |x: f64, z: f64| -> Pt { (pround(x * pxm), pround(z * pxm * se)) };

    // spots in area pixels; missing ones fall back around the fire / the map centre
    let sp = doc.get("spots").cloned().unwrap_or(Value::Null);
    let spot_p =
        |k: &str| sp.get(k).and_then(|s| s.get("p")).and_then(|p| p.as_array()).map(|p| (num(&p[0]), num(&p[1])));
    let fire = spot_p("FIRE").unwrap_or((sw / 2.0, sh / 2.0));
    let defaults: [(&str, f64, f64, i64); 9] = [
        ("FIRE", 0.0, 0.0, 0),
        ("ENTRY", -6.0, 4.0, 8),
        ("PRIEST", -3.0, -3.0, 0),
        ("MERCH", 3.0, -3.0, 0),
        ("CAIN", 4.0, 1.0, 4),
        ("WITHERS", -6.0, 1.0, 12),
        ("RECRUIT", -5.0, 2.0, 0),
        ("NECRO", 6.0, 4.0, 12),
        ("NECRO_CENTER", 9.0, 4.0, 0),
    ];
    let mut spots: Vec<(String, Spot)> = Vec::new();
    for (k, dx, dz, f) in defaults {
        let given = sp.get(k).filter(|s| s.is_object());
        let (x, y) = match spot_p(k) {
            Some((x, z)) => px(x, z),
            None => px(fire.0 + dx, fire.1 + dz),
        };
        let facing = given.map(|s| s.get("f").map(num).unwrap_or(0.0) as i64).unwrap_or(f);
        let has_f = !matches!(k, "FIRE" | "NECRO_CENTER" | "RECRUIT");
        // a generic area only keeps the spots the map really has (and its entrance)
        if sado.is_none() && given.is_none() && k != "ENTRY" {
            continue;
        }
        spots.push((k.to_string(), Spot { x, y, f: has_f.then_some(facing) }));
    }
    let mut exits: Vec<Vec<Pt>> = doc
        .get("exits")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|e| e.get("pts").and_then(|p| p.as_array()))
                .map(|pts| pts.iter().filter_map(|p| p.as_array()).map(|p| px(num(&p[0]), num(&p[1]))).collect())
                .collect()
        })
        .unwrap_or_default();
    if exits.is_empty() {
        let c = (mh / 2) as i32;
        exits = vec![vec![(0, c - 120), (60, c - 120), (60, c + 120), (0, c + 120)]];
    }
    // the arrival point must not stand in an exit (the party would be sent straight back to the world map): move it
    // just outside, on the map side of that exit
    let mut report = BuildReport { name: name.clone(), tiles: [tw, th], ..Default::default() };
    let ei = spots.iter().position(|(k, _)| k == "ENTRY").unwrap();
    let (mut ex, mut ey) = (spots[ei].1.x, spots[ei].1.y);
    for poly in &exits {
        let (x0, x1) = (poly.iter().map(|p| p.0).min().unwrap_or(0), poly.iter().map(|p| p.0).max().unwrap_or(0));
        let (y0, y1) = (poly.iter().map(|p| p.1).min().unwrap_or(0), poly.iter().map(|p| p.1).max().unwrap_or(0));
        if x0 <= ex && ex <= x1 && y0 <= ey && ey <= y1 {
            let gaps = [(x1 + 90 - ex, 0), (x0 - 90 - ex, 0), (0, y1 + 70 - ey), (0, y0 - 70 - ey)];
            let ok = gaps.iter().filter(|g| {
                let (nx, ny) = (ex + g.0, ey + g.1);
                40 < nx && nx < mw as i32 - 40 && 40 < ny && ny < mh as i32 - 40
            });
            let mut best: Option<(i32, i32)> = None;
            for g in ok {
                if best.is_none_or(|b| g.0.abs() + g.1.abs() < b.0.abs() + b.1.abs()) {
                    best = Some(*g);
                }
            }
            let (dx, dy) = best.ok_or("the ENTRY spot is in an exit with no room around it")?;
            ex += dx;
            ey += dy;
            report.entry_moved = Some([dx, dy]);
        }
    }
    spots[ei].1.x = ex;
    spots[ei].1.y = ey;

    std::fs::create_dir_all(&opts.out_dir).map_err(|e| format!("{}: {e}", opts.out_dir.display()))?;
    let mut written: Vec<String> = Vec::new();
    let mut write = |n: String, b: &[u8]| -> Result<(), String> {
        let p = opts.out_dir.join(&n);
        std::fs::write(&p, b).map_err(|e| format!("{}: {e}", p.display()))?;
        written.push(n);
        Ok(())
    };

    progress(5, "Reading the render");
    let sc = opts.scale.max(1);
    let hd_dir = format!("HD{sc}X");
    let write_hd = |n: String, b: &[u8]| -> Result<(), String> {
        let d = opts.out_dir.join(&hd_dir);
        std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        std::fs::write(d.join(&n), b).map_err(|e| format!("{}/{n}: {e}", d.display()))
    };
    let hd = load_rgb(&input.render, mw * sc, mh * sc)?;
    if sc > 1 {
        progress(7, "HD tiles (PVRZ)");
        let (t, p) = pvrz::build_scaled(&hd, tw, th, &name, sc);
        write_hd(format!("{name}.TIS"), &t)?;
        for (n, b) in &p {
            write_hd(format!("{n}.PVRZ"), b)?;
        }
        report.hd_dir = Some(hd_dir.clone());
    }
    let rgb = pvrz::downscale(&hd, mw, mh, sc);
    drop(hd);
    progress(10, "Tiles (PVRZ)");
    let (tis, pages) = pvrz::build(&rgb, tw, th, &name);
    write(format!("{name}.TIS"), &tis)?;
    report.pages = pages.len();
    for (n, b) in &pages {
        write(format!("{n}.PVRZ"), b)?;
    }

    progress(35, "Walls");
    let alpha = input.alpha.clone().unwrap_or_else(|| sibling(&input.render, "_alpha"));
    let id = input.id.clone().unwrap_or_else(|| sibling(&input.render, "_id"));
    let (mut walls, auto) = walls(&its, &models, &id, &alpha, mw, mh, pxm, pxm * se)?;
    // walls drawn by hand in the editor, not tied to a piece: their ground line is their bottom
    for poly in doc.get("walls").and_then(|v| v.as_array()).into_iter().flatten() {
        let v: Vec<Pt> = poly
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| p.as_array().map(|p| (pround(num(&p[0])), pround(num(&p[1])))))
            .collect();
        if v.len() >= 3 {
            let yb = v.iter().map(|p| p.1).max().unwrap_or(0);
            walls.push(baseline(v, yb));
        }
    }
    report.auto_walls = auto;
    report.walls = walls.len();
    write(format!("{name}.WED"), &files::wed(tw, th, &walls, &name))?;

    if let Some(n) = &input.night {
        progress(50, "Night tiles (PVRZ)");
        let nhd = load_rgb(n, mw * sc, mh * sc)?;
        let nn = format!("{name}N");
        if sc > 1 {
            let (t, p) = pvrz::build_scaled(&nhd, tw, th, &nn, sc);
            write_hd(format!("{nn}.TIS"), &t)?;
            for (n, b) in &p {
                write_hd(format!("{n}.PVRZ"), b)?;
            }
        }
        let nrgb = pvrz::downscale(&nhd, mw, mh, sc);
        drop(nhd);
        let (ntis, npages) = pvrz::build(&nrgb, tw, th, &nn);
        write(format!("{nn}.TIS"), &ntis)?;
        for (n, b) in &npages {
            write(format!("{n}.PVRZ"), b)?;
        }
        write(format!("{nn}.WED"), &files::wed(tw, th, &walls, &nn))?;
    }

    progress(75, "Search map");
    let fix = match &input.walk {
        Some(w) if w.exists() => read_doc(w)?.get("fix").and_then(|v| v.as_str()).map(String::from),
        _ => None,
    };
    let (cw, ch) = (mw / 16, mh / 12);
    let search = search_map(&its, &models, pxm, cw, ch, fix.as_deref(), &exits)?;
    report.blocked_cells = search.iter().filter(|&&v| v == BLOCK).count();
    let tpl_pal = |file: &str, fallback: &[u8]| -> Vec<u8> {
        sado.as_ref()
            .and_then(|t| std::fs::read(t.join(file)).ok())
            .and_then(|b| files::bmp_palette(&b))
            .unwrap_or_else(|| fallback.to_vec())
    };
    write(format!("{name}SR.BMP"), &files::bmp(&search, cw, ch, 4, &tpl_pal("SQCAMPSR.BMP", &palettes::SR))?)?;
    write(
        format!("{name}HT.BMP"),
        &files::bmp(&vec![8u8; cw * ch], cw, ch, 8, &tpl_pal("SQCAMPHT.BMP", &palettes::HT))?,
    )?;
    write(
        format!("{name}LM.BMP"),
        &files::bmp(&vec![0u8; cw * ch], cw, ch, 8, &tpl_pal("SQCAMPLM.BMP", &palettes::LM))?,
    )?;
    let ln_ok = match &sado {
        Some(t) => t.join("SQCAMPLN.BMP").exists(),
        None => true,
    };
    if input.night.is_some() && ln_ok {
        write(
            format!("{name}LN.BMP"),
            &files::bmp(&vec![0u8; cw * ch], cw, ch, 8, &tpl_pal("SQCAMPLN.BMP", &palettes::LN))?,
        )?;
    }

    progress(85, "Area");
    // flames: the editor's flame pieces first (camp fires), then one per light source
    let fire_pieces: Vec<(String, i32, i32)> = its
        .iter()
        .filter(|it| it.m == "flame")
        .map(|it| {
            let (x, y) = px(it.p[0], it.p[2]);
            (it.bam.unwrap_or(CAMPFIRE_BAM).to_string(), x, y)
        })
        .collect();
    let mut flames: Vec<(String, String, i32, i32)> = Vec::new();
    if !fire_pieces.is_empty() {
        for (i, (b, x, y)) in fire_pieces.iter().enumerate() {
            let n = if i == 0 { "Camp_Fire".to_string() } else { format!("Camp_Fire{}", i + 1) };
            flames.push((n, b.clone(), *x, *y));
        }
    } else if sado.is_some() {
        // older maps kept the camp fire on the FIRE spot
        let fb = sp.get("FIRE").and_then(|s| s.get("bam")).and_then(|v| v.as_str());
        if fb != Some("none") {
            let f = spots.iter().find(|(k, _)| k == "FIRE").map(|(_, s)| *s).unwrap();
            flames.push(("Camp_Fire".into(), fb.unwrap_or(CAMPFIRE_BAM).into(), f.x, f.y));
        }
    }
    let mut lights = 0;
    for it in &its {
        let Some(m) = models.get(it.m) else { continue };
        let Some(&(label, bam, share)) = FLAME_BAM.iter().find(|f| f.0 == m.label) else { continue };
        let (x, y) = px(it.p[0], it.p[2]);
        let top = m.dims[2] * it.s * share * (ELEV_DEG * PI / 180.0).cos() * pxm;
        lights += 1;
        flames.push((format!("{}_{lights}", title(label)), bam.into(), x, (y as f64 - top).trunc() as i32));
    }
    report.flames = flames.len();

    let area =
        Area { name: name.clone(), tw, th, pxm, spots, exits, walls, search, flames, night: input.night.is_some() };
    match &sado {
        Some(t) => {
            let tpl = std::fs::read(t.join("SQCAMP.ARE")).map_err(|e| format!("template SQCAMP.ARE: {e}"))?;
            write(format!("{name}.ARE"), &files::are_sadoqol(&tpl, &area)?)?;
            write("M_SQ0CMP.lua".into(), files::lua(&area).as_bytes())?;
        }
        None => {
            write(format!("{name}.ARE"), &files::are_generic(&area))?;
            // tested in BGEE 2.7.3: MoveToArea takes the selected characters there (C:Eval with LeaveAreaLUAPanic did
            // nothing most of the time)
            {
                let cmd = format!("C:MoveToArea(\"{name}\")");
                let txt = format!(
                    "{name}: a playable area made with MapSmithEE.\r\n\r\n\
                     1. Copy every file of this folder except preview.jpg and this file into the game's override folder.\r\n\
                     2. In Baldur.lua (Documents\\<game>), set: SetPrivateProfileString('Program Options','Debug Mode','1')\r\n\
                     3. Load a game, select the whole party, press Ctrl+Space and type:\r\n\r\n   {cmd}\r\n\r\n\
                     The selected characters are taken to the area. Walk into an exit with the whole party to reach the world map.\r\n"
                );
                write("HOW_TO_TEST.txt".into(), txt.as_bytes())?;
                report.console = Some(cmd);
            }
        }
    }

    progress(92, "Preview");
    write("preview.jpg".into(), &preview::draw(&rgb, mw, mh, &area)?)?;
    report.files = written;
    report.ms = t0.elapsed().as_millis() as u64;
    progress(100, "Done");
    Ok(report)
}

/// Python's str.title()
fn title(s: &str) -> String {
    let mut out = String::new();
    let mut prev_alpha = false;
    for c in s.chars() {
        out.push(if prev_alpha { c.to_ascii_lowercase() } else { c.to_ascii_uppercase() });
        prev_alpha = c.is_alphabetic();
    }
    out.replace(' ', "_")
}

// ---------------------------------------------------------------- search map

enum Foot {
    Circle(f64),
    Rect(f64, f64, f64),
}

fn footprint(it: &Item, models: &HashMap<String, Model>) -> Option<Foot> {
    let m = models.get(it.m)?;
    let (w, dp) = (m.dims[0] * it.s * it.sz[0], m.dims[1] * it.s * it.sz[2]);
    let l = m.label.as_str();
    if NOBLOCK.contains(&l) || w.max(dp) < 0.55 {
        return None;
    }
    if TREES.contains(&l) {
        return Some(Foot::Circle(0.35f64.max(0.45 * it.s)));
    }
    if l == "bush" {
        return Some(Foot::Circle(0.35 * w.max(dp)));
    }
    if l == "bg grove" {
        return Some(Foot::Circle(0.35 * w));
    }
    let [qx, qy, qz, qw] = it.q;
    // three.js Euler 'YXZ' y
    let yaw = (2.0 * (qw * qy + qx * qz)).atan2(1.0 - 2.0 * (qy * qy + qx * qx));
    Some(Foot::Rect(w * 0.85, dp * 0.85, yaw))
}

/// decode the editor's correction PNG (data URL) and scale it to the grid, nearest neighbour
fn fix_cells(url: &str, cw: usize, ch: usize) -> Result<Vec<[u8; 4]>, String> {
    use base64::Engine as _;
    let b64 = url.split_once(',').map(|x| x.1).unwrap_or(url);
    let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| format!("walk fix: {e}"))?;
    let img = image::load_from_memory(&bytes).map_err(|e| format!("walk fix: {e}"))?.to_rgba8();
    let (sw, sh) = (img.width() as usize, img.height() as usize);
    let mut out = Vec::with_capacity(cw * ch);
    for y in 0..ch {
        let sy = (((y as f64 + 0.5) * sh as f64 / ch as f64) as usize).min(sh - 1);
        for x in 0..cw {
            let sx = (((x as f64 + 0.5) * sw as f64 / cw as f64) as usize).min(sw - 1);
            out.push(img.get_pixel(sx as u32, sy as u32).0);
        }
    }
    Ok(out)
}

fn search_map(
    its: &[Item],
    models: &HashMap<String, Model>,
    pxm: f64,
    cw: usize,
    ch: usize,
    fix: Option<&str>,
    exits: &[Vec<Pt>],
) -> Result<Vec<u8>, String> {
    let cell = (16.0 / pxm, 12.0 / (pxm * sin_e()));
    let mut img = vec![0u8; cw * ch];
    let mut g = raster::Grid { px: &mut img, w: cw as i64, h: ch as i64 };
    for it in its {
        let Some(f) = footprint(it, models) else { continue };
        let (cx, cz) = (it.p[0] / cell.0, it.p[2] / cell.1);
        match f {
            Foot::Circle(r) => {
                let ax = (r / cell.0).round_ties_even().max(1.0) as i64;
                let ay = (r / cell.1).round_ties_even().max(1.0) as i64;
                g.fill_ellipse((cx.round_ties_even() as i64, cz.round_ties_even() as i64), (ax, ay), 1);
            }
            Foot::Rect(w, d, yaw) => {
                let (s, c) = (-yaw).sin_cos();
                let pts: Vec<(i64, i64)> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
                    .iter()
                    .map(|&(sx, sz)| {
                        let (x, z) = (sx * w / 2.0, sz * d / 2.0);
                        (((x * c - z * s) / cell.0 + cx) as i64, ((x * s + z * c) / cell.1 + cz) as i64)
                    })
                    .collect();
                g.fill_poly(&pts, 1);
            }
        }
    }
    let mut out: Vec<u8> = img.iter().map(|&v| if v > 0 { BLOCK } else { WALK }).collect();
    if let Some(url) = fix {
        // the editor's corrections: red = blocked, green = free
        for (i, p) in fix_cells(url, cw, ch)?.iter().enumerate() {
            if p[3] >= 128 {
                out[i] = if p[0] > p[1] { BLOCK } else { WALK };
            }
        }
    }
    // the map's rim is closed, except where an exit opens it
    let mut open = vec![0u8; cw * ch];
    let mut og = raster::Grid { px: &mut open, w: cw as i64, h: ch as i64 };
    for poly in exits {
        let pts: Vec<(i64, i64)> =
            poly.iter().map(|&(x, y)| ((x as f64 / 16.0) as i64, (y as f64 / 12.0) as i64)).collect();
        og.fill_poly(&pts, 1);
    }
    for y in 0..ch {
        for x in 0..cw {
            if (x == 0 || y == 0 || x == cw - 1 || y == ch - 1) && open[y * cw + x] == 0 {
                out[y * cw + x] = BLOCK;
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- walls

/// wall polygons: per piece taller than MIN_HEIGHT, its pixels in the id pass where the coverage pass is solid,
/// closed 5x5 (leaf gaps: one shape), outer contours, Douglas-Peucker 2 px, area >= 80, then anchored on the piece's
/// lowest row (`baseline`). `xpm` / `ypm`: area pixels per metre across / in depth. A piece's wall edit replaces
/// its traced walls. Returns the walls and, for the editor, each piece's traced ones
fn walls(
    its: &[Item],
    models: &HashMap<String, Model>,
    id: &Path,
    alpha: &Path,
    w: usize,
    h: usize,
    xpm: f64,
    ypm: f64,
) -> Result<(Vec<Vec<Pt>>, Vec<AutoWall>), String> {
    let ids = image::open(id).map_err(|e| format!("{}: {e}", id.display()))?.to_rgb8();
    let cov = image::open(alpha).map_err(|e| format!("{}: {e}", alpha.display()))?.to_rgba8();
    for (p, im) in [(id, (ids.width(), ids.height())), (alpha, (cov.width(), cov.height()))] {
        if (im.0 as usize, im.1 as usize) != (w, h) {
            return Err(format!("{}: {}x{}, the map needs {w}x{h}", p.display(), im.0, im.1));
        }
    }
    let (ids, cov) = (ids.into_raw(), cov.into_raw());
    let n = its.len();
    // per piece: pixel count and bounding box, one pass
    let label: Vec<u32> = (0..w * h)
        .into_par_iter()
        .map(|i| {
            if cov[i * 4 + 3] > 110 {
                ((ids[i * 3] as u32) << 16) | ((ids[i * 3 + 1] as u32) << 8) | ids[i * 3 + 2] as u32
            } else {
                0
            }
        })
        .collect();
    // renders made before the id pass lost its dither carry +-1 noise on some pixels (a piece's number read as its
    // neighbour's): each pixel takes the label most of its 3x3 block has (the centre wins ties)
    let label: Vec<u32> = (0..w * h)
        .into_par_iter()
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let mut seen: [(u32, u8); 9] = [(0, 0); 9];
            let mut k = 0;
            for yy in y.saturating_sub(1)..(y + 2).min(h) {
                for xx in x.saturating_sub(1)..(x + 2).min(w) {
                    let l = label[yy * w + xx];
                    match seen[..k].iter_mut().find(|s| s.0 == l) {
                        Some(s) => s.1 += 1,
                        None => {
                            seen[k] = (l, 1);
                            k += 1;
                        }
                    }
                }
            }
            let c = label[i];
            let cn = seen[..k].iter().find(|s| s.0 == c).map_or(0, |s| s.1);
            seen[..k].iter().filter(|s| s.1 > cn).max_by_key(|s| s.1).map_or(c, |s| s.0)
        })
        .collect();
    let mut boxes = vec![(0usize, usize::MAX, usize::MAX, 0usize, 0usize); n + 1];
    for y in 0..h {
        for x in 0..w {
            let k = label[y * w + x] as usize;
            if k == 0 || k > n {
                continue;
            }
            let b = &mut boxes[k];
            b.0 += 1;
            b.1 = b.1.min(x);
            b.2 = b.2.min(y);
            b.3 = b.3.max(x);
            b.4 = b.4.max(y);
        }
    }
    let per: Vec<(Vec<Vec<Pt>>, Option<AutoWall>)> = (0..n)
        .into_par_iter()
        .map(|k| {
            let it = &its[k];
            let g = (pround(it.p[0] * xpm), pround(it.p[2] * ypm));
            let traced: Vec<Vec<Pt>> = match models.get(it.m) {
                Some(m) => {
                    let ht = m.dims[2] * it.s * it.sz[1];
                    let b = boxes[k + 1];
                    if ht < MIN_HEIGHT || NOBLOCK.contains(&m.label.as_str()) || b.0 < MIN_PIXELS {
                        Vec::new()
                    } else {
                        contour::piece_contours(&label, w, h, (k + 1) as u32, [b.1, b.2, b.3, b.4], 2)
                            .into_iter()
                            .filter(|c| contour::area(c) >= MIN_AREA)
                            .map(|c| contour::approx_closed(&c, DP_EPS))
                            .filter(|a| a.len() >= 3)
                            .collect()
                    }
                }
                None => Vec::new(),
            };
            let edit = it.wall;
            let off = edit.and_then(|e| e.get("off")).and_then(|v| v.as_bool()) == Some(true);
            let own: Option<Vec<Vec<Pt>>> = edit.and_then(|e| e.get("pts")).and_then(|v| v.as_array()).map(|polys| {
                polys
                    .iter()
                    .map(|poly| {
                        poly.as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(|p| p.as_array().map(|p| (g.0 + pround(num(&p[0])), g.1 + pround(num(&p[1])))))
                            .collect::<Vec<Pt>>()
                    })
                    .filter(|v| v.len() >= 3)
                    .collect()
            });
            let used = if off { Vec::new() } else { own.unwrap_or_else(|| traced.clone()) };
            let auto = (!traced.is_empty()).then(|| AutoWall { i: k, id: it.id.map(String::from), g, pts: traced });
            // the ground line at the piece's lowest point on screen: a character whose feet are lower is in front, the
            // others behind
            let yb = used.iter().flatten().map(|p| p.1).max().unwrap_or(g.1);
            (used.into_iter().map(|v| baseline(v, yb)).collect(), auto)
        })
        .collect();
    let mut out = Vec::new();
    let mut autos = Vec::new();
    for (v, a) in per {
        out.extend(v);
        autos.extend(a);
    }
    Ok((out, autos))
}

/// The engine's "behind the wall" line is a wall's first two vertices (flag 1), extended: BioWare's WEDs start every
/// object outline with its two ground points. A character whose feet are above that line is behind. A traced outline
/// starts anywhere, so the line went any way (half of the trees hid nobody). Here every wall starts with a 2 px
/// horizontal step at row `yb` (the piece's lowest point), on a thin spike out of its right side: no added area.
fn baseline(v: Vec<Pt>, yb: i32) -> Vec<Pt> {
    let x1 = v.iter().map(|p| p.0).max().unwrap_or(0);
    // the right side's vertex nearest to the ground row: the spike leaves from there
    let ir = (0..v.len()).min_by_key(|&i| ((v[i].1 - yb).abs() - (v[i].0 - x1) * 2, -v[i].0)).unwrap_or(0);
    let mut out = vec![(x1 + 1, yb), (x1 + 3, yb)];
    out.extend_from_slice(&v[ir..]);
    out.extend_from_slice(&v[..=ir]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(validate_name("ar9900").unwrap(), "AR9900");
        assert!(validate_name("AR990").is_err());
        assert!(validate_name("AR99-0").is_err());
        assert_eq!(title("lamp post"), "Lamp_Post");
    }

    #[test]
    fn baseline_first_edge() {
        // a tent-like outline: its left foot low, its right side high
        let v = vec![(0, 100), (0, 20), (50, 0), (100, 20), (100, 60)];
        let b = baseline(v.clone(), 100);
        // the first edge is the ground line, then the outline from the right side round to it again
        assert_eq!(&b[..2], &[(101, 100), (103, 100)]);
        assert_eq!(b.len(), v.len() + 3);
        assert!(v.iter().all(|p| b.contains(p)));
    }
}
