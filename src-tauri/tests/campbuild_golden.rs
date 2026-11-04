//! Golden test: the Rust campbuild against build_camp3d.py's output for the same map and renders.
//! `cargo test --release --test campbuild_golden -- --ignored --nocapture`
//! Paths: CB_MAP, CB_WALK, CB_RENDER (day render; _alpha/_id/_n next to it), CB_MODELS, CB_TEMPLATE, CB_PY_OUT.

use base64::Engine as _;
use mapsmithee_lib::campbuild::{self, BuildInput, BuildOptions, Mode, Models};
use mapsmithee_lib::{are, bmpmap, tis, wed};
use std::path::{Path, PathBuf};

fn env(k: &str, d: &str) -> PathBuf {
    PathBuf::from(std::env::var(k).unwrap_or_else(|_| d.to_string()))
}

fn input() -> BuildInput {
    let render = env("CB_RENDER", "E:/AI/bg3d/game/claude.png");
    let night = PathBuf::from(render.to_string_lossy().replace(".png", "_n.png"));
    BuildInput {
        map: env("CB_MAP", "E:/Dev/MapSmithEE/samples/camp3d/maps/claude_camp.json"),
        walk: Some(env("CB_WALK", "E:/Dev/MapSmithEE/samples/camp3d/maps/claude_camp_walk.json")),
        render,
        alpha: None,
        id: None,
        night: Some(night),
        models: Models::Dir(env("CB_MODELS", "E:/AI/bg3d/proxies")),
    }
}

fn read(p: &Path) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

fn pixels(m: &bmpmap::BmpMap) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(&m.pixels).unwrap()
}

/// the tileset decoded through MapSmithEE's own reader, as RGB
fn decode_tiles(dir: &Path, name: &str) -> (usize, usize, Vec<u8>, usize) {
    let t = read(&dir.join(format!("{name}.TIS")));
    let n = u32::from_le_bytes(t[8..12].try_into().unwrap()) as usize;
    let mut pages = std::collections::HashMap::new();
    let wed = wed::parse(&read(&dir.join(format!("{name}.WED")))).unwrap();
    let (tw, th) = (wed.overlays[0].w as usize, wed.overlays[0].h as usize);
    assert_eq!(n, tw * th);
    let (w, h) = (tw * 64, th * 64);
    let mut rgb = vec![0u8; w * h * 3];
    for i in 0..n {
        let o = 24 + i * 12;
        let pg = i32::from_le_bytes(t[o..o + 4].try_into().unwrap());
        let px = u32::from_le_bytes(t[o + 4..o + 8].try_into().unwrap()) as usize;
        let py = u32::from_le_bytes(t[o + 8..o + 12].try_into().unwrap()) as usize;
        let page = pages
            .entry(pg)
            .or_insert_with(|| tis::pvrz(&read(&dir.join(format!("{}.PVRZ", tis::pvrz_name(name, pg))))).unwrap());
        let (tx, ty) = (i % tw, i / tw);
        for r in 0..64 {
            for c in 0..64 {
                let s = ((py + r) * page.w + px + c) * 4;
                let d = ((ty * 64 + r) * w + tx * 64 + c) * 3;
                rgb[d..d + 3].copy_from_slice(&page.px[s..s + 3]);
            }
        }
    }
    (w, h, rgb, pages.len())
}

fn psnr(a: &[u8], b: &[u8]) -> f64 {
    let mse = a.iter().zip(b).map(|(&x, &y)| (x as f64 - y as f64).powi(2)).sum::<f64>() / a.len() as f64;
    10.0 * (255.0f64 * 255.0 / mse.max(1e-9)).log10()
}

type Poly = Vec<[i16; 2]>;

fn bbox(p: &Poly) -> [i16; 4] {
    let xs = p.iter().map(|v| v[0]);
    let ys = p.iter().map(|v| v[1]);
    [xs.clone().min().unwrap(), ys.clone().min().unwrap(), xs.max().unwrap(), ys.max().unwrap()]
}

/// largest distance from a vertex of `a` to the nearest vertex of `b`, both ways
fn vdist(a: &Poly, b: &Poly) -> f64 {
    let one = |a: &Poly, b: &Poly| {
        a.iter()
            .map(|p| {
                b.iter()
                    .map(|q| (((p[0] - q[0]) as f64).powi(2) + ((p[1] - q[1]) as f64).powi(2)).sqrt())
                    .fold(f64::MAX, f64::min)
            })
            .fold(0.0, f64::max)
    };
    one(a, b).max(one(b, a))
}

