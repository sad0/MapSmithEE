//! Water in a built area, the way BG draws its rivers and lakes (AR5100, AR4600...): WED overlay 1 is a 1x1 animated
//! water tileset, drawn under the cells marked for it; those cells' tiles have holes where the water shows
//! (pvrz::Water), and an opaque copy as their secondary tile (files::wed_water).
//! - where: the painted ground's water layers (render_map.py's `water_mask.png` in the render folder, the ground in
//!   its own coordinates, north at the top: the BG camera maps it straight onto the picture), where the ground is seen
//!   (the id pass's black: a tent standing in the water hides it)
//! - which water: one of BG's own tilesets (`GAME`, in BGEE, SoD and EET), or the painted water texture itself made
//!   into one (`own_tis`: 8 frames drifting across it, a paletted TIS like BG's water tilesets), with a night version
//!   darkened as the night render darkens the water
//! - the search map: water cells are deep water (SR 12, not walkable), unless painted walkable in the editor

use std::path::Path;

/// BG's own water tilesets (6 frames each): id, the editor's name
pub const GAME: [(&str, &str); 5] =
    [("WTRIV", "river"), ("WTLAKE", "lake"), ("WTSWAM", "swamp"), ("WTWAVE", "sea waves"), ("WTSTR", "stream")];
pub const GAME_FRAMES: usize = 6;
/// frames of a tileset made from the painted texture
pub const OWN_FRAMES: usize = 8;
/// SR: deep water, not walkable
pub const SR_WATER: u8 = 12;

/// The water seen in the picture, one flag per 1x pixel (`mw` x `mh`), from the render folder's `water_mask.png`
/// and the id pass (`id`, black = ground). None without a mask (no water painted, or a render made before 0.2.4).
pub fn mask(render_dir: &Path, id: &Path, mw: usize, mh: usize) -> Result<Option<Vec<bool>>, String> {
    let mp = render_dir.join("water_mask.png");
    if !mp.is_file() {
        return Ok(None);
    }
    let m = image::open(&mp).map_err(|e| format!("{}: {e}", mp.display()))?.to_luma8();
    let ids = image::open(id).ok().map(|im| im.to_rgb8()).filter(|im| (im.width() as usize, im.height() as usize) == (mw, mh));
    let (w, h) = (m.width() as usize, m.height() as usize);
    let mut out = vec![false; mw * mh];
    for y in 0..mh {
        let sy = (y * h / mh).min(h - 1);
        for x in 0..mw {
            let sx = (x * w / mw).min(w - 1);
            let ground = ids.as_ref().is_none_or(|im| im.get_pixel(x as u32, y as u32).0 == [0, 0, 0]);
            out[y * mw + x] = ground && m.get_pixel(sx as u32, sy as u32).0[0] >= 128;
        }
    }
    Ok(out.iter().any(|&v| v).then_some(out))
}

/// Per search-map cell (16 x 12 px): mostly water
pub fn cells_sr(holes: &[bool], mw: usize, cw: usize, ch: usize) -> Vec<bool> {
    (0..cw * ch)
        .map(|c| {
            let (cx, cy) = (c % cw, c / cw);
            let n = (0..12).map(|y| (0..16).filter(|x| holes[(cy * 12 + y) * mw + cx * 16 + x]).count()).sum::<usize>();
            n * 2 >= 16 * 12
        })
        .collect()
}

/// The water's colour change from day to night (per channel), measured on the two pictures where the water shows;
/// a plain night tint when there is nothing to measure
pub fn night_tint(day: &[u8], night: &[u8], holes: &[bool]) -> [f32; 3] {
    let (mut d, mut n) = ([0f64; 3], [0f64; 3]);
    for (i, _) in holes.iter().enumerate().filter(|(_, h)| **h) {
        for c in 0..3 {
            d[c] += day[i * 3 + c] as f64;
            n[c] += night[i * 3 + c] as f64;
        }
    }
    if d.iter().any(|&v| v < 1.0) {
        return [0.25, 0.3, 0.45];
    }
    [0, 1, 2].map(|c| (n[c] / d[c]).clamp(0.02, 1.5) as f32)
}

