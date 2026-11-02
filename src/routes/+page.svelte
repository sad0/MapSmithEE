<script lang="ts">
  import { ask, open } from '@tauri-apps/plugin-dialog';
  import { getCurrentWindow } from '@tauri-apps/api/window';
  import { api } from '$lib/api';
  import { ed, REGION_KINDS, type MapKey, type Tool } from '$lib/editor.svelte';
  import MapView from '$lib/MapView.svelte';
  import Props from '$lib/Props.svelte';
  import type { Settings } from '$lib/types';

  let settings = $state<Settings | null>(null);
  let areaQuery = $state('');
  let busy = $state('');
  let error = $state('');
  let view: MapView;

  const areas = $derived.by(() => {
    const q = areaQuery.trim().toUpperCase();
    const all = ed.game?.areas ?? [];
    return q ? all.filter((a) => a.includes(q)) : all;
  });

  async function init() {
    settings = await api.settingsGet();
    const last = sessionStorage.getItem('mapsmithee.area');
    if (settings.recent_games[0] && sessionStorage.getItem('mapsmithee.game') === settings.recent_games[0]) {
      await openGame(settings.recent_games[0]);
      if (last) await openArea(last);
    }
  }
  init().catch((e) => (error = String(e)));

  /** ask before dropping unsaved edits */
  async function okToLeave(): Promise<boolean> {
    if (!ed.doc || !ed.dirty) return true;
    return await ask(`${ed.doc.name} has unsaved changes. Discard them?`, { title: 'Unsaved changes', kind: 'warning', okLabel: 'Discard', cancelLabel: 'Keep editing' });
  }

  async function pickGame() {
    const dir = await open({ directory: true, title: 'Game folder (with chitin.key)' });
    if (typeof dir === 'string') await openGame(dir);
  }
  async function openGame(root: string) {
    if (!(await okToLeave())) return;
    busy = 'Reading the game…'; error = '';
    try {
      ed.game = await api.gameOpen(root);
      ed.doc = null;
      sessionStorage.setItem('mapsmithee.game', root);
      settings = await api.settingsGet();
    } catch (e) { error = String(e); }
    busy = '';
  }
  async function openArea(name: string) {
    if (ed.doc?.name === name || !(await okToLeave())) return;
    busy = `Opening ${name}…`; error = '';
    try {
      const doc = await api.areaOpen(name);
      ed.night = false; ed.load(doc);
      sessionStorage.setItem('mapsmithee.area', name);
      ed.status = doc.source === 'project' ? 'Opened your saved version from the project folder' : '';
    } catch (e) { error = String(e); }
    busy = '';
  }
  async function save() {
    if (!ed.doc) return;
    busy = 'Saving…'; error = '';
    try {
      const files = await api.areaSave(ed.saveRequest());
      ed.markSaved(); ed.doc.source = 'project';
      ed.status = `Saved ${files.join(', ')} to ${settings?.project_dir}\\${ed.doc.name}`;
    } catch (e) { error = String(e); }
    busy = '';
  }
  async function install(target?: string) {
    if (!ed.doc) return;
    if (ed.dirty) await save();
    if (!target) {
      const dir = await open({ directory: true, title: 'Test copy of the game (its override folder gets the files)' });
      if (typeof dir !== 'string') return;
      target = dir;
    }
    busy = 'Installing…'; error = '';
    try {
      const files = await api.areaInstall(ed.doc.name, target);
      ed.status = `Copied ${files.length} files into ${target}\\override`;
      settings = await api.settingsGet();
    } catch (e) { error = String(e); }
    busy = '';
  }
  async function openCamp3d() {
    if (await okToLeave()) location.href = '/camp3d/index.html';
  }
  async function chooseProjectDir() {
    const dir = await open({ directory: true, title: 'Project folder (saved areas go here)' });
    if (typeof dir === 'string' && settings) { settings.project_dir = dir; await api.settingsSet($state.snapshot(settings)); }
  }

  const TOOLS: { id: Tool; label: string; key: string; tip: string }[] = [
    { id: 'select', label: 'Select', key: 'V', tip: 'Select, move, drag vertices; drag a midpoint to split a segment' },
    { id: 'wall', label: 'Wall', key: 'W', tip: 'Draw a wall polygon: click points, click the first one or Enter to close' },
    { id: 'region', label: 'Region', key: 'R', tip: 'Draw a travel, trap or info region' },
    { id: 'actor', label: 'Actor', key: 'A', tip: 'Click to place an actor' },
    { id: 'paint', label: 'Paint', key: 'B', tip: 'Paint the chosen map (SR, HT, LM, LN)' },
  ];
  const MAPS: { id: MapKey; label: string }[] = [
    { id: 'sr', label: 'Search (SR)' }, { id: 'ht', label: 'Height (HT)' }, { id: 'lm', label: 'Light (LM)' }, { id: 'ln', label: 'Night light (LN)' },
  ];
  const LAYERS = [
    ['walls', 'Walls'], ['regions', 'Regions'], ['doors', 'Doors'], ['actors', 'Actors'], ['containers', 'Containers'], ['entrances', 'Entrances'], ['anims', 'Animations'],
  ] as const;

  function onKey(e: KeyboardEvent) {
    if ((e.target as HTMLElement).closest('input, textarea, select')) return;
    const k = e.key.toLowerCase();
    if (e.ctrlKey && k === 'z' && !e.shiftKey) { e.preventDefault(); ed.undo(); }
    else if (e.ctrlKey && (k === 'y' || (k === 'z' && e.shiftKey))) { e.preventDefault(); ed.redo(); }
    else if (e.ctrlKey && k === 's') { e.preventDefault(); save(); }
    else if (!e.ctrlKey && !e.altKey) {
      const t = TOOLS.find((t) => t.key.toLowerCase() === k); if (t && ed.doc) ed.tool = t.id;
      if (k === 'n' && ed.doc?.night) ed.night = !ed.night;
    }
  }

  // closing the window with unsaved edits asks first
  $effect(() => {
    const w = getCurrentWindow();
    const un = w.onCloseRequested(async (ev) => { if (!(await okToLeave())) ev.preventDefault(); });
    return () => { un.then((f) => f()); };
  });
