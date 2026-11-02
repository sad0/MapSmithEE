//! Tent / rug canvas pattern images: the Rust twin of `E:/AI/bg3d/tools/canvas.py` and of `canvasTexture()` in the
//! map editor (same parameters, same layout), so a tent looks the same in the editor and in the Blender render.
//! Rows run top (v = 1, ridge / apex) to bottom (v = 0, ground).
//!
//! params: `{"c1": "#rrggbb", "c2": "#rrggbb", "pat": plain|stripes|bands|checks|chevrons|border|medallion|kilim|diamonds,
//! "n": count, "wear": 0..1, "rug": 1 for a rug (no ground edge)}`

use std::path::{Path, PathBuf};

use image::{GrayImage, RgbImage, imageops::FilterType};
use serde_json::Value;

use super::{cache_key, img_err, io_err, truthy};

pub const SIZE: u32 = 512;
/// the fabric grain, relative to the library's `tex/`
pub const GRAIN: &str = "rough_linen_diff_2k.jpg";

/// The fabric grain tile, SIZE x SIZE values in 0..1: the linen photo in grey, scaled to half size, tiled 2 x 2.
pub struct Grain {
    g: Vec<f32>,
    mean: f32,
    std: f32,
}

impl Grain {
    pub fn load(path: &Path) -> Result<Grain, String> {
        let rgb = image::open(path).map_err(|e| img_err(path, e))?.to_rgb8();
        Ok(Grain::from_rgb(&rgb))
    }

    pub fn from_rgb(rgb: &RgbImage) -> Grain {
        // PIL's convert('L'): ITU-R 601-2 luma in 16-bit fixed point
        let grey = GrayImage::from_fn(rgb.width(), rgb.height(), |x, y| {
            let p = rgb.get_pixel(x, y).0;
            let l = (p[0] as u32 * 19595 + p[1] as u32 * 38470 + p[2] as u32 * 7471 + 0x8000) >> 16;
            image::Luma([l as u8])
        });
        let half = SIZE / 2;
        let small = image::imageops::resize(&grey, half, half, FilterType::Lanczos3);
        let mut g = vec![0f32; (SIZE * SIZE) as usize];
        for y in 0..SIZE {
            for x in 0..SIZE {
                g[(y * SIZE + x) as usize] = small.get_pixel(x % half, y % half).0[0] as f32 / 255.0;
            }
        }
        let n = g.len() as f64;
        let mean = g.iter().map(|&v| v as f64).sum::<f64>() / n;
        let var = g.iter().map(|&v| (v as f64 - mean).powi(2)).sum::<f64>() / n;
        Grain { g, mean: mean as f32, std: var.sqrt() as f32 }
    }
}

fn hexrgb(v: Option<&Value>, default: &str) -> [f32; 3] {
    let parse = |s: &str| -> Option<[f32; 3]> {
        let h = s.trim_start_matches('#');
        let c = |i: usize| h.get(i..i + 2).and_then(|t| u8::from_str_radix(t, 16).ok()).map(|b| b as f32);
        Some([c(0)?, c(2)?, c(4)?])
    };
    v.and_then(Value::as_str).and_then(parse).or_else(|| parse(default)).unwrap_or([0.0; 3])
}

/// Python's `x % m` for floats (the result takes the sign of m)
fn pmod(x: f64, m: f64) -> f64 {
    x.rem_euclid(m)
}

fn odd(x: f64) -> bool {
    pmod(x.floor(), 2.0) == 1.0
}

