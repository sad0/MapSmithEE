//! The user's own pieces and ground textures ("Import…" in the Camp 3D editor), kept next to the model library in
//! `<app data>/userlib`. render_map.py (MAPSMITH_USERLIB) and the area build read it like the library:
//!   models.json              render entries (absolute paths)
//!   proxies/index_user.json  dims + label per piece (area build, fire lights)
//!   editor.json              the editor's entries: label, group, dims, kb, px, name, kind
//!   editor/<id>.glb          the editor's copy; thumbs/<id>.png its tray picture
//!   assets/<id>.glb          a model's full copy (import_model.py); sprites/<id>_tree.png, <id>_shadow.png
//!   textures.json            [{id, name}]; tex/<id>/diff_2k.jpg (render), tex/<id>.jpg (editor, 512 px)

use image::{GenericImageView, Rgba, RgbaImage};
use serde_json::{Map, Value, json};
use std::path::Path;

/// the editor builds sprites at 42 px/m and rescales them per map (`px: 1`)
const PXM0: f64 = 42.0;
const ELEV: f64 = 38.0 * std::f64::consts::PI / 180.0;

fn io(p: &Path, e: impl std::fmt::Display) -> String {
    format!("{}: {e}", p.display())
}

fn read_obj(p: &Path) -> Map<String, Value> {
    std::fs::read(p).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_json(p: &Path, v: &Value) -> Result<(), String> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d).map_err(|e| io(d, e))?;
    }
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(v).map_err(|e| e.to_string())?).map_err(|e| io(&tmp, e))?;
    std::fs::rename(&tmp, p).map_err(|e| io(p, e))
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

/// "u_<name>_<4 hex>": unique, safe in file names, never a library id (no '~': that marks ground variants)
pub fn new_id(name: &str) -> String {
    let slug: String = name
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .split('_')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("_");
    let slug: String = slug.chars().take(24).collect();
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("u_{}_{:04x}", if slug.is_empty() { "piece".into() } else { slug }, (n ^ (n >> 20)) as u16)
}

fn valid_id(id: &str) -> bool {
    id.starts_with("u_") && id.len() < 64 && id.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
}

/// what the editor needs at start: its piece entries and the ground textures
pub fn list(dir: &Path) -> Value {
    json!({
        "models": Value::Object(read_obj(&dir.join("editor.json"))),
        "textures": std::fs::read(dir.join("textures.json")).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).unwrap_or(json!([])),
    })
}

/// a file of the userlib for the editor (msuser://localhost/<path>): (bytes, content type)
pub fn serve(dir: &Path, path: &str) -> Option<(Vec<u8>, &'static str)> {
    let path = path.trim_start_matches('/');
    if path.split('/').any(|s| s.is_empty() || s == "." || s == ".." || s.contains('\\') || s.contains(':')) {
        return None;
    }
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "glb" => "model/gltf-binary",
        _ => return None,
    };
    std::fs::read(dir.join(path)).ok().map(|b| (b, mime))
}

/// add (or replace) one piece in the three tables
fn register(dir: &Path, id: &str, editor: Value, render: Value) -> Result<(), String> {
    let mut e = read_obj(&dir.join("editor.json"));
    e.insert(id.into(), editor.clone());
    write_json(&dir.join("editor.json"), &Value::Object(e))?;
    let mut m = read_obj(&dir.join("models.json"));
    m.insert(id.into(), render);
    write_json(&dir.join("models.json"), &Value::Object(m))?;
    let mut x = read_obj(&dir.join("proxies").join("index_user.json"));
    let keep = ["label", "group", "dims", "kb", "px"];
    let entry: Map<String, Value> = editor.as_object().into_iter().flatten().filter(|(k, _)| keep.contains(&k.as_str())).map(|(k, v)| (k.clone(), v.clone())).collect();
    x.insert(id.into(), Value::Object(entry));
    write_json(&dir.join("proxies").join("index_user.json"), &Value::Object(x))
}