#[test]
#[ignore]
fn golden_sqcamp_and_generic() {
    let py = env("CB_PY_OUT", "E:/AI/bg3d/game/SQCAMP_claude");
    let tpl = env("CB_TEMPLATE", "E:/Enhanced Edition/Environnement de test/EET 2.7.3 (dev)/SadoQoL/camp");
    let base = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("campbuild_golden");
    let out = base.join("SQCAMP");
    let _ = std::fs::remove_dir_all(&base);
    let inp = input();
    let t0 = std::time::Instant::now();
    let rep = campbuild::build(
        &inp,
        &BuildOptions { name: "SQCAMP".into(), mode: Mode::SadoQolCamp { template_dir: tpl }, out_dir: out.clone(), scale: 1 },
        &|_, _| {},
    )
    .expect("build");
    println!("SQCAMP built in {} ms -> {}", t0.elapsed().as_millis(), out.display());
    println!("report: {}", serde_json::to_string(&rep).unwrap());

    // search map
    let sr_py = bmpmap::parse(&read(&py.join("SQCAMPSR.BMP"))).unwrap();
    let sr_rs = bmpmap::parse(&read(&out.join("SQCAMPSR.BMP"))).unwrap();
    assert_eq!((sr_py.w, sr_py.h, sr_py.bpp), (sr_rs.w, sr_rs.h, sr_rs.bpp));
    assert_eq!(sr_py.palette, sr_rs.palette);
    let (a, b) = (pixels(&sr_py), pixels(&sr_rs));
    let diff = a.iter().zip(&b).filter(|(x, y)| x != y).count();
    let pct = 100.0 * diff as f64 / a.len() as f64;
    println!("search map: {diff} of {} cells differ ({pct:.3}%)", a.len());
    assert!(pct < 0.5);
    for f in ["SQCAMPHT.BMP", "SQCAMPLM.BMP", "SQCAMPLN.BMP"] {
        assert_eq!(read(&py.join(f)), read(&out.join(f)), "{f}");
    }

    // walls
    let w_py = wed::parse(&read(&py.join("SQCAMP.WED"))).unwrap();
    let w_rs = wed::parse(&read(&out.join("SQCAMP.WED"))).unwrap();
    let (np, nr) = (w_py.walls.len(), w_rs.walls.len());
    println!("walls: python {np}, rust {nr}");
    assert!((np as f64 - nr as f64).abs() <= (np as f64 * 0.03).max(2.0));
    let mut exact = 0;
    let mut worst = 0.0f64;
    let mut unmatched = 0;
    for p in &w_py.walls {
        if w_rs.walls.iter().any(|r| r.pts == p.pts) {
            exact += 1;
            continue;
        }
        let bp = bbox(&p.pts);
        let best = w_rs
            .walls
            .iter()
            .filter(|r| bbox(&r.pts).iter().zip(&bp).all(|(x, y)| (x - y).abs() <= 3))
            .map(|r| vdist(&p.pts, &r.pts))
            .fold(f64::MAX, f64::min);
        if best == f64::MAX {
            unmatched += 1;
        } else {
            worst = worst.max(best);
        }
    }
    println!("walls: {exact} identical, worst vertex offset of the others {worst:.2} px, {unmatched} without a match");
    assert!(unmatched as f64 <= np as f64 * 0.03);
    assert!(worst <= 2.0 || exact == np);
    let same_wed = read(&py.join("SQCAMP.WED")) == read(&out.join("SQCAMP.WED"));
    println!("WED bytes identical: {same_wed}");
    let wn = wed::parse(&read(&out.join("SQCAMPN.WED"))).unwrap();
    assert_eq!(wn.overlays[0].tileset, "SQCAMPN");
    assert_eq!(wn.walls, w_rs.walls);

    // tiles: decode through tis.rs, against the renders
    for (tis_name, render) in [("SQCAMP", inp.render.clone()), ("SQCAMPN", inp.night.clone().unwrap())] {
        let src = image::open(&render).unwrap().to_rgb8().into_raw();
        let (w, h, rs, pages) = decode_tiles(&out, tis_name);
        let (_, _, pyt, _) = decode_tiles(&py, tis_name);
        assert_eq!(src.len(), w * h * 3);
        let (p_rs, p_py) = (psnr(&rs, &src), psnr(&pyt, &src));
        println!("{tis_name}: {pages} pages, PSNR rust {p_rs:.2} dB, python {p_py:.2} dB");
        assert!(p_rs > 38.0);
    }

    // ARE
    let a_py = are::parse(&read(&py.join("SQCAMP.ARE"))).unwrap();
    let a_rs = are::parse(&read(&out.join("SQCAMP.ARE"))).unwrap();
    assert_eq!(a_py.area_type, a_rs.area_type);
    let act = |a: &are::Are| {
        a.actors.iter().map(|x| (x.name.clone(), x.pos, x.orientation, x.cre.clone())).collect::<Vec<_>>()
    };
    assert_eq!(act(&a_py), act(&a_rs));
    let reg =
        |a: &are::Are| a.regions.iter().map(|x| (x.name.clone(), x.pts.clone(), x.cursor, x.flags)).collect::<Vec<_>>();
    assert_eq!(reg(&a_py), reg(&a_rs));
    let ent = |a: &are::Are| a.entrances.iter().map(|x| (x.name.clone(), x.pos, x.orientation)).collect::<Vec<_>>();
    assert_eq!(ent(&a_py), ent(&a_rs));
    let anim = |a: &are::Are| a.anims.iter().map(|x| (x.name.clone(), x.pos, x.bam.clone())).collect::<Vec<_>>();
    assert_eq!(anim(&a_py), anim(&a_rs));
    let same_bytes = read(&py.join("SQCAMP.ARE")) == read(&out.join("SQCAMP.ARE"));
    println!("ARE: actors, regions, entrance, {} animations equal; bytes identical: {same_bytes}", a_rs.anims.len());

    // Lua: the same table (the header comment names the writer)
    let lua = |p: &Path| {
        let t = String::from_utf8(read(p)).unwrap();
        assert!(!t.replace("\r\n", "").contains('\n'), "CRLF");
        let s = t.find("SQCampSite = {").unwrap();
        t[s..s + t[s..].find("\r\n}\r\n").unwrap() + 5].to_string()
    };
    // the same table; the Python's VISION block is gone (camp sight dropped)
    assert_eq!(lua(&py.join("M_SQ0CMP.lua")), lua(&out.join("M_SQ0CMP.lua")));
    assert!(!String::from_utf8(read(&out.join("M_SQ0CMP.lua"))).unwrap().contains("VISION"));

    // generic area
    let gout = base.join("AR9900");
    let t1 = std::time::Instant::now();
    let g = campbuild::build(
        &inp,
        &BuildOptions { name: "ar9900".into(), mode: Mode::Generic, out_dir: gout.clone(), scale: 1 },
        &|_, _| {},
    )
    .expect("generic");
    println!("AR9900 built in {} ms: {:?}", t1.elapsed().as_millis(), g.files);
    let ga = are::parse(&read(&gout.join("AR9900.ARE"))).unwrap();
    assert_eq!(ga.wed, "AR9900");
    assert_eq!(ga.area_type & 0x42, 0x42);
    assert!(ga.actors.is_empty());
    assert_eq!(ga.entrances[0].name, "Entry");
    assert_eq!(ga.regions[0].name, "Exit1");
    assert_eq!(ga.regions[0].kind, 2);
    assert_eq!(ga.regions[0].cursor, 34);
    assert!(ga.regions[0].dest_area.is_empty());
    assert!(!ga.anims.is_empty());
    let gw = wed::parse(&read(&gout.join("AR9900.WED"))).unwrap();
    assert_eq!(gw.overlays[0].tileset, "AR9900");
    assert_eq!(gw.walls, w_rs.walls);
    let gn = wed::parse(&read(&gout.join("AR9900N.WED"))).unwrap();
    assert_eq!(gn.overlays[0].tileset, "AR9900N");
    let (_, _, rs, _) = decode_tiles(&gout, "AR9900");
    let src = image::open(&inp.render).unwrap().to_rgb8().into_raw();
    assert!(psnr(&rs, &src) > 38.0);
    assert!(gout.join("A9900N00.PVRZ").exists());
    for f in ["AR9900SR.BMP", "AR9900HT.BMP", "AR9900LM.BMP", "AR9900LN.BMP"] {
        bmpmap::parse(&read(&gout.join(f))).unwrap();
    }
    assert!(gout.join("preview.jpg").exists());
}

