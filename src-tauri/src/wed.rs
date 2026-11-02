//! WED V1.3: overlays and tile maps are kept as read, wall polygons and door polygons are editable.
//! Writing rebuilds the file and its wall groups from the polygons.

use crate::res::{Res, i16_at, text_at, u16_at, u32_at};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct Poly {
    pub flags: u8,
    pub height: u8,
    pub pts: Vec<[i16; 2]>,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct WedDoor {
    pub name: String,
    /// as stored: 0 open, 1 closed
    pub closed: u16,
    /// tile map cells (overlay 0) that swap to their secondary tile with the door state
    pub cells: Vec<u16>,
    pub open: Vec<Poly>,
    pub shut: Vec<Poly>,
}

#[derive(Clone, Debug)]
pub struct TileCell {
    pub start: u16,
    pub count: u16,
    pub alt: u16,
    pub mask: u8,
    pub rest: [u8; 3],
}

#[derive(Clone, Debug)]
pub struct Overlay {
    pub w: u16,
    pub h: u16,
    pub tileset: String,
    pub unique: u16,
    pub movement: u16,
    pub cells: Vec<TileCell>,
    pub lookup: Vec<u16>,
}

#[derive(Clone, Debug)]
pub struct Wed {
    pub overlays: Vec<Overlay>,
    pub walls: Vec<Poly>,
    pub doors: Vec<WedDoor>,
}

/// the editable part handed to the UI
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct WedEdit {
    pub walls: Vec<Poly>,
    pub doors: Vec<WedDoor>,
}

fn read_polys(b: &[u8], off: usize, n: usize, vert_off: usize) -> Vec<Poly> {
    (0..n)
        .map(|i| {
            let o = off + i * 0x12;
            let (start, count) = (u32_at(b, o) as usize, u32_at(b, o + 4) as usize);
            let pts = (0..count)
                .map(|k| {
                    let v = vert_off + (start + k) * 4;
                    [i16_at(b, v), i16_at(b, v + 2)]
                })
                .collect();
            Poly { flags: b[o + 8], height: b[o + 9], pts }
        })
        .collect()
}

pub fn parse(b: &[u8]) -> Res<Wed> {
    if b.len() < 0x20 || &b[0..8] != b"WED V1.3" {
        return Err("not a WED V1.3 file".into());
    }
    let (nov, ndoor, o_ov, o_sec, o_door, o_cells) = (
        u32_at(b, 8) as usize,
        u32_at(b, 12) as usize,
        u32_at(b, 16) as usize,
        u32_at(b, 20) as usize,
        u32_at(b, 24) as usize,
        u32_at(b, 28) as usize,
    );
    let mut overlays = Vec::with_capacity(nov);
    for i in 0..nov {
        let o = o_ov + i * 0x18;
        let (w, h) = (u16_at(b, o), u16_at(b, o + 2));
        let (o_tm, o_lk) = (u32_at(b, o + 0x10) as usize, u32_at(b, o + 0x14) as usize);
        let n = w as usize * h as usize;
        let cells: Vec<TileCell> = (0..n)
            .map(|k| {
                let c = o_tm + k * 10;
                TileCell {
                    start: u16_at(b, c),
                    count: u16_at(b, c + 2),
                    alt: u16_at(b, c + 4),
                    mask: b[c + 6],
                    rest: [b[c + 7], b[c + 8], b[c + 9]],
                }
            })
            .collect();
        // the lookup table length is not stored: it covers every index the cells use
        let nl = cells.iter().map(|c| c.start as usize + c.count as usize).max().unwrap_or(0);
        let nl = if n == 0 || o_lk == 0 { 0 } else { nl };
        let lookup = (0..nl).filter(|k| o_lk + k * 2 + 2 <= b.len()).map(|k| u16_at(b, o_lk + k * 2)).collect();
        overlays.push(Overlay {
            w,
            h,
            tileset: text_at(b, o + 4, 8).to_ascii_uppercase(),
            unique: u16_at(b, o + 0xc),
            movement: u16_at(b, o + 0xe),
            cells,
            lookup,
        });
    }
    let (npoly, o_poly, o_vert) = (u32_at(b, o_sec) as usize, u32_at(b, o_sec + 4) as usize, u32_at(b, o_sec + 8) as usize);
    let walls = read_polys(b, o_poly, npoly, o_vert);
    let doors = (0..ndoor)
        .map(|i| {
            let o = o_door + i * 0x1a;
            let (first, ncell) = (u16_at(b, o + 0xa) as usize, u16_at(b, o + 0xc) as usize);
            WedDoor {
                name: text_at(b, o, 8).to_ascii_uppercase(),
                closed: u16_at(b, o + 8),
                cells: (0..ncell).map(|k| u16_at(b, o_cells + (first + k) * 2)).collect(),
                open: read_polys(b, u32_at(b, o + 0x12) as usize, u16_at(b, o + 0xe) as usize, o_vert),
                shut: read_polys(b, u32_at(b, o + 0x16) as usize, u16_at(b, o + 0x10) as usize, o_vert),
            }
        })
        .collect();
    Ok(Wed { overlays, walls, doors })
}

fn bbox(p: &Poly) -> [i16; 4] {
    let (mut x0, mut x1, mut y0, mut y1) = (i16::MAX, i16::MIN, i16::MAX, i16::MIN);
    for &[x, y] in &p.pts {
        x0 = x0.min(x);
        x1 = x1.max(x);
        y0 = y0.min(y);
        y1 = y1.max(y);
    }
    if p.pts.is_empty() { [0; 4] } else { [x0, x1, y0, y1] }
}

struct Buf(Vec<u8>);
impl Buf {
    fn u8(&mut self, v: u8) {
        self.0.push(v)
    }
    fn u16(&mut self, v: u16) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }
    fn i16(&mut self, v: i16) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes())
    }
    fn resref(&mut self, s: &str) {
        let mut r = [0u8; 8];
        for (i, c) in s.bytes().take(8).enumerate() {
            r[i] = c;
        }
        self.0.extend_from_slice(&r)
    }
    fn at(&self) -> u32 {
        self.0.len() as u32
    }
    fn put_u32(&mut self, at: usize, v: u32) {
        self.0[at..at + 4].copy_from_slice(&v.to_le_bytes())
    }
}

