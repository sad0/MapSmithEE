// Editor state: the open area, the current tool and selection, undo/redo history.
import type { AreaDoc, BmpMap, GameInfo, Poly, Pt, WedEdit } from './types';

export type Tool = 'select' | 'wall' | 'region' | 'actor' | 'paint';
export type MapKey = 'sr' | 'ht' | 'lm' | 'ln';
export type Kind = 'wall' | 'region' | 'actor' | 'door' | 'container' | 'entrance' | 'anim';

/** a picked object; `verts` are the selected vertex indices of a polygon */
export interface Sel { kind: Kind; i: number; verts: number[] }
/** box selection across walls: [wall index, vertex index] */
export type Multi = [number, number][];

export const REGION_KINDS = ['Trap', 'Info point', 'Travel'];
export const SR_NAMES = ['Obstacle (blocks light)', 'Sand', 'Wood', 'Wood', 'Stone', 'Grass', 'Water', 'Stone',
  'Obstacle (no light block)', 'Wood', 'Wall', 'Water', 'Water (impassable)', 'Roof', 'Worldmap exit', 'Grass'];
export const WALL_FLAGS = ['Shade wall', 'Hovering', 'Cover animations', 'Cover animations (alt)', 'Bit 4', 'Bit 5', 'Bit 6', 'Door'];

type DocSnap = string;
type Entry =
  | { t: 'doc'; before: DocSnap; after: DocSnap; label: string }
  | { t: 'paint'; map: MapKey; idx: Uint32Array; before: Uint8Array; after: Uint8Array };

function b64ToBytes(s: string): Uint8Array {
  const bin = atob(s);
  const out = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
  return out;
}
function bytesToB64(b: Uint8Array): string {
  let s = '';
  for (let i = 0; i < b.length; i += 0x8000) s += String.fromCharCode(...b.subarray(i, i + 0x8000));
  return btoa(s);
}

class Editor {
  game = $state<GameInfo | null>(null);
  doc = $state<AreaDoc | null>(null);
  night = $state(false);
  tool = $state<Tool>('select');
  regionKind = $state(2);
  layers = $state({ walls: true, regions: true, actors: true, doors: true, containers: true, entrances: true, anims: false });
  mapLayer = $state<MapKey | null>(null);
  mapAlpha = $state(0.55);
  paintValue = $state(0);
  brush = $state(1);
  sel = $state<Sel | null>(null);
  multi = $state<Multi>([]);
  /** doors drawn closed, by WED door name */
  closed = $state<Record<string, boolean>>({});
  status = $state('');
  /** bumps when map pixels change, to redraw */
  mapVer = $state(0);
  /** bumps on every model change, to redraw */
  ver = $state(0);

  /** decoded map pixels, outside of the reactive state */
  pixels: Partial<Record<MapKey, Uint8Array>> = {};
  private undoStack: Entry[] = [];
  private redoStack: Entry[] = [];
  private savedAt = $state(0);
  private depth = $state(0);
  canUndo = $state(false);
  canRedo = $state(false);

  get dirty() { return this.depth !== this.savedAt; }

  /** the WED being edited: night when shown and kept apart from day */
  get wed(): WedEdit | null {
    const d = this.doc;
    if (!d) return null;
    return this.night && d.wed_night && !d.night_linked ? d.wed_night : d.wed_day;
  }
  get pic() { const d = this.doc; return d ? (this.night && d.night ? d.night : d.day) : null; }
  map(k: MapKey): BmpMap | null { return this.doc?.maps[k] ?? null; }

  load(doc: AreaDoc) {
    this.doc = doc;
    this.pixels = {};
    for (const k of ['sr', 'ht', 'lm', 'ln'] as MapKey[]) { const m = doc.maps[k]; if (m) this.pixels[k] = b64ToBytes(m.pixels); }
    // doors start in their saved state: ARE flag bit 0 = open
    const closed: Record<string, boolean> = {};
    for (const d of doc.are.doors) closed[d.wed_id] = (d.flags & 1) === 0;
    this.closed = closed;
    this.sel = null; this.multi = []; this.undoStack = []; this.redoStack = [];
    this.depth = 0; this.savedAt = 0; this.sync();
    this.mapVer++; this.ver++;
  }

