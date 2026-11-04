//! EE area tilesets: a TIS of PVRZ references (12-byte tiles) and the PVRZ pages (1024 x 1024 DXT1 in a PVR v3
//! container, zlib-compressed behind its 4-byte unpacked size). No per-tile palette, so no seams between tiles.
//! Water (see `Water`): the tiles over water get holes (DXT1's 1-bit alpha, as BG's own water areas: AR5100), the
//! engine draws the water overlay through them; each such cell also gets an opaque copy after the grid's tiles, its
//! WED secondary tile.

use flate2::Compression;
use flate2::write::ZlibEncoder;
use rayon::prelude::*;
use std::io::Write;

pub const PAGE: usize = 1024;
/// tiles per page side
pub const TPP: usize = PAGE / 64;

/// the engine's rule: TIS "AR0100" -> PVRZ "A0100nn" (first letter, then from the third on, then the page)
pub fn page_name(tis: &str, page: usize) -> String {
    crate::tis::pvrz_name(tis, page as i32)
}

fn to565(e: [f32; 3]) -> u16 {
    let r = e[0].round_ties_even() as u16;
    let g = e[1].round_ties_even() as u16;
    let b = e[2].round_ties_even() as u16;
    ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3)
}

fn from565(v: u16) -> [f32; 3] {
    [
        ((v >> 11 & 31) as f64 * 255.0 / 31.0) as f32,
        ((v >> 5 & 63) as f64 * 255.0 / 63.0) as f32,
        ((v & 31) as f64 * 255.0 / 31.0) as f32,
    ]
}

/// one 4x4 block: end points along the colours' principal axis (power iteration), then the nearest of the 4 colours
fn block(px: &[[f32; 3]; 16]) -> [u8; 8] {
    let mut mean = [0f32; 3];
    for p in px {
        for c in 0..3 {
            mean[c] += p[c];
        }
    }
    for m in mean.iter_mut() {
        *m /= 16.0;
    }
    let mut cov = [[0f32; 3]; 3];
    for p in px {
        let d = [p[0] - mean[0], p[1] - mean[1], p[2] - mean[2]];
        for i in 0..3 {
            for j in 0..3 {
                cov[i][j] += d[i] * d[j];
            }
        }
    }
    let mut axis = [1f32; 3];
    for _ in 0..6 {
        let a = [
            cov[0][0] * axis[0] + cov[0][1] * axis[1] + cov[0][2] * axis[2],
            cov[1][0] * axis[0] + cov[1][1] * axis[1] + cov[1][2] * axis[2],
            cov[2][0] * axis[0] + cov[2][1] * axis[1] + cov[2][2] * axis[2],
        ];
        let n = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt() + 1e-6;
        axis = [a[0] / n, a[1] / n, a[2] / n];
    }
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for p in px {
        let t = (p[0] - mean[0]) * axis[0] + (p[1] - mean[1]) * axis[1] + (p[2] - mean[2]) * axis[2];
        lo = lo.min(t);
        hi = hi.max(t);
    }
    let end = |t: f32| [0, 1, 2].map(|c| (mean[c] + axis[c] * t).clamp(0.0, 255.0));
    let mut best = encode(px, to565(end(hi)), to565(end(lo)));
    // least-squares refit of the end points to the chosen indices (beyond pvrz.py: +0.5 to 1 dB), kept when better
    for _ in 0..2 {
        let Some((e0, e1)) = refit(px, best.2) else { break };
        let t = encode(px, to565(e0), to565(e1));
        if t.3 >= best.3 {
            break;
        }
        best = t;
    }
    let (c0, c1, bits, _) = best;
    let mut out = [0u8; 8];
    out[0..2].copy_from_slice(&c0.to_le_bytes());
    out[2..4].copy_from_slice(&c1.to_le_bytes());
    out[4..8].copy_from_slice(&bits.to_le_bytes());
    out
}