/// remove a piece or a ground texture and its files
pub fn delete(dir: &Path, id: &str) -> Result<(), String> {
    if !valid_id(id) {
        return Err(format!("not a user piece: {id}"));
    }
    for f in ["editor.json", "models.json", "proxies/index_user.json"] {
        let p = dir.join(f);
        let mut t = read_obj(&p);
        if t.remove(id).is_some() {
            write_json(&p, &Value::Object(t))?;
        }
    }
    let tp = dir.join("textures.json");
    if let Ok(Value::Array(mut t)) = std::fs::read(&tp).map_err(|e| e.to_string()).and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string())) {
        let n = t.len();
        t.retain(|e| e.get("id").and_then(Value::as_str) != Some(id));
        if t.len() != n {
            write_json(&tp, &Value::Array(t))?;
        }
    }
    for f in [format!("editor/{id}.glb"), format!("thumbs/{id}.png"), format!("assets/{id}.glb"), format!("sprites/{id}_tree.png"), format!("sprites/{id}_shadow.png"), format!("tex/{id}.jpg")] {
        let _ = std::fs::remove_file(dir.join(f));
    }
    let _ = std::fs::remove_dir_all(dir.join("tex").join(id));
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// ground textures
// ---------------------------------------------------------------------------------------------------------------

