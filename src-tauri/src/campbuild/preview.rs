//! preview.jpg: the render with blocked cells (red), walls (cyan), exits (orange) and spots (yellow) drawn, half size.

use super::{Area, BLOCK};

type Rgb = [u8; 3];

struct Canvas {
    w: i32,
    h: i32,
    px: Vec<u8>,
}

impl Canvas {
    fn put(&mut self, x: i32, y: i32, c: Rgb) {
        if x >= 0 && y >= 0 && x < self.w && y < self.h {
            let o = (y * self.w + x) as usize * 3;
            self.px[o..o + 3].copy_from_slice(&c);
        }
    }

    fn disc(&mut self, cx: i32, cy: i32, r: f64, c: Rgb) {
        let ri = r.ceil() as i32;
        for dy in -ri..=ri {
            for dx in -ri..=ri {
                if ((dx * dx + dy * dy) as f64) <= r * r {
                    self.put(cx + dx, cy + dy, c);
                }
            }
        }
    }

    fn line(&mut self, a: (i32, i32), b: (i32, i32), thick: f64, c: Rgb) {
        let n = (b.0 - a.0).abs().max((b.1 - a.1).abs()).max(1);
        for i in 0..=n {
            let t = i as f64 / n as f64;
            let x = a.0 as f64 + (b.0 - a.0) as f64 * t;
            let y = a.1 as f64 + (b.1 - a.1) as f64 * t;
            self.disc(x.round() as i32, y.round() as i32, thick / 2.0, c);
        }
    }

    fn poly(&mut self, pts: &[(i32, i32)], thick: f64, c: Rgb) {
        for i in 0..pts.len() {
            self.line(pts[i], pts[(i + 1) % pts.len()], thick, c);
        }
    }

    fn ring(&mut self, cx: i32, cy: i32, r: f64, thick: f64, c: Rgb) {
        let ro = (r + thick / 2.0).ceil() as i32;
        for dy in -ro..=ro {
            for dx in -ro..=ro {
                let d = ((dx * dx + dy * dy) as f64).sqrt();
                if (d - r).abs() <= thick / 2.0 {
                    self.put(cx + dx, cy + dy, c);
                }
            }
        }
    }

    /// 5x7 capitals, `scale` px per dot, (x, y) the bottom-left corner
    fn text(&mut self, x: i32, y: i32, s: &str, scale: i32, c: Rgb) {
        let mut cx = x;
        for ch in s.chars() {
            if let Some(rows) = glyph(ch) {
                for (r, bits) in rows.iter().enumerate() {
                    for col in 0..5 {
                        if bits >> (4 - col) & 1 == 1 {
                            for dy in 0..scale {
                                for dx in 0..scale {
                                    self.put(cx + col * scale + dx, y - (7 - r as i32) * scale + dy, c);
                                }
                            }
                        }
                    }
                }
            }
            cx += 6 * scale;
        }
    }
}

fn glyph(c: char) -> Option<[u8; 7]> {
    Some(match c {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 2, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 17, 25, 21, 19, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [31, 2, 4, 2, 1, 17, 14],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 30, 1, 1, 17, 14],
        '6' => [6, 8, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 2, 12],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        _ => return None,
    })
}

const RED: Rgb = [255, 0, 0];
const CYAN: Rgb = [0, 255, 255];
const ORANGE: Rgb = [255, 140, 0];
const YELLOW: Rgb = [255, 255, 0];

pub fn draw(rgb: &[u8], w: usize, h: usize, a: &Area) -> Result<Vec<u8>, String> {
    let mut c = Canvas { w: w as i32, h: h as i32, px: rgb.to_vec() };
    // the search map has whole cells only (floor of w / 16 by h / 12): the last few pixel rows / columns have none
    let cw = w / 16;
    let ch = a.search.len() / cw.max(1);
    for y in 0..h.min(ch * 12) {
        for x in 0..w.min(cw * 16) {
            if a.search[(y / 12) * cw + x / 16] == BLOCK {
                let o = (y * w + x) * 3;
                for k in 0..3 {
                    c.px[o + k] = (c.px[o + k] as f32 * 0.6 + RED[k] as f32 * 0.4) as u8;
                }
            }
        }
    }
    for v in &a.walls {
        c.poly(v, 2.0, CYAN);
    }
    for v in &a.exits {
        c.poly(v, 4.0, ORANGE);
    }
    for (k, s) in &a.spots {
        c.ring(s.x, s.y, 14.0, 3.0, YELLOW);
        c.text(s.x + 16, s.y, k, 3, YELLOW);
    }
    // half size, 2x2 average
    let (hw, hh) = (w / 2, h / 2);
    let mut half = vec![0u8; hw * hh * 3];
    for y in 0..hh {
        for x in 0..hw {
            for k in 0..3 {
                let s: u32 = [(2 * y, 2 * x), (2 * y, 2 * x + 1), (2 * y + 1, 2 * x), (2 * y + 1, 2 * x + 1)]
                    .iter()
                    .map(|&(yy, xx)| c.px[(yy * w + xx) * 3 + k] as u32)
                    .sum();
                half[(y * hw + x) * 3 + k] = ((s + 2) / 4) as u8;
            }
        }
    }
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85)
        .encode(&half, hw as u32, hh as u32, image::ExtendedColorType::Rgb8)
        .map_err(|e| format!("preview: {e}"))?;
    Ok(out)
}
