<script lang="ts">
  import { api } from './api';
  import { ed, REGION_KINDS, SR_NAMES, WALL_FLAGS } from './editor.svelte';
  import type { Actor, Container, Door, Entrance, Poly, Region } from './types';

  let { onCenter }: { onCenter: (p: [number, number]) => void } = $props();

  const sel = $derived(ed.sel);
  const obj = $derived.by(() => { void ed.ver; return sel ? ed.objectAt(sel.kind, sel.i) : null; });

  /** one undo step per field change */
  function set<T extends object>(o: T, k: keyof T, v: unknown, label = 'Edit') {
    ed.change(label, () => { (o as Record<string, unknown>)[k as string] = v; });
  }
  const val = (e: Event) => (e.target as HTMLInputElement).value;
  const num = (e: Event) => { const s = val(e).trim(); return s.toLowerCase().startsWith('0x') ? parseInt(s, 16) >>> 0 : Number(s) >>> 0; };
  const res = (e: Event) => val(e).trim().toUpperCase().slice(0, 8);
  const hex = (n: number) => '0x' + (n >>> 0).toString(16).toUpperCase();

  let infoText = $state('');
  $effect(() => {
    const o = obj as Region | null;
    if (sel?.kind === 'region' && o && o.kind === 1 && o.info !== 0xffffffff) api.tlk(o.info).then((t) => (infoText = t)).catch(() => (infoText = ''));
    else infoText = '';
  });

  function toggleBit(o: { flags: number }, bit: number) { set(o, 'flags', (o.flags ^ (1 << bit)) >>> 0, 'Flags'); }
  const DIRS = ['S', 'SSW', 'SW', 'WSW', 'W', 'WNW', 'NW', 'NNW', 'N', 'NNE', 'NE', 'ENE', 'E', 'ESE', 'SE', 'SSE'];
  const SCRIPT_NAMES = ['Override', 'General', 'Class', 'Race', 'Default', 'Specific'];

  import type { Kind } from './editor.svelte';
  const LISTS: { kind: Kind; label: string; items: () => { name: string }[] }[] = [
    { kind: 'region', label: 'regions', items: () => ed.doc?.are.regions ?? [] },
    { kind: 'actor', label: 'actors', items: () => ed.doc?.are.actors ?? [] },
    { kind: 'door', label: 'doors', items: () => ed.doc?.are.doors ?? [] },
    { kind: 'container', label: 'containers', items: () => ed.doc?.are.containers ?? [] },
    { kind: 'entrance', label: 'entrances', items: () => ed.doc?.are.entrances ?? [] },
  ];
  /** select an object from the lists and bring it into view */
  function pick(kind: Kind, i: number) {
    ed.select({ kind, i, verts: [] });
    const o = ed.objectAt(kind, i) as { pos?: [number, number]; pts?: [number, number][]; open?: [number, number][] };
    const pts = o.pts ?? o.open;
    if (o.pos) onCenter(o.pos);
    else if (pts?.length) onCenter([pts.reduce((a, p) => a + p[0], 0) / pts.length, pts.reduce((a, p) => a + p[1], 0) / pts.length]);
  }

  const mapInfo = $derived(ed.mapLayer ? ed.map(ed.mapLayer) : null);
  const valueName = (i: number) => ed.mapLayer === 'sr' ? `${i} ${SR_NAMES[i] ?? ''}` : ed.mapLayer === 'ht' ? `height ${i}` : `${i}`;
</script>

