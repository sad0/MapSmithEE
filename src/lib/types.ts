// Shapes shared with the Rust backend (src-tauri/src/*.rs)

export type Pt = [number, number];

export interface Poly { flags: number; height: number; pts: Pt[] }
export interface WedDoor { name: string; closed: number; cells: number[]; open: Poly[]; shut: Poly[] }
export interface WedEdit { walls: Poly[]; doors: WedDoor[] }

export interface Actor {
  name: string; pos: Pt; dest: Pt; orientation: number; cre: string; dialog: string;
  scripts: [string, string, string, string, string, string];
  flags: number; schedule: number; animation: number; embedded: string | null; raw: string;
}

export interface Region {
  name: string; kind: number; pts: Pt[]; trigger_value: number; cursor: number;
  dest_area: string; entrance: string; flags: number; info: number;
  trap_detect: number; trap_remove: number; trapped: number; detected: number; launch: Pt;
  key: string; script: string; use_point: Pt; talk_point: Pt; speaker: number; dialog: string; raw: string;
}

export interface Door {
  name: string; wed_id: string; flags: number; open: Pt[]; shut: Pt[];
  open_cells: Pt[]; shut_cells: Pt[]; cursor: number; key: string; script: string; dialog: string; raw: string;
}

export interface Container { name: string; pos: Pt; kind: number; pts: Pt[]; raw: string }
export interface Entrance { name: string; pos: Pt; orientation: number; raw: string }
export interface Anim { name: string; pos: Pt; bam: string }

export interface Are {
  wed: string; flags: number; area_type: number; script: string;
  actors: Actor[]; regions: Region[]; doors: Door[]; containers: Container[]; entrances: Entrance[];
  anims: Anim[]; header: string; keep: unknown;
}

export interface BmpMap { w: number; h: number; bpp: number; palette: [number, number, number][]; pixels: string; head: string; tail: string }
export interface Maps { sr: BmpMap | null; ht: BmpMap | null; lm: BmpMap | null; ln: BmpMap | null }

export interface DoorPatch { door: string; x: number; y: number; w: number; h: number; img: string }
export interface Pic { url: string; w: number; h: number; doors: DoorPatch[] }

export interface AreaDoc {
  name: string; source: 'game' | 'project'; are: Are;
  wed_day: WedEdit; wed_night: WedEdit | null; night_linked: boolean;
  maps: Maps; day: Pic; night: Pic | null;
}

export interface Settings { project_dir: string; recent_games: string[]; install_targets: string[]; blocked: string[] }
export interface GameInfo { root: string; areas: string[]; kind: string }
