# MapSmithEE

Tauri 2 (Rust) + Svelte 5/TS desktop app: Infinity Engine area viewer/editor + the Camp 3D map editor.
Clean-room take on the feature list of Bubb's "Infinity Areas": never copy its code.

- Backend `src-tauri/src/`: `res.rs` (KEY/BIFF/override/TLK, read only), `tis.rs` (TIS + PVRZ DXT1/5), `wed.rs`,
  `are.rs`, `bmpmap.rs` (SR/HT/LM/LN), `render.rs` (area picture + door patches), `camp3d.rs` (3D editor doc store),
  `lib.rs` (commands, settings, project folder, install guard). Pictures are served at `http://msimg.localhost/<key>`.
- Writers keep every byte they don't edit: ARE records carry their original bytes (`raw`), untouched sections are copied.
  `cargo test --test real_game -- --ignored` with `MAPSMITH_GAME=<game dir>` must give 0 problems (BGEE, BG2EE, EET:
  1540 areas on 2026-11-02).
- Door tiles: the WED tile map's primary tiles show doors OPEN, the secondary tiles CLOSED.
- Frontend `src/`: `lib/editor.svelte.ts` (state, undo/redo), `lib/MapView.svelte` (canvas, tools), `lib/Props.svelte`.
- `static/camp3d/index.html` is the 3D editor, owned here (`tools/sync_camp3d.mjs` only refreshes the model library;
  `--force` would overwrite our edits). `shim.js` maps its `claude.use('db')` to `camp3d_*` commands.
- The game folder is never written. Saves go to the project folder; Install copies into `<test copy>/override` and
  refuses `settings.blocked` folders (default: "E:/Enhanced Edition/Environnement de test", "steamapps/common").
- UI test: `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9335 npm run tauri dev`, then
  `node tests/ui/cdp.mjs shot|click|drag|key|eval ...`. Private test game: `E:/Dev/MapSmithEE-scratch/testgame-bgee`.
- Credits: `python tools/gen_credits.py` rewrites CREDITS.md; README thanks everyone (the goal is to share the tool).
- Code, comments, UI in English. Replies to John in French.
- KNOWN BUG (2026-11-02): BGEE 2.7.3 crashed loading a save with MapSmithEE-written AR2600.WED/AR2600N.WED/AR2600SR.BMP in
  override (files kept in E:/Dev/MapSmithEE-scratch/held). Bisect before trusting area saves in game.
