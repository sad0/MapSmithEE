//! The area's files: WED (the build_camp3d.py layout, known to load in game), BMP maps, ARE (from SadoQoL's template
//! or from scratch) and SadoQoL's M_SQ0CMP.lua.

use super::{Area, Pt};
use crate::are;
use crate::bmpmap;
use crate::res::{u16_at, u32_at};

fn p16(b: &mut [u8], o: usize, v: u16) {
    b[o..o + 2].copy_from_slice(&v.to_le_bytes());
}
fn p32(b: &mut [u8], o: usize, v: u32) {
    b[o..o + 4].copy_from_slice(&v.to_le_bytes());
}
fn resref(b: &mut [u8], o: usize, len: usize, s: &str) {
    b[o..o + len].fill(0);
    for (i, c) in s.bytes().take(len).enumerate() {
        b[o + i] = c;
    }
}

fn bbox(v: &[Pt]) -> (i32, i32, i32, i32) {
    let xs = v.iter().map(|p| p.0);
    let ys = v.iter().map(|p| p.1);
    (xs.clone().min().unwrap_or(0), xs.max().unwrap_or(0), ys.clone().min().unwrap_or(0), ys.max().unwrap_or(0))
}

/// The water overlay of a WED: overlay 1 is the 1x1 tileset `tis` (its `frames` tiles are the animation), drawn under
/// the grid cells `cells` (ascending; no secondary tile, see pvrz::Water)
pub struct WedWater<'a> {
    pub cells: &'a [usize],
    pub tis: &'a str,
    pub frames: usize,
}

/// WED V1.3: 5 overlay slots (only the first used, one tile per cell), no doors, walls as shade polygons, wall groups
/// of 640 x 480 px listing the walls whose box touches them. Byte for byte the Python layout.
pub fn wed(tw: usize, th: usize, polys: &[Vec<Pt>], tis: &str) -> Vec<u8> {
    wed_water(tw, th, polys, tis, None)
}