</script>

<svelte:window onkeydown={onKey} />

<div class="app">
  <header>
    <b class="brand">MapSmithEE</b>
    <button onclick={pickGame} title={ed.game?.root ?? ''}>{ed.game ? `${ed.game.kind} ▾` : 'Open game…'}</button>
    {#if ed.game}
      <span class="area">{ed.doc?.name ?? 'No area'}{ed.dirty ? ' •' : ''}</span>
      <button class:on={!ed.night} disabled={!ed.doc} onclick={() => (ed.night = false)}>Day</button>
      <button class:on={ed.night} disabled={!ed.doc?.night} title={ed.doc?.night ? 'N' : 'This area has no night version'} onclick={() => (ed.night = true)}>Night</button>
      <span class="sep"></span>
      <button disabled={!ed.canUndo} title="Ctrl+Z" onclick={() => ed.undo()}>Undo</button>
      <button disabled={!ed.canRedo} title="Ctrl+Y" onclick={() => ed.redo()}>Redo</button>
      <button disabled={!ed.doc || !ed.dirty} title="Ctrl+S: into the project folder" onclick={save}>Save</button>
      <button disabled={!ed.doc} onclick={() => install(settings?.install_targets[0])} title={settings?.install_targets[0] ? `Copy into ${settings.install_targets[0]}\\override` : 'Choose a test copy of the game'}>Install</button>
      {#if settings?.install_targets.length}<button disabled={!ed.doc} title="Install into another test copy" onclick={() => install()}>…</button>{/if}
    {/if}
    <span class="grow"></span>
    {#if busy}<span class="dim">{busy}</span>{/if}
    <button onclick={openCamp3d}>Camp 3D</button>
  </header>

  {#if error}<div class="error" role="alert">{error} <button onclick={() => (error = '')}>×</button></div>{/if}

  <main>
    <nav class="left">
      {#if ed.game}
        <section class="areas">
          <h4>Areas <span class="dim">{areas.length}</span></h4>
          <input placeholder="Search areas" bind:value={areaQuery} />
          <ul>
            {#each areas.slice(0, 400) as a (a)}
              <li><button class:on={ed.doc?.name === a} onclick={() => openArea(a)}>{a}</button></li>
            {/each}
          </ul>
        </section>
        {#if ed.doc}
          <section>
            <h4>Tools</h4>
            <div class="tools">
              {#each TOOLS as t}
                <button class:on={ed.tool === t.id} title="{t.tip} ({t.key})" onclick={() => (ed.tool = t.id)}>{t.label}</button>
              {/each}
            </div>
            {#if ed.tool === 'region'}
              <div class="tools">
                {#each REGION_KINDS as k, i}<button class:on={ed.regionKind === i} onclick={() => (ed.regionKind = i)}>{k}</button>{/each}
              </div>
            {/if}
          </section>
          <section>
            <h4>Layers</h4>
            {#each LAYERS as [k, label]}
              <label><input type="checkbox" bind:checked={ed.layers[k]} /> {label}</label>
            {/each}
            <h4>Map</h4>
            <label><input type="radio" name="map" checked={!ed.mapLayer} onchange={() => (ed.mapLayer = null)} /> None</label>
            {#each MAPS as m}
              <label class:dim={!ed.map(m.id)}><input type="radio" name="map" disabled={!ed.map(m.id)} checked={ed.mapLayer === m.id} onchange={() => (ed.mapLayer = m.id)} /> {m.label}</label>
            {/each}
          </section>
        {/if}
      {:else}
        <section>
          <p>Open a game folder (the one with <code>chitin.key</code>). The game is only read; your edits go to the project folder.</p>
          {#each settings?.recent_games ?? [] as r}
            <button class="recent" onclick={() => openGame(r)}>{r}</button>
          {/each}
        </section>
      {/if}
      <section class="foot">
        <button class="link" onclick={chooseProjectDir} title="Where saved areas go">Project: {settings?.project_dir ?? ''}</button>
      </section>
    </nav>

    <MapView bind:this={view} />
    <Props onCenter={(p) => view.centerOn(p)} />
  </main>

  <footer>{ed.status}</footer>
</div>

<style>
  .app { display: flex; flex-direction: column; height: 100vh; }
  header { display: flex; align-items: center; gap: 6px; padding: 6px 10px; background: var(--panel); border-bottom: 1px solid var(--line); }
  .brand { margin-right: 6px; color: var(--accent); }
  .area { font-weight: 600; min-width: 70px; text-align: center; }
  .sep { width: 10px; } .grow { flex: 1; }
  main { flex: 1; display: flex; min-height: 0; }
  .left { width: 210px; flex: none; display: flex; flex-direction: column; background: var(--panel); border-right: 1px solid var(--line); padding: 8px 10px; gap: 10px; overflow-y: auto; }
  .left label { display: flex; align-items: center; gap: 6px; padding: 1px 0; }
  h4 { margin: 4px 0 6px; font-size: 12px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--dim); }
  .areas { display: flex; flex-direction: column; min-height: 160px; max-height: 38vh; }
  .areas input { margin-bottom: 4px; }
  .areas ul { list-style: none; margin: 0; padding: 0; overflow-y: auto; flex: 1; }
  .areas li button { width: 100%; text-align: left; border: none; border-radius: 4px; padding: 2px 6px; font-family: ui-monospace, monospace; }
  .tools { display: flex; flex-wrap: wrap; gap: 4px; margin-bottom: 6px; }
  .recent { display: block; width: 100%; text-align: left; margin-bottom: 4px; font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .foot { margin-top: auto; }
  .link { background: none; border: none; color: var(--dim); padding: 0; text-align: left; font-size: 11px; word-break: break-all; }
  .dim { color: var(--dim); }
  .error { background: var(--danger); color: #fff; padding: 6px 10px; display: flex; justify-content: space-between; align-items: center; }
  .error button { background: none; color: #fff; border: none; }
  footer { padding: 3px 10px; font-size: 12px; color: var(--dim); background: var(--panel); border-top: 1px solid var(--line); min-height: 18px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
