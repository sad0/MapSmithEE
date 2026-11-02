//! A ground layer's colour adjustment from the editor, baked into a copy of its texture: the Rust twin of
//! `E:/AI/bg3d/tools/groundadj.py`. The CSS filter chain the editor draws with, hue-rotate(h) saturate(s)
//! brightness(b) contrast(c), on sRGB values, in that order.

use std::path::{Path, PathBuf};

use image::RgbImage;
use serde_json::Value;

use super::{cache_key, img_err, io_err};

/// JPEG quality of the baked copy (PIL's `quality=92` in the original)
const QUALITY: u8 = 92;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Adj {
    pub h: f64,
    pub s: f64,
    pub b: f64,
    pub c: f64,
}

impl Adj {
    pub fn from_value(a: &Value) -> Adj {
        let f = |k: &str, d: f64| a.get(k).and_then(Value::as_f64).unwrap_or(d);
        Adj { h: f("h", 0.0), s: f("s", 1.0), b: f("b", 1.0), c: f("c", 1.0) }
    }
}

type M3 = [[f64; 3]; 3];

fn hue(deg: f64) -> M3 {
    let (s, c) = deg.to_radians().sin_cos();
    [
        [0.213 + c * 0.787 - s * 0.213, 0.715 - c * 0.715 - s * 0.715, 0.072 - c * 0.072 + s * 0.928],
        [0.213 - c * 0.213 + s * 0.143, 0.715 + c * 0.285 + s * 0.140, 0.072 - c * 0.072 - s * 0.283],
        [0.213 - c * 0.213 - s * 0.787, 0.715 - c * 0.715 + s * 0.715, 0.072 + c * 0.928 + s * 0.072],
    ]
}

fn sat(v: f64) -> M3 {
    [
        [0.213 + 0.787 * v, 0.715 - 0.715 * v, 0.072 - 0.072 * v],
        [0.213 - 0.213 * v, 0.715 + 0.285 * v, 0.072 - 0.072 * v],
        [0.213 - 0.213 * v, 0.715 - 0.715 * v, 0.072 + 0.928 * v],
    ]
}

fn mul(m: &M3, x: [f64; 3]) -> [f64; 3] {
    let r = |i: usize| (m[i][0] * x[0] + m[i][1] * x[1] + m[i][2] * x[2]).clamp(0.0, 1.0);
    [r(0), r(1), r(2)]
}

/// One pixel through the filter chain.
pub fn pixel(p: [u8; 3], hm: &M3, sm: &M3, a: &Adj) -> [u8; 3] {
    let x = p.map(|v| (v as f32 / 255.0) as f64);
    let x = mul(sm, mul(hm, x));
    let x = x.map(|v| (v * a.b).clamp(0.0, 1.0));
    let x = x.map(|v| ((v - 0.5) * a.c + 0.5).clamp(0.0, 1.0));
    x.map(|v| (v * 255.0 + 0.5) as u8)
}

pub fn apply(img: &mut RgbImage, a: &Adj) {
    let (hm, sm) = (hue(a.h), sat(a.s));
    for p in img.pixels_mut() {
        p.0 = pixel(p.0, &hm, &sm, a);
    }
}

/// `<out_dir>/<layer>_adj_<key>.jpg`: the layer's texture `src` with `a` applied, made unless already there.
pub fn make(src: &Path, layer: &str, a: &Value, out_dir: &Path) -> Result<PathBuf, String> {
    let key = cache_key(&format!("{}|{}", src.display(), a));
    let safe: String = layer.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    let path = out_dir.join(format!("{safe}_adj_{key}.jpg"));
    if path.exists() {
        return Ok(path);
    }
    std::fs::create_dir_all(out_dir).map_err(|e| io_err(out_dir, e))?;
    let mut img = image::open(src).map_err(|e| img_err(src, e))?.to_rgb8();
    apply(&mut img, &Adj::from_value(a));
    let tmp = path.with_extension("jpg.tmp");
    {
        let f = std::fs::File::create(&tmp).map_err(|e| io_err(&tmp, e))?;
        let mut w = std::io::BufWriter::new(f);
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, QUALITY)
            .encode_image(&img)
            .map_err(|e| img_err(&tmp, e))?;
    }
    std::fs::rename(&tmp, &path).map_err(|e| io_err(&path, e))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_python() {
        // groundadj.py's maths on three pixels (values from numpy)
        let px = [[10, 128, 250], [200, 30, 90], [64, 64, 64]];
        let cases = [
            (Adj { h: -2.0, s: 1.0, b: 1.0, c: 1.0 }, [[5, 130, 246], [199, 30, 95], [64, 64, 64]]),
            (Adj { h: 30.0, s: 1.44, b: 0.6, c: 1.3 }, [[28, 37, 161], [154, 0, 0], [12, 12, 12]]),
        ];
        for (a, want) in cases {
            let (hm, sm) = (hue(a.h), sat(a.s));
            for (p, w) in px.iter().zip(want) {
                assert_eq!(pixel(*p, &hm, &sm, &a), w, "{a:?} {p:?}");
            }
        }
    }

    #[test]
    fn identity() {
        let a = Adj::from_value(&serde_json::json!({}));
        let (hm, sm) = (hue(a.h), sat(a.s));
        for v in [0u8, 1, 77, 128, 254, 255] {
            assert_eq!(pixel([v, 255 - v, v / 2], &hm, &sm, &a), [v, 255 - v, v / 2]);
        }
    }
}
