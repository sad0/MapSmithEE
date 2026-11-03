/* MapSmithEE: "Import…" (your own 3D models, 2D sprites and ground textures into <app data>/userlib) and
 * "Export scene…" (the scene Blender renders, as .blend, .glb or .fbx) for the Camp 3D editor.
 * Uses the editor's globals (M, GROUNDS, CATEGORIES, fillTray, save, saveGround, mapId, toast) and Tauri.
 */
(function () {
  'use strict';
  const T = window.__TAURI__;
  if (!T) return;
  const invoke = T.core.invoke;
  const listen = T.event.listen;
  const $ = id => document.getElementById(id);
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c]));

  const MODEL_EXTS = ['glb', 'gltf', 'obj', 'fbx', 'stl', 'ply', 'dae', 'usd', 'usda', 'usdc', 'usdz', 'abc', 'x3d', 'blend'];
  const KINDS = {
    model: { name: '3D model', filters: [{ name: '3D models', extensions: MODEL_EXTS }],
      note: 'Any format Blender reads (glTF, OBJ, FBX, STL, DAE, USD…). Blender converts it, fits it on the ground and makes a light copy for the editor.' },
    sprite: { name: '2D sprite', filters: [{ name: 'Pictures and BAM', extensions: ['png', 'bam'] }],
      note: 'A transparent PNG drawn at BG\'s angle and pixel size, or a BAM frame. It stands like BG\'s own trees, facing the camera. A <i>name</i>_shadow.png next to the PNG becomes its ground shadow.' },
    texture: { name: 'Ground texture', filters: [{ name: 'Pictures', extensions: ['png', 'jpg', 'jpeg'] }],
      note: 'A seamless picture: it is tiled over the ground (2048 px at most). The editor reloads to add it to the ground list.' },
  };

  const css = `
  #ulBack,#exBack{position:fixed;inset:0;background:#000a;display:none;align-items:center;justify-content:center;z-index:100}
  #ulBox,#exBox{width:min(460px,94vw);background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:16px 18px;box-shadow:0 10px 40px #000c}
  #ulBox h2,#exBox h2{font:600 16px "Cinzel",Georgia,serif;color:var(--accent);margin:0 0 10px}
  #ulBox label,#exBox label{display:flex;justify-content:space-between;align-items:center;gap:10px;margin:8px 0;color:var(--mute)}
  #ulBox input[type=text],#ulBox input[type=number]{width:200px}
  #ulBox .note,#exBox .note{font-size:12px;color:var(--mute);margin:6px 0}
  #ulFile,#exFile{font-size:12px;color:var(--fg);word-break:break-all}
  #ulStep,#exStep{font-size:12px;color:var(--mute);min-height:16px;margin-top:8px}
  #exBar{height:8px;background:#0006;border-radius:4px;overflow:hidden;margin:10px 0 4px;display:none}
  #exBar > div{height:100%;width:0;background:var(--accent);transition:width .3s}
  .ulBtns{display:flex;gap:8px;justify-content:flex-end;margin-top:12px}
  .tile.user{outline:1px dashed var(--accent);outline-offset:-3px}
  `;
  const st = document.createElement('style'); st.textContent = css; document.head.appendChild(st);

  // ---------- Import ----------
  const labels = CATEGORIES.map(c => `<optgroup label="${esc(c.name)}">${c.labels.map(l => `<option>${esc(l)}</option>`).join('')}</optgroup>`).join('');
  const box = document.createElement('div'); box.id = 'ulBack';
  box.innerHTML = `<div id="ulBox" role="dialog" aria-modal="true" aria-labelledby="ulTitle">
    <h2 id="ulTitle">Import</h2>
    <label>What <select id="ulKind">${Object.entries(KINDS).map(([k, v]) => `<option value="${k}">${v.name}</option>`).join('')}</select></label>
    <div class="note" id="ulNote"></div>
    <label>File <button id="ulPick">Choose…</button></label>
    <div id="ulFile"></div>
    <label>Name <input type="text" id="ulName" spellcheck="false"></label>
    <label id="ulTypeRow">Type <select id="ulType">${labels}</select></label>
    <label id="ulHRow" title="Leave empty to keep the model's own size">Height (m) <input type="number" id="ulH" min="0" step="0.1" placeholder="its own size"></label>
    <label id="ulBamRow">BAM cycle / frame <span><input type="number" id="ulCyc" min="0" value="0" style="width:60px"> <input type="number" id="ulFr" min="0" value="0" style="width:60px"></span></label>
    <div id="ulStep"></div>
    <div class="ulBtns"><button id="ulClose">Close</button><button id="ulGo" class="on">Import</button></div>
  </div>`;
  document.body.appendChild(box);
  let file = '', busy = false;

  function refresh() {
    const k = $('ulKind').value, bam = /\.bam$/i.test(file);
    $('ulNote').innerHTML = KINDS[k].note;
    $('ulTypeRow').style.display = k === 'texture' ? 'none' : '';
    $('ulHRow').style.display = k === 'model' ? '' : 'none';
    $('ulBamRow').style.display = k === 'sprite' && bam ? '' : 'none';
    $('ulFile').textContent = file || 'No file chosen';
    $('ulGo').disabled = busy || !file;
  }
  $('ulKind').onchange = () => { file = ''; refresh(); };
  function open(kind) {
    if (busy) { box.style.display = 'flex'; return; }
    if (kind) $('ulKind').value = kind;
    file = ''; $('ulName').value = ''; $('ulStep').textContent = ''; refresh(); box.style.display = 'flex';
  }
  const close = () => { if (!busy) box.style.display = 'none'; };
  $('ulClose').onclick = close;
  box.addEventListener('keydown', e => { if (e.key === 'Escape') { e.stopPropagation(); close(); } });
  $('ulPick').onclick = async () => {
    const p = await invoke('plugin:dialog|open', { options: { multiple: false, directory: false, filters: KINDS[$('ulKind').value].filters } }).catch(e => { $('ulStep').textContent = String(e); return null; });
    if (!p) return;
    file = typeof p === 'string' ? p : p.path || String(p);
    if (!$('ulName').value) $('ulName').value = file.split(/[\\/]/).pop().replace(/\.[^.]+$/, '').replace(/[_-]+/g, ' ');
    refresh();
  };
  $('ulGo').onclick = async () => {
    const kind = $('ulKind').value;
    busy = true; refresh();
    $('ulStep').textContent = kind === 'model' ? 'Blender is converting the model (up to a minute)…' : 'Importing…';
    try {
      const h = parseFloat($('ulH').value);
      const r = await invoke('userlib_import', { req: { kind, path: file, name: $('ulName').value.trim(), label: $('ulType').value,
        height_m: kind === 'model' && h > 0 ? h : null, cycle: +$('ulCyc').value || 0, frame: +$('ulFr').value || 0 } });
      if (kind === 'texture') {
        // a new ground layer: the built-in layers and the masks are laid out at start, so save and start again
        $('ulStep').textContent = 'Texture imported. Reloading the editor…';
        await save().catch(() => {}); await saveGround().catch(() => {});
        location.reload(); return;
      }
      M[r.id] = { ...r.entry, user: 1 };
      fillTray();
      $('ulStep').innerHTML = `Imported <b>${esc(r.entry.name)}</b> (${r.entry.dims.map(v => v.toFixed(2)).join(' × ')} m). Pick it in the tray (dashed frame); right-click it there to delete it.`;
      file = '';
    } catch (e) {
      $('ulStep').textContent = 'Import failed: ' + e;
    } finally { busy = false; refresh(); }
  };

  // a user piece out of the library (pieces of it already placed stay in the map but no longer load)
  window.userlibDelete = async id => {
    const m = M[id]; if (!m) return;
    const used = (S.items || []).filter(it => it.m === id).length;
    if (!confirm(`Delete "${m.name || id}" from your imports?${used ? `\n${used} placed on this map will disappear from it.` : ''}`)) return;
    try {
      await invoke('userlib_delete', { id });
      delete M[id]; fillTray(); toast(`${m.name || id} deleted`);
    } catch (e) { toast('Delete failed: ' + e); }
  };

  // ---------- Export scene ----------
  const ex = document.createElement('div'); ex.id = 'exBack';
  ex.innerHTML = `<div id="exBox" role="dialog" aria-modal="true" aria-labelledby="exTitle">
    <h2 id="exTitle">Export scene</h2>
    <div class="note">The scene Blender renders for this map (BG camera, the hour's sun and fires, painted ground, every piece), to open in another 3D program.</div>
    <label>Format <select id="exFmt"><option value="blend">Blender (.blend): everything, ready to render</option><option value="glb">glTF (.glb): most 3D programs and engines</option><option value="fbx">FBX (.fbx): Autodesk tools</option></select></label>
    <label>Light <select id="exHour"><option value="day">Day</option><option value="night">Night</option></select></label>
    <div class="note">.glb and .fbx bake the painted ground into one picture; pieces keep their textures but not their own colour.</div>
    <div id="exBar"><div></div></div><div id="exStep"></div>
    <div class="ulBtns"><button id="exClose">Close</button><button id="exGo" class="on">Export…</button></div>
  </div>`;
  document.body.appendChild(ex);
  let exBusy = false;
  const exBar = (p, t) => { $('exBar').style.display = 'block'; $('exBar').firstChild.style.width = Math.max(0, Math.min(100, p)) + '%'; $('exStep').textContent = t || ''; };
  $('exClose').onclick = async () => {
    if (exBusy) { await invoke('camp3d_render_cancel').catch(() => {}); $('exStep').textContent = 'Stopping…'; return; }
    ex.style.display = 'none';
  };
  ex.addEventListener('keydown', e => { if (e.key === 'Escape' && !exBusy) { e.stopPropagation(); ex.style.display = 'none'; } });
  $('exGo').onclick = async () => {
    const fmt = $('exFmt').value;
    const path = await invoke('plugin:dialog|save', { options: { defaultPath: `${mapId}.${fmt}`, filters: [{ name: fmt.toUpperCase(), extensions: [fmt] }] } }).catch(() => null);
    if (!path) return;
    exBusy = true; $('exGo').disabled = true; $('exClose').textContent = 'Stop'; exBar(0, 'Saving the map…');
    const un = await listen('render-progress', e => exBar(e.payload.pct, e.payload.step));
    try {
      await save(); await saveGround();
      const out = await invoke('camp3d_export_scene', { req: { map_id: mapId, format: fmt, path, night: $('exHour').value === 'night' } });
      exBar(100, `Done: ${out}`);
    } catch (e) {
      $('exStep').textContent = String(e).includes('cancelled') ? 'Stopped.' : 'Export failed: ' + e;
    } finally { un(); exBusy = false; $('exGo').disabled = false; $('exClose').textContent = 'Close'; }
  };

  // ---------- buttons: tray ("Import…"), ground panel ("Import texture…"), header ("Export scene…") ----------
  function addButtons() {
    const cats = $('cats');
    if (cats && !$('ulOpen')) {
      const b = document.createElement('button'); b.id = 'ulOpen'; b.textContent = 'Import…';
      b.title = 'Add your own 3D model or 2D sprite to the tray'; b.style.width = '100%'; b.style.margin = '4px 0';
      b.onclick = () => open('model'); cats.after(b);
    }
    const sw = $('swatches');
    if (sw && !$('ulTex')) {
      const b = document.createElement('button'); b.id = 'ulTex'; b.textContent = 'Import texture…';
      b.title = 'Add your own seamless ground texture'; b.style.marginTop = '4px';
      b.onclick = () => open('texture'); sw.after(b);
    }
    const ra = $('renderArea');
    if ($('exOpen') && ra && $('exOpen').previousElementSibling !== ra) ra.after($('exOpen'));
    if (!$('exOpen')) {
      const b = document.createElement('button'); b.id = 'exOpen'; b.textContent = 'Export scene…';
      b.title = 'Save this map\'s scene for Blender or another 3D program (.blend, .glb, .fbx)';
      b.onclick = () => { if (!exBusy) { $('exStep').textContent = ''; $('exBar').style.display = 'none'; } ex.style.display = 'flex'; };
      if (ra) ra.after(b); else { const h = document.querySelector('header'); if (h) h.appendChild(b); }
    }
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', addButtons); else addButtons();
  // render.js puts "Render area…" in the header on DOMContentLoaded too: place ours after it once it is there
  setTimeout(addButtons, 0); setTimeout(addButtons, 1000);
})();
