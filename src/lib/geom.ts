import type { Pt } from './types';

export function inPoly(p: Pt, pts: Pt[]): boolean {
  let inside = false;
  for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) {
    const [xi, yi] = pts[i], [xj, yj] = pts[j];
    if ((yi > p[1]) !== (yj > p[1]) && p[0] < ((xj - xi) * (p[1] - yi)) / (yj - yi) + xi) inside = !inside;
  }
  return inside;
}

export function dist(a: Pt, b: Pt) { return Math.hypot(a[0] - b[0], a[1] - b[1]); }

export function centroid(pts: Pt[]): Pt {
  let x = 0, y = 0;
  for (const p of pts) { x += p[0]; y += p[1]; }
  return [x / Math.max(1, pts.length), y / Math.max(1, pts.length)];
}

/** polygon area, to pick the smallest of overlapping shapes first */
export function area(pts: Pt[]) {
  let a = 0;
  for (let i = 0, j = pts.length - 1; i < pts.length; j = i++) a += (pts[j][0] + pts[i][0]) * (pts[j][1] - pts[i][1]);
  return Math.abs(a / 2);
}

/** facing 0..15: 0 south, 4 west, 8 north, 12 east */
export function facingVec(o: number): Pt {
  const t = (o * Math.PI) / 8;
  return [-Math.sin(t), Math.cos(t)];
}
export function facingFrom(dx: number, dy: number): number {
  const t = Math.atan2(-dx, dy);
  return ((Math.round((t * 8) / Math.PI) % 16) + 16) % 16;
}

export const round = (p: Pt): Pt => [Math.round(p[0]), Math.round(p[1])];
