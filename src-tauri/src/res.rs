//! Read-only access to a game install: chitin.key + BIFF V1 (files and tilesets) + override, and dialog.tlk.

use memmap2::Mmap;
use std::collections::{BTreeSet, HashMap};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub type Res<T> = Result<T, String>;

pub fn ext_type(ext: &str) -> Option<u16> {
    Some(match ext {
        "BMP" => 0x001,
        "BAM" => 0x3e8,
        "WED" => 0x3e9,
        "TIS" => 0x3eb,
        "CRE" => 0x3f1,
        "ARE" => 0x3f2,
        "2DA" => 0x3f4,
        "PVRZ" => 0x404,
        _ => return None,
    })
}

pub fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
pub fn i16_at(b: &[u8], o: usize) -> i16 {
    u16_at(b, o) as i16
}
pub fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}
pub fn i32_at(b: &[u8], o: usize) -> i32 {
    u32_at(b, o) as i32
}
/// 8-byte resource reference or fixed text field, NUL-trimmed, upper-cased for resrefs by the caller
pub fn text_at(b: &[u8], o: usize, len: usize) -> String {
    let s = &b[o..o + len];
    let end = s.iter().position(|&c| c == 0).unwrap_or(len);
    s[..end].iter().map(|&c| c as char).collect()
}

pub struct Game {
    pub root: PathBuf,
    bifs: Vec<String>,
    res: HashMap<(String, u16), u32>,
    override_files: HashMap<String, PathBuf>,
    open_bifs: Mutex<HashMap<usize, std::sync::Arc<Mmap>>>,
    tlk: Option<Mmap>,
}

impl Game {
    pub fn open(root: &Path) -> Res<Game> {
        let key = std::fs::read(root.join("chitin.key")).map_err(|e| format!("chitin.key: {e}"))?;
        if &key[0..8] != b"KEY V1  " {
            return Err("chitin.key: not a KEY V1 file".into());
        }
        let (nb, nr, ob, or) = (
            u32_at(&key, 8) as usize,
            u32_at(&key, 12) as usize,
            u32_at(&key, 16) as usize,
            u32_at(&key, 20) as usize,
        );
        let mut bifs = Vec::with_capacity(nb);
        for i in 0..nb {
            let e = ob + i * 12;
            let (no, nl) = (u32_at(&key, e + 4) as usize, u16_at(&key, e + 8) as usize);
            bifs.push(text_at(&key, no, nl).replace('\\', "/"));
        }
        let mut res = HashMap::with_capacity(nr);
        for i in 0..nr {
            let e = or + i * 14;
            res.insert((text_at(&key, e, 8).to_ascii_uppercase(), u16_at(&key, e + 8)), u32_at(&key, e + 10));
        }
        let mut override_files = HashMap::new();
        if let Ok(rd) = std::fs::read_dir(root.join("override")) {
            for f in rd.flatten() {
                override_files.insert(f.file_name().to_string_lossy().to_ascii_uppercase(), f.path());
            }
        }
        let tlk = Self::find_tlk(root).and_then(|p| File::open(p).ok()).and_then(|f| unsafe { Mmap::map(&f) }.ok());
        Ok(Game { root: root.to_path_buf(), bifs, res, override_files, open_bifs: Mutex::new(HashMap::new()), tlk })
    }

    fn find_tlk(root: &Path) -> Option<PathBuf> {
        let lang = root.join("lang");
        let en = lang.join("en_US").join("dialog.tlk");
        if en.exists() {
            return Some(en);
        }
        std::fs::read_dir(&lang).ok()?.flatten().map(|d| d.path().join("dialog.tlk")).find(|p| p.exists())
    }

    /// names of every resource of a type, override included
    pub fn names(&self, ext: &str) -> Vec<String> {
        let t = ext_type(ext).unwrap_or(0);
        let suffix = format!(".{ext}");
        let mut out: BTreeSet<String> = self.res.keys().filter(|(_, ty)| *ty == t).map(|(n, _)| n.clone()).collect();
        for f in self.override_files.keys() {
            if let Some(n) = f.strip_suffix(&suffix) {
                out.insert(n.to_string());
            }
        }
        out.into_iter().collect()
    }

    pub fn exists(&self, name: &str, ext: &str) -> bool {
        let name = name.to_ascii_uppercase();
        self.override_files.contains_key(&format!("{name}.{ext}"))
            || ext_type(ext).is_some_and(|t| self.res.contains_key(&(name, t)))
    }

    fn bif(&self, i: usize) -> Res<std::sync::Arc<Mmap>> {
        let mut m = self.open_bifs.lock().unwrap();
        if let Some(b) = m.get(&i) {
            return Ok(b.clone());
        }
        let rel = self.bifs.get(i).ok_or("bad BIF index")?;
        let f = File::open(self.root.join(rel)).map_err(|e| format!("{rel}: {e}"))?;
        let map = std::sync::Arc::new(unsafe { Mmap::map(&f) }.map_err(|e| e.to_string())?);
        if &map[0..8] != b"BIFFV1  " {
            return Err(format!("{rel}: compressed or unknown BIF format"));
        }
        m.insert(i, map.clone());
        Ok(map)
    }

    /// a resource's bytes, override first. A tileset comes back wrapped in a TIS V1 header.
    pub fn get(&self, name: &str, ext: &str) -> Res<Option<Vec<u8>>> {
        let name = name.to_ascii_uppercase();
        if let Some(p) = self.override_files.get(&format!("{name}.{ext}")) {
            return std::fs::read(p).map(Some).map_err(|e| e.to_string());
        }
        let Some(t) = ext_type(ext) else { return Ok(None) };
        let Some(&loc) = self.res.get(&(name, t)) else { return Ok(None) };
        let b = self.bif((loc >> 20) as usize)?;
        let (nf, nt, off) = (u32_at(&b, 8) as usize, u32_at(&b, 12) as usize, u32_at(&b, 16) as usize);
        let ti = (loc >> 14) & 0x3f;
        if ext == "TIS" && ti != 0 {
            for j in 0..nt {
                let e = off + nf * 16 + j * 20;
                if (u32_at(&b, e) >> 14) & 0x3f == ti {
                    let (o, n, sz) = (u32_at(&b, e + 4) as usize, u32_at(&b, e + 8), u32_at(&b, e + 12));
                    let mut out = Vec::with_capacity(24 + (n * sz) as usize);
                    out.extend_from_slice(b"TIS V1  ");
                    for v in [n, sz, 0x18, 64] {
                        out.extend_from_slice(&v.to_le_bytes());
                    }
                    out.extend_from_slice(&b[o..o + (n * sz) as usize]);
                    return Ok(Some(out));
                }
            }
            return Ok(None);
        }
        for j in 0..nf {
            let e = off + j * 16;
            if u32_at(&b, e) & 0x3fff == loc & 0x3fff {
                let (o, sz) = (u32_at(&b, e + 4) as usize, u32_at(&b, e + 8) as usize);
                return Ok(Some(b[o..o + sz].to_vec()));
            }
        }
        Ok(None)
    }

    pub fn string(&self, strref: u32) -> String {
        let Some(t) = &self.tlk else { return String::new() };
        if strref == u32::MAX || &t[0..4] != b"TLK " {
            return String::new();
        }
        let (n, off) = (u32_at(t, 10), u32_at(t, 14) as usize);
        if strref >= n {
            return String::new();
        }
        let e = 18 + strref as usize * 26;
        let (o, len) = (u32_at(t, e + 18) as usize, u32_at(t, e + 22) as usize);
        String::from_utf8_lossy(&t[off + o..off + o + len]).into_owned()
    }
}
