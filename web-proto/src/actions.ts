import { getState, setState, toast, newId, type Drawing, type Tool, type ViewMode, type State } from './store';
import { battleTime, getSnapshot } from './data';
import { MAPS, gridRef, type MapId, type V2 } from './geo';
import { CAT_LABEL } from './symbols';
import { DEMO } from './mode';
import { cfg, syncPut, syncDel, onRemote, canEdit } from './live';

let ME = DEMO ? '朔风' : cfg.nick();
export function refreshNick() { if (!DEMO) ME = cfg.nick(); }
export function myName() { return ME; }
const live = !DEMO;
function editAllowed(id?: string) {
  if (!live || canEdit(id)) return true;
  toast('同步尚未就绪，请稍后编辑', { tone: 'warn' }); return false;
}
const mapOf = (d: Drawing): MapId => (d.scope === 'air' ? 'air' : 'ground');
onRemote((k, d) => {
  if (k === 'add') toast(`${d.by || '队友'} 添加了${kindName(d)} · ${gridRef(mapOf(d), d.pts[0])}`, { tone: 'sync' });
  else toast(`${d.by || '队友'} 删除了${kindName(d)}`, { tone: 'sync' });
});
const PL_NAMES = ['PL 红', 'PL 蓝', 'PL 绿', 'PL 白'];
export const AREA_LABEL = { airstrike: '空袭区', recon: '侦察区', warning: '警戒区' } as const;
export const ROUTE_LABEL = { route: '路线', arrow: '进攻箭头', phase: '阶段线' } as const;

export function kindName(d: Drawing) {
  switch (d.kind) {
    case 'point': return CAT_LABEL[d.cat!] + '点';
    case 'route': return '路线';
    case 'arrow': return '进攻箭头';
    case 'phase': return '阶段线';
    case 'area': return AREA_LABEL[d.areaType!];
    case 'circle': return AREA_LABEL[d.areaType!] + '（圆）';
    case 'source': return '攻击来源';
  }
}

export function setMode(mode: ViewMode) {
  const s = getState();
  setState({ mode, activeMap: mode === 'dual' ? s.activeMap : mode, lastSingle: mode === 'dual' ? s.lastSingle : mode, draft: null, moving: false });
}

export function selectTool(tool: Tool) {
  const s = getState();
  setState({ panelOpen: tool !== 'select', selectedUnit: null });
  if (tool === 'layers') { setState({ tool: s.tool === 'layers' ? 'select' : 'layers', draft: null, moving: false }); return; }
  setState({ tool, draft: null, moving: false, selectedId: tool === 'select' ? s.selectedId : null, ruler: tool === 'ruler' ? s.ruler : s.ruler });
}

function pushUndo(op: State['undo'][number]) {
  setState((s) => ({ undo: [...s.undo.slice(-30), op] }));
}

export function addDrawing(d: Omit<Drawing, 'id' | 'v' | 't' | 'by' | 'scope'> & Partial<Drawing>, by = ME, silent = false) {
  const full: Drawing = { id: newId(), v: 1, t: battleTime(), by, scope: getState().activeMap, ...d } as Drawing;
  full.map_context = MAPS[mapOf(full)].context;
  if (live && by === ME && !editAllowed()) return full;
  setState((s) => ({ drawings: [...s.drawings, full] }));
  if (by === ME) pushUndo({ kind: 'add', after: full });
  if (live && by === ME) syncPut(full);
  if (!silent && by === ME) toast(`已添加${kindName(full)} · ${gridRef(mapOf(full), full.pts[0])}`);
  return full;
}

export function updateDrawing(id: string, patch: Partial<Drawing>, record = true) {
  const s = getState();
  const before = s.drawings.find((d) => d.id === id);
  if (!before || !editAllowed(id)) return;
  const after = { ...before, ...patch, v: before.v + 1 };
  setState({ drawings: s.drawings.map((d) => (d.id === id ? after : d)) });
  if (record) pushUndo({ kind: 'upd', before, after });
  if (live) syncPut(after);
}

export function deleteDrawing(id: string, by = ME) {
  const s = getState();
  const before = s.drawings.find((d) => d.id === id);
  if (!before || !editAllowed(id)) return;
  setState({ drawings: s.drawings.filter((d) => d.id !== id), selectedId: s.selectedId === id ? null : s.selectedId, moving: false });
  if (live && by === ME) syncDel(id, ME, before.sv);
  if (by === ME) { pushUndo({ kind: 'del', before }); toast(`已删除${kindName(before)}`, { undo: true }); }
}

