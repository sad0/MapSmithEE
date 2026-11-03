/* MapSmithEE: "Render area" for the Camp 3D editor. Saves the map, renders it in Blender (day, optional night) and
 * builds a playable area (ARE/TIS/PVRZ/WED and maps) into the project folder. The model library and Blender are
 * checked first; the library downloads from the GitHub release when missing.
 * Uses the editor's globals (save, saveGround, mapId, maps) and Tauri through window.__TAURI__.
 */
(function () {
  'use strict';
  const T = window.__TAURI__;
  if (!T) return;
  const invoke = T.core.invoke;
  const listen = T.event.listen;

  const css = `
  #rdBack{position:fixed;inset:0;background:#000a;display:none;align-items:center;justify-content:center;z-index:100}
  #rdBox{width:min(440px,94vw);background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:16px 18px;box-shadow:0 10px 40px #000c}
  #rdBox h2{font:600 16px "Cinzel",Georgia,serif;color:var(--accent);margin:0 0 10px}
  #rdBox label{display:flex;justify-content:space-between;align-items:center;gap:10px;margin:8px 0;color:var(--mute)}
  #rdBox input[type=text]{width:120px;text-transform:uppercase;font-family:ui-monospace,monospace}
  #rdBox .chk{justify-content:flex-start;color:var(--fg)}
  #rdBox .note{font-size:12px;color:var(--mute);margin:6px 0}
  #rdBox .bad{color:var(--warn)}
  #rdBar{height:8px;background:#0006;border-radius:4px;overflow:hidden;margin:10px 0 4px;display:none}
  #rdBar > div{height:100%;width:0;background:var(--accent);transition:width .3s}
  #rdStep{font-size:12px;color:var(--mute);min-height:16px}
  #rdBtns{display:flex;gap:8px;justify-content:flex-end;margin-top:12px}
  #rdBox button:disabled{opacity:.4;cursor:default}
  `;
  const st = document.createElement('style'); st.textContent = css; document.head.appendChild(st);

  const box = document.createElement('div'); box.id = 'rdBack';
  box.innerHTML = `<div id="rdBox" role="dialog" aria-modal="true" aria-labelledby="rdTitle">
    <h2 id="rdTitle">Render area</h2>
    <label>Area name <input type="text" id="rdName" maxlength="6" spellcheck="false" placeholder="AR9900"></label>
    <label class="chk"><input type="checkbox" id="rdNight" checked> Night version too</label>
    <label>Quality <select id="rdQ"><option value="16">Draft (fast)</option><option value="64" selected>Good</option><option value="128">Final</option></select></label>
    <div class="note" id="rdLib">Checking the model library…</div>
    <div class="note" id="rdBl">Looking for Blender…</div>
    <div id="rdBar"><div></div></div><div id="rdStep"></div>
    <div id="rdBtns"><button id="rdDl" hidden>Download models</button><button id="rdCancel">Close</button><button id="rdGo" class="on">Render</button></div>
  </div>`;
  document.body.appendChild(box);
  const $ = id => document.getElementById(id);

  let busy = false, libOk = false, blOk = false, lastOut = '';
  const setBar = (pct, step) => { $('rdBar').style.display = 'block'; $('rdBar').firstChild.style.width = Math.max(0, Math.min(100, pct)) + '%'; $('rdStep').textContent = step || ''; };
  const sizeGb = n => (n / 1e9).toFixed(2) + ' GB';
  const validName = s => /^[A-Z0-9_#-]{6}$/.test(s);

  function refreshGo() {
    const n = $('rdName').value.trim().toUpperCase();
    $('rdGo').disabled = busy || !libOk || !blOk || !validName(n);
    $('rdName').title = validName(n) ? '' : 'Exactly 6 characters (the game names the PVRZ pages after it)';
  }
  $('rdName').oninput = refreshGo;

  async function check() {
    try {
      const s = await invoke('library_status');
      libOk = s.ready;
      $('rdLib').textContent = s.ready ? `Model library: ${s.dir}` : 'The full-quality models (about 2.6 GB) are not downloaded yet.';
      $('rdLib').className = 'note' + (s.ready ? '' : ' bad');
      $('rdDl').hidden = s.ready;
    } catch (e) { $('rdLib').textContent = 'Model library: ' + e; }
    try {
      const b = await invoke('camprender_find_blender');
      blOk = !!b;
      $('rdBl').innerHTML = b ? `Blender: ${b}` : 'Blender 5.x is needed: install it from <a href="https://www.blender.org/download/" target="_blank" style="color:var(--accent)">blender.org</a>, then reopen this window.';
      $('rdBl').className = 'note' + (b ? '' : ' bad');
    } catch (e) { blOk = false; $('rdBl').textContent = 'Blender: ' + e; }
    refreshGo();
  }

  function open() {
    if (busy) { box.style.display = 'flex'; return; }
    let saved = ''; try { saved = localStorage.getItem('mapsmithee.render.' + mapId) || ''; } catch {}
    $('rdName').value = saved || 'AR9900';
    $('rdStep').textContent = ''; $('rdBar').style.display = 'none';
    box.style.display = 'flex'; check(); $('rdName').focus();
  }
  function close() { if (!busy) box.style.display = 'none'; }
  $('rdCancel').onclick = async () => {
    if (busy) { await invoke('camp3d_render_cancel').catch(() => {}); $('rdStep').textContent = 'Stopping…'; return; }
    close();
  };
  box.addEventListener('keydown', e => { if (e.key === 'Escape') { e.stopPropagation(); close(); } });

  $('rdDl').onclick = async () => {
    busy = true; refreshGo(); $('rdDl').disabled = true; $('rdCancel').textContent = 'Stop';
    const un = await listen('library-progress', ev => { const p = ev.payload; setBar(p.total ? p.done * 100 / p.total : 0, `${p.step} · ${sizeGb(p.done)} / ${sizeGb(p.total)}`); });
    try { await invoke('library_install'); setBar(100, 'Models ready'); }
    catch (e) { $('rdStep').textContent = 'Download stopped: ' + e + ' (run it again to resume)'; }
    finally { un(); busy = false; $('rdDl').disabled = false; $('rdCancel').textContent = 'Close'; check(); }
  };

  $('rdGo').onclick = async () => {
    const name = $('rdName').value.trim().toUpperCase();
    if (!validName(name)) return;
    try { localStorage.setItem('mapsmithee.render.' + mapId, name); } catch {}
    busy = true; refreshGo(); $('rdCancel').textContent = 'Stop';
    setBar(0, 'Saving the map…');
    const un = await listen('render-progress', ev => setBar(ev.payload.pct, ev.payload.step));
    try {
      await save(); await saveGround();
      const r = await invoke('camp3d_render_area', { req: { map_id: mapId, name, night: $('rdNight').checked, samples: +$('rdQ').value } });
      lastOut = r.out_dir;
      setBar(100, `Done: ${r.files.length} files in ${r.out_dir}`);
      $('rdGo').textContent = 'Render again';
    } catch (e) {
      $('rdStep').textContent = String(e).includes('cancelled') ? 'Stopped.' : 'Render failed: ' + e;
    } finally { un(); busy = false; $('rdCancel').textContent = 'Close'; refreshGo(); }
  };

  // the editor's "Preview render" asked a Claude session; in MapSmithEE the header gets "Render area…" instead
  function addButton() {
    const pv = document.getElementById('preview');
    const b = document.createElement('button'); b.id = 'renderArea'; b.textContent = 'Render area…';
    b.title = 'Render this map in Blender and build a playable area';
    b.onclick = open;
    if (pv) { pv.replaceWith(b); } else { const h = document.querySelector('header'); if (h) h.appendChild(b); }
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', addButton); else addButton();
})();
