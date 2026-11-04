//! The light maps (LM by day, LN by night) from the render's light pass (`<picture>_light.png`, render_map.py).
//!
//! The game tints the characters with the light map under their feet, one cell per 16x12 area pixels. BG's own light
//! maps are white wherever the sun reaches and a blue-grey where a shadow falls (AR2300LM, AR3800LM: (90, 90, 123));
//! the night ones are a moonlit blue, darker in the shadows, warm round fires and lamps. Here each cell takes the
//! light measured on the 3D scene where the render puts it:
//! - the light pass holds the direct light (no surface colour), open ground in full sun or moon = `REF`, and in its
//!   alpha whether a surface takes light at all (BG sprite cards are lit by their own colour: no measure there)
//! - only the ground is measured (the id pass's black: where the characters stand), like BG's maps which hold the
//!   shadows cast on the ground, not the light on roofs and leaves
//! - a cell's light is its median pixel (by brightness) among those measured; a cell with none takes its neighbours'
//! - day: shadow colour -> white with the share of sunlight; night: shadow -> moonlit blue with the moonlight, plus the
//!   fires' own colour where they outshine the moon

const REF: f32 = 32.0;
const CW: usize = 16;
const CH: usize = 12;
/// BG's shadow in its day light maps
pub const DAY_SHADOW: [f32; 3] = [90.0, 90.0, 123.0];
/// BG's night light maps: a moonlit cell, a shadowed one (AR2600LN)
pub const NIGHT_LIT: [f32; 3] = [88.0, 90.0, 136.0];
pub const NIGHT_SHADOW: [f32; 3] = [56.0, 57.0, 92.0];

fn lum(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

/// Each cell's light (RGB, open ground in full light = 1 in brightness), from the light pass `rgba` (`w` x `h`, the
/// area's 1x size), measured where `ground` (one flag per pixel; None: everywhere). Cells without a measure take the
/// mean of their measured neighbours, spreading inwards; white (1, 1, 1) when the whole map has none.
pub fn cells(
    rgba: &[u8],
    ground: Option<&[bool]>,
    w: usize,
    h: usize,
) -> (Vec<[f32; 3]>, usize, usize) {
    let (cw, ch) = (w / CW, h / CH);
    let mut out: Vec<Option<[f32; 3]>> = vec![None; cw * ch];
    let mut px: Vec<[f32; 3]> = Vec::with_capacity(CW * CH);
    for cy in 0..ch {
        for cx in 0..cw {
            px.clear();
            for y in cy * CH..(cy + 1) * CH {
                for x in cx * CW..(cx + 1) * CW {
                    let i = (y * w + x) * 4;
                    if rgba[i + 3] >= 128 && ground.is_none_or(|g| g[y * w + x]) {
                        px.push([
                            rgba[i] as f32 / REF,
                            rgba[i + 1] as f32 / REF,
                            rgba[i + 2] as f32 / REF,
                        ]);
                    }
                }
            }
            if !px.is_empty() {
                px.sort_by(|a, b| lum(*a).total_cmp(&lum(*b)));
                out[cy * cw + cx] = Some(px[px.len() / 2]);
            }
        }
    }
    // unmeasured cells (under sprite cards): their measured neighbours' mean, one ring per pass
    loop {
        let mut next = out.clone();
        let mut changed = false;
        for cy in 0..ch {
            for cx in 0..cw {
                if out[cy * cw + cx].is_some() {
                    continue;
                }
                let (mut s, mut n) = ([0f32; 3], 0);
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (x, y) = (cx as i32 + dx, cy as i32 + dy);
                    if x < 0 || y < 0 || x >= cw as i32 || y >= ch as i32 {
                        continue;
                    }
                    if let Some(c) = out[y as usize * cw + x as usize] {
                        (0..3).for_each(|k| s[k] += c[k]);
                        n += 1;
                    }
                }
                if n > 0 {
                    next[cy * cw + cx] = Some(s.map(|v| v / n as f32));
                    changed = true;
                }
            }
        }
        out = next;
        if !changed {
            break;
        }
    }
    (
        out.into_iter().map(|c| c.unwrap_or([1.0; 3])).collect(),
        cw,
        ch,
    )
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [0, 1, 2].map(|k| a[k] + (b[k] - a[k]) * t)
}

/// Day colours: shadow -> white with the cell's sunlight.
pub fn day(cells: &[[f32; 3]]) -> Vec<[u8; 3]> {
    cells
        .iter()
        .map(|c| mix(DAY_SHADOW, [255.0; 3], lum(*c).clamp(0.0, 1.0)).map(|v| v.round() as u8))
        .collect()
}