/// `wed` with water: overlay 1 and, on the water cells, overlay mask bit 1 (value 2), as BG's
/// own water areas (AR5100: WTRIV under the river's cells)
pub fn wed_water(tw: usize, th: usize, polys: &[Vec<Pt>], tis: &str, water: Option<&WedWater>) -> Vec<u8> {
    let (gw, gh) = (tw.div_ceil(10), (th * 2 + 14) / 15);
    let mut groups = Vec::new();
    let mut glist: Vec<u16> = Vec::new();
    let boxes: Vec<_> = polys.iter().map(|v| bbox(v)).collect();
    for gy in 0..gh {
        for gx in 0..gw {
            let (rx, ry) = (gx as i32 * 640, gy as i32 * 480);
            let start = glist.len();
            for (i, b) in boxes.iter().enumerate() {
                if b.1 >= rx && b.0 < rx + 640 && b.3 >= ry && b.2 < ry + 480 {
                    glist.push(i as u16);
                }
            }
            groups.push((start, glist.len() - start));
        }
    }
    let n = tw * th;
    let o_ov = 0x20;
    let o_sh = o_ov + 5 * 0x18;
    let o_tm = o_sh + 0x14;
    let o_ti = o_tm + n * 10;
    // overlay 1's tile map (one cell) and its lookup (the animation's frames), after overlay 0's
    let frames = water.map_or(0, |w| w.frames);
    let o_tm1 = o_ti + n * 2;
    let o_ti1 = o_tm1 + if water.is_some() { 10 } else { 0 };
    let o_poly = o_ti1 + frames * 2;
    let o_vert = o_poly + polys.len() * 0x12;
    let nv: usize = polys.iter().map(|v| v.len()).sum();
    let o_grp = o_vert + nv * 4;
    let o_plk = o_grp + groups.len() * 4;
    let end = o_plk + glist.len() * 2;
    let mut out = vec![0u8; end];
    out[0..8].copy_from_slice(b"WED V1.3");
    for (k, v) in [5u32, 0, o_ov as u32, o_sh as u32, end as u32, end as u32].iter().enumerate() {
        p32(&mut out, 8 + k * 4, *v);
    }
    p16(&mut out, o_ov, tw as u16);
    p16(&mut out, o_ov + 2, th as u16);
    resref(&mut out, o_ov + 4, 8, tis);
    p32(&mut out, o_ov + 0x10, o_tm as u32);
    p32(&mut out, o_ov + 0x14, o_ti as u32);
    for (k, v) in [polys.len(), o_poly, o_vert, o_grp, o_plk].iter().enumerate() {
        p32(&mut out, o_sh + k * 4, *v as u32);
    }
    for i in 0..n {
        let o = o_tm + i * 10;
        p16(&mut out, o, i as u16);
        p16(&mut out, o + 2, 1);
        p16(&mut out, o + 4, 0xffff);
        p16(&mut out, o_ti + i * 2, i as u16);
    }
    if let Some(w) = water {
        let o1 = o_ov + 0x18;
        p16(&mut out, o1, 1);
        p16(&mut out, o1 + 2, 1);
        resref(&mut out, o1 + 4, 8, w.tis);
        p32(&mut out, o1 + 0x10, o_tm1 as u32);
        p32(&mut out, o1 + 0x14, o_ti1 as u32);
        p16(&mut out, o_tm1 + 2, w.frames as u16);
        p16(&mut out, o_tm1 + 4, 0xffff);
        for f in 0..w.frames {
            p16(&mut out, o_ti1 + f * 2, f as u16);
        }
        for &c in w.cells {
            out[o_tm + c * 10 + 6] = 2;
        }
    }
    let mut vi = 0usize;
    for (i, v) in polys.iter().enumerate() {
        let o = o_poly + i * 0x12;
        let (x0, x1, y0, y1) = boxes[i];
        p32(&mut out, o, vi as u32);
        p32(&mut out, o + 4, v.len() as u32);
        out[o + 8] = 1;
        out[o + 9] = 255;
        for (k, c) in [x0, x1, y0, y1].iter().enumerate() {
            p16(&mut out, o + 10 + k * 2, *c as i16 as u16);
        }
        for &(x, y) in v {
            p16(&mut out, o_vert + vi * 4, x as i16 as u16);
            p16(&mut out, o_vert + vi * 4 + 2, y as i16 as u16);
            vi += 1;
        }
    }
    for (i, (s, c)) in groups.iter().enumerate() {
        p16(&mut out, o_grp + i * 4, *s as u16);
        p16(&mut out, o_grp + i * 4 + 2, *c as u16);
    }
    for (i, p) in glist.iter().enumerate() {
        p16(&mut out, o_plk + i * 2, *p);
    }
    out
}

/// the palette (4 bytes per colour) of a BMP file
pub fn bmp_palette(b: &[u8]) -> Option<Vec<u8>> {
    if b.len() < 54 || &b[0..2] != b"BM" {
        return None;
    }
    let hsz = u32_at(b, 14) as usize;
    let bpp = u16_at(b, 28);
    let ncol = match u32_at(b, 46) {
        0 => 1usize << bpp,
        n => n as usize,
    };
    b.get(14 + hsz..14 + hsz + ncol * 4).map(|p| p.to_vec())
}

/// palette-indexed BMP (4 or 8 bpp), bottom-up, written by bmpmap from a fresh header
pub fn bmp(values: &[u8], w: usize, h: usize, bpp: u16, palette: &[u8]) -> Result<Vec<u8>, String> {
    bmpmap::write(&bmpmap::BmpMap::new(w as u32, h as u32, bpp, palette, values))
}