export function undo() {
  const s = getState();
  const op = s.undo[s.undo.length - 1];
  if (!op) { toast('没有可撤销的操作'); return; }
  const id = op.after?.id ?? op.before?.id;
  if (!editAllowed(id)) return;
  const current = s.drawings.find(d => d.id === id);
  if (live && op.kind !== 'del' && current?.sv !== op.after?.sv) { toast('该标绘已被队友修改，无法撤销', { tone: 'warn' }); return; }
  setState({ undo: s.undo.slice(0, -1) });
  const ds = getState().drawings;
  if (op.kind === 'add' && op.after) { setState({ drawings: ds.filter((d) => d.id !== op.after!.id), selectedId: null }); if (live) syncDel(op.after.id, ME); }
  if (op.kind === 'del' && op.before) { setState({ drawings: [...ds, op.before] }); if (live) syncPut(op.before); }
  if (op.kind === 'upd' && op.before) { setState({ drawings: ds.map((d) => (d.id === op.before!.id ? op.before! : d)) }); if (live) syncPut({ ...op.before, sv: current?.sv }); }
  toast('已撤销');
}

export function cancelDraft() { setState({ draft: null }); }
export function undoPoint() {
  const s = getState();
  if (s.tool === 'ruler' && s.ruler) { setState({ ruler: s.ruler.pts.length > 1 ? { ...s.ruler, pts: s.ruler.pts.slice(0, -1) } : null }); return; }
  if (!s.draft) return;
  const pts = s.draft.pts.slice(0, -1);
  setState({ draft: pts.length ? { ...s.draft, pts } : null });
}
export function finishDraft() {
  const s = getState();
  const d = s.draft;
  if (!d || !editAllowed()) return;
  if (s.tool === 'route') {
    if (d.pts.length < 2) { toast('至少需要两个点'); return; }
    const kind = s.routeType;
    const nPhase = s.drawings.filter((x) => x.kind === 'phase').length;
    addDrawing({ kind, scope: d.map, pts: d.pts, label: kind === 'phase' ? PL_NAMES[nPhase % PL_NAMES.length] : '', status: s.newStatus, showTime: false, smooth: kind !== 'phase' && s.smooth });
    setState({ draft: null });
  }
}

/** a tap (not a drag) on the map. hitId = topmost drawing under the finger. */
export function tapMap(map: MapId, w: V2, hitId: string | null, dbl = false) {
  const s = getState();
  if (s.tool !== 'select' && s.tool !== 'layers' && s.tool !== 'ruler' && !editAllowed()) return;
  switch (s.tool) {
    case 'select':
    case 'layers': {
      if (s.moving && s.selectedId) {
        const d = s.drawings.find((x) => x.id === s.selectedId);
        if (d) {
          const anchor = d.kind === 'point' || d.kind === 'source' || d.kind === 'circle' ? d.pts[0] : centroid(d.pts);
          const dx = w[0] - anchor[0], dy = w[1] - anchor[1];
          updateDrawing(d.id, { pts: d.pts.map((p) => [p[0] + dx, p[1] + dy] as V2) });
          toast(`已移动到 ${gridRef(map, w)}`);
        }
        setState({ moving: false });
        return;
      }
      if (hitId && s.layers.lockDrawings) { toast('标绘图层已锁定（图层 → 解锁）'); return; }
      setState({ selectedId: hitId, panelOpen: !!hitId, selectedUnit: null, showMore: false, tool: hitId && s.tool === 'layers' ? 'select' : s.tool });
      return;
    }
    case 'point': {
      const cat = s.pointCat;
      const d = addDrawing({ kind: 'point', scope: map, cat, pts: [w], label: '', status: s.newStatus, showTime: cat === 'enemy' || cat === 'danger' });
      setState({ selectedId: d.id });
      return;
    }
    case 'route': {
      const pts = s.draft?.map === map ? s.draft.pts : [];
      if (dbl && pts.length >= 2) { finishDraft(); return; }
      // tapping the last point again finishes
      setState({ draft: { tool: 'route', map, pts: [...pts, w] } });
      return;
    }
    case 'area': {
      if (!s.draft) { setState({ draft: { tool: 'area', map, pts: [w] } }); return; }
      const p0 = s.draft.pts[0];
      if (s.areaShape === 'rect') {
        addDrawing({ kind: 'area', scope: map, areaType: s.areaType, pts: [p0, w], label: AREA_LABEL[s.areaType], status: s.newStatus, showTime: false });
      } else {
        addDrawing({ kind: 'circle', scope: map, areaType: s.areaType, pts: [p0, w], label: AREA_LABEL[s.areaType], status: s.newStatus, showTime: false });
      }
      setState({ draft: null });
      return;
    }
    case 'source': {
      if (!s.draft) { setState({ draft: { tool: 'source', map, pts: [w] } }); return; }
      addDrawing({ kind: 'source', scope: map, pts: [s.draft.pts[0], w], sector: s.sourceSector, label: '未确认', status: 'present', showTime: true });
      setState({ draft: null });
      return;
    }
    case 'ruler': {
      const r = s.ruler && s.ruler.map === map ? s.ruler.pts : [];
      setState({ ruler: { map, pts: [...r, w] } });
      return;
    }
  }
}

