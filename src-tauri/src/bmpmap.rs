//! Area bitmaps (SR search map 4 bpp, HT height map, LM/LN light maps 8 bpp): palette-indexed BMP files.
//! The UI paints palette indices; writing keeps the original header and palette.

use crate::res::{Res, i32_at, u16_at, u32_at};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct BmpMap {
    pub w: u32,
    pub h: u32,
    pub bpp: u16,
    /// RGB per palette entry
    pub palette: Vec<[u8; 3]>,
    /// one palette index per pixel, top row first (base64)
    pub pixels: String,
    /// original file up to the pixel data (base64), reused when writing
    pub head: String,
    /// bytes after the pixel rows in the original file (some LM/LN carry 2), kept as they were
    #[serde(default)]
    pub tail: String,
}

pub fn parse(b: &[u8]) -> Res<BmpMap> {
    if b.len() < 54 || &b[0..2] != b"BM" {
        return Err("not a BMP file".into());
    }
    let off = u32_at(b, 10) as usize;
    let hs = u32_at(b, 14) as usize;
    let (w, hh) = (i32_at(b, 18), i32_at(b, 22));
    let bpp = u16_at(b, 28);
    if bpp != 4 && bpp != 8 {
        return Err(format!("{bpp}-bit BMP not supported (4 or 8)"));
    }
    let (w, h, top_down) = (w as usize, hh.unsigned_abs() as usize, hh < 0);
    let ncol = match u32_at(b, 46) {
        0 => 1usize << bpp,
        n => n as usize,
    };
    let pal_at = 14 + hs;
    let palette = (0..ncol)
        .filter(|i| pal_at + i * 4 + 3 <= b.len())
        .map(|i| {
            let o = pal_at + i * 4;
            [b[o + 2], b[o + 1], b[o]]
        })
        .collect();
    let stride = (w * bpp as usize).div_ceil(32) * 4;
    let mut px = vec![0u8; w * h];
    for y in 0..h {
        let row = if top_down { y } else { h - 1 - y };
        let r = off + row * stride;
        for x in 0..w {
            let v = if bpp == 8 {
                b.get(r + x).copied().unwrap_or(0)
            } else {
                let byte = b.get(r + x / 2).copied().unwrap_or(0);
                if x % 2 == 0 { byte >> 4 } else { byte & 15 }
            };
            px[y * w + x] = v;
        }
    }
    let end = (off + stride * h).min(b.len());
    Ok(BmpMap { w: w as u32, h: h as u32, bpp, palette, pixels: B64.encode(&px), head: B64.encode(&b[..off]), tail: B64.encode(&b[end..]) })
}

impl BmpMap {
    /// a new bitmap: `palette` 4 bytes per colour (BGR + 0), `px` one index per pixel, top row first
    pub fn new(w: u32, h: u32, bpp: u16, palette: &[u8], px: &[u8]) -> BmpMap {
        let ncol = palette.len() / 4;
        let off = 14 + 40 + ncol * 4;
        let mut head = vec![0u8; 14];
        head[0..2].copy_from_slice(b"BM");
        head[10..14].copy_from_slice(&(off as u32).to_le_bytes());
        for v in [40u32, w, h] {
            head.extend_from_slice(&v.to_le_bytes());
        }
        head.extend_from_slice(&1u16.to_le_bytes());
        head.extend_from_slice(&bpp.to_le_bytes());
        for v in [0u32, 0, 2835, 2835, ncol as u32, 0] {
            head.extend_from_slice(&v.to_le_bytes());
        }
        head.extend_from_slice(&palette[..ncol * 4]);
        let pal = palette.chunks(4).map(|c| [c[2], c[1], c[0]]).collect();
        BmpMap { w, h, bpp, palette: pal, pixels: B64.encode(px), head: B64.encode(&head), tail: String::new() }
    }
}

pub fn write(m: &BmpMap) -> Res<Vec<u8>> {
    let px = B64.decode(&m.pixels).map_err(|e| e.to_string())?;
    let mut out = B64.decode(&m.head).map_err(|e| e.to_string())?;
    let (w, h) = (m.w as usize, m.h as usize);
    if px.len() != w * h {
        return Err("bitmap size mismatch".into());
    }
    let top_down = i32_at(&out, 22) < 0;
    let stride = (w * m.bpp as usize).div_ceil(32) * 4;
    let off = out.len();
    out.resize(off + stride * h, 0);
    for y in 0..h {
        let row = if top_down { y } else { h - 1 - y };
        let r = off + row * stride;
        for x in 0..w {
            let v = px[y * w + x];
            if m.bpp == 8 {
                out[r + x] = v;
            } else if x % 2 == 0 {
                out[r + x / 2] |= (v & 15) << 4;
            } else {
                out[r + x / 2] |= v & 15;
            }
        }
    }
    out.extend_from_slice(&B64.decode(&m.tail).unwrap_or_default());
    let size = out.len() as u32;
    out[2..6].copy_from_slice(&size.to_le_bytes());
    out[34..38].copy_from_slice(&((stride * h) as u32).to_le_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make(bpp: u16, w: usize, h: usize) -> Vec<u8> {
        let ncol = 1usize << bpp;
        let off = 54 + ncol * 4;
        let stride = (w * bpp as usize).div_ceil(32) * 4;
        let mut b = vec![0u8; off + stride * h];
        b[0..2].copy_from_slice(b"BM");
        b[10..14].copy_from_slice(&(off as u32).to_le_bytes());
        b[14..18].copy_from_slice(&40u32.to_le_bytes());
        b[18..22].copy_from_slice(&(w as i32).to_le_bytes());
        b[22..26].copy_from_slice(&(h as i32).to_le_bytes());
        b[26..28].copy_from_slice(&1u16.to_le_bytes());
        b[28..30].copy_from_slice(&bpp.to_le_bytes());
        b
    }

    #[test]
    fn paint_roundtrip_4bpp() {
        let f = make(4, 5, 3);
        let mut m = parse(&f).unwrap();
        let mut px = B64.decode(&m.pixels).unwrap();
        px[0] = 7;
        px[14] = 12;
        m.pixels = B64.encode(&px);
        let back = parse(&write(&m).unwrap()).unwrap();
        assert_eq!(B64.decode(&back.pixels).unwrap(), px);
    }
}