/// SQCAMP.ARE from SadoQoL's template: its header, actor, region, entrance and animation records are the models
/// (the build_camp3d.py layout; sections the camp doesn't use are emptied)
pub fn are_sadoqol(a: &[u8], ar: &Area) -> Result<Vec<u8>, String> {
    if a.len() < 0x11c || &a[0..8] != b"AREAV1.0" {
        return Err("template SQCAMP.ARE: not an ARE V1.0 file".into());
    }
    let s32 = |o: usize| u32_at(a, o) as usize;
    let rec = |o: usize, len: usize| -> Result<Vec<u8>, String> {
        a.get(o..o + len).map(|r| r.to_vec()).ok_or_else(|| "template SQCAMP.ARE: truncated".to_string())
    };
    let actor_tpl = rec(s32(0x54), 0x110)?;
    let mut actors = Vec::new();
    for (name, cre, key) in [("Wandering priest", "SQPRIEST", "PRIEST"), ("Camp merchant", "SQMERCH", "MERCH")] {
        let s = ar.spot(key).ok_or(format!("no {key} spot"))?;
        let mut e = actor_tpl.clone();
        resref(&mut e, 0, 32, name);
        for (k, v) in [s.x, s.y, s.x, s.y].iter().enumerate() {
            p16(&mut e, 0x20 + k * 2, *v as u16);
        }
        p32(&mut e, 0x34, s.f.unwrap_or(0) as u32);
        resref(&mut e, 0x80, 8, cre);
        actors.extend_from_slice(&e);
    }
    let reg_tpl = rec(s32(0x5c), 0xc4)?;
    let mut regions = Vec::new();
    let mut verts: Vec<Pt> = Vec::new();
    for (i, poly) in ar.exits.iter().enumerate() {
        let (x0, x1, y0, y1) = bbox(poly);
        let mut r = reg_tpl.clone();
        let name = if i == 0 { "SQCampExit".to_string() } else { format!("SQCampExit{}", i + 1) };
        resref(&mut r, 0, 32, &name);
        for (k, v) in [x0, y0, x1, y1, poly.len() as i32].iter().enumerate() {
            p16(&mut r, 0x22 + k * 2, *v as u16);
        }
        p32(&mut r, 0x2c, verts.len() as u32);
        p32(&mut r, 0x34, super::REGION_CURSOR);
        regions.extend_from_slice(&r);
        verts.extend_from_slice(poly);
    }
    let mut ent = rec(s32(0x68), 0x68)?;
    let e = ar.spot("ENTRY").ok_or("no ENTRY spot")?;
    for (k, v) in [e.x, e.y, e.f.unwrap_or(0) as i32].iter().enumerate() {
        p16(&mut ent, 0x20 + k * 2, *v as u16);
    }
    let mut anims = Vec::new();
    let na = s32(0xac);
    if na > 0 {
        // the template's fire-pit entry is the model for every flame
        let ents: Vec<Vec<u8>> = (0..na).filter_map(|i| rec(s32(0xb0) + i * 0x4c, 0x4c).ok()).collect();
        let tpl_e = ents
            .iter()
            .find(|e| &e[0x28..0x2c] == b"FPIT")
            .or(ents.first())
            .ok_or("template SQCAMP.ARE: bad animations")?;
        for (name, bam, x, y) in &ar.flames {
            let mut e = tpl_e.clone();
            resref(&mut e, 0, 32, &name[..name.len().min(31)]);
            p16(&mut e, 0x20, *x as u16);
            p16(&mut e, 0x22, *y as u16);
            resref(&mut e, 0x28, 8, bam);
            anims.extend_from_slice(&e);
        }
    }
    let songs = rec(s32(0xbc), 0x90)?;
    let rest = rec(s32(0xc0), 0xe4)?;
    let mut h = a[..0x11c].to_vec();
    let t = u16_at(a, 0x48);
    p16(&mut h, 0x48, if ar.night { t | 0x42 } else { t & !0x42 });
    let mut body = Vec::new();
    let mut put = |blob: &[u8]| -> u32 {
        let at = 0x11c + body.len();
        body.extend_from_slice(blob);
        at as u32
    };
    let vb: Vec<u8> =
        verts.iter().flat_map(|&(x, y)| [(x as u16).to_le_bytes(), (y as u16).to_le_bytes()].concat()).collect();
    let o_act = put(&actors);
    let o_reg = put(&regions);
    let o_vert = put(&vb);
    let o_ent = put(&ent);
    let o_anim = put(&anims);
    let o_song = put(&songs);
    let o_rest = put(&rest);
    let o_end = 0x11c + body.len() as u32;
    let fields: [(usize, bool, u32); 31] = [
        (0x54, true, o_act),
        (0x58, false, (actors.len() / 0x110) as u32),
        (0x5a, false, ar.exits.len() as u32),
        (0x5c, true, o_reg),
        (0x60, true, o_end),
        (0x64, true, 0),
        (0x68, true, o_ent),
        (0x6c, true, 1),
        (0x70, true, o_end),
        (0x74, false, 0),
        (0x76, false, 0),
        (0x78, true, o_end),
        (0x7c, true, o_vert),
        (0x80, false, verts.len() as u32),
        (0x82, false, 0),
        (0x84, true, o_end),
        (0x88, true, o_end),
        (0x8c, true, 0),
        (0x90, true, 0),
        (0x9c, true, 0),
        (0xa0, true, o_end),
        (0xa4, true, 0),
        (0xa8, true, o_end),
        (0xac, true, (anims.len() / 0x4c) as u32),
        (0xb0, true, o_anim),
        (0xb4, true, 0),
        (0xb8, true, o_end),
        (0xbc, true, o_song),
        (0xc0, true, o_rest),
        (0xc4, true, o_end),
        (0xc8, true, 0),
    ];
    for (o, wide, v) in fields {
        if wide { p32(&mut h, o, v) } else { p16(&mut h, o, v as u16) }
    }
    p32(&mut h, 0xcc, o_end);
    p32(&mut h, 0xd0, 0);
    h.extend_from_slice(&body);
    Ok(h)
}

