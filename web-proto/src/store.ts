import { useSyncExternalStore } from 'react';
import type { MapId, V2 } from './geo';
import { DEMO } from './mode';

export type ViewMode = 'ground' | 'air' | 'dual';
export type Density = 'command' | 'member';
export type Tool = 'select' | 'point' | 'route' | 'area' | 'source' | 'ruler' | 'layers';
export type PointCat = 'rally' | 'attack' | 'defend' | 'observe' | 'enemy' | 'danger' | 'airstrike' | 'recon';
export type RouteType = 'route' | 'arrow' | 'phase';
export type AreaType = 'airstrike' | 'recon' | 'warning';
export type AreaShape = 'rect' | 'circle';
export type Status = 'present' | 'planned';
export type EnemyPolicy = 'off' | 'team_visible' | 'fused';
export type Scope = 'both' | 'ground' | 'air';

export interface Drawing {
  id: string;
  kind: 'point' | 'route' | 'arrow' | 'phase' | 'area' | 'circle' | 'source';
  pts: V2[];                 // canonical world
  cat?: PointCat;
  areaType?: AreaType;
  sector?: number;           // source: full sector width deg (0 = line)
  smooth?: boolean;
  label: string;
  status: Status;
  t: number;                 // battle seconds at creation
  showTime: boolean;
  by: string;
  scope: Scope;
  v: number;                 // local edit counter
  sv?: number;               // server receive time of the last write (live)
}

export interface ViewState { cx: number; cy: number; z: number; }

export interface Draft { tool: Tool; map: MapId; pts: V2[]; }

export interface Toast { id: number; text: string; undo?: boolean; tone?: 'sync' | 'info' | 'warn'; }

export interface State {
  mode: ViewMode;
  lastSingle: MapId;
  density: Density;
  tool: Tool;
  pointCat: PointCat;
  routeType: RouteType;
  areaType: AreaType;
  areaShape: AreaShape;
  sourceSector: number;
  newStatus: Status;
  smooth: boolean;
  freePan: boolean;
  drawings: Drawing[];
  selectedId: string | null;
  moving: boolean;
  draft: Draft | null;
  ruler: { map: MapId; pts: V2[] } | null;
  rulerSpeed: number;          // km/h
  hover: { map: MapId; w: V2 } | null;
  views: Record<MapId, ViewState | null>;
  policy: EnemyPolicy;
  layers: { grid: boolean; friendly: boolean; enemy: boolean; drawings: boolean; time: boolean; labels: boolean; lockDrawings: boolean };
  collab: boolean;
  sheet: null | 'settings';
  toasts: Toast[];
  undo: { kind: 'add' | 'del' | 'upd'; before?: Drawing; after?: Drawing }[];
  showMore: boolean;
  /** bumps whenever live map metadata (image / grid) changes */
  mapRev: number;
}

const LS_KEY = DEMO ? 'wt-tactical-proto-v1' : 'wt-tactical-live-v1';

function initial(): State {
  const base: State = {
    mode: 'ground', lastSingle: 'ground', density: 'command', tool: 'select',
    pointCat: 'rally', routeType: 'route', areaType: 'airstrike', areaShape: 'rect', sourceSector: 30,
    newStatus: 'present', smooth: false, freePan: true,
    drawings: [], selectedId: null, moving: false, draft: null, ruler: null, rulerSpeed: 30, hover: null,
    views: { ground: null, air: null },
    policy: 'team_visible',
    layers: { grid: true, friendly: true, enemy: true, drawings: true, time: true, labels: true, lockDrawings: false },
    collab: true, sheet: null, toasts: [], undo: [], showMore: false, mapRev: 0,
  };
  try {
    const raw = localStorage.getItem(LS_KEY);
    if (raw) {
      const p = JSON.parse(raw);
      Object.assign(base, {
        drawings: DEMO ? (p.drawings ?? base.drawings) : [], density: p.density ?? base.density,
        mode: p.mode ?? base.mode, lastSingle: p.lastSingle ?? base.lastSingle,
        policy: p.policy ?? base.policy, layers: { ...base.layers, ...(p.layers ?? {}) },
        collab: DEMO ? (p.collab ?? true) : false, views: p.views ?? base.views,
      });
    }
  } catch { /* ignore */ }
  const q = new URLSearchParams(location.search);
  const m = q.get('mode'); if (m === 'ground' || m === 'air' || m === 'dual') base.mode = m;
  const d = q.get('ui'); if (d === 'command' || d === 'member') base.density = d;
  const pol = q.get('policy'); if (pol === 'off' || pol === 'team_visible' || pol === 'fused') base.policy = pol;
  if (q.get('fresh') === '1') { base.drawings = []; base.views = { ground: null, air: null }; }
  if (q.get('collab') === '0') base.collab = false;
  return base;
}

let state: State = initial();
const listeners = new Set<() => void>();
let saveTimer: number | undefined;

export function getState() { return state; }
export function setState(patch: Partial<State> | ((s: State) => Partial<State>)) {
  const p = typeof patch === 'function' ? patch(state) : patch;
  state = { ...state, ...p };
  listeners.forEach((l) => l());
  clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    try {
      localStorage.setItem(LS_KEY, JSON.stringify({
        drawings: DEMO ? state.drawings : [], density: state.density, mode: state.mode, lastSingle: state.lastSingle,
        policy: state.policy, layers: state.layers, collab: state.collab, views: state.views,
      }));
    } catch { /* ignore */ }
  }, 300);
}
export function subscribe(l: () => void) { listeners.add(l); return () => { listeners.delete(l); }; }
export function useStore<T>(sel: (s: State) => T): T {
  return useSyncExternalStore(subscribe, () => sel(state));
}

let toastSeq = 1;
export function toast(text: string, opts: { undo?: boolean; tone?: Toast['tone'] } = {}) {
  const t: Toast = { id: toastSeq++, text, ...opts };
  setState((s) => ({ toasts: [...s.toasts.slice(-1), t] }));
  window.setTimeout(() => setState((s) => ({ toasts: s.toasts.filter((x) => x.id !== t.id) })), opts.undo ? 4200 : 2800);
}

let idSeq = Date.now() % 100000;
export function newId() { return 'ann-' + (idSeq++).toString(36); }