/// rebuild the whole file. Wall groups: one per 10 x 7.5 tiles block of overlay 0 (640 x 480 px), listing the walls
/// whose bounding box touches it.
pub fn write(w: &Wed) -> Vec<u8> {
    let mut b = Buf(Vec::new());
    b.0.extend_from_slice(b"WED V1.3");
    b.u32(w.overlays.len() as u32);
    b.u32(w.doors.len() as u32);
    b.u32(0x20); // overlays
    let o_sec_field = 20;
    b.u32(0);
    let o_door_field = 24;
    b.u32(0);
    let o_cells_field = 28;
    b.u32(0);
    // overlay headers first, their tile maps afterwards
    let ov_hdr = b.0.len();
    for ov in &w.overlays {
        b.u16(ov.w);
        b.u16(ov.h);
        b.resref(&ov.tileset);
        b.u16(ov.unique);
        b.u16(ov.movement);
        b.u32(0);
        b.u32(0);
    }
    let sec = b.at();
    b.put_u32(o_sec_field, sec);
    for _ in 0..5 {
        b.u32(0);
    }
    let door_hdr = b.at();
    b.put_u32(o_door_field, door_hdr);
    for _ in &w.doors {
        b.0.extend_from_slice(&[0u8; 0x1a]);
    }
    for (i, ov) in w.overlays.iter().enumerate() {
        let tm = b.at();
        for c in &ov.cells {
            b.u16(c.start);
            b.u16(c.count);
            b.u16(c.alt);
            b.u8(c.mask);
            b.0.extend_from_slice(&c.rest);
        }
        let lk = b.at();
        for &v in &ov.lookup {
            b.u16(v);
        }
        let o = ov_hdr + i * 0x18;
        b.put_u32(o + 0x10, tm);
        b.put_u32(o + 0x14, lk);
    }
    let cells_at = b.at();
    b.put_u32(o_cells_field, cells_at);
    let mut first = 0u16;
    for (i, d) in w.doors.iter().enumerate() {
        for &c in &d.cells {
            b.u16(c);
        }
        let o = door_hdr as usize + i * 0x1a;
        let mut name = [0u8; 8];
        for (k, c) in d.name.bytes().take(8).enumerate() {
            name[k] = c;
        }
        b.0[o..o + 8].copy_from_slice(&name);
        b.0[o + 8..o + 10].copy_from_slice(&d.closed.to_le_bytes());
        b.0[o + 0xa..o + 0xc].copy_from_slice(&first.to_le_bytes());
        b.0[o + 0xc..o + 0xe].copy_from_slice(&(d.cells.len() as u16).to_le_bytes());
        b.0[o + 0xe..o + 0x10].copy_from_slice(&(d.open.len() as u16).to_le_bytes());
        b.0[o + 0x10..o + 0x12].copy_from_slice(&(d.shut.len() as u16).to_le_bytes());
        first += d.cells.len() as u16;
    }
    // polygons: walls, then each door's open and closed ones, then all vertices
    let all: Vec<&Poly> = w.walls.iter().chain(w.doors.iter().flat_map(|d| d.open.iter().chain(d.shut.iter()))).collect();
    let poly_at = b.at();
    let mut vi = 0u32;
    for p in &all {
        let [x0, x1, y0, y1] = bbox(p);
        b.u32(vi);
        b.u32(p.pts.len() as u32);
        b.u8(p.flags);
        b.u8(p.height);
        b.i16(x0);
        b.i16(x1);
        b.i16(y0);
        b.i16(y1);
        vi += p.pts.len() as u32;
    }
    let mut k = w.walls.len() as u32;
    for (i, d) in w.doors.iter().enumerate() {
        let o = door_hdr as usize + i * 0x1a;
        let open_at = poly_at + k * 0x12;
        k += d.open.len() as u32;
        let shut_at = poly_at + k * 0x12;
        k += d.shut.len() as u32;
        b.put_u32(o + 0x12, open_at);
        b.put_u32(o + 0x16, shut_at);
    }
    let vert_at = b.at();
    for p in &all {
        for &[x, y] in &p.pts {
            b.i16(x);
            b.i16(y);
        }
    }
    // wall groups
    let (tw, th) = w.overlays.first().map(|o| (o.w as i32, o.h as i32)).unwrap_or((0, 0));
    let (gw, gh) = ((tw + 9) / 10, (th * 2 + 14) / 15);
    let mut lists: Vec<Vec<u16>> = vec![Vec::new(); (gw * gh).max(0) as usize];
    for (pi, p) in w.walls.iter().enumerate() {
        let [x0, x1, y0, y1] = bbox(p);
        let gx0 = (x0 as i32 / 640).clamp(0, gw - 1);
        let gx1 = (x1 as i32 / 640).clamp(0, gw - 1);
        let gy0 = (y0 as i32 / 480).clamp(0, gh - 1);
        let gy1 = (y1 as i32 / 480).clamp(0, gh - 1);
        for gy in gy0..=gy1 {
            for gx in gx0..=gx1 {
                lists[(gy * gw + gx) as usize].push(pi as u16);
            }
        }
    }
    let grp_at = b.at();
    let mut start = 0u16;
    for l in &lists {
        b.u16(start);
        b.u16(l.len() as u16);
        start += l.len() as u16;
    }
    let plk_at = b.at();
    for l in &lists {
        for &p in l {
            b.u16(p);
        }
    }
    let s = sec as usize;
    b.put_u32(s, w.walls.len() as u32);
    b.put_u32(s + 4, poly_at);
    b.put_u32(s + 8, vert_at);
    b.put_u32(s + 12, grp_at);
    b.put_u32(s + 16, plk_at);
    b.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_keeps_polys() {
        let w = Wed {
            overlays: vec![Overlay {
                w: 2,
                h: 2,
                tileset: "AR0100".into(),
                unique: 4,
                movement: 0,
                cells: (0..4).map(|i| TileCell { start: i, count: 1, alt: 0xffff, mask: 0, rest: [0; 3] }).collect(),
                lookup: vec![0, 1, 2, 3],
            }],
            walls: vec![Poly { flags: 1, height: 255, pts: vec![[1, 2], [50, 2], [50, 60]] }],
            doors: vec![WedDoor {
                name: "DOOR01".into(),
                closed: 1,
                cells: vec![1, 3],
                open: vec![Poly { flags: 0x81, height: 255, pts: vec![[0, 0], [5, 5], [0, 9]] }],
                shut: vec![],
            }],
        };
        let back = parse(&write(&w)).unwrap();
        assert_eq!(back.walls, w.walls);
        assert_eq!(back.doors, w.doors);
        assert_eq!(back.overlays[0].lookup, w.overlays[0].lookup);
    }
}
