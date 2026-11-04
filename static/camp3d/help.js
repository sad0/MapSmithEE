/* Help menu ("?" in the top bar), the log, the update notice and the first-launch checks.
 * - every window error, unhandled rejection and warning in the status line goes to the app's log
 *   (<app data>/logs/mapsmithee.log, src-tauri/src/support.rs)
 * - Report a bug copies a report (version, Blender, model library, last log lines) and opens the GitHub issues
 * - once a day, a newer release on GitHub shows a notice with its link (nothing is installed by itself)
 * - at start, a missing Blender or too little disk space for the model library shows a notice, until it is fixed or
 *   dismissed */
(() => {
  const invoke = window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke;
  if (!invoke) return;
  const $ = id => document.getElementById(id);
  const REPO = 'https://github.com/sad0/MapSmithEE';
  const esc = s => String(s).replace(/[&<>"]/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' })[c]);
  const ls = { get: k => { try { return localStorage.getItem(k); } catch { return null; } }, set: (k, v) => { try { localStorage.setItem(k, v); } catch {} } };

  // ---------- log ----------
  const log = (level, msg) => invoke('app_log', { level, msg: String(msg) }).catch(() => {});
  addEventListener('error', e => log('error', `${e.message} (${(e.filename || '').split('/').pop()}:${e.lineno})`));
  addEventListener('unhandledrejection', e => log('error', 'unhandled: ' + ((e.reason && (e.reason.stack || e.reason.message)) || e.reason)));
  // the status line's warnings (failed saves, renders...) are what a bug report needs most
  const st = $('status');
  if (st) new MutationObserver(() => { if (st.classList.contains('warn') && st.textContent !== 'Local only') log('warn', st.textContent); }).observe(st, { attributes: true, childList: true, characterData: true, subtree: true });

  // ---------- styles ----------
  const css = document.createElement('style');
  css.textContent = `
  #hpNotes{position:fixed;left:50%;top:52px;transform:translateX(-50%);z-index:40;width:min(640px,calc(100vw - 32px));display:flex;flex-direction:column;gap:6px}
  .hpNote{background:var(--panel);border:1px solid var(--accent);border-radius:6px;padding:8px 10px;box-shadow:0 6px 20px #0008;display:flex;gap:10px;align-items:flex-start}
  .hpNote .tx{flex:1}.hpNote a{color:var(--accent);cursor:pointer}
  #hpKeys{position:fixed;inset:0;z-index:50;background:#0009;display:none;align-items:center;justify-content:center}
  #hpKeys>div{background:var(--panel);border:1px solid var(--line);border-radius:8px;padding:14px 18px;max-width:min(520px,calc(100vw - 32px));max-height:calc(100vh - 32px);overflow:auto}
  #hpKeys table{border-collapse:collapse}#hpKeys td{padding:2px 10px 2px 0;vertical-align:top}#hpKeys td:first-child{white-space:nowrap;color:var(--accent)}`;
  document.head.appendChild(css);

  // ---------- notices, stacked under the top bar (one per kind; a dismissed key is remembered) ----------
  function note(html, key, kind) {
    if (key && ls.get('help.dismiss.' + key)) return;
    let box = $('hpNotes'); if (!box) { box = document.createElement('div'); box.id = 'hpNotes'; document.body.appendChild(box); }
    const old = kind && box.querySelector(`[data-kind="${kind}"]`); if (old) old.remove();
    const n = document.createElement('div'); n.className = 'hpNote'; if (kind) n.dataset.kind = kind;
    n.innerHTML = `<div class="tx">${html}</div><button title="Close">✕</button>`;
    n.lastChild.onclick = () => { n.remove(); if (key) ls.set('help.dismiss.' + key, '1'); };
    box.appendChild(n);
    return n;
  }
  const openUrl = url => invoke('open_url', { url }).catch(e => window.toast && toast(String(e)));

  // ---------- keyboard shortcuts ----------
  const KEYS = [
    ['Click a piece, click the ground', 'Place it (Shift: keep placing)'], ['Q', 'Select tool'], ['W / E / R', 'Move / rotate / scale'],
    ['G', 'Put the selection back on the ground'], ['Ctrl+D', 'Duplicate'], ['Del', 'Delete'], ['Ctrl+Z / Ctrl+Y', 'Undo / redo'],
    ['Shift+click or box', 'Add to the selection'], ['Ctrl+click', 'Remove from the selection'], ['I', 'Paint: pick the ground layer under the cursor'], ['[ / ]', 'Paint, tree brush: smaller / bigger brush'],
    ['Esc', 'Deselect, stop placing, cancel the pipette'], ['F1', 'This list'], ['Walls: Enter / Esc', 'Close the drawn wall / stop drawing'],
  ];
  const keys = document.createElement('div'); keys.id = 'hpKeys';
  keys.innerHTML = `<div role="dialog" aria-modal="true"><h2 style="margin-top:0">Keyboard and mouse</h2><table>${KEYS.map(([k, v]) => `<tr><td>${esc(k)}</td><td>${esc(v)}</td></tr>`).join('')}</table>
    <div class="row" style="margin-top:10px"><button id="hpKeysX">Close</button></div></div>`;
  document.body.appendChild(keys);
  keys.onclick = e => { if (e.target === keys) keys.style.display = 'none'; };
  $('hpKeysX').onclick = () => keys.style.display = 'none';

  // ---------- update ----------
  async function checkUpdate(manual) {
    const u = await invoke('update_check').catch(() => null);
    ls.set('help.updateAt', String(Date.now()));
    if (u) note(`<b>MapSmithEE ${esc(u.version)} is out.</b> <a id="hpUpd">Download it</a> (your maps and imports stay).`, manual ? null : 'upd-' + u.version, 'upd');
    else if (manual) window.toast && toast(navigator.onLine === false ? 'No internet: could not check' : 'You have the latest version');
    const a = $('hpUpd'); if (a && u) a.onclick = () => openUrl(u.url);
  }

  // ---------- first-launch checks ----------
  const GB = b => (b / 1e9).toFixed(1) + ' GB';
  async function checkSystem() {
    const s = await invoke('system_check').catch(() => null); if (!s) return;
    const say = [];
    if (!s.blender) say.push(`<b>Blender not found.</b> The editor works without it, but rendering an area needs <a id="hpBl">Blender 5.x</a>. Installed somewhere else, or portable? Pick its blender.exe with <i>Choose…</i> in <i>Render area…</i>.`);
    if (s.need && s.free != null && s.free < s.need) say.push(`<b>Not enough disk space for the model library.</b> The first render downloads it: about ${GB(s.need)} needed, ${GB(s.free)} free on the drive of ${esc(s.library_dir)}.`);
    if (!say.length) return;
    note(say.join('<br><br>'), 'sys-' + (!s.blender ? 'bl' : '') + (s.need && s.free < s.need ? 'disk' : ''), 'sys');
    const a = $('hpBl'); if (a) a.onclick = () => invoke('open_url', { url: REPO + '/blob/main/docs/TUTORIAL.md#1-install' }).catch(() => {});
  }

  // ---------- menu ----------
  async function report() {
    const text = await invoke('app_report').catch(e => 'Could not build the report: ' + e);
    let copied = false; try { await navigator.clipboard.writeText(text); copied = true; } catch {}
    note(`${copied ? '<b>Bug report copied.</b> Paste it in a new issue, with what you did and what happened (a screenshot helps).' : '<b>Copy this report</b> into a new issue:'}
      <br><a id="hpIss">Open the GitHub issues</a> · <a id="hpLogs">Show the log folder</a>${copied ? '' : `<textarea readonly style="width:100%;height:140px;margin-top:6px">${esc(text)}</textarea>`}`, null, 'rep');
    $('hpIss').onclick = () => openUrl(REPO + '/issues/new');
    $('hpLogs').onclick = () => invoke('open_logs').catch(() => {});
  }
  function addMenu() {
    const h = document.querySelector('header'); if (!h || $('hpBtn')) return;
    const g = document.createElement('div'); g.className = 'grp';
    g.innerHTML = `<button id="hpBtn" title="Help, shortcuts, bug report, updates">?</button><div id="hpMenu" class="menu" hidden>
      <button data-u="${REPO}/blob/main/docs/TUTORIAL.md">Tutorial</button><button id="hpK">Keyboard and mouse</button><hr>
      <button id="hpRep">Report a bug…</button><button id="hpLog">Show the log folder</button><button data-u="https://discord.gg/3XzeQv7HeS">Discord</button><hr>
      <button id="hpUp">Check for updates</button><button data-u="${REPO}/releases">Releases</button><div class="hint" id="hpVer" style="padding:2px 8px"></div></div>`;
    const status = $('status'); if (status) h.insertBefore(g, status); else h.appendChild(g);
    $('hpBtn').onclick = e => { e.stopPropagation(); $('hpMenu').hidden = !$('hpMenu').hidden; };
    addEventListener('click', () => { $('hpMenu').hidden = true; });
    for (const b of g.querySelectorAll('[data-u]')) b.onclick = () => openUrl(b.dataset.u);
    $('hpK').onclick = () => keys.style.display = 'flex';
    $('hpRep').onclick = report; $('hpLog').onclick = () => invoke('open_logs').catch(() => {});
    $('hpUp').onclick = () => checkUpdate(true);
    const app = window.__TAURI__.app; if (app && app.getVersion) app.getVersion().then(v => { $('hpVer').textContent = 'MapSmithEE ' + v; }).catch(() => {});
  }
  addEventListener('keydown', e => { if (e.key === 'F1') { e.preventDefault(); keys.style.display = keys.style.display === 'flex' ? 'none' : 'flex'; } });

  function start() {
    addMenu(); checkSystem();
    if (Date.now() - (+ls.get('help.updateAt') || 0) > 864e5) setTimeout(() => checkUpdate(false), 4000);
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', start); else start();
})();
