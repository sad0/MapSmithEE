//! Tilesets: TIS V1 with palette tiles (5120 bytes) or PVRZ references (12 bytes), and PVRZ pages (DXT1/DXT5).

use crate::res::{Game, Res, i32_at, u32_at};
use flate2::read::ZlibDecoder;
use std::collections::HashMap;
use std::io::Read;

pub struct Rgba {
    pub w: usize,
    pub h: usize,
    pub px: Vec<u8>,
}

fn c565(v: u16) -> [u8; 3] {
    let r = ((v >> 11) & 31) as u32;
    let g = ((v >> 5) & 63) as u32;
    let b = (v & 31) as u32;
    [(r * 255 / 31) as u8, (g * 255 / 63) as u8, (b * 255 / 31) as u8]
}

/// DXT1 (7) or DXT5 (11) blocks -> RGBA (alpha of DXT5 is kept)
fn dxt(data: &[u8], w: usize, h: usize, fmt: u64) -> Rgba {
    let bs = if fmt == 7 { 8 } else { 16 };
    let mut px = vec![0u8; w * h * 4];
    let (bw, bh) = (w / 4, h / 4);
    for by in 0..bh {
        for bx in 0..bw {
            let o = (by * bw + bx) * bs;
            if o + bs > data.len() {
                continue;
            }
            let blk = &data[o..o + bs];
            let mut alpha = [255u8; 16];
            if fmt != 7 {
                let (a0, a1) = (blk[0] as u32, blk[1] as u32);
                let mut bits: u64 = 0;
                for i in 0..6 {
                    bits |= (blk[2 + i] as u64) << (8 * i);
                }
                for (i, a) in alpha.iter_mut().enumerate() {
                    let k = ((bits >> (3 * i)) & 7) as u32;
                    *a = match (k, a0 > a1) {
                        (0, _) => a0,
                        (1, _) => a1,
                        (k, true) => ((8 - k) * a0 + (k - 1) * a1) / 7,
                        (6, false) => 0,
                        (7, false) => 255,
                        (k, false) => ((6 - k) * a0 + (k - 1) * a1) / 5,
                    } as u8;
                }
            }
            let cb = &blk[bs - 8..];
            let (v0, v1) = (u16::from_le_bytes([cb[0], cb[1]]), u16::from_le_bytes([cb[2], cb[3]]));
            let (p0, p1) = (c565(v0), c565(v1));
            let mut pal = [[0u8; 4]; 4];
            pal[0] = [p0[0], p0[1], p0[2], 255];
            pal[1] = [p1[0], p1[1], p1[2], 255];
            let four = v0 > v1 || fmt != 7;
            for c in 0..3 {
                let (a, b) = (p0[c] as u32, p1[c] as u32);
                if four {
                    pal[2][c] = ((2 * a + b) / 3) as u8;
                    pal[3][c] = ((a + 2 * b) / 3) as u8;
                } else {
                    pal[2][c] = ((a + b) / 2) as u8;
                    pal[3][c] = 0;
                }
            }
            pal[2][3] = 255;
            pal[3][3] = if four { 255 } else { 0 };
            let bits = u32::from_le_bytes([cb[4], cb[5], cb[6], cb[7]]);
            for i in 0..16 {
                let c = pal[((bits >> (2 * i)) & 3) as usize];
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                let d = (y * w + x) * 4;
                px[d..d + 3].copy_from_slice(&c[..3]);
                px[d + 3] = if fmt == 7 { c[3] } else { alpha[i] };
            }
        }
    }
    Rgba { w, h, px }
}

pub fn pvrz(data: &[u8]) -> Res<Rgba> {
    let mut raw = Vec::new();
    ZlibDecoder::new(&data[4..]).read_to_end(&mut raw).map_err(|e| format!("PVRZ: {e}"))?;
    let fmt = u64::from_le_bytes(raw[8..16].try_into().unwrap());
    let (h, w) = (u32_at(&raw, 24) as usize, u32_at(&raw, 28) as usize);
    let meta = u32_at(&raw, 48) as usize;
    if fmt != 7 && fmt != 11 {
        return Err(format!("PVRZ: pixel format {fmt} not supported"));
    }
    Ok(dxt(&raw[52 + meta..], w, h, fmt))
}

pub fn pvrz_name(tis: &str, page: i32) -> String {
    let t = tis.to_ascii_uppercase();
    format!("{}{}{:02}", &t[..1], t.get(2..).unwrap_or(""), page)
}

/// a tileset ready to hand out 64x64 RGBA tiles
pub struct Tileset {
    name: String,
    data: Vec<u8>,
    count: usize,
    size: usize,
    off: usize,
    pages: HashMap<i32, Option<Rgba>>,
}

impl Tileset {
    pub fn load(game: &Game, name: &str) -> Res<Tileset> {
        let data = game.get(name, "TIS")?.ok_or(format!("{name}.TIS not found"))?;
        if &data[0..4] != b"TIS " {
            return Err(format!("{name}.TIS: not a TIS file"));
        }
        let (count, size, off) = (u32_at(&data, 8) as usize, u32_at(&data, 12) as usize, u32_at(&data, 16) as usize);
        Ok(Tileset { name: name.to_ascii_uppercase(), data, count, size, off, pages: HashMap::new() })
    }

    /// copy tile `i` into `dst` (RGBA, row length `stride` pixels) at (x, y)
    pub fn blit(&mut self, game: &Game, i: usize, dst: &mut [u8], stride: usize, x: usize, y: usize) {
        if i >= self.count {
            return;
        }
        let o = self.off + i * self.size;
        if self.size == 12 {
            let (pg, tx, ty) = (i32_at(&self.data, o), u32_at(&self.data, o + 4) as usize, u32_at(&self.data, o + 8) as usize);
            if pg < 0 {
                return;
            }
            let name = pvrz_name(&self.name, pg);
            let page = self
                .pages
                .entry(pg)
                .or_insert_with(|| game.get(&name, "PVRZ").ok().flatten().and_then(|d| pvrz(&d).ok()));
            let Some(p) = page else { return };
            for r in 0..64 {
                if ty + r >= p.h {
                    break;
                }
                let s = ((ty + r) * p.w + tx) * 4;
                let d = ((y + r) * stride + x) * 4;
                let n = 64.min(p.w - tx) * 4;
                dst[d..d + n].copy_from_slice(&p.px[s..s + n]);
            }
        } else {
            let pal = &self.data[o..o + 1024];
            let idx = &self.data[o + 1024..o + 1024 + 4096];
            for r in 0..64 {
                for c in 0..64 {
                    let k = idx[r * 64 + c] as usize * 4;
                    let d = ((y + r) * stride + x + c) * 4;
                    dst[d] = pal[k + 2];
                    dst[d + 1] = pal[k + 1];
                    dst[d + 2] = pal[k];
                    dst[d + 3] = 255;
                }
            }
        }
    }
}