/// an image becomes a ground texture (tiled: it should be seamless): 2048 px at most for the render, 512 for the editor
pub fn import_texture(dir: &Path, src: &Path, name: &str) -> Result<Value, String> {
    let img = image::open(src).map_err(|e| io(src, e))?.to_rgb8();
    let id = new_id(name);
    let td = dir.join("tex").join(&id);
    std::fs::create_dir_all(&td).map_err(|e| io(&td, e))?;
    let fit = |img: &image::RgbImage, side: u32| -> image::RgbImage {
        let (w, h) = img.dimensions();
        if w.max(h) <= side {
            img.clone()
        } else {
            let k = side as f64 / w.max(h) as f64;
            image::imageops::resize(img, ((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1), image::imageops::FilterType::Lanczos3)
        }
    };
    let save = |img: &image::RgbImage, p: &Path| -> Result<(), String> {
        let f = std::fs::File::create(p).map_err(|e| io(p, e))?;
        image::codecs::jpeg::JpegEncoder::new_with_quality(std::io::BufWriter::new(f), 90).encode_image(img).map_err(|e| io(p, e))
    };
    save(&fit(&img, 2048), &td.join("diff_2k.jpg"))?;
    save(&fit(&img, 512), &dir.join("tex").join(format!("{id}.jpg")))?;
    let tp = dir.join("textures.json");
    let mut t: Vec<Value> = std::fs::read(&tp).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let e = json!({ "id": id, "name": name.trim() });
    t.push(e.clone());
    write_json(&tp, &Value::Array(t))?;
    Ok(e)
}

// ---------------------------------------------------------------------------------------------------------------
// 2D sprites: a cut-out facing BG's camera at its size in game pixels, its shadow painted on the ground
// ---------------------------------------------------------------------------------------------------------------

pub struct Sprite {
    pub tree: RgbaImage,
    pub shadow: RgbaImage,
    /// the pixel that stands on the ground
    pub foot: (f64, f64),
}

/// the trunk's base: the lowest opaque rows, their median x
fn foot_of(img: &RgbaImage) -> (f64, f64) {
    let (w, h) = img.dimensions();
    let Some(yb) = (0..h).rev().find(|&y| (0..w).any(|x| img.get_pixel(x, y)[3] > 128)) else {
        return (w as f64 / 2.0, h as f64);
    };
    let mut xs: Vec<u32> = (yb.saturating_sub(6)..=yb).flat_map(|y| (0..w).filter(move |&x| img.get_pixel(x, y)[3] > 128)).collect();
    xs.sort_unstable();
    (xs[xs.len() / 2] as f64, yb as f64 + 1.0)
}

/// a BAM V1 / BAMC frame (cycle, frame in the cycle) as RGBA, with its centre (BG's anchor: the feet)
pub fn bam_frame(data: &[u8], cycle: usize, frame: usize) -> Result<(RgbaImage, (i32, i32)), String> {
    let un;
    let d: &[u8] = if data.starts_with(b"BAMC") {
        use std::io::Read;
        let mut v = Vec::new();
        flate2::read::ZlibDecoder::new(data.get(12..).ok_or("BAMC: too short")?).read_to_end(&mut v).map_err(|e| format!("BAMC: {e}"))?;
        un = v;
        &un
    } else {
        data
    };
    if d.starts_with(b"BAM V2") {
        return Err("BAM V2 (PVRZ) files are not supported yet: export the frame as PNG".into());
    }
    if !d.starts_with(b"BAM V1  ") {
        return Err("not a BAM file".into());
    }
    let u16_ = |o: usize| d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or("BAM: truncated");
    let u32_ = |o: usize| d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or("BAM: truncated");
    let (nf, nc, ck) = (u16_(8)? as usize, d[10] as usize, d[11]);
    let (ofr, opal, olut) = (u32_(12)? as usize, u32_(16)? as usize, u32_(20)? as usize);
    if cycle >= nc {
        return Err(format!("BAM: cycle {cycle} of {nc}"));
    }
    let co = ofr + nf * 12 + cycle * 4;
    let (cnt, first) = (u16_(co)? as usize, u16_(co + 2)? as usize);
    if frame >= cnt {
        return Err(format!("BAM: frame {frame} of {cnt} in cycle {cycle}"));
    }
    let fi = u16_(olut + (first + frame) * 2)? as usize;
    let fo = ofr + fi * 12;
    let (w, h) = (u16_(fo)? as u32, u16_(fo + 2)? as u32);
    let (cx, cy) = (u16_(fo + 4)? as i16 as i32, u16_(fo + 6)? as i16 as i32);
    let off = u32_(fo + 8)?;
    let n = (w * h) as usize;
    let mut raw = Vec::with_capacity(n);
    let start = (off & 0x7fff_ffff) as usize;
    if off & 0x8000_0000 != 0 {
        raw.extend_from_slice(d.get(start..start + n).ok_or("BAM: truncated frame")?);
    } else {
        let mut p = start;
        while raw.len() < n {
            let b = *d.get(p).ok_or("BAM: truncated frame")?;
            p += 1;
            if b == ck {
                let k = *d.get(p).ok_or("BAM: truncated frame")? as usize + 1;
                p += 1;
                raw.extend(std::iter::repeat_n(ck, k));
            } else {
                raw.push(b);
            }
        }
    }
    let pal = d.get(opal..opal + 1024).ok_or("BAM: no palette")?;
    let mut img = RgbaImage::new(w, h);
    for (i, &ix) in raw.iter().take(n).enumerate() {
        let q = &pal[ix as usize * 4..ix as usize * 4 + 4];
        let a = if ix == 0 { 0 } else if q[3] == 0 { 255 } else { q[3] };
        img.put_pixel(i as u32 % w, i as u32 / w, Rgba([q[2], q[1], q[0], a]));
    }
    Ok((img, (cx, cy)))
}

/// a picture's shadow layer: `<stem>_shadow.png` next to it (BG element library: a darkening mask, placed by its
/// catalog.json entry's `shadow` offset), else none
fn sibling_shadow(src: &Path) -> Option<(image::GrayImage, (i32, i32))> {
    let stem = src.file_stem()?.to_string_lossy().into_owned();
    let sp = src.with_file_name(format!("{stem}_shadow.png"));
    let sh = image::open(&sp).ok()?;
    // an RGBA shadow carries its darkness in alpha; a grey one is the darkness itself
    let g = if sh.color().has_alpha() { image::GrayImage::from_fn(sh.width(), sh.height(), |x, y| image::Luma([sh.get_pixel(x, y)[3]])) } else { sh.to_luma8() };
    let off = src
        .parent()
        .and_then(Path::parent)
        .and_then(|d| std::fs::read(d.join("catalog.json")).ok())
        .and_then(|b| serde_json::from_slice::<Vec<Value>>(&b).ok())
        .and_then(|c| c.into_iter().find(|e| e.get("id").and_then(Value::as_str) == Some(&stem)))
        .and_then(|e| e.get("shadow").and_then(Value::as_array).map(|a| (a[0].as_f64().unwrap_or(0.0) as i32, a[1].as_f64().unwrap_or(0.0) as i32)))
        .unwrap_or((0, 0));
    Some((g, off))
}

/// PNG (with an optional `_shadow` sibling) or BAM frame -> sprite on one canvas
pub fn load_sprite(src: &Path, cycle: usize, frame: usize) -> Result<Sprite, String> {
    let ext = src.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let (tree, anchor) = if ext == "bam" {
        let (img, (cx, cy)) = bam_frame(&std::fs::read(src).map_err(|e| io(src, e))?, cycle, frame)?;
        let inside = cx > 0 && cy > 0 && (cx as u32) < img.width() && (cy as u32) <= img.height();
        (img, inside.then_some((cx as f64, cy as f64)))
    } else {
        (image::open(src).map_err(|e| io(src, e))?.to_rgba8(), None)
    };
    let shadow = if ext == "bam" { None } else { sibling_shadow(src) };
    let (tw, th) = tree.dimensions();
    let (sw, sh, dx, dy) = shadow.as_ref().map(|(g, o)| (g.width() as i32, g.height() as i32, o.0, o.1)).unwrap_or((0, 0, 0, 0));
    let (x0, y0) = (0.min(dx), 0.min(dy));
    let (x1, y1) = ((tw as i32).max(dx + sw), (th as i32).max(dy + sh));
    let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
    let mut t = RgbaImage::new(w, h);
    image::imageops::overlay(&mut t, &tree, -x0 as i64, -y0 as i64);
    let mut s = RgbaImage::new(w, h);
    if let Some((g, _)) = &shadow {
        for (x, y, p) in g.enumerate_pixels() {
            s.put_pixel((x as i32 + dx - x0) as u32, (y as i32 + dy - y0) as u32, Rgba([0, 0, 0, p[0]]));
        }
    }
    let foot = anchor.unwrap_or_else(|| foot_of(&tree));
    Ok(Sprite { tree: t, shadow: s, foot: (foot.0 - x0 as f64, foot.1 - y0 as f64) })
}

fn png(img: &RgbaImage) -> Vec<u8> {
    let mut b = std::io::Cursor::new(Vec::new());
    img.write_to(&mut b, image::ImageFormat::Png).expect("png in memory");
    b.into_inner()
}

/// the editor's copy (tools/bgsprites.py's): node "bgsprite" with the cut-out "bgtree" and its shadow "bgshadow"
pub fn sprite_glb(sp: &Sprite) -> Vec<u8> {
    let (w, h) = (sp.tree.width() as f64, sp.tree.height() as f64);
    let (fx, fy) = sp.foot;
    let (s, c) = (ELEV.sin(), ELEV.cos());
    let corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
    let tq: Vec<[f32; 3]> = corners.iter().map(|&(x, y)| [((x - fx) / PXM0) as f32, (c * (fy - y) / PXM0) as f32, (-s * (fy - y) / PXM0) as f32]).collect();
    let sq: Vec<[f32; 3]> = corners.iter().map(|&(x, y)| [((x - fx) / PXM0) as f32, 0.015, ((y - fy) / (PXM0 * s)) as f32]).collect();
    let uv: Vec<[f32; 2]> = corners.iter().map(|&(x, y)| [(x / w) as f32, (y / h) as f32]).collect();
    let mut bin: Vec<u8> = Vec::new();
    let mut views: Vec<Value> = Vec::new();
    let mut add = |bin: &mut Vec<u8>, data: &[u8], target: Option<u32>| -> usize {
        while bin.len() % 4 != 0 {
            bin.push(0);
        }
        let mut v = json!({ "buffer": 0, "byteOffset": bin.len(), "byteLength": data.len() });
        if let Some(t) = target {
            v["target"] = json!(t);
        }
        bin.extend_from_slice(data);
        views.push(v);
        views.len() - 1
    };
    let idx: Vec<u8> = [0u16, 2, 1, 0, 3, 2].iter().flat_map(|v| v.to_le_bytes()).collect();
    let f3 = |v: &[[f32; 3]]| -> Vec<u8> { v.iter().flatten().flat_map(|f| f.to_le_bytes()).collect() };
    let v_idx = add(&mut bin, &idx, Some(34963));
    let v_uv = add(&mut bin, &uv.iter().flatten().flat_map(|f| f.to_le_bytes()).collect::<Vec<u8>>(), Some(34962));
    let v_t = add(&mut bin, &f3(&tq), Some(34962));
    let v_s = add(&mut bin, &f3(&sq), Some(34962));
    let i_t = add(&mut bin, &png(&sp.tree), None);
    let i_s = add(&mut bin, &png(&sp.shadow), None);
    let mm = |v: &[[f32; 3]]| -> (Vec<f32>, Vec<f32>) {
        ((0..3).map(|i| v.iter().map(|p| p[i]).fold(f32::MAX, f32::min)).collect(), (0..3).map(|i| v.iter().map(|p| p[i]).fold(f32::MIN, f32::max)).collect())
    };
    let ((tmin, tmax), (smin, smax)) = (mm(&tq), mm(&sq));
    while bin.len() % 4 != 0 {
        bin.push(0);
    }
    let js = json!({
        "asset": { "version": "2.0", "generator": "MapSmithEE userlib" },
        "extensionsUsed": ["KHR_materials_unlit"],
        "scene": 0, "scenes": [{ "nodes": [0] }],
        "nodes": [{ "name": "bgsprite", "children": [1, 2] }, { "name": "bgtree", "mesh": 0 }, { "name": "bgshadow", "mesh": 1 }],
        "meshes": [
            { "name": "bgtree", "primitives": [{ "attributes": { "POSITION": 2, "TEXCOORD_0": 1 }, "indices": 0, "material": 0 }] },
            { "name": "bgshadow", "primitives": [{ "attributes": { "POSITION": 3, "TEXCOORD_0": 1 }, "indices": 0, "material": 1 }] }],
        "materials": [
            { "name": "bgtree", "doubleSided": true, "alphaMode": "MASK", "alphaCutoff": 0.5, "extensions": { "KHR_materials_unlit": {} },
              "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 }, "metallicFactor": 0 } },
            { "name": "bgshadow", "doubleSided": true, "alphaMode": "BLEND", "extensions": { "KHR_materials_unlit": {} },
              "pbrMetallicRoughness": { "baseColorFactor": [0, 0, 0, 0.55], "baseColorTexture": { "index": 1 }, "metallicFactor": 0 } }],
        "textures": [{ "source": 0, "sampler": 0 }, { "source": 1, "sampler": 0 }],
        "samplers": [{ "magFilter": 9729, "minFilter": 9987, "wrapS": 33071, "wrapT": 33071 }],
        "images": [{ "bufferView": i_t, "mimeType": "image/png" }, { "bufferView": i_s, "mimeType": "image/png" }],
        "accessors": [
            { "bufferView": v_idx, "componentType": 5123, "count": 6, "type": "SCALAR" },
            { "bufferView": v_uv, "componentType": 5126, "count": 4, "type": "VEC2" },
            { "bufferView": v_t, "componentType": 5126, "count": 4, "type": "VEC3", "min": tmin, "max": tmax },
            { "bufferView": v_s, "componentType": 5126, "count": 4, "type": "VEC3", "min": smin, "max": smax }],
        "bufferViews": views,
        "buffers": [{ "byteLength": bin.len() }],
    });
    let mut j = serde_json::to_vec(&js).expect("json");
    while j.len() % 4 != 0 {
        j.push(b' ');
    }
    let mut out = Vec::with_capacity(28 + j.len() + bin.len());
    for v in [0x4654_6C67u32, 2, (12 + 8 + j.len() + 8 + bin.len()) as u32, j.len() as u32] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(b"JSON");
    out.extend_from_slice(&j);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(b"BIN\0");
    out.extend_from_slice(&bin);
    out
}

/// a sprite becomes a user piece (`label`: its type, e.g. "bush", which sets its tray category)
pub fn import_sprite(dir: &Path, src: &Path, name: &str, label: &str, cycle: usize, frame: usize) -> Result<Value, String> {
    let sp = load_sprite(src, cycle, frame)?;
    let id = new_id(name);
    for d in ["sprites", "editor", "thumbs"] {
        std::fs::create_dir_all(dir.join(d)).map_err(|e| io(&dir.join(d), e))?;
    }
    let (tp, shp) = (dir.join("sprites").join(format!("{id}_tree.png")), dir.join("sprites").join(format!("{id}_shadow.png")));
    sp.tree.save(&tp).map_err(|e| io(&tp, e))?;
    sp.shadow.save(&shp).map_err(|e| io(&shp, e))?;
    let g = sprite_glb(&sp);
    let gp = dir.join("editor").join(format!("{id}.glb"));
    std::fs::write(&gp, &g).map_err(|e| io(&gp, e))?;
    // tray picture: the shadow a little lighter, the cut-out over it
    let mut th = RgbaImage::new(sp.tree.width(), sp.tree.height());
    for (x, y, p) in sp.shadow.enumerate_pixels() {
        th.put_pixel(x, y, Rgba([0, 0, 0, (p[3] as f32 * 0.6) as u8]));
    }
    image::imageops::overlay(&mut th, &sp.tree, 0, 0);
    let th = image::DynamicImage::ImageRgba8(th).thumbnail(128, 128);
    let thp = dir.join("thumbs").join(format!("{id}.png"));
    th.save(&thp).map_err(|e| io(&thp, e))?;
    // footprint for the walkable map: part of the cut-out's width, a trunk's depth; height: the cut-out's
    let (mut xa, mut xb, mut ya, mut yb) = (u32::MAX, 0, u32::MAX, 0);
    for (x, y, p) in sp.tree.enumerate_pixels() {
        if p[3] > 128 {
            (xa, xb, ya, yb) = (xa.min(x), xb.max(x), ya.min(y), yb.max(y));
        }
    }
    if xa > xb {
        return Err("the picture has no opaque pixel".into());
    }
    let dims = [((xb - xa) as f64 / PXM0 * 0.6 * 100.0).round() / 100.0, 1.0, ((yb - ya) as f64 / PXM0 / ELEV.cos() * 100.0).round() / 100.0];
    let (w, h) = sp.tree.dimensions();
    let editor = json!({ "label": label, "group": "user", "dims": dims, "kb": g.len() / 1024, "px": 1, "name": name.trim(), "kind": "sprite" });
    let render = json!({ "label": label, "group": "user", "px": 1, "credit": "imported by the user",
        "src": { "bgsprite": { "tree": path_str(&tp), "shadow": path_str(&shp), "size": [w, h], "foot": [sp.foot.0, sp.foot.1] } } });
    register(dir, &id, editor.clone(), render)?;
    Ok(json!({ "id": id, "entry": editor }))
}

// ---------------------------------------------------------------------------------------------------------------
// 3D models: Blender converts any format it reads (import_model.py)
// ---------------------------------------------------------------------------------------------------------------

/// the extensions import_model.py takes
pub const MODEL_EXTS: &[&str] = &["glb", "gltf", "obj", "fbx", "stl", "ply", "dae", "usd", "usda", "usdc", "usdz", "abc", "x3d", "blend"];

pub fn import_model(dir: &Path, blender: &Path, scripts: &Path, src: &Path, name: &str, label: &str, height_m: Option<f64>) -> Result<Value, String> {
    let ext = src.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !MODEL_EXTS.contains(&ext.as_str()) {
        return Err(format!(".{ext} files are not supported (supported: {})", MODEL_EXTS.join(", ")));
    }
    let id = new_id(name);
    std::fs::create_dir_all(dir).map_err(|e| io(dir, e))?;
    let mut cmd = std::process::Command::new(blender);
    cmd.arg("-b").args(["--python-exit-code", "1"]).arg("-P").arg(scripts.join("import_model.py")).arg("--");
    cmd.arg(src).arg(path_str(dir)).arg(&id).arg(height_m.filter(|h| *h > 0.0).map(|h| h.to_string()).unwrap_or_else(|| "-".into()));
    cmd.stdin(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| format!("cannot start {}: {e}", blender.display()))?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    if let Some(l) = text.lines().find(|l| l.contains("MAPSMITH error: ")) {
        return Err(l.split("MAPSMITH error: ").nth(1).unwrap_or(l).trim().to_string());
    }
    let res: Value = text
        .lines()
        .find_map(|l| l.strip_prefix("MAPSMITH result "))
        .and_then(|j| serde_json::from_str(j).ok())
        .ok_or_else(|| format!("Blender could not import the model:\n{}", text.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n")))?;
    let kb = std::fs::metadata(dir.join("editor").join(format!("{id}.glb"))).map(|m| m.len() / 1024).unwrap_or(0);
    let editor = json!({ "label": label, "group": "user", "dims": res["dims"], "kb": kb, "name": name.trim(), "kind": "model", "tris": res["tris"] });
    let render = json!({ "label": label, "group": "user", "credit": "imported by the user", "tris": 6000,
        "src": { "glb": path_str(&dir.join("assets").join(format!("{id}.glb"))) }, "fit": res["fit"], "scale": res["scale"] });
    register(dir, &id, editor.clone(), render)?;
    Ok(json!({ "id": id, "entry": editor }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mapsmithee-userlib-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn ids_are_safe() {
        let id = new_id("My Tree #2 (big)");
        assert!(valid_id(&id), "{id}");
        assert!(id.starts_with("u_my_tree_2_big_"));
        assert!(!valid_id("dirt~2") && !valid_id("../x"));
    }

    #[test]
    fn sprite_import_and_delete() {
        let d = tmp("sprite");
        let src = d.join("rock.png");
        let mut img = RgbaImage::new(40, 30);
        for y in 10..30 {
            for x in 5..35 {
                img.put_pixel(x, y, Rgba([120, 110, 90, 255]));
            }
        }
        img.save(&src).unwrap();
        let mut sh = image::GrayImage::new(50, 20);
        sh.put_pixel(3, 3, image::Luma([200]));
        sh.save(d.join("rock_shadow.png")).unwrap();
        let ul = d.join("ul");
        let r = import_sprite(&ul, &src, "Rock", "rock", 0, 0).unwrap();
        let id = r["id"].as_str().unwrap().to_string();
        let m = read_obj(&ul.join("models.json"));
        let b = &m[&id]["src"]["bgsprite"];
        assert_eq!(b["size"], json!([50, 30]));
        assert_eq!(b["foot"], json!([20.0, 30.0]));
        let glb = std::fs::read(ul.join("editor").join(format!("{id}.glb"))).unwrap();
        assert_eq!(&glb[0..4], b"glTF");
        assert_eq!(u32::from_le_bytes(glb[8..12].try_into().unwrap()) as usize, glb.len());
        assert!(list(&ul)["models"][&id]["dims"].is_array());
        delete(&ul, &id).unwrap();
        assert!(read_obj(&ul.join("models.json")).is_empty());
        assert!(!ul.join("editor").join(format!("{id}.glb")).exists());
    }

    #[test]
    fn bam_v1_frame() {
        // 2 x 1 frame, RLE off, palette: 0 transparent green, 1 red
        let mut b = b"BAM V1  ".to_vec();
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&[1, 0]);
        let (ofr, opal, olut) = (24u32, 24 + 12 + 4, 24 + 12 + 4 + 1024);
        for v in [ofr, opal, olut] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        let data_at = olut + 2;
        for v in [2u16, 1, 1, 1] {
            b.extend_from_slice(&v.to_le_bytes());
        }
        b.extend_from_slice(&(data_at | 0x8000_0000).to_le_bytes());
        b.extend_from_slice(&1u16.to_le_bytes());
        b.extend_from_slice(&0u16.to_le_bytes());
        let mut pal = vec![0u8; 1024];
        pal[0..4].copy_from_slice(&[0, 255, 0, 0]);
        pal[4..8].copy_from_slice(&[0, 0, 255, 0]);
        b.extend_from_slice(&pal);
        b.extend_from_slice(&0u16.to_le_bytes());
        b.extend_from_slice(&[0, 1]);
        let (img, c) = bam_frame(&b, 0, 0).unwrap();
        assert_eq!(c, (1, 1));
        assert_eq!(img.get_pixel(0, 0)[3], 0);
        assert_eq!(*img.get_pixel(1, 0), Rgba([255, 0, 0, 255]));
    }

    #[test]
    fn texture_import() {
        let d = tmp("tex");
        let src = d.join("t.png");
        image::RgbImage::from_pixel(3000, 1500, image::Rgb([10, 20, 30])).save(&src).unwrap();
        let ul = d.join("ul");
        let e = import_texture(&ul, &src, "Cobbles").unwrap();
        let id = e["id"].as_str().unwrap();
        assert_eq!(image::open(ul.join("tex").join(id).join("diff_2k.jpg")).unwrap().dimensions(), (2048, 1024));
        assert_eq!(list(&ul)["textures"][0]["name"], "Cobbles");
        delete(&ul, id).unwrap();
        assert_eq!(list(&ul)["textures"], json!([]));
    }
}