  private snap(): DocSnap {
    const d = this.doc!;
    return JSON.stringify({ are: $state.snapshot(d.are), wed_day: $state.snapshot(d.wed_day), wed_night: $state.snapshot(d.wed_night) });
  }
  private restore(s: DocSnap) {
    const v = JSON.parse(s);
    const d = this.doc!;
    d.are = v.are; d.wed_day = v.wed_day; d.wed_night = v.wed_night;
    // a selection may point past the end after undo
    if (this.sel && !this.objectAt(this.sel.kind, this.sel.i)) this.sel = null;
    this.multi = [];
    this.ver++;
  }
  private sync() { this.canUndo = this.undoStack.length > 0; this.canRedo = this.redoStack.length > 0; }
  private push(e: Entry) {
    this.undoStack.push(e);
    if (this.undoStack.length > 300) this.undoStack.shift();
    this.redoStack = [];
    if (this.savedAt > this.depth) this.savedAt = -1; // the saved state can't be reached by redo any more
    this.depth++; this.sync();
  }

  /** run a model change as one undo step */
  change(label: string, fn: () => void) {
    if (!this.doc) return;
    const before = this.snap();
    fn();
    const after = this.snap();
    if (after !== before) this.push({ t: 'doc', before, after, label });
    this.ver++;
  }
  /** a change spread over several events (drags): begin, then end */
  private pending: DocSnap | null = null;
  begin() { if (this.doc) this.pending = this.snap(); }
  end(label: string) {
    if (!this.doc || this.pending === null) return;
    const after = this.snap();
    if (after !== this.pending) this.push({ t: 'doc', before: this.pending, after, label });
    this.pending = null;
  }

  /** painting: changed pixel indices with their old and new values */
  pushPaint(map: MapKey, changes: Map<number, [number, number]>) {
    if (!changes.size) return;
    const idx = new Uint32Array(changes.size), before = new Uint8Array(changes.size), after = new Uint8Array(changes.size);
    let k = 0;
    for (const [i, [b, a]] of changes) { idx[k] = i; before[k] = b; after[k] = a; k++; }
    this.push({ t: 'paint', map, idx, before, after });
  }

  undo() {
    const e = this.undoStack.pop(); if (!e) return;
    this.apply(e, true); this.redoStack.push(e); this.depth--; this.sync();
  }
  redo() {
    const e = this.redoStack.pop(); if (!e) return;
    this.apply(e, false); this.undoStack.push(e); this.depth++; this.sync();
  }
  private apply(e: Entry, back: boolean) {
    if (e.t === 'doc') this.restore(back ? e.before : e.after);
    else {
      const px = this.pixels[e.map]!; const v = back ? e.before : e.after;
      for (let k = 0; k < e.idx.length; k++) px[e.idx[k]] = v[k];
      this.mapLayer = e.map; this.mapVer++;
    }
  }
  markSaved() { this.savedAt = this.depth; }

  /** the save request: map pixels encoded back, linked night WED follows day */
  saveRequest() {
    const d = this.doc!;
    const maps = $state.snapshot(d.maps);
    // only painted maps are written: the others stay the game's own files
    for (const k of ['sr', 'ht', 'lm', 'ln'] as MapKey[]) {
      const m = maps[k]; const px = this.pixels[k];
      if (!m || !px) continue;
      const enc = bytesToB64(px);
      if (enc === m.pixels && this.doc!.source === 'game') maps[k] = null; else m.pixels = enc;
    }
    const day = $state.snapshot(d.wed_day);
    return {
      name: d.name, are: $state.snapshot(d.are), wed_day: day,
      wed_night: d.wed_night ? (d.night_linked ? day : $state.snapshot(d.wed_night)) : null, maps,
    };
  }

  objectAt(kind: Kind, i: number): unknown {
    const d = this.doc; if (!d) return null;
    switch (kind) {
      case 'wall': return this.wed?.walls[i];
      case 'region': return d.are.regions[i];
      case 'actor': return d.are.actors[i];
      case 'door': return d.are.doors[i];
      case 'container': return d.are.containers[i];
      case 'entrance': return d.are.entrances[i];
      case 'anim': return d.are.anims[i];
    }
  }
  /** the vertex list of a selectable polygon, or null */
  polyOf(kind: Kind, i: number): Pt[] | null {
    if (kind === 'wall') return (this.objectAt(kind, i) as Poly | undefined)?.pts ?? null;
    if (kind === 'region' || kind === 'container') return (this.objectAt(kind, i) as { pts: Pt[] } | undefined)?.pts ?? null;
    return null;
  }

  select(s: Sel | null) { this.sel = s; this.multi = []; }
}

export const ed = new Editor();