/// area type: outdoor, weather; day/night + extended night (own WED/tileset) when there is a night render
const AREA_OUTDOOR: u16 = 0x01 | 0x04;
const AREA_NIGHT: u16 = 0x02 | 0x40;
/// the camp's music: day, night (90), battle (75)
const SONGS: [u32; 4] = [90, 90, 0, 75];
/// animation flags: enabled, blended (black is transparent), shown in combat
const ANIM_FLAGS: u32 = 0x1003;
/// travel region flags: the whole party must be there
const REGION_PARTY: u32 = 0x04;

/// a self-contained ARE: entrance "Entry", exits Exit1.. to the world map, flames; no actors, no rest encounters
pub fn are_generic(ar: &Area) -> Vec<u8> {
    let mut h = vec![0u8; 0x11c];
    h[0..8].copy_from_slice(b"AREAV1.0");
    resref(&mut h, 0x08, 8, &ar.name);
    p16(&mut h, 0x4a, 20); // rain
    p16(&mut h, 0x50, 5); // lightning
    let pt = |x: i32, y: i32| [x as i16, y as i16];
    let regions = ar
        .exits
        .iter()
        .enumerate()
        .map(|(i, poly)| are::Region {
            name: format!("Exit{}", i + 1),
            kind: 2,
            pts: poly.iter().map(|&(x, y)| pt(x, y)).collect(),
            cursor: super::REGION_CURSOR,
            flags: REGION_PARTY,
            info: 0xffff_ffff,
            ..Default::default()
        })
        .collect();
    let e = ar.spot("ENTRY").expect("ENTRY spot");
    let entrances = vec![are::Entrance {
        name: "Entry".into(),
        pos: pt(e.x, e.y),
        orientation: e.f.unwrap_or(0) as u16,
        raw: String::new(),
    }];
    let mut anims = Vec::new();
    for (name, bam, x, y) in &ar.flames {
        let mut r = vec![0u8; 0x4c];
        resref(&mut r, 0, 32, &name[..name.len().min(31)]);
        p16(&mut r, 0x20, *x as u16);
        p16(&mut r, 0x22, *y as u16);
        p32(&mut r, 0x24, 0x00ff_ffff); // every hour
        resref(&mut r, 0x28, 8, bam);
        p32(&mut r, 0x34, ANIM_FLAGS);
        anims.extend_from_slice(&r);
    }
    let mut songs = vec![0u8; 0x90];
    for (k, v) in SONGS.iter().enumerate() {
        p32(&mut songs, k * 4, *v);
    }
    use base64::Engine as _;
    let b64 = |b: &[u8]| base64::engine::general_purpose::STANDARD.encode(b);
    let a = are::Are {
        wed: ar.name.clone(),
        area_type: AREA_OUTDOOR | if ar.night { AREA_NIGHT } else { 0 },
        regions,
        entrances,
        header: b64(&h),
        keep: are::Keep {
            anims: (ar.flames.len() as u32, b64(&anims)),
            songs: b64(&songs),
            rest: b64(&[0u8; 0xe4]),
            ..Default::default()
        },
        ..Default::default()
    };
    are::write(&a)
}

