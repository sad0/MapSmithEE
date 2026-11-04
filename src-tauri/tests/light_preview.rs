//! The light maps a light pass gives, as pictures to look at (one pixel per cell, scaled up 4x).
//! `MAPSMITH_LIGHT=<day_light.png> [MAPSMITH_ID=<day_id.png>] [MAPSMITH_NIGHT=1] MAPSMITH_OUT=<lm.png> cargo test --test light_preview -- --ignored`

use mapsmithee_lib::campbuild::light;

#[test]
#[ignore]
fn light_preview() {
    let var = |k: &str| std::env::var(k).unwrap_or_else(|_| panic!("set {k}"));
    let img = image::open(var("MAPSMITH_LIGHT")).unwrap().to_rgba8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    // MAPSMITH_ID: the day id pass, to measure on the ground only (as the build does)
    let ground: Option<Vec<bool>> = std::env::var("MAPSMITH_ID").ok().map(|p| {
        image::open(p)
            .unwrap()
            .to_rgb8()
            .pixels()
            .map(|p| p.0 == [0, 0, 0])
            .collect()
    });
    let (c, cw, ch) = light::cells(img.as_raw(), ground.as_deref(), w, h);
    let night = std::env::var("MAPSMITH_NIGHT").is_ok_and(|v| v == "1");
    let cols = if night {
        light::night(&c)
    } else {
        light::day(&c)
    };
    let (pal, _) = light::palettize(&cols);
    println!("{cw}x{ch} cells, {} palette colours", pal.len() / 4);
    let mut out = image::RgbImage::new(cw as u32 * 4, ch as u32 * 4);
    for (x, y, p) in out.enumerate_pixels_mut() {
        *p = image::Rgb(cols[(y as usize / 4) * cw + x as usize / 4]);
    }
    out.save(var("MAPSMITH_OUT")).unwrap();
}
