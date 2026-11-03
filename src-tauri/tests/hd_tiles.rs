//! HD tiles: builds a map's area from its last render pictures (the app's render work folder) into HD_OUT.
//! `cargo test --release --test hd_tiles -- --ignored --nocapture`
//! HD_SCALE: the pictures' scale (1, 2, 4). Paths: HD_MAP (map doc), HD_WORK (day.png + passes, night.png), HD_MODELS, HD_OUT; HD_NAME (6 chars).

use mapsmithee_lib::campbuild::{self, BuildInput, BuildOptions, Mode, Models};
use std::path::PathBuf;

fn env(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.to_string())
}

#[test]
#[ignore]
fn build_from_work() {
    let app = env("APPDATA", "") + "/com.sad0.mapsmithee";
    let work = PathBuf::from(env("HD_WORK", &format!("{app}/render/claude_camp")));
    let map = PathBuf::from(env("HD_MAP", &format!("{app}/camp3d/maps/claude_camp.json")));
    let walk = PathBuf::from(map.to_string_lossy().replace(".json", "_walk.json"));
    let input = BuildInput {
        map,
        walk: Some(walk).filter(|p| p.exists()),
        render: work.join("day.png"),
        alpha: Some(work.join("day_alpha.png")),
        id: Some(work.join("day_id.png")),
        night: Some(work.join("night.png")),
        models: Models::Dir(PathBuf::from(env("HD_MODELS", "E:/AI/bg3d/proxies"))),
    };
    let opts = BuildOptions { name: env("HD_NAME", "MSHDCP"), mode: Mode::Generic, out_dir: PathBuf::from(env("HD_OUT", "")), scale: env("HD_SCALE", "1").parse().unwrap() };
    let rep = campbuild::build(&input, &opts, &|_, _| {}).unwrap();
    println!("{:?}", rep.files);
}