/// A paletted water tileset (TIS V1, 64 x 64 tiles of 256 colours, like BG's WTRIV) from a seamless 64 x 64 RGB
/// picture: `frames` tiles, each shifted by 64 / frames px right and half that down (wrapping), so the loop drifts
/// smoothly; colours times `tint` (1 by day)
pub fn own_tis(tex: &[u8], frames: usize, tint: [f32; 3]) -> Vec<u8> {
    let px: Vec<[u8; 3]> = (0..64 * 64)
        .map(|i| [0, 1, 2].map(|c| (tex[i * 3 + c] as f32 * tint[c]).round().clamp(0.0, 255.0) as u8))
        .collect();
    let (pal, idx) = quantize(&px);
    let mut out = Vec::with_capacity(24 + frames * 5120);
    out.extend_from_slice(b"TIS V1  ");
    for v in [frames as u32, 5120, 0x18, 64] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    for f in 0..frames {
        for c in &pal {
            out.extend_from_slice(&[c[2], c[1], c[0], 0]);
        }
        out.resize(out.len() + (256 - pal.len()) * 4, 0);
        let (dx, dy) = (f * 64 / frames, f * 32 / frames);
        for y in 0..64 {
            for x in 0..64 {
                out.push(idx[((y + 64 - dy) % 64) * 64 + (x + 64 - dx) % 64]);
            }
        }
    }
    out
}

/// at most 256 colours: the most used (5 bits a channel), the others to their nearest
fn quantize(px: &[[u8; 3]]) -> (Vec<[u8; 3]>, Vec<u8>) {
    let q = |c: [u8; 3]| c.map(|v| v & 0xf8);
    let mut count: std::collections::HashMap<[u8; 3], usize> = Default::default();
    for p in px {
        *count.entry(q(*p)).or_default() += 1;
    }
    let mut pal: Vec<([u8; 3], usize)> = count.into_iter().collect();
    pal.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    pal.truncate(256);
    let pal: Vec<[u8; 3]> = pal.into_iter().map(|(c, _)| c.map(|v| v | 4)).collect();
    let idx = px
        .iter()
        .map(|p| {
            let d = |c: &[u8; 3]| (0..3).map(|k| (c[k] as i32 - p[k] as i32).pow(2)).sum::<i32>();
            (0..pal.len()).min_by_key(|&i| d(&pal[i])).unwrap_or(0) as u8
        })
        .collect();
    (pal, idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_tileset() {
        let tex: Vec<u8> = (0..64 * 64).flat_map(|i| [20u8, 90 + (i % 64) as u8, 140]).collect();
        let t = own_tis(&tex, OWN_FRAMES, [1.0; 3]);
        assert_eq!(&t[0..8], b"TIS V1  ");
        assert_eq!(t.len(), 24 + OWN_FRAMES * 5120);
        // frame 1 is frame 0 moved 8 px right and 4 down
        let f = |k: usize, x: usize, y: usize| t[24 + k * 5120 + 1024 + y * 64 + x];
        assert_eq!(f(1, 8, 4), f(0, 0, 0));
        assert_eq!(f(1, 0, 0), f(0, 56, 60));
    }

    #[test]
    fn sr_cells_and_tint() {
        let (mw, cw, ch) = (32usize, 2usize, 1usize);
        let holes: Vec<bool> = (0..mw * 12).map(|i| i % mw < 16 || (i % mw < 20 && i / mw < 2)).collect();
        assert_eq!(cells_sr(&holes, mw, cw, ch), vec![true, false]);
        let day = vec![100u8; mw * 12 * 3];
        let night = vec![25u8; mw * 12 * 3];
        assert_eq!(night_tint(&day, &night, &holes), [0.25; 3]);
    }
}