/// true where the second colour goes; u, v in 0..1
pub fn motif(u: f64, v: f64, pat: &str, n: f64) -> bool {
    match pat {
        "stripes" => return odd(u * n * 2.0),
        "bands" => return odd(v * n * 2.0),
        "checks" => return odd((u * n * 2.0).floor() + (v * n).floor()),
        "chevrons" => return odd(u * n * 2.0 + 2.0 * (pmod(v * 4.0, 1.0) - 0.5).abs()),
        "border" => return (v - 0.46).abs() < 0.045 || v < 0.05 || v > 0.96,
        _ => {}
    }
    // rugs (u along the length, v across)
    let b = u.min(1.0 - u).min(v.min(1.0 - v));
    match pat {
        "medallion" => {
            let m = (u - 0.5).abs() / 0.26 + (v - 0.5).abs() / 0.36;
            b < 0.07 || (b > 0.10 && b < 0.12) || (m < 1.0 && m > 0.78) || m < 0.38
        }
        "kilim" => odd(v * n * 2.0 + 0.5 * (pmod(u * n * 3.0, 1.0) - 0.5).abs()) || b < 0.05,
        "diamonds" => {
            let d = (pmod(u * n, 1.0) - 0.5).abs() + (pmod(v * n * 0.66, 1.0) - 0.5).abs();
            (d < 0.28 && d > 0.14) || b < 0.06
        }
        _ => false,
    }
}

/// smooth value noise from a hashed lattice (same formula as the editor's)
pub fn noise(u: f64, v: f64) -> f64 {
    let cells = 12.0;
    let (x, y) = (u * cells, v * cells);
    let (xi, yi) = (x.floor(), y.floor());
    let (fx, fy) = (x - xi, y - yi);
    let h = |a: f64, b: f64| pmod((a * 127.1 + b * 311.7).sin() * 43758.5453, 1.0);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    (h(xi, yi) * (1.0 - sx) + h(xi + 1.0, yi) * sx) * (1.0 - sy) + (h(xi, yi + 1.0) * (1.0 - sx) + h(xi + 1.0, yi + 1.0) * sx) * sy
}

/// The pattern image for one look.
pub fn image(params: &Value, grain: &Grain) -> RgbImage {
    let pat = params.get("pat").and_then(Value::as_str).unwrap_or("plain");
    let n = params.get("n").and_then(Value::as_f64).map(f64::trunc).unwrap_or(6.0).max(1.0);
    let c1 = hexrgb(params.get("c1"), "#d8cbb0");
    let c2 = hexrgb(params.get("c2"), "#7a2a1e");
    let w = params.get("wear").and_then(Value::as_f64).unwrap_or(0.3);
    let rug = params.get("rug").is_some_and(truthy);
    let k = (grain.std * 3.0 + 1e-6) as f32;
    let mut out = RgbImage::new(SIZE, SIZE);
    for row in 0..SIZE {
        let v = 1.0 - (row as f64 + 0.5) / SIZE as f64;
        for colx in 0..SIZE {
            let u = (colx as f64 + 0.5) / SIZE as f64;
            let base = if motif(u, v, pat, n) { c2 } else { c1 };
            // fabric grain (float32 like the numpy original)
            let g = grain.g[(row * SIZE + colx) as usize];
            let gf = 0.72f32 + 0.4f32 * (g - grain.mean) / k + 0.28f32;
            // a tent gets mud and soot near the ground; a rug lies flat: no ground edge
            let mut f = if rug { 1.0 } else { 1.0 - w * 0.7 * (1.0 - v / 0.3).clamp(0.0, 1.0) };
            f *= 1.0 - w * 0.4 * noise(u, v); // blotches
            let c = base.map(|b| (b * gf) as f64 * f);
            let grey = (c[0] + c[1] + c[2]) / 3.0;
            let px = c.map(|x| (x + (grey - x) * (w * 0.35)).clamp(0.0, 255.0) as u8); // faded
            out.put_pixel(colx, row, image::Rgb(px));
        }
    }
    out
}

