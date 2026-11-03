//! EE area tilesets: a TIS of PVRZ references (12-byte tiles) and the PVRZ pages (1024 x 1024 DXT1 in a PVR v3
//! container, zlib-compressed behind its 4-byte unpacked size). No per-tile palette, so no seams between tiles.

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

/// RGB (w x h, multiples of 4) -> DXT1 blocks, row-major
pub fn dxt1(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(w * h / 2);
    let mut px = [[0f32; 3]; 16];
    for by in 0..h / 4 {
        for bx in 0..w / 4 {
            for i in 0..16 {
                let o = ((by * 4 + i / 4) * w + bx * 4 + i % 4) * 3;
                px[i] = [rgb[o] as f32, rgb[o + 1] as f32, rgb[o + 2] as f32];
            }
            out.extend_from_slice(&block(&px));
        }
    }
    out
}

/// one zlib-packed PVR v3 DXT1 page
pub fn pvr(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(52 + w * h / 2);
    for v in [0x03525650u32, 0] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    data.extend_from_slice(&7u64.to_le_bytes()); // DXT1 / BC1
    for v in [0u32, 0, h as u32, w as u32, 1, 1, 1, 1, 0] {
        data.extend_from_slice(&v.to_le_bytes());
    }
    data.extend_from_slice(&dxt1(rgb, w, h));
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

/// the same at `scale` x (IEX HD tiles, see IEX docs/HD_TILES.md): the picture is (tw*64*scale) x (th*64*scale), the
/// tiles 64*scale texels (TIS header +0x14), the pages (1024*scale)^2 with the same names, the entries' x, y in
/// texels of the HD page; the tile grid (tw x th) stays the 1x one
pub fn build_scaled(rgb: &[u8], tw: usize, th: usize, tis_name: &str, scale: usize) -> (Vec<u8>, Vec<(String, Vec<u8>)>) {
    let (ts, ps) = (64 * scale, PAGE * scale);
    let (iw, ih) = (tw * ts, th * ts);
    let (pw, ph) = (tw.div_ceil(TPP), th.div_ceil(TPP));
    let pages: Vec<(String, Vec<u8>)> = (0..pw * ph)
        .into_par_iter()
        .map(|i| {
            let (px, py) = (i % pw, i / pw);
            let mut page = vec![0u8; ps * ps * 3];
            for y in 0..ps.min(ih.saturating_sub(py * ps)) {
                let sy = py * ps + y;
                let x0 = px * ps;
                let n = ps.min(iw - x0);
                page[y * ps * 3..(y * ps + n) * 3].copy_from_slice(&rgb[(sy * iw + x0) * 3..(sy * iw + x0 + n) * 3]);
            }
            (page_name(tis_name, i), pvr(&page, ps, ps))
        })
        .collect();
    let mut tis = Vec::with_capacity(24 + tw * th * 12);
    tis.extend_from_slice(b"TIS V1  ");
    for v in [(tw * th) as u32, 0x0c, 0x18, ts as u32] {
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