<aside class="props">
  {#if ed.tool === 'paint' || ed.mapLayer}
    <section>
      <h3>Map painting</h3>
      {#if mapInfo}
        <p class="dim">{ed.mapLayer?.toUpperCase()} · {mapInfo.w}×{mapInfo.h} cells · {mapInfo.bpp} bpp</p>
        <label>Brush <input type="range" min="1" max="12" bind:value={ed.brush} /> {ed.brush}</label>
        <label>Overlay <input type="range" min="0.1" max="1" step="0.05" bind:value={ed.mapAlpha} /></label>
        <p>Value: <b>{valueName(ed.paintValue)}</b> <span class="dim">(right-click the map to pick)</span></p>
        <div class="swatches" class:big={ed.mapLayer === 'sr'}>
          {#each mapInfo.palette.slice(0, ed.mapLayer === 'sr' ? 16 : 256) as c, i}
            <button
              class="sw" class:on={ed.paintValue === i}
              title={valueName(i)}
              style="background: rgb({c[0]},{c[1]},{c[2]})"
              onclick={() => (ed.paintValue = i)}
            >{ed.mapLayer === 'sr' ? i : ''}</button>
          {/each}
        </div>
      {:else}
        <p class="dim">Choose a map in Layers to paint it.</p>
      {/if}
    </section>
  {/if}

  {#if ed.multi.length}
    <section><h3>Vertices</h3><p>{ed.multi.length} selected in several walls. Drag one to move them all, Delete removes them.</p></section>
  {:else if sel && obj}
    <section>
      {#if sel.kind === 'wall'}
        {@const w = obj as Poly}
        <h3>Wall polygon #{sel.i}</h3>
        <p class="dim">{w.pts.length} vertices{sel.verts.length ? `, ${sel.verts.length} selected` : ''}</p>
        <div class="flags">
          {#each WALL_FLAGS as f, b}
            <label><input type="checkbox" checked={(w.flags & (1 << b)) !== 0} onchange={() => toggleBit(w, b)} /> {f}</label>
          {/each}
        </div>
        <label>Height <input type="number" min="0" max="255" value={w.height} onchange={(e) => set(w, 'height', Math.min(255, num(e)))} /></label>
        {#if ed.doc?.wed_night}
          <p class="dim">{ed.doc.night_linked ? 'Day and night share these walls.' : `Editing the ${ed.night ? 'night' : 'day'} WED only.`}</p>
        {/if}
      {:else if sel.kind === 'region'}
        {@const r = obj as Region}
        <h3>Region</h3>
        <label>Name <input value={r.name} maxlength="32" onchange={(e) => set(r, 'name', val(e))} /></label>
        <label>Type
          <select value={r.kind} onchange={(e) => set(r, 'kind', Number(val(e)), 'Region type')}>
            {#each REGION_KINDS as k, i}<option value={i}>{k}</option>{/each}
          </select>
        </label>
        {#if r.kind === 2}
          <label>Destination area <input value={r.dest_area} onchange={(e) => set(r, 'dest_area', res(e))} /></label>
          <label>Entrance <input value={r.entrance} maxlength="32" onchange={(e) => set(r, 'entrance', val(e))} /></label>
          <label class="check"><input type="checkbox" checked={(r.flags & 4) !== 0} onchange={() => toggleBit(r, 2)} /> Whole party required</label>
        {/if}
        {#if r.kind === 1}
          <label>Text (strref) <input value={r.info === 0xffffffff ? -1 : r.info} onchange={(e) => set(r, 'info', Number(val(e)) >>> 0)} /></label>
          {#if infoText}<blockquote>{infoText}</blockquote>{/if}
        {/if}
        {#if r.kind === 0}
          <label class="check"><input type="checkbox" checked={r.trapped !== 0} onchange={() => set(r, 'trapped', r.trapped ? 0 : 1)} /> Trapped</label>
          <label class="check"><input type="checkbox" checked={(r.flags & 8) !== 0} onchange={() => toggleBit(r, 3)} /> Detectable</label>
          <div class="row">
            <label>Detect <input type="number" value={r.trap_detect} onchange={(e) => set(r, 'trap_detect', num(e))} /></label>
            <label>Remove <input type="number" value={r.trap_remove} onchange={(e) => set(r, 'trap_remove', num(e))} /></label>
          </div>
          <div class="kv">Launch point <span class="pt">{r.launch[0]}, {r.launch[1]}</span></div>
        {/if}
        <label>Script <input value={r.script} onchange={(e) => set(r, 'script', res(e))} /></label>
        <label>Key item <input value={r.key} onchange={(e) => set(r, 'key', res(e))} /></label>
        <label>Dialog <input value={r.dialog} onchange={(e) => set(r, 'dialog', res(e))} /></label>
        <div class="row">
          <label>Cursor <input type="number" value={r.cursor} onchange={(e) => set(r, 'cursor', num(e))} /></label>
          <label>Flags <input value={hex(r.flags)} onchange={(e) => set(r, 'flags', num(e))} /></label>
        </div>
        <p class="dim">{r.pts.length} vertices</p>
      {:else if sel.kind === 'actor'}
        {@const a = obj as Actor}
        <h3>Actor</h3>
        <label>Name <input value={a.name} maxlength="32" onchange={(e) => set(a, 'name', val(e))} /></label>
        <label>Creature (CRE) <input value={a.cre} onchange={(e) => set(a, 'cre', res(e))} /></label>
        <div class="kv">Position <span class="pt">{a.pos[0]}, {a.pos[1]}</span></div>
        <div class="compass">
          {#each DIRS as d, i}
            <button class:on={a.orientation === i} title={d} style="--a: {i * 22.5}deg" onclick={() => set(a, 'orientation', i, 'Turn actor')}></button>
          {/each}
          <span>{DIRS[a.orientation] ?? a.orientation}</span>
        </div>
        <p class="dim">Drag the dot at the arrow's tip, or [ and ], to turn.</p>
        <label>Dialog <input value={a.dialog} onchange={(e) => set(a, 'dialog', res(e))} /></label>
        {#each SCRIPT_NAMES as n, k}
          <label>{n} script <input value={a.scripts[k]} onchange={(e) => { const v = res(e); ed.change('Script', () => { a.scripts[k] = v; }); }} /></label>
        {/each}
        <div class="row">
          <label>Schedule <input value={hex(a.schedule)} onchange={(e) => set(a, 'schedule', num(e))} /></label>
          <label>Flags <input value={hex(a.flags)} onchange={(e) => set(a, 'flags', num(e))} /></label>
        </div>
        {#if a.embedded}<p class="dim">Carries its own CRE data.</p>{/if}
      {:else if sel.kind === 'door'}
        {@const d = obj as Door}
        <h3>Door</h3>
        <label>Name <input value={d.name} maxlength="32" onchange={(e) => set(d, 'name', val(e))} /></label>
        <div class="kv">WED door <span class="pt">{d.wed_id}</span></div>
        <button class="wide" onclick={() => (ed.closed = { ...ed.closed, [d.wed_id]: !ed.closed[d.wed_id] })}>
          Show {ed.closed[d.wed_id] ? 'open' : 'closed'}
        </button>
        <label class="check"><input type="checkbox" checked={(d.flags & 1) !== 0} onchange={() => toggleBit(d, 0)} /> Open when the game starts</label>
        <label class="check"><input type="checkbox" checked={(d.flags & 2) !== 0} onchange={() => toggleBit(d, 1)} /> Locked</label>
        <label>Key item <input value={d.key} onchange={(e) => set(d, 'key', res(e))} /></label>
        <label>Script <input value={d.script} onchange={(e) => set(d, 'script', res(e))} /></label>
        <label>Dialog <input value={d.dialog} onchange={(e) => set(d, 'dialog', res(e))} /></label>
        <label>Flags <input value={hex(d.flags)} onchange={(e) => set(d, 'flags', num(e))} /></label>
      {:else if sel.kind === 'container'}
        {@const c = obj as Container}
        <h3>Container</h3>
        <label>Name <input value={c.name} maxlength="32" onchange={(e) => set(c, 'name', val(e))} /></label>
        <p class="dim">{c.pts.length} vertices · items are kept as they are</p>
      {:else if sel.kind === 'entrance'}
        {@const n = obj as Entrance}
        <h3>Entrance</h3>
        <label>Name <input value={n.name} maxlength="32" onchange={(e) => set(n, 'name', val(e))} /></label>
        <div class="kv">Position <span class="pt">{n.pos[0]}, {n.pos[1]}</span></div>
        <label>Facing <input type="number" min="0" max="15" value={n.orientation} onchange={(e) => set(n, 'orientation', num(e) % 16)} /></label>
      {:else if sel.kind === 'anim'}
        {@const a = obj as { name: string; bam: string }}
        <h3>Animation</h3><p>{a.name} · {a.bam}</p><p class="dim">Read only.</p>
      {/if}
      {#if 'pos' in (obj as object)}
        <button class="link" onclick={() => onCenter((obj as { pos: [number, number] }).pos)}>Center view</button>
      {/if}
    </section>
  {:else if ed.doc}
    <section>
      <h3>{ed.doc.name}</h3>
      <p class="dim">Read from the {ed.doc.source === 'project' ? 'project folder' : 'game'}.</p>
      <p class="dim">{ed.wed?.walls.length} walls</p>
      {#each LISTS as l}
        {@const items = l.items()}
        <details>
          <summary>{items.length} {l.label}</summary>
          <ul class="objs">
            {#each items as it, i}
              <li><button onclick={() => pick(l.kind, i)}>{it.name || '(no name)'}</button></li>
            {/each}
          </ul>
        </details>
      {/each}
      <p class="dim">Click a shape to edit it. Click again on the same spot to reach the one underneath. Drag on empty ground to box-select wall vertices.</p>
    </section>
  {/if}
</aside>

<style>
  .props { width: 290px; flex: none; overflow-y: auto; overflow-x: hidden; box-sizing: border-box; background: var(--panel); border-left: 1px solid var(--line); padding: 10px 12px; }
  section { margin-bottom: 16px; }
  h3 { margin: 0 0 8px; font-size: 14px; }
  label { display: flex; flex-direction: column; gap: 2px; font-size: 12px; color: var(--dim); margin-bottom: 8px; }
  label.check { flex-direction: row; align-items: center; gap: 6px; color: var(--fg); }
  .flags label { flex-direction: row; gap: 6px; color: var(--fg); margin-bottom: 3px; }
  label input:not([type='checkbox']):not([type='range']), label select { width: 100%; box-sizing: border-box; }
  .row { display: flex; gap: 8px; } .row label { flex: 1; min-width: 0; }
  .kv { font-size: 12px; color: var(--dim); margin-bottom: 8px; }
  .pt { color: var(--fg); font-family: ui-monospace, monospace; }
  .dim { color: var(--dim); font-size: 12px; }
  blockquote { margin: 0 0 8px; padding: 6px 8px; background: var(--bg); border-left: 3px solid #ffd34f; font-size: 12px; max-height: 140px; overflow: auto; white-space: pre-wrap; }
  details { margin-bottom: 4px; } summary { cursor: pointer; font-size: 13px; padding: 2px 0; }
  .objs { list-style: none; margin: 2px 0 6px; padding: 0 0 0 12px; max-height: 220px; overflow-y: auto; }
  .objs button { border: none; background: none; padding: 1px 4px; text-align: left; width: 100%; font-size: 12px; }
  .objs button:hover { color: var(--accent); }
  .wide { width: 100%; margin-bottom: 8px; }
  .link { background: none; border: none; color: var(--accent); padding: 0; cursor: pointer; }
  .swatches { display: grid; grid-template-columns: repeat(16, 1fr); gap: 2px; }
  .swatches.big { grid-template-columns: repeat(4, 1fr); }
  .sw { aspect-ratio: 1; min-width: 0; padding: 0; border: 1px solid #0006; border-radius: 3px; font-size: 11px; color: #fff; text-shadow: 0 0 2px #000; }
  .swatches.big .sw { aspect-ratio: 2; }
  .sw.on { outline: 2px solid var(--accent); outline-offset: 1px; }
  .compass { position: relative; width: 120px; height: 120px; margin: 4px auto 6px; border-radius: 50%; background: var(--bg); }
  .compass button { position: absolute; left: 52px; top: 52px; width: 16px; height: 16px; padding: 0; border-radius: 50%;
    transform: rotate(var(--a)) translateY(46px); border: 1px solid var(--line); background: var(--panel); }
  .compass button.on { background: var(--accent); }
  .compass span { position: absolute; inset: 0; display: grid; place-items: center; font-size: 12px; }
</style>
