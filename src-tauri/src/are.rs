//! ARE V1.0. Actors, regions, doors and entrances are decoded and editable; every record also keeps its original bytes
//! so fields we don't expose survive a save. The sections we never touch are copied as they were.

use crate::res::{Res, i16_at, text_at, u16_at, u32_at};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};

pub type Pt = [i16; 2];

fn b64(b: &[u8]) -> String {
    B64.encode(b)
}
fn unb64(s: &str, len: usize) -> Vec<u8> {
    let mut v = B64.decode(s).unwrap_or_default();
    v.resize(len, 0);
    v
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Actor {
    pub name: String,
    pub pos: Pt,
    pub dest: Pt,
    pub orientation: u16,
    pub cre: String,
    pub dialog: String,
    pub scripts: [String; 6], // override, general, class, race, default, specific
    pub flags: u32,
    pub schedule: u32,
    pub animation: u32,
    /// embedded CRE (base64) when the actor carries one
    pub embedded: Option<String>,
    pub raw: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Region {
    pub name: String,
    /// 0 trap (proximity trigger), 1 info point, 2 travel
    pub kind: u16,
    pub pts: Vec<Pt>,
    pub trigger_value: u32,
    pub cursor: u32,
    pub dest_area: String,
    pub entrance: String,
    pub flags: u32,
    pub info: u32,
    pub trap_detect: u16,
    pub trap_remove: u16,
    pub trapped: u16,
    pub detected: u16,
    pub launch: Pt,
    pub key: String,
    pub script: String,
    pub use_point: Pt,
    pub talk_point: Pt,
    pub speaker: u32,
    pub dialog: String,
    pub raw: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Door {
    pub name: String,
    pub wed_id: String,
    pub flags: u32,
    pub open: Vec<Pt>,
    pub shut: Vec<Pt>,
    pub open_cells: Vec<Pt>,
    pub shut_cells: Vec<Pt>,
    pub cursor: u32,
    pub key: String,
    pub script: String,
    pub dialog: String,
    pub raw: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Container {
    pub name: String,
    pub pos: Pt,
    pub kind: u16,
    pub pts: Vec<Pt>,
    pub raw: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Entrance {
    pub name: String,
    pub pos: Pt,
    pub orientation: u16,
    pub raw: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Anim {
    pub name: String,
    pub pos: Pt,
    pub bam: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Are {
    pub wed: String,
    pub flags: u32,
    pub area_type: u16,
    pub script: String,
    pub actors: Vec<Actor>,
    pub regions: Vec<Region>,
    pub doors: Vec<Door>,
    pub containers: Vec<Container>,
    pub entrances: Vec<Entrance>,
    /// read only, shown on the map
    pub anims: Vec<Anim>,
    /// original header (base64)
    pub header: String,
    /// sections we copy unchanged (base64): spawn points, items, ambients, variables, explored, animations, notes,
    /// tiled objects, projectile traps, songs, rest interruptions
    pub keep: Keep,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Keep {
    pub spawns: (u32, String),
    pub items: (u32, String),
    pub ambients: (u32, String),
    pub variables: (u32, String),
    pub explored: (u32, String),
    pub anims: (u32, String),
    pub notes: (u32, String),
    pub tiled: (u32, String),
    pub traps: (u32, String),
    pub songs: String,
    pub rest: String,
}

fn rr(b: &[u8], o: usize) -> String {
    text_at(b, o, 8).to_ascii_uppercase()
}
fn pt(b: &[u8], o: usize) -> Pt {
    [i16_at(b, o), i16_at(b, o + 2)]
}

fn slice(b: &[u8], off: u32, len: usize) -> &[u8] {
    let o = off as usize;
    if o == 0 || o >= b.len() || len == 0 { &[] } else { &b[o..(o + len).min(b.len())] }
}

pub fn parse(b: &[u8]) -> Res<Are> {
    if b.len() < 0x11c || &b[0..8] != b"AREAV1.0" {
        return Err("not an ARE V1.0 file".into());
    }
    let o_vert = u32_at(b, 0x7c) as usize;
    let nvert = u16_at(b, 0x80) as usize;
    let verts = |first: usize, n: usize| -> Vec<Pt> {
        (first..first + n).filter(|&k| k < nvert).map(|k| pt(b, o_vert + k * 4)).collect()
    };

    let mut actors = Vec::new();
    let (o_act, n_act) = (u32_at(b, 0x54) as usize, u16_at(b, 0x58) as usize);
    for i in 0..n_act {
        let r = &b[o_act + i * 0x110..o_act + (i + 1) * 0x110];
        let (co, cs) = (u32_at(r, 0x88) as usize, u32_at(r, 0x8c) as usize);
        actors.push(Actor {
            name: text_at(r, 0, 32),
            pos: pt(r, 0x20),
            dest: pt(r, 0x24),
            orientation: u16_at(r, 0x34),
            cre: rr(r, 0x80),
            dialog: rr(r, 0x48),
            scripts: [rr(r, 0x50), rr(r, 0x58), rr(r, 0x60), rr(r, 0x68), rr(r, 0x70), rr(r, 0x78)],
            flags: u32_at(r, 0x28),
            schedule: u32_at(r, 0x40),
            animation: u32_at(r, 0x30),
            embedded: (co != 0 && cs != 0 && co + cs <= b.len()).then(|| b64(&b[co..co + cs])),
            raw: b64(r),
        });
    }

    let mut regions = Vec::new();
    let (o_reg, n_reg) = (u32_at(b, 0x5c) as usize, u16_at(b, 0x5a) as usize);
    for i in 0..n_reg {
        let r = &b[o_reg + i * 0xc4..o_reg + (i + 1) * 0xc4];
        regions.push(Region {
            name: text_at(r, 0, 32),
            kind: u16_at(r, 0x20),
            pts: verts(u32_at(r, 0x2c) as usize, u16_at(r, 0x2a) as usize),
            trigger_value: u32_at(r, 0x30),
            cursor: u32_at(r, 0x34),
            dest_area: rr(r, 0x38),
            entrance: text_at(r, 0x40, 32),
            flags: u32_at(r, 0x60),
            info: u32_at(r, 0x64),
            trap_detect: u16_at(r, 0x68),
            trap_remove: u16_at(r, 0x6a),
            trapped: u16_at(r, 0x6c),
            detected: u16_at(r, 0x6e),
            launch: pt(r, 0x70),
            key: rr(r, 0x74),
            script: rr(r, 0x7c),
            use_point: pt(r, 0x84),
            talk_point: pt(r, 0xb4),
            speaker: u32_at(r, 0xb8),
            dialog: rr(r, 0xbc),
            raw: b64(r),
        });
    }

    let mut doors = Vec::new();
    let (o_door, n_door) = (u32_at(b, 0xa8) as usize, u32_at(b, 0xa4) as usize);
    for i in 0..n_door {
        let r = &b[o_door + i * 0xc8..o_door + (i + 1) * 0xc8];
        doors.push(Door {
            name: text_at(r, 0, 32),
            wed_id: rr(r, 0x20),
            flags: u32_at(r, 0x28),
            open: verts(u32_at(r, 0x2c) as usize, u16_at(r, 0x30) as usize),
            shut: verts(u32_at(r, 0x34) as usize, u16_at(r, 0x32) as usize),
            open_cells: verts(u32_at(r, 0x48) as usize, u16_at(r, 0x4c) as usize),
            shut_cells: verts(u32_at(r, 0x50) as usize, u16_at(r, 0x4e) as usize),
            cursor: u32_at(r, 0x68),
            key: rr(r, 0x78),
            script: rr(r, 0x80),
            dialog: rr(r, 0xb8),
            raw: b64(r),
        });
    }

    let mut containers = Vec::new();
    let (o_con, n_con) = (u32_at(b, 0x70) as usize, u16_at(b, 0x74) as usize);
    for i in 0..n_con {
        let r = &b[o_con + i * 0xc0..o_con + (i + 1) * 0xc0];
        containers.push(Container {
            name: text_at(r, 0, 32),
            pos: pt(r, 0x20),
            kind: u16_at(r, 0x24),
            pts: verts(u32_at(r, 0x50) as usize, u16_at(r, 0x54) as usize),
            raw: b64(r),
        });
    }

    let mut entrances = Vec::new();
    let (o_ent, n_ent) = (u32_at(b, 0x68) as usize, u32_at(b, 0x6c) as usize);
    for i in 0..n_ent {
        let r = &b[o_ent + i * 0x68..o_ent + (i + 1) * 0x68];
        entrances.push(Entrance { name: text_at(r, 0, 32), pos: pt(r, 0x20), orientation: u16_at(r, 0x24), raw: b64(r) });
    }

    let (o_anim, n_anim) = (u32_at(b, 0xb0) as usize, u32_at(b, 0xac) as usize);
    let anims = (0..n_anim)
        .map(|i| {
            let r = &b[o_anim + i * 0x4c..o_anim + (i + 1) * 0x4c];
            Anim { name: text_at(r, 0, 32), pos: pt(r, 0x20), bam: rr(r, 0x28) }
        })
        .collect();

    let n_items = u16_at(b, 0x76) as u32;
    let n_amb = u16_at(b, 0x82) as u32;
    let keep = Keep {
        spawns: (u32_at(b, 0x64), b64(slice(b, u32_at(b, 0x60), u32_at(b, 0x64) as usize * 0xc8))),
        items: (n_items, b64(slice(b, u32_at(b, 0x78), n_items as usize * 0x14))),
        ambients: (n_amb, b64(slice(b, u32_at(b, 0x84), n_amb as usize * 0xd4))),
        variables: (u32_at(b, 0x8c), b64(slice(b, u32_at(b, 0x88), u32_at(b, 0x8c) as usize * 0x54))),
        explored: (u32_at(b, 0x9c), b64(slice(b, u32_at(b, 0xa0), u32_at(b, 0x9c) as usize))),
        anims: (n_anim as u32, b64(slice(b, o_anim as u32, n_anim * 0x4c))),
        notes: (u32_at(b, 0xc8), b64(slice(b, u32_at(b, 0xc4), u32_at(b, 0xc8) as usize * 0x34))),
        tiled: (u32_at(b, 0xb4), b64(slice(b, u32_at(b, 0xb8), u32_at(b, 0xb4) as usize * 0x6c))),
        traps: (u32_at(b, 0xd0), b64(slice(b, u32_at(b, 0xcc), u32_at(b, 0xd0) as usize * 0x1c))),
        songs: b64(slice(b, u32_at(b, 0xbc), 0x90)),
        rest: b64(slice(b, u32_at(b, 0xc0), 0xe4)),
    };
    // tiled objects point into a flags table (0x90 offset / 0x92 count words) we keep inside the header
    Ok(Are {
        wed: rr(b, 0x08),
        flags: u32_at(b, 0x14),
        area_type: u16_at(b, 0x48),
        script: rr(b, 0x94),
        actors,
        regions,
        doors,
        containers,
        entrances,
        anims,
        header: b64(&b[..0x11c]),
        keep,
    })
}

/// text fields are latin-1; an unchanged value keeps its original bytes (case, and whatever follows the NUL)
fn put_text(r: &mut [u8], o: usize, len: usize, s: &str) {
    if text_at(r, o, len).eq_ignore_ascii_case(s) {
        return;
    }
    r[o..o + len].fill(0);
    for (i, c) in s.chars().take(len).enumerate() {
        r[o + i] = if (c as u32) < 256 { c as u8 } else { b'?' };
    }
}
fn put16(r: &mut [u8], o: usize, v: u16) {
    r[o..o + 2].copy_from_slice(&v.to_le_bytes())
}
fn put32(r: &mut [u8], o: usize, v: u32) {
    r[o..o + 4].copy_from_slice(&v.to_le_bytes())
}
fn put_pt(r: &mut [u8], o: usize, p: Pt) {
    put16(r, o, p[0] as u16);
    put16(r, o + 2, p[1] as u16);
}
/// bounding box (left, top, right, bottom) at `o`: the stored one stays while the polygon still fits inside it
/// (the original files' boxes are often a little loose, and the engine only uses them to filter hits)
fn put_bbox(r: &mut [u8], o: usize, pts: &[Pt]) {
    let old = [i16_at(r, o), i16_at(r, o + 2), i16_at(r, o + 4), i16_at(r, o + 6)];
    let fits = old != [0; 4] && pts.iter().all(|p| p[0] >= old[0] && p[0] <= old[2] && p[1] >= old[1] && p[1] <= old[3]);
    if !fits {
        for (k, v) in bbox(pts).iter().enumerate() {
            put16(r, o + k * 2, *v as u16);
        }
    }
}

fn bbox(pts: &[Pt]) -> [i16; 4] {
    if pts.is_empty() {
        return [0; 4];
    }
    let mut v = [i16::MAX, i16::MAX, i16::MIN, i16::MIN];
    for p in pts {
        v[0] = v[0].min(p[0]);
        v[1] = v[1].min(p[1]);
        v[2] = v[2].max(p[0]);
        v[3] = v[3].max(p[1]);
    }
    v
}

/// rebuild the file: header, then every section in a fixed order, vertices rebuilt from the polygons
pub fn write(a: &Are) -> Vec<u8> {
    let mut h = unb64(&a.header, 0x11c);
    h[0..8].copy_from_slice(b"AREAV1.0");
    put_text(&mut h, 0x08, 8, &a.wed);
    put32(&mut h, 0x14, a.flags);
    put16(&mut h, 0x48, a.area_type);
    put_text(&mut h, 0x94, 8, &a.script);

    let mut verts: Vec<Pt> = Vec::new();
    // an empty list keeps whatever index it had (the originals often hold a stale one)
    let mut add = |rec: &[u8], at: usize, pts: &[Pt]| -> u32 {
        if pts.is_empty() {
            return u32_at(rec, at);
        }
        let first = verts.len() as u32;
        verts.extend_from_slice(pts);
        first
    };

    let mut regions = Vec::new();
    for r in &a.regions {
        let mut b = unb64(&r.raw, 0xc4);
        put_text(&mut b, 0, 32, &r.name);
        put16(&mut b, 0x20, r.kind);
        put_bbox(&mut b, 0x22, &r.pts);
        put16(&mut b, 0x2a, r.pts.len() as u16);
        let i = add(&b, 0x2c, &r.pts);
        put32(&mut b, 0x2c, i);
        put32(&mut b, 0x30, r.trigger_value);
        put32(&mut b, 0x34, r.cursor);
        put_text(&mut b, 0x38, 8, &r.dest_area);
        put_text(&mut b, 0x40, 32, &r.entrance);
        put32(&mut b, 0x60, r.flags);
        put32(&mut b, 0x64, r.info);
        put16(&mut b, 0x68, r.trap_detect);
        put16(&mut b, 0x6a, r.trap_remove);
        put16(&mut b, 0x6c, r.trapped);
        put16(&mut b, 0x6e, r.detected);
        put_pt(&mut b, 0x70, r.launch);
        put_text(&mut b, 0x74, 8, &r.key);
        put_text(&mut b, 0x7c, 8, &r.script);
        put_pt(&mut b, 0x84, r.use_point);
        put_pt(&mut b, 0xb4, r.talk_point);
        put32(&mut b, 0xb8, r.speaker);
        put_text(&mut b, 0xbc, 8, &r.dialog);
        regions.push(b);
    }
    let mut containers = Vec::new();
    for c in &a.containers {
        let mut b = unb64(&c.raw, 0xc0);
        put_text(&mut b, 0, 32, &c.name);
        put_pt(&mut b, 0x20, c.pos);
        put16(&mut b, 0x24, c.kind);
        put_bbox(&mut b, 0x38, &c.pts);
        let i = add(&b, 0x50, &c.pts);
        put32(&mut b, 0x50, i);
        put16(&mut b, 0x54, c.pts.len() as u16);
        containers.push(b);
    }
    let mut doors = Vec::new();
    for d in &a.doors {
        let mut b = unb64(&d.raw, 0xc8);
        put_text(&mut b, 0, 32, &d.name);
        put_text(&mut b, 0x20, 8, &d.wed_id);
        put32(&mut b, 0x28, d.flags);
        let i = add(&b, 0x2c, &d.open);
        put32(&mut b, 0x2c, i);
        put16(&mut b, 0x30, d.open.len() as u16);
        put16(&mut b, 0x32, d.shut.len() as u16);
        let i = add(&b, 0x34, &d.shut);
        put32(&mut b, 0x34, i);
        put_bbox(&mut b, 0x38, &d.open);
        put_bbox(&mut b, 0x40, &d.shut);
        let i = add(&b, 0x48, &d.open_cells);
        put32(&mut b, 0x48, i);
        put16(&mut b, 0x4c, d.open_cells.len() as u16);
        put16(&mut b, 0x4e, d.shut_cells.len() as u16);
        let i = add(&b, 0x50, &d.shut_cells);
        put32(&mut b, 0x50, i);
        put32(&mut b, 0x68, d.cursor);
        put_text(&mut b, 0x78, 8, &d.key);
        put_text(&mut b, 0x80, 8, &d.script);
        put_text(&mut b, 0xb8, 8, &d.dialog);
        doors.push(b);
    }
    let mut actors = Vec::new();
    for ac in &a.actors {
        let mut b = unb64(&ac.raw, 0x110);
        put_text(&mut b, 0, 32, &ac.name);
        put_pt(&mut b, 0x20, ac.pos);
        put_pt(&mut b, 0x24, ac.dest);
        put32(&mut b, 0x28, ac.flags);
        put32(&mut b, 0x30, ac.animation);
        put16(&mut b, 0x34, ac.orientation);
        put32(&mut b, 0x40, ac.schedule);
        put_text(&mut b, 0x48, 8, &ac.dialog);
        for (k, s) in ac.scripts.iter().enumerate() {
            put_text(&mut b, 0x50 + k * 8, 8, s);
        }
        put_text(&mut b, 0x80, 8, &ac.cre);
        // the CRE resref's first letter is cached at 0x2e
        if !rr(&b, 0x80).eq(&rr(&unb64(&ac.raw, 0x110), 0x80)) || ac.raw.is_empty() {
            b[0x2e] = ac.cre.bytes().next().unwrap_or(0);
        }
        actors.push(b);
    }
    let mut entrances = Vec::new();
    for e in &a.entrances {
        let mut b = unb64(&e.raw, 0x68);
        put_text(&mut b, 0, 32, &e.name);
        put_pt(&mut b, 0x20, e.pos);
        put16(&mut b, 0x24, e.orientation);
        entrances.push(b);
    }

    let mut out = h;
    let place = |out: &mut Vec<u8>, blob: &[u8]| -> u32 {
        let at = out.len() as u32;
        out.extend_from_slice(blob);
        at
    };
    let o_act = place(&mut out, &actors.concat());
    let o_reg = place(&mut out, &regions.concat());
    let k = &a.keep;
    let o_spawn = place(&mut out, &B64.decode(&k.spawns.1).unwrap_or_default());
    let o_ent = place(&mut out, &entrances.concat());
    let o_con = place(&mut out, &containers.concat());
    let o_items = place(&mut out, &B64.decode(&k.items.1).unwrap_or_default());
    let vbytes: Vec<u8> = verts.iter().flat_map(|p| [p[0].to_le_bytes(), p[1].to_le_bytes()].concat()).collect();
    let o_vert = place(&mut out, &vbytes);
    let o_amb = place(&mut out, &B64.decode(&k.ambients.1).unwrap_or_default());
    let o_var = place(&mut out, &B64.decode(&k.variables.1).unwrap_or_default());
    let o_expl = place(&mut out, &B64.decode(&k.explored.1).unwrap_or_default());
    let o_door = place(&mut out, &doors.concat());
    let o_anim = place(&mut out, &B64.decode(&k.anims.1).unwrap_or_default());
    let o_tiled = place(&mut out, &B64.decode(&k.tiled.1).unwrap_or_default());
    let o_song = place(&mut out, &B64.decode(&k.songs).unwrap_or_default());
    let o_rest = place(&mut out, &B64.decode(&k.rest).unwrap_or_default());
    let o_notes = place(&mut out, &B64.decode(&k.notes.1).unwrap_or_default());
    let o_traps = place(&mut out, &B64.decode(&k.traps.1).unwrap_or_default());
    // embedded CREs go last; their actors point at them
    for (i, ac) in a.actors.iter().enumerate() {
        let rec = o_act as usize + i * 0x110;
        match ac.embedded.as_ref().and_then(|s| B64.decode(s).ok()) {
            Some(cre) if !cre.is_empty() => {
                let at = place(&mut out, &cre);
                put32(&mut out, rec + 0x88, at);
                put32(&mut out, rec + 0x8c, cre.len() as u32);
            }
            // no CRE inside: the record keeps its fields (some originals hold an offset with size 0)
            _ => {}
        }
    }
    let h = &mut out;
    put32(h, 0x54, o_act);
    put16(h, 0x58, a.actors.len() as u16);
    put16(h, 0x5a, a.regions.len() as u16);
    put32(h, 0x5c, o_reg);
    put32(h, 0x60, o_spawn);
    put32(h, 0x64, k.spawns.0);
    put32(h, 0x68, o_ent);
    put32(h, 0x6c, a.entrances.len() as u32);
    put32(h, 0x70, o_con);
    put16(h, 0x74, a.containers.len() as u16);
    put16(h, 0x76, k.items.0 as u16);
    put32(h, 0x78, o_items);
    put32(h, 0x7c, o_vert);
    put16(h, 0x80, verts_len(&vbytes));
    put16(h, 0x82, k.ambients.0 as u16);
    put32(h, 0x84, o_amb);
    put32(h, 0x88, o_var);
    put32(h, 0x8c, k.variables.0);
    put32(h, 0x9c, k.explored.0);
    put32(h, 0xa0, o_expl);
    put32(h, 0xa4, a.doors.len() as u32);
    put32(h, 0xa8, o_door);
    put32(h, 0xac, k.anims.0);
    put32(h, 0xb0, o_anim);
    put32(h, 0xb4, k.tiled.0);
    put32(h, 0xb8, o_tiled);
    put32(h, 0xbc, o_song);
    put32(h, 0xc0, o_rest);
    put32(h, 0xc4, o_notes);
    put32(h, 0xc8, k.notes.0);
    put32(h, 0xcc, o_traps);
    put32(h, 0xd0, k.traps.0);
    out
}

fn verts_len(v: &[u8]) -> u16 {
    (v.len() / 4) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_area_roundtrip() {
        let mut h = vec![0u8; 0x11c];
        h[0..8].copy_from_slice(b"AREAV1.0");
        let a = Are {
            wed: "AR9999".into(),
            header: b64(&h),
            regions: vec![Region {
                name: "Tran1".into(),
                kind: 2,
                pts: vec![[10, 10], [100, 10], [100, 80]],
                dest_area: "AR0100".into(),
                entrance: "Exit1".into(),
                ..Default::default()
            }],
            actors: vec![Actor { name: "Bob".into(), pos: [300, 400], orientation: 7, cre: "BOB".into(), ..Default::default() }],
            doors: vec![Door { name: "D1".into(), wed_id: "DOOR01".into(), open: vec![[1, 1], [2, 2], [3, 1]], ..Default::default() }],
            ..Default::default()
        };
        let back = parse(&write(&a)).unwrap();
        assert_eq!(back.regions[0].pts, a.regions[0].pts);
        assert_eq!(back.regions[0].entrance, "Exit1");
        assert_eq!(back.actors[0].pos, [300, 400]);
        assert_eq!(back.actors[0].orientation, 7);
        assert_eq!(back.doors[0].open, a.doors[0].open);
        // writing what we read gives the same bytes again
        let once = write(&back);
        assert_eq!(write(&parse(&once).unwrap()), once);
    }
}