/// timing on an 80 x 60 tile map: CB_BIG = a folder with big.json, big.png, big_alpha.png, big_id.png, big_n.png
#[test]
#[ignore]
fn big_map_timing() {
    let Ok(dir) = std::env::var("CB_BIG") else { return };
    let d = PathBuf::from(dir);
    let inp = BuildInput {
        map: d.join("big.json"),
        walk: None,
        render: d.join("big.png"),
        alpha: None,
        id: None,
        night: Some(d.join("big_n.png")),
        models: Models::Dir(env("CB_MODELS", "E:/AI/bg3d/proxies")),
    };
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("campbuild_big");
    let t0 = std::time::Instant::now();
    let last = std::sync::Mutex::new(std::time::Instant::now());
    let r = campbuild::build(
        &inp,
        &BuildOptions { name: "AR9901".into(), mode: Mode::Generic, out_dir: out, scale: 1 },
        &|pct, step| {
            let mut l = last.lock().unwrap();
            println!("{:>6} ms  {pct:>3}% {step}", l.elapsed().as_millis());
            *l = std::time::Instant::now();
        },
    )
    .unwrap();
    println!("{:?} tiles, {} pages, {} walls in {} ms", r.tiles, r.pages, r.walls, t0.elapsed().as_millis());
    assert_eq!(r.tiles, [80, 60]);
    assert!(t0.elapsed().as_secs() < 60);
}