/// SadoQoL's M_SQ0CMP.lua: the camp's spots and exits (the ARE's numbers), CRLF (no sight circle: dropped 2026-11-03)
pub fn lua(ar: &Area) -> String {
    let pt = |s: &super::Spot| match s.f {
        Some(f) => format!("{{ {}, {}, {} }}", s.x, s.y, f),
        None => format!("{{ {}, {} }}", s.x, s.y),
    };
    let ex: Vec<String> = ar
        .exits
        .iter()
        .map(|poly| {
            format!(
                "\t\t{{ {} }}",
                poly.iter().map(|p| format!("{{ {}, {} }}", p.0, p.1)).collect::<Vec<_>>().join(", ")
            )
        })
        .collect();
    let rows: Vec<String> = ["FIRE", "PRIEST", "MERCH", "CAIN", "WITHERS", "RECRUIT", "NECRO", "NECRO_CENTER"]
        .iter()
        .filter_map(|k| ar.spot(k).map(|s| format!("\t{k:<12} = {},", pt(&s))))
        .collect();
    let t = format!(
        "--[[
SadoQoL rest camp: where things stand in SQCAMP when it is a Camp Map 3D map (E:/AI/bg3d). WRITTEN BY
MapSmithEE (campbuild, the port of build_camp3d.py) from the editor's spots (the ARE uses the same numbers). Points are
{{ x, y }} or {{ x, y, facing }} (0 south, 4 west, 8 north, 12 east).
--]]

SQCampSite = {{
\tENTRY   = {},
\tEXITS   = {{
{},
\t}},
{}
}}
",
        pt(&ar.spot("ENTRY").unwrap()),
        ex.join(",\n"),
        rows.join("\n")
    );
    t.replace('\n', "\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wed_water_overlay() {
        let w = WedWater { cells: &[1, 3], tis: "AR9900W", frames: 8 };
        let b = wed_water(2, 2, &[], "AR9900", Some(&w));
        let u16_at = |o: usize| u16::from_le_bytes([b[o], b[o + 1]]);
        let u32_at = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap()) as usize;
        let (ov, tm0) = (u32_at(0x10), u32_at(0x20 + 0x10));
        assert_eq!((u16_at(ov + 0x18), u16_at(ov + 0x1a)), (1, 1));
        assert_eq!(&b[ov + 0x18 + 4..ov + 0x18 + 11], b"AR9900W");
        let (tm1, ti1) = (u32_at(ov + 0x18 + 0x10), u32_at(ov + 0x18 + 0x14));
        assert_eq!(u16_at(tm1 + 2), 8);
        assert_eq!((0..8).map(|f| u16_at(ti1 + f * 2)).collect::<Vec<_>>(), (0..8).collect::<Vec<_>>());
        assert_eq!((u16_at(tm0 + 10 + 4), b[tm0 + 10 + 6]), (0xffff, 2));
        assert_eq!((u16_at(tm0 + 30 + 4), b[tm0 + 30 + 6]), (0xffff, 2));
        assert_eq!((u16_at(tm0 + 4), b[tm0 + 6]), (0xffff, 0));
        // and the WED reader takes it
        let parsed = crate::wed::parse(&b).unwrap();
        assert_eq!(parsed.overlays.len(), 5);
        assert_eq!(parsed.overlays[1].tileset, "AR9900W");
    }

    #[test]
    fn wed_parses() {
        let polys = vec![vec![(10, 10), (700, 20), (650, 500)], vec![(1, 1), (5, 1), (5, 5)]];
        let b = wed(12, 9, &polys, "AR9900");
        let w = crate::wed::parse(&b).unwrap();
        assert_eq!(w.overlays[0].w, 12);
        assert_eq!(w.overlays[0].tileset, "AR9900");
        assert_eq!(w.walls.len(), 2);
        assert_eq!(w.walls[0].pts[1], [700, 20]);
    }
}
