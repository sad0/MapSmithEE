<script lang="ts">
  import { ed, type Kind, type MapKey, type Sel } from './editor.svelte';
  import { area, dist, facingFrom, facingVec, inPoly, round } from './geom';
  import type { Actor, Poly, Pt, Region } from './types';

  let canvas: HTMLCanvasElement;
  let box: HTMLDivElement;
  let view = { x: 0, y: 0, z: 0.25 };
  let size = { w: 800, h: 600 };
  let img: HTMLImageElement | null = null;
  let imgUrl = '';
  const doorImgs = new Map<string, HTMLImageElement>();
  let mapCanvas: HTMLCanvasElement | null = null;
  let mouse: Pt = $state([0, 0]);
  let screenMouse: Pt = [0, 0];
  let drawing: Pt[] = $state([]);
  let spaceDown = false;
  let zoomPct = $state(25);

  type Drag =
    | { t: 'pan'; sx: number; sy: number; vx: number; vy: number }
    | { t: 'verts'; start: Pt; orig: Pt[]; label: string }
    | { t: 'multi'; start: Pt; orig: Pt[] }
    | { t: 'body'; start: Pt; orig: Pt[] }
    | { t: 'point'; start: Pt; orig: Pt }
    | { t: 'facing' }
    | { t: 'box'; start: Pt; shift: boolean }
    | { t: 'paint'; map: MapKey; changes: Map<number, [number, number]>; last: Pt };
  let drag: Drag | null = null;
  let boxEnd: Pt = [0, 0];
  let lastClick: { p: Pt; t: number } | null = null;

  const COLORS = {
    wall: '#4fd1ff', wallDoor: '#ff9df2', region: ['#ff5a5a', '#ffd34f', '#5aa0ff'], door: '#c77dff',
    container: '#5fe08a', entrance: '#ffffff', actor: '#ffb347', anim: '#ff7ab8', sel: '#ffffff',
  };

  // ---------- view ----------
  const toWorld = (sx: number, sy: number): Pt => [sx / view.z + view.x, sy / view.z + view.y];
  const toScreen = (p: Pt): Pt => [(p[0] - view.x) * view.z, (p[1] - view.y) * view.z];

  export function fit() {
    const p = ed.pic; if (!p) return;
    view.z = Math.min(size.w / p.w, size.h / p.h) * 0.98;
    view.x = (p.w - size.w / view.z) / 2; view.y = (p.h - size.h / view.z) / 2;
    redraw();
  }
  export function centerOn(p: Pt) { view.z = Math.max(view.z, 1); view.x = p[0] - size.w / 2 / view.z; view.y = p[1] - size.h / 2 / view.z; redraw(); }

  // ---------- images ----------
  function loadImg(url: string, then: (i: HTMLImageElement) => void) {
    const i = new Image();
    i.onload = () => { then(i); redraw(); };
    i.src = url;
  }
  $effect(() => {
    const p = ed.pic;
    if (!p) { img = null; return; }
    if (p.url !== imgUrl) {
      const first = !imgUrl || !img || img.naturalWidth !== p.w;
      imgUrl = p.url;
      loadImg(p.url, (i) => { if (imgUrl === p.url) { img = i; if (first) fit(); } });
      for (const d of p.doors) if (!doorImgs.has(d.img)) loadImg(d.img, (i) => doorImgs.set(d.img, i));
    }
  });
  // map layer -> small canvas drawn scaled up
  $effect(() => {
    void ed.mapVer;
    const k = ed.mapLayer;
    const m = k ? ed.map(k) : null; const px = k ? ed.pixels[k] : null;
    if (!m || !px) { mapCanvas = null; redraw(); return; }
    const c = mapCanvas && mapCanvas.width === m.w && mapCanvas.height === m.h ? mapCanvas : document.createElement('canvas');
    c.width = m.w; c.height = m.h;
    const g = c.getContext('2d')!; const id = g.createImageData(m.w, m.h);
    for (let i = 0; i < px.length; i++) {
      const col = m.palette[px[i]] ?? [255, 0, 255];
      id.data[i * 4] = col[0]; id.data[i * 4 + 1] = col[1]; id.data[i * 4 + 2] = col[2]; id.data[i * 4 + 3] = 255;
    }
    g.putImageData(id, 0, 0); mapCanvas = c; redraw();
  });
  $effect(() => { void ed.ver; void ed.sel; void ed.multi; void ed.layers; void ed.closed; void ed.mapAlpha; void ed.night; void ed.tool; redraw(); });

  // ---------- drawing ----------
  let frame = 0;
  function redraw() { if (!frame) frame = requestAnimationFrame(() => { frame = 0; paint(); }); }

  function path(g: CanvasRenderingContext2D, pts: Pt[], close = true) {
    g.beginPath();
    pts.forEach((p, i) => (i ? g.lineTo(p[0], p[1]) : g.moveTo(p[0], p[1])));
    if (close) g.closePath();
  }
  function poly(g: CanvasRenderingContext2D, pts: Pt[], color: string, fill = 0.12, lw = 1.5) {
    if (pts.length < 2) return;
    path(g, pts);
    g.globalAlpha = fill; g.fillStyle = color; g.fill();
    g.globalAlpha = 1; g.strokeStyle = color; g.lineWidth = lw / view.z; g.stroke();
  }
  function doorPolys(i: number): Pt[] {
    const d = ed.doc!.are.doors[i];
    return ed.closed[d.wed_id] ? d.shut : d.open;
  }

  function paint() {
    if (!canvas) return;
    zoomPct = Math.round(view.z * 100);
    const dpr = window.devicePixelRatio || 1;
    const g = canvas.getContext('2d')!;
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.fillStyle = '#111'; g.fillRect(0, 0, canvas.width, canvas.height);
    const d = ed.doc, pic = ed.pic;
    if (!d || !pic) return;
    g.setTransform(view.z * dpr, 0, 0, view.z * dpr, -view.x * view.z * dpr, -view.y * view.z * dpr);
    g.imageSmoothingEnabled = view.z < 1;
    if (img) g.drawImage(img, 0, 0, pic.w, pic.h);
    for (const p of pic.doors) {
      const di = doorImgs.get(p.img);
      if (di && ed.closed[p.door]) g.drawImage(di, p.x, p.y, p.w, p.h);
    }
    if (mapCanvas) {
      g.imageSmoothingEnabled = false; g.globalAlpha = ed.mapAlpha;
      g.drawImage(mapCanvas, 0, 0, pic.w, pic.h); g.globalAlpha = 1;
    }
    const L = ed.layers, wed = ed.wed!;
    if (L.containers) d.are.containers.forEach((c) => poly(g, c.pts, COLORS.container));
    if (L.regions) d.are.regions.forEach((r) => poly(g, r.pts, COLORS.region[r.kind] ?? '#aaa'));
    if (L.doors) d.are.doors.forEach((_, i) => poly(g, doorPolys(i), COLORS.door, 0.15, 2));
    if (L.walls) wed.walls.forEach((w) => poly(g, w.pts, w.flags & 0x80 ? COLORS.wallDoor : COLORS.wall, 0.08));
    const r = 10 / view.z;
    if (L.entrances) for (const e of d.are.entrances) marker(g, e.pos, COLORS.entrance, 'diamond', e.orientation);
    if (L.anims) for (const a of d.are.anims) marker(g, a.pos, COLORS.anim, 'square');
    if (L.actors) for (const a of d.are.actors) marker(g, a.pos, COLORS.actor, 'circle', a.orientation);
    // labels when zoomed in
    if (view.z > 0.6) {
      g.font = `${11 / view.z}px system-ui`; g.fillStyle = '#fff'; g.textAlign = 'center';
      if (L.actors) for (const a of d.are.actors) g.fillText(a.name || a.cre, a.pos[0], a.pos[1] + r * 2.2);
      if (L.regions) for (const rg of d.are.regions) if (rg.pts.length) g.fillText(rg.name, ...labelAt(rg.pts));
    }
    // selection
    const s = ed.sel;
    if (s) {
      const pts = ed.polyOf(s.kind, s.i);
      if (pts) {
        path(g, pts); g.strokeStyle = COLORS.sel; g.lineWidth = 2.5 / view.z; g.setLineDash([6 / view.z, 4 / view.z]); g.stroke(); g.setLineDash([]);
        handles(g, pts, s.verts);
      } else if (s.kind === 'door') {
        path(g, doorPolys(s.i)); g.strokeStyle = COLORS.sel; g.lineWidth = 2.5 / view.z; g.stroke();
      } else {
        const o = ed.objectAt(s.kind, s.i) as { pos: Pt } | undefined;
        if (o) {
          g.beginPath(); g.arc(o.pos[0], o.pos[1], r * 1.6, 0, Math.PI * 2); g.strokeStyle = COLORS.sel; g.lineWidth = 2 / view.z; g.stroke();
          if (s.kind === 'actor') {
            const tip = facingTip(o as Actor);
            g.beginPath(); g.arc(tip[0], tip[1], 5 / view.z, 0, Math.PI * 2); g.fillStyle = COLORS.sel; g.fill();
          }
        }
      }
    }
    if (ed.multi.length) {
      g.fillStyle = '#fff';
      for (const [w, v] of ed.multi) { const p = wed.walls[w]?.pts[v]; if (p) g.fillRect(p[0] - 4 / view.z, p[1] - 4 / view.z, 8 / view.z, 8 / view.z); }
    }
    // tool previews
    if (drawing.length) {
      const pts = [...drawing, mouse];
      path(g, pts, false); g.strokeStyle = ed.tool === 'wall' ? COLORS.wall : COLORS.region[ed.regionKind];
      g.lineWidth = 2 / view.z; g.stroke();
      handles(g, drawing, []);
    }
    if (drag?.t === 'box') {
      g.strokeStyle = '#fff'; g.lineWidth = 1 / view.z; g.setLineDash([4 / view.z, 3 / view.z]);
      g.strokeRect(drag.start[0], drag.start[1], boxEnd[0] - drag.start[0], boxEnd[1] - drag.start[1]); g.setLineDash([]);
    }
    if (ed.tool === 'paint' && ed.mapLayer) {
      const c = cellSize(ed.mapLayer);
      if (c) { g.strokeStyle = '#fff'; g.lineWidth = 1 / view.z; g.beginPath(); g.arc(mouse[0], mouse[1], (ed.brush - 0.5) * c[0] + c[0] / 2, 0, Math.PI * 2); g.stroke(); }
    }
  }

  function labelAt(pts: Pt[]): [number, number] {
    let x = 0, y = 0; for (const p of pts) { x += p[0]; y += p[1]; } return [x / pts.length, y / pts.length];
  }
  function facingTip(a: Actor): Pt { const v = facingVec(a.orientation); const L = 30 / view.z; return [a.pos[0] + v[0] * L, a.pos[1] + v[1] * L]; }
  function marker(g: CanvasRenderingContext2D, p: Pt, color: string, shape: 'circle' | 'diamond' | 'square', facing?: number) {
    const r = 9 / view.z;
    g.beginPath();
    if (shape === 'circle') g.arc(p[0], p[1], r, 0, Math.PI * 2);
    else if (shape === 'diamond') { g.moveTo(p[0], p[1] - r); g.lineTo(p[0] + r, p[1]); g.lineTo(p[0], p[1] + r); g.lineTo(p[0] - r, p[1]); g.closePath(); }
    else g.rect(p[0] - r * 0.8, p[1] - r * 0.8, r * 1.6, r * 1.6);
    g.globalAlpha = 0.45; g.fillStyle = color; g.fill(); g.globalAlpha = 1; g.strokeStyle = color; g.lineWidth = 2 / view.z; g.stroke();
    if (facing !== undefined) {
      const v = facingVec(facing); const L = 24 / view.z;
      g.beginPath(); g.moveTo(p[0], p[1]); g.lineTo(p[0] + v[0] * L, p[1] + v[1] * L); g.strokeStyle = color; g.lineWidth = 2.5 / view.z; g.stroke();
    }
  }
  function handles(g: CanvasRenderingContext2D, pts: Pt[], selected: number[]) {
    const h = 4 / view.z;
    // midpoints: drag one to split the segment
    g.strokeStyle = '#fff'; g.lineWidth = 1 / view.z;
    if (ed.tool === 'select') for (let i = 0; i < pts.length; i++) {
      const a = pts[i], b = pts[(i + 1) % pts.length];
      g.beginPath(); g.arc((a[0] + b[0]) / 2, (a[1] + b[1]) / 2, h * 0.8, 0, Math.PI * 2); g.stroke();
    }
    pts.forEach((p, i) => {
      g.fillStyle = selected.includes(i) ? '#ffde59' : '#fff';
      g.fillRect(p[0] - h, p[1] - h, h * 2, h * 2);
      g.strokeStyle = '#000'; g.strokeRect(p[0] - h, p[1] - h, h * 2, h * 2);
    });
  }

  // ---------- picking ----------
  /** every object under a point, small ones first */
  function hits(p: Pt): Sel[] {
    const d = ed.doc!; const out: { s: Sel; a: number }[] = []; const L = ed.layers; const r = 12 / view.z;
    if (L.actors) d.are.actors.forEach((a, i) => dist(a.pos, p) < r && out.push({ s: { kind: 'actor', i, verts: [] }, a: 0 }));
    if (L.entrances) d.are.entrances.forEach((e, i) => dist(e.pos, p) < r && out.push({ s: { kind: 'entrance', i, verts: [] }, a: 0 }));
    if (L.anims) d.are.anims.forEach((e, i) => dist(e.pos, p) < r && out.push({ s: { kind: 'anim', i, verts: [] }, a: 0 }));
    const polyHit = (kind: Kind, list: Pt[][]) => list.forEach((pts, i) => pts.length > 2 && inPoly(p, pts) && out.push({ s: { kind, i, verts: [] }, a: area(pts) }));
    if (L.regions) polyHit('region', d.are.regions.map((x) => x.pts));
    if (L.containers) polyHit('container', d.are.containers.map((x) => x.pts));
    if (L.doors) polyHit('door', d.are.doors.map((_, i) => doorPolys(i)));
    if (L.walls) polyHit('wall', ed.wed!.walls.map((x) => x.pts));
    return out.sort((a, b) => a.a - b.a).map((o) => o.s);
  }
  const same = (a: Sel | null, b: Sel) => !!a && a.kind === b.kind && a.i === b.i;
  function vertexAt(pts: Pt[], sp: Pt): number {
    for (let i = pts.length - 1; i >= 0; i--) if (dist(toScreen(pts[i]), sp) < 7) return i;
    return -1;
  }
  function midAt(pts: Pt[], sp: Pt): number {
    for (let i = 0; i < pts.length; i++) {
      const a = pts[i], b = pts[(i + 1) % pts.length];
      if (dist(toScreen([(a[0] + b[0]) / 2, (a[1] + b[1]) / 2]), sp) < 6) return i;
    }
    return -1;
  }
  function cellSize(k: MapKey): Pt | null {
    const m = ed.map(k), p = ed.pic; return m && p ? [p.w / m.w, p.h / m.h] : null;
  }

  // ---------- painting ----------
  function paintAt(p: Pt, changes: Map<number, [number, number]>) {
    const k = ed.mapLayer!; const m = ed.map(k)!; const px = ed.pixels[k]!; const c = cellSize(k)!;
    const cx = Math.floor(p[0] / c[0]), cy = Math.floor(p[1] / c[1]); const r = ed.brush - 1;
    let any = false;
    for (let y = cy - r; y <= cy + r; y++) for (let x = cx - r; x <= cx + r; x++) {
      if (x < 0 || y < 0 || x >= m.w || y >= m.h || (x - cx) ** 2 + (y - cy) ** 2 > r * r + r) continue;
      const i = y * m.w + x; const old = px[i];
      if (old === ed.paintValue) continue;
      const prev = changes.get(i);
      changes.set(i, [prev ? prev[0] : old, ed.paintValue]); px[i] = ed.paintValue; any = true;
    }
    if (any) ed.mapVer++;
  }
  function pickValue(p: Pt) {
    const k = ed.mapLayer; if (!k) return; const m = ed.map(k)!; const c = cellSize(k)!;
    const x = Math.floor(p[0] / c[0]), y = Math.floor(p[1] / c[1]);
    if (x >= 0 && y >= 0 && x < m.w && y < m.h) ed.paintValue = ed.pixels[k]![y * m.w + x];
  }

  // ---------- creating ----------
  function finishDrawing() {
    if (drawing.length < 3) { drawing = []; redraw(); return; }
    const pts = drawing.map(round); drawing = [];
    if (ed.tool === 'wall') {
      ed.change('Add wall', () => { ed.wed!.walls.push({ flags: 1, height: 255, pts } as Poly); });
      ed.select({ kind: 'wall', i: ed.wed!.walls.length - 1, verts: [] });
    } else {
      const kind = ed.regionKind; const regions = ed.doc!.are.regions;
      const base = ['Trap', 'Info', 'Tran'][kind];
      let n = 1; while (regions.some((r) => r.name === `${base}${n}`)) n++;
      const reg: Region = {
        name: `${base}${n}`, kind, pts, trigger_value: 0, cursor: kind === 2 ? 34 : kind === 1 ? 22 : 0,
        dest_area: '', entrance: '', flags: 0, info: kind === 1 ? 0xffffffff : 0, trap_detect: 0, trap_remove: 0,
        trapped: 0, detected: 0, launch: [0, 0], key: '', script: '', use_point: [0, 0], talk_point: [0, 0],
        speaker: 0xffffffff, dialog: '', raw: '',
      };
      ed.change('Add region', () => { regions.push(reg); });
      ed.select({ kind: 'region', i: regions.length - 1, verts: [] });
    }
  }
  function addActor(p: Pt) {
    const actors = ed.doc!.are.actors;
    let n = 1; while (actors.some((a) => a.name === `Actor ${n}`)) n++;
    const a: Actor = {
      name: `Actor ${n}`, pos: round(p), dest: round(p), orientation: 0, cre: '', dialog: '',
      scripts: ['', '', '', '', '', ''], flags: 1, schedule: 0xffffffff, animation: 0, embedded: null, raw: '',
    };
    ed.change('Add actor', () => { actors.push(a); });
    ed.select({ kind: 'actor', i: actors.length - 1, verts: [] });
  }

  // ---------- events ----------
  function onDown(e: MouseEvent) {
    if (!ed.doc) return;
    canvas.focus();
    const sp: Pt = [e.offsetX, e.offsetY]; const p = toWorld(...sp); mouse = p;
    if (e.button === 1 || e.button === 2 && ed.tool !== 'paint' || (e.button === 0 && spaceDown)) {
      drag = { t: 'pan', sx: sp[0], sy: sp[1], vx: view.x, vy: view.y }; return;
    }
    if (ed.tool === 'paint') {
      if (!ed.mapLayer) { ed.status = 'Pick a map layer (SR, HT, LM, LN) to paint on'; return; }
      if (e.button === 2 || e.altKey) { pickValue(p); return; }
      drag = { t: 'paint', map: ed.mapLayer, changes: new Map(), last: p }; paintAt(p, drag.changes); return;
    }
    if (e.button !== 0) return;
    if (ed.tool === 'wall' || ed.tool === 'region') {
      if (drawing.length >= 3 && dist(toScreen(drawing[0]), sp) < 9) { finishDrawing(); return; }
      drawing = [...drawing, round(p)]; redraw(); return;
    }
    if (ed.tool === 'actor') { addActor(p); return; }

    // select tool: handles of the current selection first, unless this is a second click on the same spot over
    // several shapes (then it goes to the next shape, even under a small shape's handles)
    const s = ed.sel;
    const again = !!lastClick && dist(lastClick.p, sp) < 4;
    const list = hits(p);
    const cycling = again && list.length > 1;
    if (ed.multi.length && ed.multi.some(([w, v]) => { const q = ed.wed!.walls[w]?.pts[v]; return q && dist(toScreen(q), sp) < 7; })) {
      ed.begin(); drag = { t: 'multi', start: p, orig: ed.multi.map(([w, v]) => [...ed.wed!.walls[w].pts[v]] as Pt) }; return;
    }
    if (s && !cycling) {
      const pts = ed.polyOf(s.kind, s.i);
      if (pts) {
        const v = vertexAt(pts, sp);
        if (v >= 0) {
          if (e.shiftKey) { s.verts = s.verts.includes(v) ? s.verts.filter((x) => x !== v) : [...s.verts, v]; redraw(); return; }
          if (!s.verts.includes(v)) s.verts = [v];
          ed.begin(); drag = { t: 'verts', start: p, orig: pts.map((q) => [...q] as Pt), label: 'Move vertices' }; return;
        }
        const m = midAt(pts, sp);
        if (m >= 0) {
          ed.begin();
          const a = pts[m], b = pts[(m + 1) % pts.length];
          pts.splice(m + 1, 0, round([(a[0] + b[0]) / 2, (a[1] + b[1]) / 2]));
          s.verts = [m + 1];
          drag = { t: 'verts', start: p, orig: pts.map((q) => [...q] as Pt), label: 'Split segment' }; ed.ver++; return;
        }
      }
      if (s.kind === 'actor') {
        const a = ed.doc.are.actors[s.i];
        if (a && dist(toScreen(facingTip(a)), sp) < 8) { ed.begin(); drag = { t: 'facing' }; return; }
      }
    }
    lastClick = { p: sp, t: Date.now() };
    if (!list.length) { drag = { t: 'box', start: p, shift: e.shiftKey }; boxEnd = p; return; }
    const cur = list.findIndex((h) => same(s, h));
    const wasSelected = cur >= 0;
    const pick = again && wasSelected ? list[(cur + 1) % list.length] : wasSelected ? list[cur] : list[0];
    if (!same(s, pick)) ed.select({ ...pick, verts: [] });
    if (list.length > 1) ed.status = `${list.length} shapes here: click again for the next one`;
    const o = ed.objectAt(pick.kind, pick.i) as { pos?: Pt; pts?: Pt[] };
    if (pick.kind === 'actor' || pick.kind === 'entrance') { ed.begin(); drag = { t: 'point', start: p, orig: [...o.pos!] as Pt }; }
    else if (wasSelected && !again && o.pts && pick.kind !== 'door') { ed.begin(); drag = { t: 'body', start: p, orig: o.pts.map((q) => [...q] as Pt) }; }
  }

  function onMove(e: MouseEvent) {
    const sp: Pt = [e.offsetX, e.offsetY]; screenMouse = sp; const p = toWorld(...sp); mouse = p;
    if (!drag) { if (drawing.length || ed.tool === 'paint') redraw(); return; }
    const dx = p[0] - ('start' in drag ? drag.start[0] : 0), dy = p[1] - ('start' in drag ? drag.start[1] : 0);
    const s = ed.sel;
    switch (drag.t) {
      case 'pan': view.x = drag.vx - (sp[0] - drag.sx) / view.z; view.y = drag.vy - (sp[1] - drag.sy) / view.z; break;
      case 'verts': { const pts = ed.polyOf(s!.kind, s!.i)!; for (const v of s!.verts) pts[v] = round([drag.orig[v][0] + dx, drag.orig[v][1] + dy]); ed.ver++; break; }
      case 'body': { const pts = ed.polyOf(s!.kind, s!.i)!; drag.orig.forEach((q, k) => (pts[k] = round([q[0] + dx, q[1] + dy]))); ed.ver++; break; }
      case 'multi': ed.multi.forEach(([w, v], k) => (ed.wed!.walls[w].pts[v] = round([(drag as { orig: Pt[] }).orig[k][0] + dx, (drag as { orig: Pt[] }).orig[k][1] + dy]))); ed.ver++; break;
      case 'point': { const o = ed.objectAt(s!.kind, s!.i) as { pos: Pt; dest?: Pt }; o.pos = round([drag.orig[0] + dx, drag.orig[1] + dy]); if (o.dest) o.dest = [...o.pos]; ed.ver++; break; }
      case 'facing': { const a = ed.doc!.are.actors[s!.i]; a.orientation = facingFrom(p[0] - a.pos[0], p[1] - a.pos[1]); ed.ver++; break; }
      case 'box': boxEnd = p; break;
      case 'paint': {
        // a fast stroke skips cells: fill the segment from the last point
        const c = cellSize(drag.map)!; const steps = Math.max(1, Math.ceil(dist(drag.last, p) / (Math.min(c[0], c[1]) / 2)));
        for (let k = 1; k <= steps; k++) paintAt([drag.last[0] + ((p[0] - drag.last[0]) * k) / steps, drag.last[1] + ((p[1] - drag.last[1]) * k) / steps], drag.changes);
        drag.last = p; break;
      }
    }
    redraw();
  }

  function onUp() {
    const d = drag; drag = null;
    if (!d) return;
    if (d.t === 'verts') ed.end(d.label);
    else if (d.t === 'body') ed.end('Move shape');
    else if (d.t === 'multi') ed.end('Move vertices');
    else if (d.t === 'point') ed.end('Move');
    else if (d.t === 'facing') ed.end('Turn actor');
    else if (d.t === 'paint') ed.pushPaint(d.map, d.changes);
    else if (d.t === 'box') {
      const [x0, x1] = [Math.min(d.start[0], boxEnd[0]), Math.max(d.start[0], boxEnd[0])];
      const [y0, y1] = [Math.min(d.start[1], boxEnd[1]), Math.max(d.start[1], boxEnd[1])];
      const inBox = (q: Pt) => q[0] >= x0 && q[0] <= x1 && q[1] >= y0 && q[1] <= y1;
      if ((x1 - x0) * view.z < 4 && (y1 - y0) * view.z < 4) { if (!d.shift) ed.select(null); }
      else {
        const s = ed.sel; const pts = s ? ed.polyOf(s.kind, s.i) : null;
        if (s && pts && s.kind !== 'wall') {
          s.verts = pts.map((q, i) => (inBox(q) ? i : -1)).filter((i) => i >= 0);
        } else if (ed.layers.walls) {
          const m: [number, number][] = [];
          ed.wed!.walls.forEach((w, wi) => w.pts.forEach((q, vi) => inBox(q) && m.push([wi, vi])));
          const walls = new Set(m.map(([w]) => w));
          if (walls.size === 1) { const wi = [...walls][0]; ed.select({ kind: 'wall', i: wi, verts: m.map(([, v]) => v) }); }
          else { ed.sel = null; ed.multi = m; ed.status = m.length ? `${m.length} vertices in ${walls.size} walls: drag one to move them, Delete to remove` : ''; }
        }
      }
    }
    redraw();
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const before = toWorld(e.offsetX, e.offsetY);
    view.z = Math.min(8, Math.max(0.03, view.z * Math.pow(1.0015, -e.deltaY)));
    view.x = before[0] - e.offsetX / view.z; view.y = before[1] - e.offsetY / view.z;
    redraw();
  }

  function deleteSelection() {
    const s = ed.sel;
    if (ed.multi.length) {
      const by = new Map<number, number[]>();
      for (const [w, v] of ed.multi) by.set(w, [...(by.get(w) ?? []), v]);
      ed.change('Delete vertices', () => {
        const walls = ed.wed!.walls;
        const gone: number[] = [];
        for (const [w, vs] of by) { walls[w].pts = walls[w].pts.filter((_, i) => !vs.includes(i)); if (walls[w].pts.length < 3) gone.push(w); }
        for (const w of gone.sort((a, b) => b - a)) walls.splice(w, 1);
      });
      ed.multi = []; return;
    }
    if (!s) return;
    const pts = ed.polyOf(s.kind, s.i);
    if (pts && s.verts.length && pts.length - s.verts.length >= 3) {
      ed.change('Delete vertices', () => { const keep = pts.filter((_, i) => !s.verts.includes(i)); pts.splice(0, pts.length, ...keep); });
      s.verts = []; return;
    }
    const lists: Partial<Record<Kind, unknown[]>> = { wall: ed.wed!.walls, region: ed.doc!.are.regions, actor: ed.doc!.are.actors, entrance: ed.doc!.are.entrances, container: ed.doc!.are.containers };
    const list = lists[s.kind];
    if (!list) { ed.status = `Deleting ${s.kind}s is not supported`; return; }
    ed.change(`Delete ${s.kind}`, () => { list.splice(s.i, 1); });
    ed.select(null);
  }

  function onKey(e: KeyboardEvent) {
    if ((e.target as HTMLElement).closest('input, textarea, select')) return;
    if (e.key === ' ') { spaceDown = e.type === 'keydown'; return; }
    if (e.type !== 'keydown') return;
    if (e.key === 'Escape') { if (drawing.length) drawing = []; else { ed.select(null); } redraw(); }
    else if (e.key === 'Enter' && drawing.length) finishDrawing();
    else if ((e.key === 'Delete' || e.key === 'Backspace') && !drawing.length) { e.preventDefault(); deleteSelection(); }
    else if ((e.key === '[' || e.key === ']') && ed.sel?.kind === 'actor') {
      const i = ed.sel.i;
      ed.change('Turn actor', () => { const a = ed.doc!.are.actors[i]; a.orientation = (a.orientation + (e.key === ']' ? 1 : 15)) % 16; });
    } else if (e.key === 'f' && !e.ctrlKey) fit();
  }

  function onDbl() { if (drawing.length >= 3) { drawing = drawing.slice(0, -1); finishDrawing(); } }

  $effect(() => {
    const ro = new ResizeObserver(() => {
      const r = box.getBoundingClientRect(); const dpr = window.devicePixelRatio || 1;
      size = { w: r.width, h: r.height };
      canvas.width = Math.round(r.width * dpr); canvas.height = Math.round(r.height * dpr);
      canvas.style.width = r.width + 'px'; canvas.style.height = r.height + 'px';
      redraw();
    });
    ro.observe(box);
    return () => ro.disconnect();
  });
  // switching tool drops an unfinished shape
  $effect(() => { void ed.tool; drawing = []; });
</script>

<svelte:window onkeydown={onKey} onkeyup={onKey} onmouseup={onUp} />
<div class="view" bind:this={box}>
  <canvas
    bind:this={canvas}
    tabindex="0"
    class:cross={ed.tool !== 'select'}
    onmousedown={onDown}
    onmousemove={onMove}
    onwheel={onWheel}
    ondblclick={onDbl}
    oncontextmenu={(e) => e.preventDefault()}
  ></canvas>
  {#if ed.doc}
    <div class="coords">{Math.round(mouse[0])}, {Math.round(mouse[1])} · {zoomPct}%</div>
  {/if}
</div>

<style>
  .view { position: relative; flex: 1; min-width: 0; min-height: 0; overflow: hidden; background: #111; }
  canvas { display: block; outline: none; }
  canvas.cross { cursor: crosshair; }
  .coords { position: absolute; right: 8px; bottom: 6px; font: 11px system-ui; color: #ccc; background: #0009; padding: 2px 6px; border-radius: 4px; pointer-events: none; }
</style>
