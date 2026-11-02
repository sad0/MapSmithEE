//! A real Blender render through the Rust path (slow: needs Blender and the model library).
//! `MAPSMITH_LIBRARY=<bg3d library> MAPSMITH_MAP=<maps/<id>.json> MAPSMITH_OUT=<out.png> [MAPSMITH_SAMPLES=8]
//!  [MAPSMITH_CROP="x0 y0 x1 y1"] [MAPSMITH_ID=1] cargo test --test camprender_render -- --ignored --nocapture`
//! The ground doc and masks are read next to the map (`<id>_ground.json`, `<id>_g<n>.json`).

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use mapsmithee_lib::camprender::{RenderRequest, find_blender, render};
use serde_json::Value;

fn read(p: &std::path::Path) -> Value {
    serde_json::from_slice(&std::fs::read(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))).unwrap()
}

#[test]
#[ignore]
fn render_sample_map() {
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
    let map = PathBuf::from(var("MAPSMITH_MAP"));
    let base = map.to_string_lossy().trim_end_matches(".json").to_string();
    let ground = read(&PathBuf::from(format!("{base}_ground.json")));
    let layers = mapsmithee_lib::camprender::doc_data(&ground)["layers"].as_array().map_or(0, Vec::len);
    let masks = (0..layers)
        .map(|n| PathBuf::from(format!("{base}_g{n}.json")))
        .map(|p| if p.exists() { read(&p) } else { Value::Null })
        .collect();
    let out = PathBuf::from(var("MAPSMITH_OUT"));
    let crop = std::env::var("MAPSMITH_CROP").ok().map(|c| {
        let v: Vec<u32> = c.split_whitespace().map(|x| x.parse().unwrap()).collect();
        [v[0], v[1], v[2], v[3]]
    });
    let req = RenderRequest {
        map_doc: read(&map),
        ground_doc: ground,
        ground_masks: masks,
        offsets: None,
        out_png: out.clone(),
        samples: std::env::var("MAPSMITH_SAMPLES").ok().and_then(|s| s.parse().ok()).unwrap_or(8),
        hour: None,
        id_pass: std::env::var("MAPSMITH_ID").is_ok_and(|v| v == "1"),
        crop,
        blender_exe: None,
        library_dir: PathBuf::from(var("MAPSMITH_LIBRARY")),
        work_dir: out.parent().unwrap().join("work"),
        scripts_dir: None,
    };
    println!("blender: {:?}", find_blender());
    let t0 = std::time::Instant::now();
    let last = std::sync::Mutex::new((u32::MAX, String::new()));
    let progress = |pct: u32, step: &str| {
        let mut l = last.lock().unwrap();
        if l.0 != pct || l.1 != step {
            println!("[{:6.1}s] {pct:3}% {step}", t0.elapsed().as_secs_f64());
        }
        *l = (pct, step.to_string());
    };
    let r = render(&req, &progress, &AtomicBool::new(false)).unwrap();
    println!("{r:?}");
    assert!(r.out_png.is_file());
}