/// 4-colour block from two 565 end points: (c0, c1, index bits, squared error)
fn encode(px: &[[f32; 3]; 16], a: u16, b: u16) -> (u16, u16, u32, f32) {
    let (c0, c1) = if a < b { (b, a) } else { (a, b) };
    let (p0, p1) = (from565(c0), from565(c1));
    let pal = if c0 == c1 {
        [p0; 4]
    } else {
        [p0, p1, [0, 1, 2].map(|c| (2.0 * p0[c] + p1[c]) / 3.0), [0, 1, 2].map(|c| (p0[c] + 2.0 * p1[c]) / 3.0)]
    };
    let (mut bits, mut err) = (0u32, 0f32);
    for (i, p) in px.iter().enumerate() {
        let mut best = (f32::MAX, 0u32);
        for (k, q) in pal.iter().enumerate() {
            let d = (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2);
            if d < best.0 {
                best = (d, k as u32);
            }
        }
        err += best.0;
        if c0 != c1 {
            bits |= best.1 << (2 * i);
        }
    }
    (c0, c1, bits, err)
}

/// end points minimising the error for fixed indices (weights 1, 0, 2/3, 1/3 of c0)
fn refit(px: &[[f32; 3]; 16], bits: u32) -> Option<([f32; 3], [f32; 3])> {
    const W: [f32; 4] = [1.0, 0.0, 2.0 / 3.0, 1.0 / 3.0];
    let (mut aa, mut ab, mut bb) = (0f32, 0f32, 0f32);
    let (mut ax, mut bx) = ([0f32; 3], [0f32; 3]);
    for (i, p) in px.iter().enumerate() {
        let w = W[(bits >> (2 * i) & 3) as usize];
        let v = 1.0 - w;
        aa += w * w;
        ab += w * v;
        bb += v * v;
        for c in 0..3 {
            ax[c] += w * p[c];
            bx[c] += v * p[c];
        }
    }
    let det = aa * bb - ab * ab;
    if det.abs() < 1e-6 {
        return None;
    }
    let e0 = [0, 1, 2].map(|c| ((ax[c] * bb - bx[c] * ab) / det).clamp(0.0, 255.0));
    let e1 = [0, 1, 2].map(|c| ((bx[c] * aa - ax[c] * ab) / det).clamp(0.0, 255.0));
    Some((e0, e1))
}

/// a block with transparent pixels (bit i of `holes`): DXT1's 3-colour mode (c0 <= c1), index 3 = transparent; the
/// end points from the opaque pixels only (power iteration as `block`)
fn block_holes(px: &[[f32; 3]; 16], holes: u16) -> [u8; 8] {
    let solid: Vec<[f32; 3]> = (0..16).filter(|i| holes >> i & 1 == 0).map(|i| px[i]).collect();
    let (mut c0, mut c1) = (0u16, 0u16);
    if !solid.is_empty() {
        let n = solid.len() as f32;
        let mean = [0, 1, 2].map(|c| solid.iter().map(|p| p[c]).sum::<f32>() / n);
        let mut cov = [[0f32; 3]; 3];
        for p in &solid {
            let d = [p[0] - mean[0], p[1] - mean[1], p[2] - mean[2]];
            for i in 0..3 {
                for j in 0..3 {
                    cov[i][j] += d[i] * d[j];
                }
            }
        }
        let mut axis = [1f32; 3];
        for _ in 0..6 {
            let a = [0, 1, 2].map(|i| cov[i][0] * axis[0] + cov[i][1] * axis[1] + cov[i][2] * axis[2]);
            let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt() + 1e-6;
            axis = a.map(|v| v / l);
        }
        let ts = solid.iter().map(|p| (p[0] - mean[0]) * axis[0] + (p[1] - mean[1]) * axis[1] + (p[2] - mean[2]) * axis[2]);
        let (lo, hi) = ts.fold((f32::MAX, f32::MIN), |(lo, hi), t| (lo.min(t), hi.max(t)));
        let end = |t: f32| [0, 1, 2].map(|c| (mean[c] + axis[c] * t).clamp(0.0, 255.0));
        let (a, b) = (to565(end(lo)), to565(end(hi)));
        (c0, c1) = if a <= b { (a, b) } else { (b, a) };
    }
    let (p0, p1) = (from565(c0), from565(c1));
    let pal = [p0, p1, [0, 1, 2].map(|c| (p0[c] + p1[c]) / 2.0)];
    let mut bits = 0u32;
    for (i, p) in px.iter().enumerate() {
        let k = if holes >> i & 1 == 1 {
            3
        } else {
            let d = |q: &[f32; 3]| (p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2);
            (0..3).min_by(|&x, &y| d(&pal[x]).total_cmp(&d(&pal[y]))).unwrap_or(0) as u32
        };
        bits |= k << (2 * i);
    }
    let mut out = [0u8; 8];
    out[0..2].copy_from_slice(&c0.to_le_bytes());
    out[2..4].copy_from_slice(&c1.to_le_bytes());
    out[4..8].copy_from_slice(&bits.to_le_bytes());
    out
}

