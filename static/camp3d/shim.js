/* MapSmithEE shim for the Camp Map 3D editor (a claude.ai artifact page run locally).
 *
 * The editor talks to `claude.use('db')` (document store) and `claude.use('user')` (permissions). Here the store is
 * backed by Tauri commands (window.__TAURI__.core.invoke, app.withGlobalTauri): one JSON file per document under
 * <app data>/camp3d/<collection>/<doc>.json.
 *
 *   db.doc('maps/claude_camp') -> { id, path, get(), set(data), update(data), delete() }
 *   get() -> { exists, id, data() }   data() returns a fresh copy each call
 *   update(data): shallow merge; a value { __delete__: true } (or db.deleteField()) removes that key
 *
 * Writes are cheap: per document, writes run one at a time and a write queued behind a running one is replaced by
 * the newest data (the editor autosaves the whole map often, only the last state matters). get() waits for that
 * document's pending writes, so it always reads what was last written.
 */
(function () {
  'use strict';
  const invoke = (cmd, args) => {
    const t = window.__TAURI__;
    if (!t || !t.core || !t.core.invoke) return Promise.reject(Object.assign(new Error('Tauri is not available'), { code: 'unavailable' }));
    return t.core.invoke(cmd, args).catch(e => {
      const msg = typeof e === 'string' ? e : (e && e.message) || String(e);
      const code = /^([a-z_]+):/.exec(msg);
      throw Object.assign(new Error(msg), { code: code ? code[1] : 'internal' });
    });
  };
  const copy = v => (v === undefined ? undefined : JSON.parse(JSON.stringify(v)));

  // per-path write chain: { tail: Promise of the last scheduled write, waiting: queued set() not started yet }
  const queues = new Map();
  function write(path, kind, data) {
    let q = queues.get(path);
    if (!q) { q = { tail: Promise.resolve(), waiting: null }; queues.set(path, q); }
    if (kind === 'set' && q.waiting) { q.waiting.data = data; return q.waiting.promise; } // coalesce: newest wins
    const job = { kind, data };
    job.promise = q.tail.then(() => {
      if (q.waiting === job) q.waiting = null;
      return job.kind === 'delete' ? invoke('camp3d_delete', { path })
        : invoke(job.kind === 'set' ? 'camp3d_set' : 'camp3d_update', { path, data: job.data });
    }).then(() => undefined);
    q.tail = job.promise.catch(() => {});
    q.waiting = kind === 'set' ? job : null; // an update/delete is never jumped over by a later set
    return job.promise;
  }
  const settled = path => (queues.get(path) || { tail: Promise.resolve() }).tail;

  const DELETE = Object.freeze({ __delete__: true });
  function doc(path) {
    path = String(path);
    const id = path.split('/').pop();
    return {
      id, path,
      async get() {
        await settled(path);
        const v = await invoke('camp3d_get', { path });
        const exists = v !== null && v !== undefined;
        return { id, exists, data: () => (exists ? copy(v) : undefined) };
      },
      set(data) { return write(path, 'set', copy(data)); },
      update(data) { return write(path, 'update', copy(data)); },
      delete() { return write(path, 'delete'); },
    };
  }
  const db = { doc, collection: name => ({ doc: id => doc(name + '/' + id) }), deleteField: () => DELETE };
  const user = { can: async () => true };

  window.claude = Object.assign(window.claude || {}, {
    async use(name) {
      if (name === 'db') {
        if (!window.__TAURI__) throw Object.assign(new Error('Tauri is not available'), { code: 'unavailable' });
        return db;
      }
      if (name === 'user') return user;
      throw new Error('claude.use: unknown capability ' + name);
    },
  });

  // "← Areas": back to MapSmithEE's main page. The area editor is not released yet (John, 2026-11-02): the app opens on
  // this page and the link only shows for development, with localStorage 'mapsmithee.areas' = '1'. Sits at the start of the editor's header bar (no overlap with its
  // controls); a small fixed corner link if the header is not there.
  function addBackLink() {
    let areas = false; try { areas = localStorage.getItem('mapsmithee.areas') === '1'; } catch {}
    if (!areas || document.getElementById('msBack')) return;
    const a = document.createElement('a');
    a.id = 'msBack'; a.href = '/'; a.textContent = '← Areas'; a.title = 'Back to MapSmithEE areas';
    a.style.cssText = 'color:var(--mute,#a8987d);text-decoration:none;font-size:12px;padding:2px 6px;border:1px solid var(--line,#3a3127);border-radius:4px;white-space:nowrap';
    a.onmouseenter = () => { a.style.color = 'var(--accent,#d9a441)'; };
    a.onmouseleave = () => { a.style.color = 'var(--mute,#a8987d)'; };
    const header = document.querySelector('header');
    if (header) header.insertBefore(a, header.firstChild);
    else { a.style.cssText += ';position:fixed;left:6px;top:6px;z-index:50;background:#201b15'; document.body.appendChild(a); }
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', addBackLink); else addBackLink();
})();
