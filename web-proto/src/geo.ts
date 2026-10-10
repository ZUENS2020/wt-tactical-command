// Map metadata = real /map_info.json values captured 2026-10-10 (session b6a7).
export type MapId = 'ground' | 'air';
export type V2 = [number, number];

export interface MapCfg {
  id: MapId;
  title: string;
  sub: string;
  img: string;
  min: V2; max: V2;            // world extent (8111 world units ≈ m)
  gridZero: V2; gridSteps: V2; gridSize: V2;
  hudType: number;
  imgPx: number;
  /** live: a map of this kind is known for the current battle */
  available: boolean;
  hash: string | null;
}

export const MAPS: Record<MapId, MapCfg> = {
  ground: {
    id: 'ground', title: '陆战', sub: '靶场 · firing_range', img: 'maps/ground_firing_range.jpg',
    min: [0, 0], max: [4096, 4096],
    gridZero: [1519.449951171875, 2497.25], gridSteps: [225, 225], gridSize: [1600, 1600],
    hudType: 1, imgPx: 2048, available: true, hash: 'demo-ground',
  },
  air: {
    id: 'air', title: '空战', sub: '丹麦 · air_denmark', img: 'maps/air_denmark.jpg',
    min: [-65536, -65536], max: [65536, 65536],
    gridZero: [6494.30078125, 19547.5], gridSteps: [5500, 5500], gridSize: [52719.398, 55385.301],
    hudType: 0, imgPx: 2048, available: true, hash: 'demo-air',
  },
};

// Canonical world for drawings = ground-map normalized coords × 4096 (m, y down).
// Ground→air transform is MOCKED (the two real captures are different missions):
// air_norm = A + K * ground_norm   (server would fit this by least squares, plan §5.3)
const K = 0.1875;
const A: V2 = [0.574, 0.39];
export const CANON = 4096;

/** live mode: each map has its own canonical space (norm × 4096); no ground→air transform is known */
let SEPARATE = false;
export function setSeparateMaps(v: boolean) { SEPARATE = v; }
export function separateMaps() { return SEPARATE; }

export function canonToNorm(map: MapId, w: V2): V2 {
  const gx = w[0] / CANON, gy = w[1] / CANON;
  if (map === 'ground' || SEPARATE) return [gx, gy];
  return [A[0] + K * gx, A[1] + K * gy];
}
export function normToCanon(map: MapId, n: V2): V2 {
  if (map === 'ground' || SEPARATE) return [n[0] * CANON, n[1] * CANON];
  return [((n[0] - A[0]) / K) * CANON, ((n[1] - A[1]) / K) * CANON];
}
/** metres per canonical unit when measured on the given map */
export function metersPerCanon(map: MapId) {
  if (SEPARATE) return (MAPS[map].max[0] - MAPS[map].min[0]) / CANON;
  return map === 'ground' ? 1 : (K * 131072) / CANON;
}
export function metersPerNorm(map: MapId) {
  const c = MAPS[map];
  return c.max[0] - c.min[0];
}

/** Grid box in normalized coords (x right, y down). 8111 world y is up. */
export function gridBox(map: MapId) {
  const c = MAPS[map];
  const sx = c.max[0] - c.min[0], sy = c.max[1] - c.min[1];
  const x0 = (c.gridZero[0] - c.min[0]) / sx;
  const y0 = (c.max[1] - c.gridZero[1]) / sy;
  const w = c.gridSize[0] / sx, h = c.gridSize[1] / sy;
  const stepX = c.gridSteps[0] / sx, stepY = c.gridSteps[1] / sy;
  const cols = Math.ceil(c.gridSize[0] / c.gridSteps[0] - 0.02);
  const rows = Math.ceil(c.gridSize[1] / c.gridSteps[1] - 0.02);
  return { x0, y0, w, h, stepX, stepY, cols, rows };
}

const LETTERS = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ';
export function rowLetter(i: number) { return LETTERS[i] ?? '?'; }

/** War Thunder style grid reference ("D4") for a canonical point on a map */
export function gridRef(map: MapId, w: V2): string {
  const n = canonToNorm(map, w);
  const g = gridBox(map);
  const c = Math.floor((n[0] - g.x0) / g.stepX);
  const r = Math.floor((n[1] - g.y0) / g.stepY);
  if (c < 0 || r < 0 || c >= g.cols || r >= g.rows) return '界外';
  return `${rowLetter(r)}${c + 1}`;
}

export function bearingDeg(a: V2, b: V2) {
  // y down → north is -y ; heading = atan2(dx, -dy)
  const d = (Math.atan2(b[0] - a[0], -(b[1] - a[1])) * 180) / Math.PI;
  return (d + 360) % 360;
}
export function dist(a: V2, b: V2) { return Math.hypot(b[0] - a[0], b[1] - a[1]); }

export function fmtDist(m: number) {
  if (m >= 10000) return `${(m / 1000).toFixed(0)} km`;
  if (m >= 1000) return `${(m / 1000).toFixed(1)} km`;
  return `${Math.round(m)} m`;
}
export function fmtClock(sec: number) {
  const s = Math.max(0, Math.floor(sec));
  return `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
}
export function fmtBearing(d: number) { return String(Math.round(d) % 360).padStart(3, '0') + '°'; }