/// RGB (w x h, multiples of 4) -> DXT1 blocks, row-major
pub fn dxt1(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    dxt1_holes(rgb, None, w, h)
}

/// `dxt1` with transparent pixels (`holes`, one flag per pixel)
pub fn dxt1_holes(rgb: &[u8], holes: Option<&[bool]>, w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h / 2);
    let mut px = [[0f32; 3]; 16];
    for by in 0..h / 4 {
        for bx in 0..w / 4 {
            let mut hm = 0u16;
            for i in 0..16 {
                let pi = (by * 4 + i / 4) * w + bx * 4 + i % 4;
                let o = pi * 3;
                px[i] = [rgb[o] as f32, rgb[o + 1] as f32, rgb[o + 2] as f32];
                if holes.is_some_and(|hl| hl[pi]) {
                    hm |= 1 << i;
                }
            }
            out.extend_from_slice(&if hm == 0 { block(&px) } else { block_holes(&px, hm) });
        }
    }
    out
}

/// one zlib-packed PVR v3 DXT1 page
pub fn pvr(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    pvr_holes(rgb, None, w, h)
}

/// `pvr` with transparent pixels
pub fn pvr_holes(rgb: &[u8], holes: Option<&[bool]>, w: usize, h: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(52 + w * h / 2);
    for v in [0x03525650u32, 0] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    data.extend_from_slice(&7u64.to_le_bytes()); // DXT1 / BC1
    for v in [0u32, 0, h as u32, w as u32, 1, 1, 1, 1, 0] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    data.extend_from_slice(&dxt1_holes(rgb, holes, w, h));
    let mut z = ZlibEncoder::new(Vec::new(), Compression::best());
    z.write_all(&data).expect("zlib in memory");
    let mut out = (data.len() as u32).to_le_bytes().to_vec();
    out.extend_from_slice(&z.finish().expect("zlib in memory"));
    out
}

/// RGB picture of (tw*64) x (th*64) -> (TIS bytes, [(PVRZ name, bytes)])
pub fn build(rgb: &[u8], tw: usize, th: usize, tis_name: &str) -> (Vec<u8>, Vec<(String, Vec<u8>)>) {
    build_scaled(rgb, tw, th, tis_name, 1)
}

/// The water of an area: `holes` one flag per 1x pixel (tw*64 x th*64, true = water seen there), `cells` the tiles
/// (index in the grid, ascending) holding any. Cell `cells[k]`'s opaque copy is tile tw*th + k.
pub struct Water {
    pub holes: Vec<bool>,
    pub cells: Vec<usize>,
}

impl Water {
    /// from the 1x water flags: a tile counts when at least `min` of its pixels are water (a few stray pixels on a
    /// shore are not worth an overlay); the holes stay in those tiles only
    pub fn new(holes: Vec<bool>, tw: usize, th: usize, min: usize) -> Water {
        let w = tw * 64;
        let cells: Vec<usize> = (0..tw * th)
            .filter(|&c| {
                let (tx, ty) = (c % tw, c / tw);
                (0..64).map(|y| (0..64).filter(|x| holes[(ty * 64 + y) * w + tx * 64 + x]).count()).sum::<usize>() >= min
            })
            .collect();
        let mut keep = vec![false; tw * th];
        cells.iter().for_each(|&c| keep[c] = true);
        let holes = holes.iter().enumerate().map(|(i, &h)| h && keep[(i / w / 64) * tw + (i % w) / 64]).collect();
        Water { holes, cells }
    }
}

/// `build_scaled` with water: holes in the water tiles, and their opaque copies after the grid (pages after the grid's)
pub fn build_water(rgb: &[u8], tw: usize, th: usize, tis_name: &str, scale: usize, water: &Water) -> (Vec<u8>, Vec<(String, Vec<u8>)>) {
    build_inner(rgb, tw, th, tis_name, scale, Some(water))
}

