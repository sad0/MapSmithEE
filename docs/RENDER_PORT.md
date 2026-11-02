# Camp 3D Render in MapSmithEE (plan, 2026-11-03)

John's choices: the whole pipeline in Rust (users install only Blender), the result is a **generic named area**
(e.g. `AR9900`), with an optional "SadoQoL camp (SQCAMP)" mode that reproduces today's build; the full-quality models
(2.2 GB) are split archives in a GitHub release, downloaded at the first Render.

Reference implementation (Python, read only): `E:/AI/bg3d/tools/` — `render_scene.py` (prep + Blender call),
`canvas.py` (tent/rug cloth textures), `groundadj.py` (ground layer colour), `render_map.py` + `norm.py`, `tents.py`,
`logseat.py`, `bgsprites.py` (run inside Blender), `build_camp3d.py` + `pvrz.py` (the area). Data layout:
`E:/AI/bg3d/docs/PIPELINE.md`.

## Pieces

1. **campbuild** (Rust, `src-tauri/src/campbuild/`): `build_camp3d.py` + `pvrz.py` in Rust.
   Inputs: map doc, walk doc, day render (+ `_alpha`, `_id` passes), optional night render, model dims, options
   `{ name: "AR9900", mode: Generic | SadoQolCamp { template_dir } }`. Outputs: `<NAME>.TIS` + PVRZ pages (DXT1),
   `<NAME>.WED` (walls from the id/alpha passes: per piece mask, close 5x5, outer contours, Douglas-Peucker 2 px, area
   >= 80), night `<NAME>N.TIS/WED` with the same walls, `SR/HT/LM/LN.BMP` (palettes built in, no template), `<NAME>.ARE`
   (generic: entrance from the ENTRY spot moved out of exits, exits as travel regions, flame animations; SadoQoL mode adds
   the priest/merchant actors and `M_SQ0CMP.lua` + vision circle), `preview.jpg`. Reuse `are.rs`, `wed.rs`, `bmpmap.rs`.
   PVRZ naming rule: TIS `AR9900` -> pages `A990000..`; a generic name must be 6 characters so pages stay 8.
2. **camprender** (Rust, `src-tauri/src/camprender/`): `render_scene.py` + `canvas.py` + `groundadj.py` in Rust
   (cloth PNGs, adjusted ground textures), then `blender -b -P render_map.py -- ...` with paths from settings (Blender
   exe, model library folder). The Blender scripts ship in `src-tauri/resources/blender/` with every `E:/AI/bg3d` path
   replaced by arguments/env. Progress parsed from Blender's stdout (samples / tiles) -> events to the page.
3. **assets**: model library = `E:/AI/bg3d` layout (`models.json`, `assets/`, `mesh/`, `proxies/`, `tex/`, `ext/`).
   Release archives `mapsmithee-models-<ver>.partN.zip` (< 2 GB each) + `manifest.json` (sha256); download + unpack
   into `<app data>/library`, resumable.
4. **UI**: a Render dialog in the Camp 3D page (area name, day/night/both, quality, output), progress bar, then
   "Open in MapSmithEE" / "Install into a test copy".

## Checks

- Golden test: build `claude_camp` from the same renders with Python and Rust; compare search map cells, wall count
  and vertices (within 1 px), TIS/PVRZ decode (MapSmithEE's own reader) PSNR > 40 dB against the render.
- The generic area loads in a private game copy (EET 2.7.3 copy), walk, exits, night switch.
