# MapSmithEE

A desktop tool to view and edit Infinity Engine areas (Baldur's Gate EE, Siege of Dragonspear, Baldur's Gate II EE,
EET), plus a 3D map editor that builds new playable areas.

> Work in progress (v0.1). Windows first.

## Features

**Area editor**
- Open any area of a game install: ARE, WED and TIS (palette or PVRZ tilesets), day and night versions.
- Wall polygons (WED): draw, move/add/delete vertices, split a segment, box-select, edit the polygon flags.
- Regions (ARE): travel, trap and info points with all their fields.
- Actors: place them, set their facing.
- Doors: toggle open/closed to see both states.
- Area bitmaps: search map (SR), height map (HT), light maps (LM / LN): view and paint.
- Undo/redo, unsaved-changes prompt, click on empty space to deselect, cycle through overlapping shapes.
- The game folder stays read-only: edits are saved to a project folder, and a button copies them into the
  `override` folder of a test copy of the game.

**Camp 3D editor**
- Lay out a map with 3D models (props, trees, tents, ground layers), day or night.
- Rendered in Blender at the Baldur's Gate camera angle, then built into a playable area (ARE/TIS/PVRZ/WED and maps).
- Ships with an example map, **Camp**, and its playable area.

## Thanks

- **Bubb**, for "Infinity Areas" and its feature list, which inspired this tool
  (https://www.gibberlings3.net/forums/topic/38550-feedback-wanted-infinity-areas-tool-–-pre-alpha/).
  MapSmithEE is an independent implementation; it contains none of its code.
- Everyone who gave feedback in that thread: undo/redo, unsaved-changes prompt, overlapping regions, door toggle and more
  come from your requests.
- **IESDP** contributors, for documenting the Infinity Engine file formats (https://gibberlings3.github.io/iesdp/).
- **The Gibberlings Three** and **Beamdog forums** communities.
- **BioWare** and **Beamdog**, for the games and their art.
- **Acifer**, for the Baldur's Gate ground textures.
- **Gwendolyne**, for sharing the BG1 tree templates.
- **Poly Haven** and its artists, and every **Sketchfab / Objaverse** author whose model is in the library:
  each one is listed by name in [CREDITS.md](CREDITS.md).
- Open-source projects: **Tauri**, **Svelte**, **three.js**, **Blender**, **Rust** and their crates.

## Licences

- MapSmithEE code: MIT.
- 3D models and textures: their own licences, listed in [CREDITS.md](CREDITS.md) (CC0 and CC BY 4.0).
- Baldur's Gate art (BG1 tree cut-outs, Acifer textures, game animations): non-commercial fan use for the Infinity
  Engine games only.

## Build

Needs Rust (stable) and Node.js 20+.

```
npm install
npm run tauri dev      # run
npm run tauri build    # Windows installer
```
