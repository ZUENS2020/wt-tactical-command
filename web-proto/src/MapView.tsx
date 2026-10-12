import { useEffect, useRef } from 'react';
import { MAPS, canonToNorm, type MapId, type V2 } from './geo';
import { getState, setState, useStore, type ViewState } from './store';
import { tapMap } from './actions';
import { getSnapshot } from './data';
import { useFresh } from './TopBar';
import { DEMO } from './mode';
import { defaultView, transform, zoomAt } from './viewport';
import { hitAt, paintBase, paintDrawings, paintUnits, prepare, type Hit } from './mapRenderer';
import { iconCacheSize } from './vehicleIcons';
export { defaultView } from './viewport';
const images = new Map<string, HTMLImageElement>();
function mapImage(src: string) {
  let image = images.get(src);
  if (!image) { image = new Image(); const current = image; image.onerror = () => { setTimeout(() => { if (images.get(src) === current) images.delete(src); }, 2000); }; image.src = src; images.set(src, image); if (images.size > 8) images.delete(images.keys().next().value!); }
  return image;
}
export default function MapView({ map }: { map: MapId }) {
  const host = useRef<HTMLDivElement>(null);
  const base = useRef<HTMLCanvasElement>(null), units = useRef<HTMLCanvasElement>(null), drawing = useRef<HTMLCanvasElement>(null), preview = useRef<HTMLCanvasElement>(null);
  const hits = useRef<Hit[]>([]), size = useRef({ width: 1, height: 1, aspect: 1 });
  const active = useStore(s => s.activeMap === map), tool = useStore(s => s.tool);
  const snap = useFresh();
  const cfg = MAPS[map];
  const age = snap.mapAge?.[map] ?? (DEMO ? 0 : Infinity);
  const status = !cfg.available ? '等待地图' : age > 2000 ? '数据断流' : `${Math.round(age)} ms`;
  const view = () => getState().views[map] ?? defaultView(map);
  const setView = (v: ViewState) => setState(s => ({ views: { ...s.views, [map]: v } }));
  const activate = () => { if (getState().activeMap !== map) setState({ activeMap: map, draft: null, hover: null, selectedId: null, selectedUnit: null, moving: false }); };
  useEffect(() => {
    let frame = 0, baseKey = '', liveKey = '', drawingKey = '', previewKey = '';
    let lastSnapshot: ReturnType<typeof getSnapshot> | null = null;
    let liveHits: Hit[] = [], drawHits: Hit[] = [];
    let previousDrawings: ReturnType<typeof getState>['drawings'] | null = null;
    let lastFrame = performance.now(), context: string | undefined;
    const positions = new Map<string, V2>();
    const render = () => {
      frame = requestAnimationFrame(render);
      const el = host.current; if (!el) return;
      const width = el.clientWidth, height = el.clientHeight; if (!width || !height) return;
      const ui = getState(), snapshot = getSnapshot(), v = ui.views[map] ?? defaultView(map);
      const image = MAPS[map].available && MAPS[map].img ? mapImage(MAPS[map].img!) : null;
      const aspect = image?.naturalWidth ? image.naturalWidth / image.naturalHeight : 1;
      size.current = { width, height, aspect };
      const dpr = Math.min(3, window.devicePixelRatio || 1);
      const geometry = `${width}:${height}:${aspect}:${dpr}:${v.cx}:${v.cy}:${v.z}:${ui.mapRev}`;
      const bk = `${geometry}:${MAPS[map].available}:${MAPS[map].img}:${image?.naturalWidth}:${image?.naturalHeight}:${ui.layers.grid}`;
      const started = performance.now(); let changed = false;
      if (baseKey !== bk) { paintBase(prepare(base.current!, width, height, dpr), map, width, height, v, image, ui.layers.grid, aspect); baseKey = bk; changed = true; }
      const now = performance.now(), alpha = 1 - Math.exp(-Math.min(50, now - lastFrame) / 70); lastFrame = now;
      if (context !== MAPS[map].context) { positions.clear(); context = MAPS[map].context; }
      let animating = false; const visible = new Set<string>();
      const smooth = <T extends { id: string; w: V2; only?: MapId; air: boolean; ageMs: number }>(u: T): T => {
        if ((u.only ?? (u.air ? 'air' : 'ground')) !== map || u.ageMs > 2000) return u;
        visible.add(u.id); const previous = positions.get(u.id) ?? u.w;
        const moving = Math.hypot(u.w[0] - previous[0], u.w[1] - previous[1]) > .15;
        const w: V2 = moving ? [previous[0] + (u.w[0] - previous[0]) * alpha, previous[1] + (u.w[1] - previous[1]) * alpha] : u.w;
        positions.set(u.id, w); animating ||= moving; return { ...u, w };
      };
      const displayed = { ...snapshot, friends: snapshot.friends.map(smooth), contacts: snapshot.contacts.map(smooth) };
      for (const id of positions.keys()) if (!visible.has(id)) positions.delete(id);
      const lk = `${geometry}:${ui.selectedUnit}:${ui.layers.friendly}:${ui.layers.enemy}:${ui.layers.spawns}:${ui.layers.labels}:${ui.policy}:${iconCacheSize()}`;
      if (snapshot !== lastSnapshot || lk !== liveKey || animating) { liveHits = paintUnits(prepare(units.current!, width, height, dpr), map, width, height, v, displayed, ui, dpr, aspect); lastSnapshot = snapshot; liveKey = lk; changed = true; }
      const dk = `${geometry}:${ui.selectedId}:${ui.layers.drawings}:${ui.layers.time}:${ui.layers.time && ui.drawings.some(d => d.showTime) ? Math.floor(snapshot.battle) : 0}`;
      if (dk !== drawingKey || previousDrawings !== ui.drawings) { drawHits = paintDrawings(prepare(drawing.current!, width, height, dpr), map, width, height, v, ui, snapshot.battle, aspect, 'drawings'); drawingKey = dk; previousDrawings = ui.drawings; changed = true; }
      const pk = `${geometry}:${JSON.stringify([ui.draft, ui.hover, ui.ruler, ui.routeType, ui.areaType, ui.areaShape, ui.sourceSector, ui.newStatus, ui.rulerSpeed])}`;
      if (pk !== previewKey) { paintDrawings(prepare(preview.current!, width, height, dpr), map, width, height, v, ui, snapshot.battle, aspect, 'preview'); previewKey = pk; changed = true; }
      hits.current = [...liveHits, ...drawHits];
      if (changed) { const metrics = (window as any).__wtRenderMetrics ??= {}; const m = metrics[map] ??= { frames: 0, totalMs: 0, maxMs: 0 }; const ms = performance.now() - started; m.frames++; m.totalMs += ms; m.maxMs = Math.max(ms, m.maxMs); m.lastMs = ms; m.iconCache = iconCacheSize(); }
    };
    frame = requestAnimationFrame(render); return () => cancelAnimationFrame(frame);
  }, [map]);
  const pointers = useRef(new Map<number, V2>());
  const gesture = useRef<{ view: ViewState; start: V2; moved: boolean; distance?: number; anchor?: V2 }>({ view: view(), start: [0, 0], moved: false });
  const lastTap = useRef({ at: 0, p: [0, 0] as V2 });
  const local = (e: { clientX: number; clientY: number }): V2 => { const r = host.current!.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top]; };
  const projection = (v = view()) => transform(map, size.current.width, size.current.height, v, size.current.aspect);
  const down = (e: React.PointerEvent) => {
    if ((e.target as Element).closest('button')) return;
    activate(); e.currentTarget.setPointerCapture(e.pointerId); const p = local(e); pointers.current.set(e.pointerId, p);
    gesture.current = { view: view(), start: p, moved: false };
    if (pointers.current.size === 2) { const [a, b] = [...pointers.current.values()]; const m: V2 = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2]; gesture.current = { view: view(), start: m, moved: true, distance: Math.hypot(a[0] - b[0], a[1] - b[1]), anchor: projection().screenToNorm(m) }; }
  };
  const move = (e: React.PointerEvent) => {
    const p = local(e), ui = getState();
    if (!pointers.current.has(e.pointerId)) { if (ui.activeMap === map && (ui.draft || ui.ruler)) setState({ hover: { map, w: projection().screenToWorld(p) } }); return; }
    pointers.current.set(e.pointerId, p); const g = gesture.current;
    if (pointers.current.size === 2 && g.distance && g.anchor) {
      const [a, b] = [...pointers.current.values()], m: V2 = [(a[0] + b[0]) / 2, (a[1] + b[1]) / 2];
      const z = Math.max(.5, Math.min(64, g.view.z * Math.hypot(a[0] - b[0], a[1] - b[1]) / g.distance));
      const t = projection({ ...g.view, z }); setView({ z, cx: g.anchor[0] - (m[0] - size.current.width / 2) / t.xScale, cy: g.anchor[1] - (m[1] - size.current.height / 2) / t.yScale }); return;
    }
    if (Math.hypot(p[0] - g.start[0], p[1] - g.start[1]) > (e.pointerType === 'mouse' ? 4 : 9)) g.moved = true;
    if (g.moved && (ui.freePan || ui.tool === 'select' || ui.tool === 'layers')) { const t = projection(g.view); setView({ ...g.view, cx: g.view.cx - (p[0] - g.start[0]) / t.xScale, cy: g.view.cy - (p[1] - g.start[1]) / t.yScale }); }
  };
  const up = (e: React.PointerEvent) => {
    if (!pointers.current.has(e.pointerId)) return;
    pointers.current.delete(e.pointerId); const g = gesture.current, p = local(e);
    if (e.type === 'pointercancel' || g.moved || pointers.current.size) { g.moved = true; return; }
    const ui = getState(), hit = (ui.tool === 'select' || ui.tool === 'layers') ? hitAt(hits.current, p) : null;
    if (hit?.kind === 'unit' && !ui.moving) setState({ selectedUnit: hit.id, selectedId: null, panelOpen: true, tool: 'select' });
    else { const double = performance.now() - lastTap.current.at < 330 && Math.hypot(p[0] - lastTap.current.p[0], p[1] - lastTap.current.p[1]) < 16; tapMap(map, projection().screenToWorld(p), hit?.kind === 'drawing' ? hit.id : null, double); }
    lastTap.current = { at: performance.now(), p }; if (e.pointerType !== 'mouse') setState({ hover: null });
  };
  useEffect(() => { const el = host.current!; const wheel = (e: WheelEvent) => { e.preventDefault(); activate(); setView(zoomAt(map, size.current.width, size.current.height, view(), local(e), Math.exp(-e.deltaY * .0015), size.current.aspect)); }; el.addEventListener('wheel', wheel, { passive: false }); return () => el.removeEventListener('wheel', wheel); }, [map]);
  const zoom = (factor: number) => { activate(); setView(zoomAt(map, size.current.width, size.current.height, view(), [size.current.width / 2, size.current.height / 2], factor, size.current.aspect)); };
  const center = () => { activate(); const me = getSnapshot().friends.find(f => f.self && (f.only ?? (f.air ? 'air' : 'ground')) === map); if (me) { const n = canonToNorm(map, me.w); setView({ ...view(), cx: n[0], cy: n[1] }); } };
  return <section ref={host} className={`mapview${active ? ' active' : ''}`} data-map={map} aria-label={`${map === 'ground' ? '陆战' : '空战'}地图`} onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerCancel={up} onContextMenu={e => e.preventDefault()} style={{ cursor: tool === 'select' || tool === 'layers' ? 'grab' : 'crosshair' }}>
    <canvas ref={base} className="layer" data-layer="base" /><canvas ref={units} className="layer" data-layer="units" /><canvas ref={drawing} className="layer" data-layer="drawings" /><canvas ref={preview} className="layer" data-layer="preview" />
    <button className="map-heading" onClick={activate}><b>{map === 'ground' ? '陆战' : '空战'}</b><span>{cfg.sub}</span><i className={age > 2000 ? 'stale' : ''}>{status}</i></button>
    {!cfg.available && <div className="mapidle"><b>{map === 'ground' ? '陆战态势' : '空战态势'}</b><span>{snap.conn === 'auth' ? '在连接设置中输入房间密码' : '等待采集端回传地图'}</span></div>}
    <div className="corner"><button onClick={() => zoom(1.5)} aria-label="放大">＋</button><button onClick={() => zoom(1 / 1.5)} aria-label="缩小">−</button><button onClick={center} aria-label="居中本人">◎</button><button onClick={() => { activate(); setView(defaultView(map)); }} aria-label="复位视图">⌗</button></div>
  </section>;
}