/// the same at `scale` x (IEX HD tiles, see IEX docs/HD_TILES.md): the picture is (tw*64*scale) x (th*64*scale), the
/// tiles 64*scale texels (TIS header +0x14), the pages (1024*scale)^2 with the same names, the entries' x, y in
/// texels of the HD page; the tile grid (tw x th) stays the 1x one
pub fn build_scaled(rgb: &[u8], tw: usize, th: usize, tis_name: &str, scale: usize) -> (Vec<u8>, Vec<(String, Vec<u8>)>) {
    build_inner(rgb, tw, th, tis_name, scale, None)
}

fn build_inner(rgb: &[u8], tw: usize, th: usize, tis_name: &str, scale: usize, water: Option<&Water>) -> (Vec<u8>, Vec<(String, Vec<u8>)>) {
    let (ts, ps) = (64 * scale, PAGE * scale);
    let (iw, ih) = (tw * ts, th * ts);
    let (pw, ph) = (tw.div_ceil(TPP), th.div_ceil(TPP));
    let extra = water.map_or(0, |w| w.cells.len());
    let xpages = extra.div_ceil(TPP * TPP);
    let pages: Vec<(String, Vec<u8>)> = (0..pw * ph + xpages)
        .into_par_iter()
        .map(|i| {
            let mut page = vec![0u8; ps * ps * 3];
            let mut holes = water.map(|_| vec![false; ps * ps]);
            if i < pw * ph {
                let (px, py) = (i % pw, i / pw);
                for y in 0..ps.min(ih.saturating_sub(py * ps)) {
                    let sy = py * ps + y;
                    let x0 = px * ps;
                    let n = ps.min(iw - x0);
                    page[y * ps * 3..(y * ps + n) * 3].copy_from_slice(&rgb[(sy * iw + x0) * 3..(sy * iw + x0 + n) * 3]);
                    if let (Some(h), Some(w)) = (holes.as_mut(), water) {
                        for x in 0..n {
                            h[y * ps + x] = w.holes[(sy / scale) * (tw * 64) + (x0 + x) / scale];
                        }
                    }
                }
            } else if let Some(w) = water {
                // the water cells' opaque copies, 256 to a page in reading order
                let first = (i - pw * ph) * TPP * TPP;
                for (k, &c) in w.cells.iter().enumerate().skip(first).take(TPP * TPP) {
                    let (tx, ty, slot) = (c % tw, c / tw, k - first);
                    let (dx, dy) = ((slot % TPP) * ts, (slot / TPP) * ts);
                    for y in 0..ts {
                        let s = ((ty * ts + y) * iw + tx * ts) * 3;
                        let d = ((dy + y) * ps + dx) * 3;
                        page[d..d + ts * 3].copy_from_slice(&rgb[s..s + ts * 3]);
                    }
                }
            }
            (page_name(tis_name, i), pvr_holes(&page, holes.as_deref(), ps, ps))
        })
        .collect();
    let mut tis = Vec::with_capacity(24 + (tw * th + extra) * 12);
    tis.extend_from_slice(b"TIS V1  ");
    for v in [(tw * th + extra) as u32, 0x0c, 0x18, ts as u32] {
        tis.extend_from_slice(&v.to_le_bytes());
    }
    for ty in 0..th {
        for tx in 0..tw {
            let page = ((ty / TPP) * pw + tx / TPP) as i32;
            for v in [page, ((tx % TPP) * ts) as i32, ((ty % TPP) * ts) as i32] {
                tis.extend_from_slice(&v.to_le_bytes());
            }
        }
    }
    for k in 0..extra {
        let slot = k % (TPP * TPP);
        for v in [(pw * ph + k / (TPP * TPP)) as i32, ((slot % TPP) * ts) as i32, ((slot / TPP) * ts) as i32] {
            tis.extend_from_slice(&v.to_le_bytes());
        }
    }
    (tis, pages)
}

