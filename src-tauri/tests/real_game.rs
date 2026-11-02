//! Checks against a real game install (read only). Run with:
//! MAPSMITH_GAME="<game dir>" [MAPSMITH_OUT=<dir for PNGs>] cargo test --release --test real_game -- --ignored --nocapture

use mapsmithee_lib::{are, render, res::Game, wed};
use base64::Engine as _;
use std::path::PathBuf;

/// a record's bytes without its bounding boxes: the writer fixes boxes that don't hold their polygon
fn nobox(raw: &str, boxes: &[usize]) -> Vec<u8> {
    let mut b = base64::engine::general_purpose::STANDARD.decode(raw).unwrap();
    for &o in boxes {
        b[o..o + 8].fill(0);
    }
    b
}

fn game() -> Option<Game> {
    let root = std::env::var("MAPSMITH_GAME").ok()?;
    Some(Game::open(&PathBuf::from(root)).expect("open game"))
}

#[test]
#[ignore]
fn every_area_roundtrips() {
    let Some(g) = game() else { return };
    let (mut n, mut fails) = (0, Vec::new());
    for name in g.names("ARE") {
        let Ok(Some(b)) = g.get(&name, "ARE") else { continue };
        let a = match are::parse(&b) {
            Ok(a) => a,
            Err(e) => {
                fails.push(format!("{name}.ARE parse: {e}"));
                continue;
            }
        };
        // rewritten file must decode to the same records and be stable
        let once = are::write(&a);
        let back = are::parse(&once).unwrap();
        let same = back.actors.len() == a.actors.len()
            && back.regions.iter().zip(&a.regions).all(|(x, y)| x.pts == y.pts && nobox(&x.raw, &[0x22]) == nobox(&y.raw, &[0x22]))
            && back.doors.iter().zip(&a.doors).all(|(x, y)| x.open == y.open && x.shut_cells == y.shut_cells && nobox(&x.raw, &[0x38, 0x40]) == nobox(&y.raw, &[0x38, 0x40]))
            && back.containers.iter().zip(&a.containers).all(|(x, y)| x.pts == y.pts && nobox(&x.raw, &[0x38]) == nobox(&y.raw, &[0x38]))
            && back.actors.iter().zip(&a.actors).all(|(x, y)| x.raw == y.raw && x.embedded == y.embedded)
            && back.keep.items == a.keep.items
            && back.keep.variables == a.keep.variables;
        if !same {
            fails.push(format!("{name}.ARE changed on rewrite"));
        }
        if are::write(&back) != once {
            fails.push(format!("{name}.ARE unstable"));
        }
        let wname = if a.wed.is_empty() { name.clone() } else { a.wed.clone() };
        for wn in [wname.clone(), format!("{wname}N")] {
            let Ok(Some(wb)) = g.get(&wn, "WED") else { continue };
            match wed::parse(&wb) {
                Ok(w) => {
                    let w2 = wed::parse(&wed::write(&w)).unwrap();
                    if w2.walls != w.walls || w2.doors != w.doors || w2.overlays[0].lookup != w.overlays[0].lookup {
                        fails.push(format!("{wn}.WED changed on rewrite"));
                    }
                }
                Err(e) => fails.push(format!("{wn}.WED parse: {e}")),
            }
        }
        n += 1;
    }
    println!("{n} areas checked, {} problems", fails.len());
    for f in fails.iter().take(40) {
        println!("  {f}");
    }
    assert!(fails.is_empty());
}

#[test]
#[ignore]
fn render_some_areas() {
    let Some(g) = game() else { return };
    let out = PathBuf::from(std::env::var("MAPSMITH_OUT").unwrap_or_else(|_| ".".into()));
    let list = std::env::var("MAPSMITH_AREAS").unwrap_or_else(|_| "AR2600".into());
    for name in list.split(',') {
        let a = are::parse(&g.get(name, "ARE").unwrap().unwrap()).unwrap();
        for wn in [a.wed.clone(), format!("{}N", a.wed)] {
            let Ok(Some(wb)) = g.get(&wn, "WED") else { continue };
            let w = wed::parse(&wb).unwrap();
            let t = std::time::Instant::now();
            let p = render::draw(&g, &w, &wn).unwrap();
            println!("{wn}: {}x{} {} doors, {:?}", p.w, p.h, p.doors.len(), t.elapsed());
            std::fs::write(out.join(format!("{wn}.png")), &p.png).unwrap();
            for (d, png) in &p.doors {
                std::fs::write(out.join(format!("{wn}_{}.png", d.door)), png).unwrap();
            }
        }
    }
}