/// Night colours: shadow -> moonlit blue with the moonlight, then towards the colour of the light beyond the moon's
/// (fires, lamps) as it grows (fully its colour at 4 times the moonlight). The moon's colour is measured on the map:
/// the median colour of the bluish cells lit about as open ground, and taken out of the fires' colour.
pub fn night(cells: &[[f32; 3]]) -> Vec<[u8; 3]> {
    let mut moon: Vec<[f32; 3]> = cells
        .iter()
        .copied()
        .filter(|c| (0.8..1.25).contains(&lum(*c)) && c[2] >= c[0])
        .collect();
    moon.sort_by(|a, b| (a[2] / lum(*a).max(1e-6)).total_cmp(&(b[2] / lum(*b).max(1e-6))));
    // no moonlit cell: Blender's moon colour (render_map.py), at brightness 1
    let m = moon.get(moon.len() / 2).copied().unwrap_or([0.868, 1.008, 1.318]);
    let ml = lum(m).max(1e-6);
    cells
        .iter()
        .map(|c| {
            let l = lum(*c);
            let base = mix(NIGHT_SHADOW, NIGHT_LIT, l.min(1.0));
            let tint = [0, 1, 2].map(|k| c[k] / (m[k] / ml).max(0.05));
            let top = tint.iter().copied().fold(1e-6, f32::max);
            let t = ((l - 1.0) / 3.0).clamp(0.0, 1.0);
            mix(base, tint.map(|v| v / top * 255.0), t).map(|v| v.clamp(0.0, 255.0).round() as u8)
        })
        .collect()
}

/// An 8 bpp palette (BGR + 0, at most 256 colours) and one index per cell. Colours are taken 4 levels apart; past
/// 256 the most used ones stay and the others take their nearest.
pub fn palettize(cols: &[[u8; 3]]) -> (Vec<u8>, Vec<u8>) {
    let q = |c: [u8; 3]| c.map(|v| (v / 4) * 4);
    let mut count: std::collections::HashMap<[u8; 3], usize> = Default::default();
    for c in cols {
        *count.entry(q(*c)).or_default() += 1;
    }
    let mut pal: Vec<([u8; 3], usize)> = count.into_iter().collect();
    pal.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    pal.truncate(256);
    let near = |c: [u8; 3]| -> u8 {
        let d = |p: [u8; 3]| {
            (0..3)
                .map(|k| (p[k] as i32 - c[k] as i32).pow(2))
                .sum::<i32>()
        };
        pal.iter()
            .enumerate()
            .min_by_key(|(_, p)| d(p.0))
            .map(|(i, _)| i as u8)
            .unwrap_or(0)
    };
    let mut memo: std::collections::HashMap<[u8; 3], u8> = Default::default();
    let idx = cols
        .iter()
        .map(|c| *memo.entry(q(*c)).or_insert_with(|| near(q(*c))))
        .collect();
    let mut bytes = Vec::with_capacity(pal.len() * 4);
    for (c, _) in &pal {
        bytes.extend_from_slice(&[c[2], c[1], c[0], 0]);
    }
    (bytes, idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// a 2x2-cell light pass: sun, shadow, a sprite card (no measure), sun
    fn pass(vals: [Option<[u8; 3]>; 4]) -> Vec<u8> {
        let (w, h) = (2 * CW, 2 * CH);
        let mut b = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let c = vals[(y / CH) * 2 + x / CW];
                let i = (y * w + x) * 4;
                if let Some(c) = c {
                    b[i..i + 3].copy_from_slice(&c);
                    b[i + 3] = 255;
                }
            }
        }
        b
    }

    #[test]
    fn day_shadow_and_fill() {
        let p = pass([
            Some([32, 32, 32]),
            Some([0, 0, 0]),
            None,
            Some([32, 32, 32]),
        ]);
        let (c, cw, ch) = cells(&p, None, 2 * CW, 2 * CH);
        assert_eq!((cw, ch), (2, 2));
        let d = day(&c);
        assert_eq!(d[0], [255, 255, 255]);
        assert_eq!(d[1], [90, 90, 123]);
        // the card's cell: the mean of its neighbours (sun and shadow... here sun above, sun right)
        assert_eq!(d[2], [255, 255, 255]);
        let (pal, idx) = palettize(&d);
        assert_eq!(pal.len(), 8);
        assert_ne!(idx[0], idx[1]);
    }

    #[test]
    fn night_moon_and_fire() {
        // moonlight bluish (lum ~ 1), a shadow, a fire 4x brighter in orange
        let p = pass([
            Some([27, 31, 42]),
            Some([3, 3, 4]),
            Some([200, 110, 40]),
            Some([27, 31, 42]),
        ]);
        let (c, _, _) = cells(&p, None, 2 * CW, 2 * CH);
        let n = night(&c);
        let near = |a: [u8; 3], b: [f32; 3]| (0..3).all(|k| (a[k] as f32 - b[k]).abs() <= 3.0);
        assert!(near(n[0], NIGHT_LIT), "{:?}", n[0]);
        assert!(n[1][2] < 100 && n[1][0] < 65, "{:?}", n[1]);
        assert!(
            n[2][0] > n[2][2] && n[2][0] > 200,
            "fire stays warm: {:?}",
            n[2]
        );
        assert!(
            day(&cells(&pass([None; 4]), None, 2 * CW, 2 * CH).0)
                .iter()
                .all(|c| *c == [255; 3])
        );
        // only the ground counts: the shadow cell is a roof (not ground), it takes its neighbours' sun
        let p = pass([
            Some([32, 32, 32]),
            Some([0, 0, 0]),
            Some([32, 32, 32]),
            Some([32, 32, 32]),
        ]);
        let g: Vec<bool> = (0..4 * CW * CH)
            .map(|i| (i % (2 * CW)) < CW || (i / (2 * CW)) >= CH)
            .collect();
        assert_eq!(
            day(&cells(&p, Some(&g), 2 * CW, 2 * CH).0)[1],
            [255, 255, 255]
        );
    }
}