/// box filter: (w*s) x (h*s) RGB -> w x h
pub fn downscale(rgb: &[u8], w: usize, h: usize, s: usize) -> Vec<u8> {
    if s == 1 {
        return rgb.to_vec();
    }
    let (sw, n) = (w * s, (s * s) as u32);
    let mut out = vec![0u8; w * h * 3];
    out.par_chunks_mut(w * 3).enumerate().for_each(|(y, row)| {
        for x in 0..w {
            for c in 0..3 {
                let mut sum = 0u32;
                for dy in 0..s {
                    let i = ((y * s + dy) * sw + x * s) * 3 + c;
                    sum += (0..s).map(|dx| rgb[i + dx * 3] as u32).sum::<u32>();
                }
                row[x * 3 + c] = ((sum + n / 2) / n) as u8;
            }
        }
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dxt1_holes_decode_transparent() {
        let (w, h) = (8usize, 4usize);
        let rgb: Vec<u8> = (0..w * h).flat_map(|i| [(i * 7) as u8, 120, 200]).collect();
        let holes: Vec<bool> = (0..w * h).map(|i| i % w >= 5).collect();
        let d = dxt1_holes(&rgb, Some(&holes), w, h);
        assert_eq!(d.len(), 16);
        // first block opaque (4-colour: c0 > c1 or equal), second block with holes (c0 <= c1, index 3 where holes)
        let c = |o: usize| u16::from_le_bytes([d[o], d[o + 1]]);
        assert!(c(8) <= c(10));
        let bits = u32::from_le_bytes(d[12..16].try_into().unwrap());
        for i in 0..16 {
            let x = 4 + i % 4;
            assert_eq!(bits >> (2 * i) & 3 == 3, x >= 5, "pixel {i}");
        }
    }

    #[test]
    fn water_tiles_after_the_grid() {
        let (tw, th) = (2usize, 2usize);
        let rgb = vec![90u8; tw * 64 * th * 64 * 3];
        let mut holes = vec![false; tw * 64 * th * 64];
        for y in 64..128 {
            for x in 0..64 {
                holes[y * 128 + x] = x > 20;
            }
        }
        holes[5] = true; // one stray pixel in cell 0: not worth an overlay
        let w = Water::new(holes, tw, th, 16);
        assert_eq!(w.cells, vec![2]);
        assert!(!w.holes[5]);
        let (tis, pages) = build_water(&rgb, tw, th, "AR9900", 1, &w);
        assert_eq!(u32::from_le_bytes(tis[8..12].try_into().unwrap()), 5);
        assert_eq!(pages.len(), 2);
        let last = &tis[24 + 4 * 12..24 + 5 * 12];
        assert_eq!(i32::from_le_bytes(last[0..4].try_into().unwrap()), 1);
    }

    #[test]
    fn page_roundtrip_through_reader() {
        let (w, h) = (64, 64);
        let rgb: Vec<u8> = (0..w * h).flat_map(|i| [(i % w * 4) as u8, (i / w * 4) as u8, 128]).collect();
        let page = pvr(&rgb, w, h);
        let back = crate::tis::pvrz(&page).unwrap();
        assert_eq!((back.w, back.h), (w, h));
        let err: f64 = (0..w * h)
            .map(|i| (0..3).map(|c| (back.px[i * 4 + c] as f64 - rgb[i * 3 + c] as f64).powi(2)).sum::<f64>())
            .sum::<f64>()
            / (w * h * 3) as f64;
        assert!(10.0 * (255.0f64 * 255.0 / err.max(1e-9)).log10() > 35.0);
    }

    #[test]
    fn hd_entries_in_page_texels() {
        let (tw, th, s) = (17, 2, 2);
        let rgb = vec![90u8; tw * 64 * s * th * 64 * s * 3];
        let (tis, pages) = build_scaled(&rgb, tw, th, "AR9900", s);
        assert_eq!(u32::from_le_bytes(tis[20..24].try_into().unwrap()), 128);
        assert_eq!(pages.len(), 2);
        let e = |i: usize, k: usize| i32::from_le_bytes(tis[24 + i * 12 + k * 4..28 + i * 12 + k * 4].try_into().unwrap());
        assert_eq!((e(15, 0), e(15, 1), e(15, 2)), (0, 15 * 128, 0));
        assert_eq!((e(16, 0), e(16, 1)), (1, 0));
        assert_eq!((e(tw + 1, 1), e(tw + 1, 2)), (128, 128));
        let back = crate::tis::pvrz(&pages[0].1).unwrap();
        assert_eq!((back.w, back.h), (2048, 2048));
        assert_eq!(downscale(&[10, 20, 30, 12, 20, 30, 10, 20, 30, 10, 20, 31], 1, 1, 2), vec![11, 20, 30]);
    }

    #[test]
    fn names_follow_engine_rule() {
        assert_eq!(page_name("SQCAMP", 0), "SCAMP00");
        assert_eq!(page_name("AR9900N", 12), "A9900N12");
    }
}
