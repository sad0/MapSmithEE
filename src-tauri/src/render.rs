//! Area pictures: overlay 0 of a WED drawn with its tileset, plus one patch per door with its secondary tiles.

use crate::res::{Game, Res};
use crate::tis::Tileset;
use crate::wed::Wed;
use serde::Serialize;

#[derive(Serialize, Clone, Debug)]
pub struct DoorPatch {
    pub door: String,
    /// pixel rectangle of the patch in the area
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    /// image key in the picture cache
    pub img: String,
}

pub struct Picture {
    pub w: u32,
    pub h: u32,
    pub png: Vec<u8>,
    pub doors: Vec<(DoorPatch, Vec<u8>)>,
}

pub fn png(w: u32, h: u32, rgba: &[u8]) -> Res<Vec<u8>> {
    let mut out = Vec::new();
    {
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(png::ColorType::Rgba);
        e.set_depth(png::BitDepth::Eight);
        e.set_compression(png::Compression::Fast);
        let mut wr = e.write_header().map_err(|e| e.to_string())?;
        wr.write_image_data(rgba).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

pub fn draw(game: &Game, wed: &Wed, key: &str) -> Res<Picture> {
    let ov = wed.overlays.first().ok_or("WED has no overlay")?;
    let mut ts = Tileset::load(game, &ov.tileset)?;
    let (tw, th) = (ov.w as usize, ov.h as usize);
    let (w, h) = (tw * 64, th * 64);
    let mut px = vec![0u8; w * h * 4];
    for (i, c) in ov.cells.iter().enumerate() {
        let tile = ov.lookup.get(c.start as usize).copied().unwrap_or(c.start) as usize;
        ts.blit(game, tile, &mut px, w, (i % tw) * 64, (i / tw) * 64);
    }
    // overlay pixels are opaque: the engine shows the area black where a tileset has holes
    for a in px.chunks_exact_mut(4) {
        a[3] = 255;
    }
    let mut doors = Vec::new();
    for d in &wed.doors {
        let cells: Vec<usize> = d.cells.iter().map(|&c| c as usize).filter(|&c| c < ov.cells.len()).collect();
        if cells.is_empty() {
            continue;
        }
        let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
        for &c in &cells {
            x0 = x0.min(c % tw);
            y0 = y0.min(c / tw);
            x1 = x1.max(c % tw);
            y1 = y1.max(c / tw);
        }
        let (pw, ph) = ((x1 - x0 + 1) * 64, (y1 - y0 + 1) * 64);
        let mut p = vec![0u8; pw * ph * 4];
        let mut any = false;
        for &c in &cells {
            let alt = ov.cells[c].alt;
            if alt == 0xffff {
                continue;
            }
            any = true;
            ts.blit(game, alt as usize, &mut p, pw, (c % tw - x0) * 64, (c / tw - y0) * 64);
            for r in 0..64 {
                let o = (((c / tw - y0) * 64 + r) * pw + (c % tw - x0) * 64) * 4;
                for k in 0..64 {
                    p[o + k * 4 + 3] = 255;
                }
            }
        }
        if !any {
            continue;
        }
        let patch = DoorPatch {
            door: d.name.clone(),
            x: (x0 * 64) as u32,
            y: (y0 * 64) as u32,
            w: pw as u32,
            h: ph as u32,
            img: format!("{key}-door-{}", d.name),
        };
        doors.push((patch, png(pw as u32, ph as u32, &p)?));
    }
    Ok(Picture { w: w as u32, h: h as u32, png: png(w as u32, h as u32, &px)?, doors })
}