export function centroid(pts: V2[]): V2 {
  const n = pts.length;
  return [pts.reduce((a, p) => a + p[0], 0) / n, pts.reduce((a, p) => a + p[1], 0) / n];
}

// ---------- seeded room content (as if already drawn before you joined) ----------
const g = (x: number, y: number): V2 => [x * 4096, y * 4096];
export function seedIfEmpty() {
  if (live) return;
  const s = getState();
  if (s.drawings.length) return;
  const mk = (d: Partial<Drawing>): Drawing => ({ id: newId(), v: 1, t: 340, by: '死神', scope: 'ground', label: '', status: 'present', showTime: false, pts: [], kind: 'point', ...d } as Drawing);
  setState({
    drawings: [
      mk({ kind: 'point', cat: 'rally', pts: [g(0.402, 0.553)], label: '集合 A', t: 301 }),
      mk({ kind: 'phase', pts: [g(0.523, 0.432), g(0.531, 0.548), g(0.522, 0.70)], label: 'PL 红', status: 'planned', by: '铁砧', t: 318 }),
      mk({ kind: 'arrow', pts: [g(0.425, 0.648), g(0.488, 0.664), g(0.566, 0.628)], label: '主攻', status: 'planned', by: '铁砧', t: 322 }),
      mk({ kind: 'area', areaType: 'recon', pts: [g(0.545, 0.405), g(0.612, 0.468)], label: '侦察区', by: '夜枭', t: 335 }),
      mk({ kind: 'point', cat: 'enemy', pts: [g(0.618, 0.532)], label: '虎王?', showTime: true, by: '夜枭', t: 331 }),
    ],
  });
}

// ---------- simulated second user ----------
let collabStep = 0;
export function startCollab() {
  if (live) return;
  const run = () => {
    const s = getState();
    if (s.collab) collabAction();
    window.setTimeout(run, 21000 + Math.random() * 12000);
  };
  window.setTimeout(run, 9000);
}
function collabAction() {
  const snap = getSnapshot();
  const sishen = snap.friends.find((f) => f.id === 'm2');
  const tiezhen = snap.friends.find((f) => f.id === 'm3');
  const cont = snap.contacts.filter((c) => c.visible && !c.air && c.id.startsWith('g'));
  const step = collabStep++ % 5;
  const s = getState();
  const mine = (by: string) => s.drawings.filter((d) => d.by === by);
  if (step === 0 && sishen) {
    const tgt = cont[2]?.w ?? g(0.57, 0.55);
    const a = sishen.w;
    const mid: V2 = [(a[0] + tgt[0]) / 2 + 60, (a[1] + tgt[1]) / 2 - 90];
    const d = addDrawing({ kind: 'route', scope: 'ground', pts: [a, mid, [tgt[0] - 50, tgt[1] + 30]], label: '', status: 'planned', showTime: false }, '死神');
    toast(`死神 添加了进攻路线 · ${gridRef('ground', d.pts[2])}`, { tone: 'sync' });
  } else if (step === 1) {
    const c = cont[Math.floor(Math.random() * cont.length)];
    const p: V2 = c ? [c.w[0] + 18, c.w[1] - 12] : g(0.6, 0.6);
    const d = addDrawing({ kind: 'point', scope: 'ground', cat: 'enemy', pts: [p], label: '', status: 'present', showTime: true }, '夜枭');
    toast(`夜枭 标记了敌军 · ${gridRef('ground', d.pts[0])}`, { tone: 'sync' });
  } else if (step === 2) {
    const d = addDrawing({ kind: 'area', scope: 'ground', areaType: 'airstrike', pts: [g(0.592, 0.515), g(0.632, 0.56)], label: '空袭区', status: 'planned', showTime: false }, '死神');
    toast(`死神 添加了空袭区 · ${gridRef('ground', d.pts[0])}`, { tone: 'sync' });
  } else if (step === 3 && tiezhen) {
    const p = tiezhen.w;
    const d = addDrawing({ kind: 'source', scope: 'ground', pts: [p, [p[0] + 520, p[1] - 160]], sector: 30, label: '未确认', status: 'present', showTime: true }, '铁砧');
    toast(`铁砧 报告了攻击来源 · ${gridRef('ground', d.pts[0])}`, { tone: 'warn' });
  } else {
    const old = [...mine('死神'), ...mine('夜枭'), ...mine('铁砧')].filter((d) => d.t > 360).sort((a, b) => a.t - b.t)[0];
    if (old) {
      setState((st) => ({ drawings: st.drawings.filter((d) => d.id !== old.id), selectedId: st.selectedId === old.id ? null : st.selectedId }));
      toast(`${old.by} 删除了${kindName(old)}`, { tone: 'sync' });
    }
  }
}