/// `<out_dir>/canvas_<key>.png` for this look, made unless it is already there. `grain` is loaded on first need.
pub fn make(params: &Value, out_dir: &Path, grain: &mut Option<Grain>, grain_path: &Path) -> Result<PathBuf, String> {
    let path = out_dir.join(format!("canvas_{}.png", cache_key(&params.to_string())));
    if path.exists() {
        return Ok(path);
    }
    std::fs::create_dir_all(out_dir).map_err(|e| io_err(out_dir, e))?;
    if grain.is_none() {
        *grain = Some(Grain::load(grain_path)?);
    }
    let img = image(params, grain.as_ref().unwrap());
    let tmp = path.with_extension("png.tmp");
    img.save_with_format(&tmp, image::ImageFormat::Png).map_err(|e| img_err(&tmp, e))?;
    std::fs::rename(&tmp, &path).map_err(|e| io_err(&path, e))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_matches_python() {
        // canvas.noise(np.array([...]), np.array([...])) from canvas.py
        let want = [0.46244493, 0.68141499, 0.66108474, 0.20532675];
        for (i, (u, v)) in [(0.1, 0.2), (0.37, 0.81), (0.9, 0.05), (0.5, 0.5)].into_iter().enumerate() {
            assert!((noise(u, v) - want[i]).abs() < 1e-7, "{i}: {}", noise(u, v));
        }
    }

    #[test]
    fn motifs_match_python() {
        // canvas.motif on a 16 x 16 grid (rows top to bottom), from canvas.py
        let cases = [
            ("stripes", 8.0, "01".repeat(128)),
            ("chevrons", 3.0, "0110001110011100001110011100011000111001110001100110001110011100011000111001110000111001110001100011100111000110011000111001110001100011100111000011100111000110001110011100011001100011100111000110001110011100001110011100011000111001110001100110001110011100".into()),
            ("medallion", 6.0, "1111111111111111100000000000000110000000000000011000000110000001100000100100000110000010010000011000010000100001100010011001000110001001100100011000010000100001100000100100000110000010010000011000000110000001100000000000000110000000000000011111111111111111".into()),
            ("kilim", 5.0, "1111111111111111111111111111111110000000000000011111110000111111111111111111111110000000000000011100000000000011111111111111111110000000000000011000000000000001111111111111111110000011110000011000000000000001111111111111111110111111111111011111111111111111".into()),
            ("diamonds", 4.0, "1111111111111111111001100110011110000000000000011000000000000001100000000000000110000000000000011110011001100111111001100110011110000000000000011000000000000001100000000000000110000000000000011110011001100111111001100110011110000000000000011111111111111111".into()),
            ("border", 6.0, "1111111111111111000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000011111111111111110000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000001111111111111111".into()),
            ("checks", 3.0, "0001100011100111000110001110011100011000111001110001100011100111000110001110011111100111000110001110011100011000111001110001100011100111000110001110011100011000111001110001100000011000111001110001100011100111000110001110011100011000111001110001100011100111".into()),
            ("bands", 3.0, "1111111111111111111111111111111111111111111111110000000000000000000000000000000011111111111111111111111111111111111111111111111100000000000000000000000000000000000000000000000011111111111111111111111111111111000000000000000000000000000000000000000000000000".into()),
        ];
        for (pat, n, want) in cases {
            let mut got = String::new();
            for row in 0..16 {
                for col in 0..16 {
                    let (u, v) = ((col as f64 + 0.5) / 16.0, 1.0 - (row as f64 + 0.5) / 16.0);
                    got.push(if motif(u, v, pat, n) { '1' } else { '0' });
                }
            }
            assert_eq!(got, want, "{pat}");
        }
    }

    #[test]
    fn plain_look_without_wear_is_flat() {
        let grain = Grain::from_rgb(&RgbImage::from_pixel(64, 64, image::Rgb([128, 128, 128])));
        let img = image(&serde_json::json!({"c1": "#204060", "pat": "plain", "wear": 0}), &grain);
        assert_eq!(img.dimensions(), (SIZE, SIZE));
        assert_eq!(img.get_pixel(10, 500).0, [0x20, 0x40, 0x60]);
    }
}
