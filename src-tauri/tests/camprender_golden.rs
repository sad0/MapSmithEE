//! Rust canvas / ground colour images against the Python originals (E:/AI/bg3d/tools canvas.py, groundadj.py).
//! `MAPSMITH_LIBRARY=<bg3d library> MAPSMITH_REF=<dir> cargo test --test camprender_golden -- --ignored --nocapture`
//! where `<dir>/cases.json` = `{"canvas": [{params, png}], "ground": [{layer, adj, jpg, exact}]}` lists Python's outputs
//! (`exact`: groundadj.py's colour maths before its JPEG save; the Rust copy is a PNG, so it is held to that).

use std::path::PathBuf;

use mapsmithee_lib::camprender::{canvas, groundadj};
use serde_json::Value;

fn env_dir(var: &str) -> PathBuf {
    PathBuf::from(std::env::var(var).unwrap_or_else(|_| panic!("set {var}")))
}

/// mean absolute difference per channel, in 0..255 units, and the largest one
fn diff(a: &image::RgbImage, b: &image::RgbImage) -> (f64, u8) {
    assert_eq!(a.dimensions(), b.dimensions());
    let (mut sum, mut max) = (0u64, 0u8);
    for (p, q) in a.as_raw().iter().zip(b.as_raw()) {
        let d = p.abs_diff(*q);
        sum += d as u64;
        max = max.max(d);
    }
    (sum as f64 / a.as_raw().len() as f64, max)
}

#[test]
#[ignore]
fn images_match_python() {
    let lib = env_dir("MAPSMITH_LIBRARY");
    let refd = env_dir("MAPSMITH_REF");
    let cases: Value = serde_json::from_slice(&std::fs::read(refd.join("cases.json")).unwrap()).unwrap();
    let out = refd.join("rust");
    let _ = std::fs::remove_dir_all(&out);
    let mut grain = None;
    for c in cases["canvas"].as_array().unwrap() {
        let png = canvas::make(&c["params"], &out.join("canvas"), &mut grain, &lib.join("tex").join(canvas::GRAIN)).unwrap();
        let (mean, max) = diff(&image::open(&png).unwrap().to_rgb8(), &image::open(c["png"].as_str().unwrap()).unwrap().to_rgb8());
        println!("canvas {}: mean {mean:.3}/255, max {max}", c["params"]);
        assert!(mean < 2.0, "canvas {} differs: {mean}", c["params"]);
    }
    for c in cases["ground"].as_array().unwrap() {
        let layer = c["layer"].as_str().unwrap();
        let src = lib.join("tex").join(layer).join("diff_2k.jpg");
        let png = image::open(groundadj::make(&src, layer, &c["adj"], &out.join("ground")).unwrap()).unwrap().to_rgb8();
        let (mean, max) = diff(&png, &image::open(c["exact"].as_str().unwrap()).unwrap().to_rgb8());
        let (jmean, jmax) = diff(&png, &image::open(c["jpg"].as_str().unwrap()).unwrap().to_rgb8());
        println!("ground {layer} {}: mean {mean:.3}/255, max {max} (vs Python's JPEG: mean {jmean:.3}, max {jmax})", c["adj"]);
        assert!(mean < 2.0, "ground {layer} differs: {mean}");
    }
}
