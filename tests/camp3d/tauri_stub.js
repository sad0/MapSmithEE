// @ts-nocheck
// Test-only fake of window.__TAURI__.core.invoke for the camp3d_* commands (see src-tauri/src/camp3d.rs).
// In-memory store seeded from /__test/seed. Returned values are deep-frozen to catch edits of read-only data.
// Every call is logged in window.__camp3dCalls ({cmd, path, data}).
(function () {
  const store = new Map();
  const seeded = fetch('/__test/seed').then(r => r.json()).then(d => { for (const k in d) store.set(k, JSON.stringify(d[k])); });
  const freeze = v => { if (v && typeof v === 'object') { Object.values(v).forEach(freeze); Object.freeze(v); } return v; };
  const ok = p => /^[A-Za-z0-9_\-][A-Za-z0-9_.\-]*\/[A-Za-z0-9_\-][A-Za-z0-9_.\-]*$/.test(p);
  window.__camp3dCalls = [];
  async function invoke(cmd, args) {
    await seeded;
    const { path, data } = args || {};
    window.__camp3dCalls.push({ cmd, path, data });
    if (!ok(path)) throw `invalid_argument: bad document path '${path}'`;
    switch (cmd) {
      case 'camp3d_get': return store.has(path) ? freeze(JSON.parse(store.get(path))) : null;
      case 'camp3d_set': if (!data || typeof data !== 'object' || Array.isArray(data)) throw 'invalid_argument: not an object'; store.set(path, JSON.stringify(data)); return null;
      case 'camp3d_update': {
        if (!store.has(path)) throw `not_found: no document '${path}'`;
        const cur = JSON.parse(store.get(path));
        for (const k in data) { const v = data[k]; if (v && v.__delete__ === true && Object.keys(v).length === 1) delete cur[k]; else cur[k] = v; }
        store.set(path, JSON.stringify(cur)); return null;
      }
      case 'camp3d_delete': store.delete(path); return null;
      default: throw `unknown command ${cmd}`;
    }
  }
  window.__camp3dStore = store;
  window.__TAURI__ = { core: { invoke } };
})();
