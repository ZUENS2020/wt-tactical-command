import { useEffect, useMemo, useRef, useState, useCallback } from 'react';
import { MAPS, canonToNorm, normToCanon, gridBox, rowLetter, metersPerNorm, metersPerCanon, bearingDeg, dist, fmtDist, fmtBearing, fmtClock, separateMaps, type MapId, type V2 } from './geo';
import { getState, setState, useStore, type Drawing, type ViewState } from './store';
import { tapMap } from './actions';
import { getSnapshot, ICON_SIDC, ICON_ZH, battleTime } from './data';
import { useFresh } from './TopBar';
import { DEMO } from './mode';
import { unitSprite, PointGlyph } from './symbols';
import { sampleSmooth, polyD, headD, attackArrowD, polyDist, wedgeD, trimEnd } from './draw';
import { C } from './theme';

const imgCache = new Map<string, HTMLImageElement>();
function getImg(src: string, onload: () => void) {
  let im = imgCache.get(src);
  if (!im) {
    im = new Image(); im.src = src; imgCache.set(src, im);
    // live map images may be uploaded a moment after the map switch: retry
    const me = im;
    im.addEventListener('error', () => window.setTimeout(() => { if (imgCache.get(src) === me) imgCache.delete(src); onload(); }, 2500), { once: true });
  }
  if (!im.complete) im.addEventListener('load', onload, { once: true });
  return im;
}

export function defaultView(map: MapId): ViewState {
  const b = gridBox(map);
  return { cx: b.x0 + b.w / 2, cy: b.y0 + b.h / 2, z: 1 / (Math.max(b.w, b.h) * 1.06) };
}
const ZMAX: Record<MapId, number> = { ground: 28, air: 90 };
function clampView(v: ViewState): ViewState {
  const z = Math.min(ZMAX.ground * 4, Math.max(1, v.z));
  const h = 0.5 / z;
  const cx = Math.min(Math.max(v.cx, h), 1 - h);
  const cy = Math.min(Math.max(v.cy, h), 1 - h);
  return { cx: z <= 1 ? 0.5 : cx, cy: z <= 1 ? 0.5 : cy, z };
}

interface Props { map: MapId; size: number; interactive: boolean; dual: boolean; feed?: boolean; }

export default function MapView({ map, size: S, interactive, dual, feed }: Props) {
  const baseRef = useRef<HTMLCanvasElement>(null);
  const liveRef = useRef<HTMLCanvasElement>(null);
  const [view, setViewLocal] = useState<ViewState>(() => clampView(getState().views[map] ?? defaultView(map)));
  const viewRef = useRef(view); viewRef.current = view;
  const [, force] = useState(0);
  const dpr = Math.min(2.5, window.devicePixelRatio || 1);
  const drawings = useStore((s) => s.drawings);
  const layers = useStore((s) => s.layers);
  const selectedId = useStore((s) => s.selectedId);
  const draft = useStore((s) => s.draft);
  const ruler = useStore((s) => s.ruler);
  const hover = useStore((s) => s.hover);
  const tool = useStore((s) => s.tool);
  const moving = useStore((s) => s.moving);
  const rulerSpeed = useStore((s) => s.rulerSpeed);
  useStore((s) => s.mapRev);
  const cfg = MAPS[map];
  const compact = S < 330;

  const setView = useCallback((v: ViewState) => {
    const c = clampView(v);
    setViewLocal(c);
    setState((s) => ({ views: { ...s.views, [map]: c } }));
  }, [map]);

  // projections
  const toS = useCallback((n: V2, v = viewRef.current): V2 => [(n[0] - v.cx) * S * v.z + S / 2, (n[1] - v.cy) * S * v.z + S / 2], [S]);
  const fromS = useCallback((p: V2, v = viewRef.current): V2 => [(p[0] - S / 2) / (S * v.z) + v.cx, (p[1] - S / 2) / (S * v.z) + v.cy], [S]);
  const wToS = useCallback((w: V2, v = viewRef.current) => toS(canonToNorm(map, w), v), [toS, map]);

  // ---------- base layer: image + grid ----------
  useEffect(() => {
    const cv = baseRef.current; if (!cv) return;
    cv.width = Math.round(S * dpr); cv.height = Math.round(S * dpr);
    const ctx = cv.getContext('2d')!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.fillStyle = '#d9d6ca'; ctx.fillRect(0, 0, S, S);
    if (!cfg.available) return;
    const im = cfg.img ? getImg(cfg.img, () => force((x) => x + 1)) : null;
    const v = view;
    const o = toS([0, 0], v), full = S * v.z;
    if (im && im.complete && im.naturalWidth) {
      // source rect for visible part (keeps huge zooms cheap); proportional only
      const n0 = fromS([0, 0], v), n1 = fromS([S, S], v);
      const sx0 = Math.max(0, n0[0]), sy0 = Math.max(0, n0[1]), sx1 = Math.min(1, n1[0]), sy1 = Math.min(1, n1[1]);
      const W = im.naturalWidth;
      if (sx1 > sx0 && sy1 > sy0) {
        ctx.imageSmoothingQuality = 'high';
        ctx.drawImage(im, sx0 * W, sy0 * W, (sx1 - sx0) * W, (sy1 - sy0) * W, o[0] + sx0 * full, o[1] + sy0 * full, (sx1 - sx0) * full, (sy1 - sy0) * full);
      }
    }
    const b = gridBox(map);
    const bx0 = toS([b.x0, b.y0], v), bx1 = toS([b.x0 + b.w, b.y0 + b.h], v);
    // dim outside the battle grid
    ctx.save();
    ctx.beginPath(); ctx.rect(0, 0, S, S); ctx.rect(bx0[0], bx0[1], bx1[0] - bx0[0], bx1[1] - bx0[1]);
    ctx.fillStyle = 'rgba(78,80,68,0.20)'; ctx.fill('evenodd');
    ctx.restore();
    if (layers.grid) {
      ctx.strokeStyle = 'rgba(38,44,34,0.40)'; ctx.lineWidth = 1;
      ctx.beginPath();
      for (let i = 0; i <= b.cols; i++) {
        const x = Math.round(toS([Math.min(b.x0 + i * b.stepX, b.x0 + b.w), 0], v)[0]) + 0.5;
        ctx.moveTo(x, Math.max(0, bx0[1])); ctx.lineTo(x, Math.min(S, bx1[1]));
      }
      for (let j = 0; j <= b.rows; j++) {
        const y = Math.round(toS([0, Math.min(b.y0 + j * b.stepY, b.y0 + b.h)], v)[1]) + 0.5;
        ctx.moveTo(Math.max(0, bx0[0]), y); ctx.lineTo(Math.min(S, bx1[0]), y);
      }
      ctx.stroke();
      ctx.strokeStyle = 'rgba(38,44,34,0.65)'; ctx.lineWidth = 1.4;
      ctx.strokeRect(bx0[0], bx0[1], bx1[0] - bx0[0], bx1[1] - bx0[1]);
      // WT style labels: letters (rows) on the left, numbers (cols) on top
      const fs = compact ? 10 : 12;
      ctx.font = `700 ${fs}px "Noto Sans SC", "PingFang SC", "Microsoft YaHei", Arial, sans-serif`;
      ctx.textBaseline = 'middle'; ctx.lineJoin = 'round';
      const cellPx = b.stepX * S * v.z;
      const lx = Math.min(Math.max(bx0[0], 0), S) + (compact ? 4 : 6);
      const ty = Math.min(Math.max(bx0[1], 0), S) + (compact ? 7 : 9);
      const lab = (t: string, x: number, y: number, align: CanvasTextAlign) => {
        ctx.textAlign = align;
        ctx.strokeStyle = 'rgba(244,242,233,0.9)'; ctx.lineWidth = 3; ctx.strokeText(t, x, y);
        ctx.fillStyle = '#2d3528'; ctx.fillText(t, x, y);
      };
      if (cellPx > 16) {
        for (let j = 0; j < b.rows; j++) {
          const yc = toS([0, b.y0 + (j + 0.5) * b.stepY], v)[1];
          if (yc > ty + 8 && yc < S - 4 && yc < bx1[1]) lab(rowLetter(j), lx, yc, 'left');
        }
        for (let i = 0; i < b.cols; i++) {
          const xc = toS([b.x0 + (i + 0.5) * b.stepX, 0], v)[0];
          if (xc > lx + 14 && xc < S - 4 && xc < bx1[0]) lab(String(i + 1), xc, ty, 'center');
        }
      }
    }
    if (map === 'air' && !separateMaps()) {
      // ground battle area (mock ground→air transform) as dashed outline on the air overview
      const gb = gridBox('ground');
      const a = toS(canonToNorm('air', [gb.x0 * 4096, gb.y0 * 4096]), v), c2 = toS(canonToNorm('air', [(gb.x0 + gb.w) * 4096, (gb.y0 + gb.h) * 4096]), v);
      ctx.save(); ctx.setLineDash([5, 4]); ctx.strokeStyle = 'rgba(43,50,38,0.75)'; ctx.lineWidth = 1.3;
      ctx.strokeRect(a[0], a[1], c2[0] - a[0], c2[1] - a[1]); ctx.restore();
      ctx.font = `600 10px "Noto Sans SC", Arial, sans-serif`; ctx.textAlign = 'left'; ctx.textBaseline = 'bottom';
      ctx.strokeStyle = 'rgba(244,242,233,0.9)'; ctx.lineWidth = 3; ctx.strokeText('陆战区', a[0] + 2, a[1] - 2);
      ctx.fillStyle = '#2d3528'; ctx.fillText('陆战区', a[0] + 2, a[1] - 2);
    }
    // scale bar (bottom-left)
    const mpp = metersPerNorm(map) / (S * v.z);
    const target = (compact ? 56 : 80) * mpp;
    const pow = Math.pow(10, Math.floor(Math.log10(target)));
    const nice = [1, 2, 5, 10].map((k) => k * pow).reduce((a, c) => (Math.abs(c - target) < Math.abs(a - target) ? c : a));
    const px = nice / mpp;
    const sx = 8, sy = dual ? S - 30 : S - 10;
    ctx.strokeStyle = 'rgba(244,242,233,0.9)'; ctx.lineWidth = 4;
    ctx.beginPath(); ctx.moveTo(sx, sy - 4); ctx.lineTo(sx, sy); ctx.lineTo(sx + px, sy); ctx.lineTo(sx + px, sy - 4); ctx.stroke();
    ctx.strokeStyle = '#2d3528'; ctx.lineWidth = 1.5; ctx.stroke();
    ctx.font = `600 10px Arial, sans-serif`; ctx.textAlign = 'left'; ctx.textBaseline = 'bottom';
    ctx.strokeStyle = 'rgba(244,242,233,0.9)'; ctx.lineWidth = 3; ctx.strokeText(fmtDist(nice), sx + 3, sy - 3);
    ctx.fillStyle = '#2d3528'; ctx.fillText(fmtDist(nice), sx + 3, sy - 3);
  });

  // ---------- live layer (canvas, rAF) ----------
  useEffect(() => {
    let raf = 0;
    const cv = liveRef.current!; cv.width = Math.round(S * dpr); cv.height = Math.round(S * dpr);
    const ctx = cv.getContext('2d')!;
    const frame = () => {
      raf = requestAnimationFrame(frame);
      drawLive(ctx, S, dpr, map, viewRef.current, compact);
    };
    raf = requestAnimationFrame(frame);
    return () => cancelAnimationFrame(raf);
  }, [S, dpr, map, compact]);

  // ---------- interaction ----------
  const ptrs = useRef(new Map<number, { x: number; y: number; type: string }>());
  const gesture = useRef<{ kind: 'none' | 'tap' | 'pan' | 'pinch' | 'dead'; sx: number; sy: number; v0: ViewState; d0?: number; m0?: V2; n0?: V2; t: number }>({ kind: 'none', sx: 0, sy: 0, v0: view, t: 0 });
  const lastTap = useRef({ t: 0, x: 0, y: 0 });
  const elRef = useRef<HTMLDivElement>(null);

  const local = (e: { clientX: number; clientY: number }): V2 => {
    const r = elRef.current!.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  };
  const panAllowed = () => {
    const s = getState();
    if (!interactive) return true;
    if (s.freePan) return true;
    return s.tool === 'select' || s.tool === 'layers';
  };

  const onDown = (e: React.PointerEvent) => {
    (e.target as Element).setPointerCapture?.(e.pointerId);
    const p = local(e);
    ptrs.current.set(e.pointerId, { x: p[0], y: p[1], type: e.pointerType });
    const g = gesture.current;
    if (ptrs.current.size === 2) {
      const [a, b] = [...ptrs.current.values()];
      const m: V2 = [(a.x + b.x) / 2, (a.y + b.y) / 2];
      gesture.current = { kind: 'pinch', sx: m[0], sy: m[1], v0: viewRef.current, d0: Math.hypot(a.x - b.x, a.y - b.y), m0: m, n0: fromS(m), t: Date.now() };
    } else if (ptrs.current.size === 1) {
      gesture.current = { kind: 'tap', sx: p[0], sy: p[1], v0: viewRef.current, t: Date.now() };
    } else { g.kind = 'dead'; }
  };
  const onMove = (e: React.PointerEvent) => {
    const p = local(e);
    if (interactive && e.pointerType === 'mouse' && ptrs.current.size === 0) {
      const s = getState();
      if (s.draft || (s.tool === 'ruler' && s.ruler)) setState({ hover: { map, w: normToCanon(map, fromS(p)) } });
    }
    if (!ptrs.current.has(e.pointerId)) return;
    ptrs.current.set(e.pointerId, { x: p[0], y: p[1], type: e.pointerType });
    const g = gesture.current;
    if (g.kind === 'pinch' && ptrs.current.size >= 2) {
      const [a, b] = [...ptrs.current.values()];
      const m: V2 = [(a.x + b.x) / 2, (a.y + b.y) / 2];
      const d = Math.hypot(a.x - b.x, a.y - b.y);
      const z = Math.min(ZMAX[map], Math.max(1, g.v0.z * d / (g.d0 || 1)));
      setView({ z, cx: g.n0![0] - (m[0] - S / 2) / (S * z), cy: g.n0![1] - (m[1] - S / 2) / (S * z) });
      return;
    }
    const moved = Math.hypot(p[0] - g.sx, p[1] - g.sy);
    const thr = e.pointerType === 'mouse' ? 4 : 9;
    if (g.kind === 'tap' && moved > thr) g.kind = panAllowed() ? 'pan' : 'dead';
    if (g.kind === 'pan') {
      const v0 = g.v0;
      setView({ ...v0, cx: v0.cx - (p[0] - g.sx) / (S * v0.z), cy: v0.cy - (p[1] - g.sy) / (S * v0.z) });
    }
  };
  const onUp = (e: React.PointerEvent) => {
    const p = local(e);
    const g = gesture.current;
    ptrs.current.delete(e.pointerId);
    if (g.kind === 'pinch') { if (ptrs.current.size === 0) g.kind = 'none'; else g.kind = 'dead'; return; }
    if (g.kind === 'tap' && ptrs.current.size === 0) {
      g.kind = 'none';
      if (!interactive) return;
      const now = Date.now();
      const dbl = now - lastTap.current.t < 330 && Math.hypot(p[0] - lastTap.current.x, p[1] - lastTap.current.y) < 14;
      lastTap.current = { t: now, x: p[0], y: p[1] };
      const w = normToCanon(map, fromS(p));
      const s = getState();
      const hit = s.tool === 'select' || s.tool === 'layers' ? hitTest(s.drawings, map, p, wToS, s.layers.drawings) : null;
      tapMap(map, w, hit, dbl);
      if (e.pointerType !== 'mouse') setState({ hover: null });
    }
    if (ptrs.current.size === 0) g.kind = 'none';
  };
  const onWheel = (e: React.WheelEvent) => {
    const p = local(e);
    const v = viewRef.current;
    const n = fromS(p);
    const z = Math.min(ZMAX[map], Math.max(1, v.z * Math.pow(1.0018, -e.deltaY)));
    setView({ z, cx: n[0] - (p[0] - S / 2) / (S * z), cy: n[1] - (p[1] - S / 2) / (S * z) });
  };
  useEffect(() => {
    const el = elRef.current; if (!el) return;
    const prevent = (ev: WheelEvent) => ev.preventDefault();
    el.addEventListener('wheel', prevent, { passive: false });
    return () => el.removeEventListener('wheel', prevent);
  }, []);

  const zoomBy = (k: number) => { const v = viewRef.current; setView({ ...v, z: Math.min(ZMAX[map], Math.max(1, v.z * k)) }); };
  const centerSelf = () => {
    const me = getSnapshot().friends.find((f) => f.self && (!f.only || f.only === map));
    if (!me) return;
    const n = canonToNorm(map, me.w);
    const v = viewRef.current;
    setView({ cx: n[0], cy: n[1], z: Math.max(v.z, defaultView(map).z * 1.6) });
  };
  const reset = () => setView(defaultView(map));

  const btn = S < 460 ? 26 : 30;
  const cursor = interactive && tool !== 'select' && tool !== 'layers' ? 'crosshair' : (moving ? 'copy' : 'default');

  return (
    <div className="mapview" style={{ width: S, height: S }} data-map={map}>
      <canvas ref={baseRef} className="layer" style={{ width: S, height: S }} />
      <DrawingsSvg map={map} S={S} view={view} drawings={drawings} wToS={wToS} layers={layers} selectedId={interactive ? selectedId : null} compact={compact}
        draft={interactive && draft && draft.map === map ? draft : null}
        ruler={interactive && ruler && ruler.map === map ? ruler.pts : null}
        hover={interactive && hover && hover.map === map ? hover.w : null}
        rulerSpeed={rulerSpeed} />
      <canvas ref={liveRef} className="layer" style={{ width: S, height: S, pointerEvents: 'none' }} />
      <div ref={elRef} className="layer hit" style={{ cursor }}
        onPointerDown={onDown} onPointerMove={onMove} onPointerUp={onUp} onPointerCancel={onUp} onWheel={onWheel}
        onPointerLeave={(e) => { if (e.pointerType === 'mouse' && ptrs.current.size === 0) setState({ hover: null }); }}
        onContextMenu={(e) => e.preventDefault()} />
      {dual && <div className="mapchip">{cfg.title}<span>{cfg.available ? cfg.sub : '—'}</span></div>}
      {!DEMO && <MapStatus map={map} />}
      {feed && <FeedOverlay compact={compact} />}
      <div className="corner" style={{ gap: compact ? 4 : 6 }}>
        <button className="cbtn" style={{ width: btn, height: btn }} onClick={() => zoomBy(1.5)} aria-label="放大" title="放大">
          <svg viewBox="0 0 20 20"><path d="M10 4v12M4 10h12" /></svg></button>
        <button className="cbtn" style={{ width: btn, height: btn }} onClick={() => zoomBy(1 / 1.5)} aria-label="缩小" title="缩小">
          <svg viewBox="0 0 20 20"><path d="M4 10h12" /></svg></button>
        <button className="cbtn" style={{ width: btn, height: btn }} onClick={centerSelf} aria-label="居中本人" title="居中本人">
          <svg viewBox="0 0 20 20"><circle cx="10" cy="10" r="4.5" /><path d="M10 1.5v4M10 14.5v4M1.5 10h4M14.5 10h4" /></svg></button>
        <button className="cbtn" style={{ width: btn, height: btn }} onClick={reset} aria-label="复位视图" title="复位视图">
          <svg viewBox="0 0 20 20"><path d="M3 7V3h4M17 7V3h-4M3 13v4h4M17 13v4h-4" /></svg></button>
      </div>
    </div>
  );
}

// ---------- hit testing (screen space) ----------
function hitTest(ds: Drawing[], map: MapId, p: V2, wToS: (w: V2) => V2, visible: boolean): string | null {
  if (!visible) return null;
  let best: { id: string; d: number; pri: number } | null = null;
  const consider = (id: string, d: number, pri: number) => {
    if (!best || pri < best.pri || (pri === best.pri && d < best.d)) best = { id, d, pri };
  };
  for (const d of ds) {
    if (d.scope !== 'both' && d.scope !== map) continue;
    const P = d.pts.map((w) => wToS(w));
    if (d.kind === 'point') { const dd = Math.hypot(P[0][0] - p[0], P[0][1] - p[1]); if (dd < 20) consider(d.id, dd, 0); }
    else if (d.kind === 'source') { const dd = Math.min(Math.hypot(P[0][0] - p[0], P[0][1] - p[1]), polyDist(p, P)); if (dd < 14) consider(d.id, dd, 1); }
    else if (d.kind === 'route' || d.kind === 'arrow' || d.kind === 'phase') {
      const dd = polyDist(p, d.smooth ? sampleSmooth(P) : P); if (dd < (d.kind === 'arrow' ? 16 : 12)) consider(d.id, dd, 1);
    } else if (d.kind === 'area') {
      const x0 = Math.min(P[0][0], P[1][0]), x1 = Math.max(P[0][0], P[1][0]), y0 = Math.min(P[0][1], P[1][1]), y1 = Math.max(P[0][1], P[1][1]);
      const inside = p[0] >= x0 && p[0] <= x1 && p[1] >= y0 && p[1] <= y1;
      const edge = Math.min(Math.abs(p[0] - x0), Math.abs(p[0] - x1), Math.abs(p[1] - y0), Math.abs(p[1] - y1));
      if (inside) consider(d.id, edge < 12 ? edge : 30 + edge, edge < 12 ? 1 : 2);
    } else if (d.kind === 'circle') {
      const r = Math.hypot(P[1][0] - P[0][0], P[1][1] - P[0][1]);
      const dc = Math.hypot(p[0] - P[0][0], p[1] - P[0][1]);
      if (dc <= r + 10) consider(d.id, Math.abs(dc - r), Math.abs(dc - r) < 12 ? 1 : 2);
    }
  }
  return (best as { id: string } | null)?.id ?? null;
}

// ---------- symbol scale ----------
// v2: symbols ~60 % of v1 at the default zoom, growing gently with zoom (≈95 % at 5×) so they stay readable without covering the map.
export function symK(z: number) { return Math.max(0.6, Math.min(1, 0.6 * Math.pow(Math.max(1, z), 0.3))); }
/** text shrinks less than symbols */
export function txtK(z: number) { return Math.max(0.8, Math.min(1, 0.8 * Math.pow(Math.max(1, z), 0.15))); }

// ---------- SVG drawings ----------
function areaColor(t?: string) { return t === 'recon' ? C.recon : t === 'warning' ? C.enemy : C.order; }

function DrawingsSvg(props: {
  map: MapId; S: number; view: ViewState; drawings: Drawing[]; wToS: (w: V2, v?: ViewState) => V2; layers: ReturnType<typeof getState>['layers'];
  selectedId: string | null; compact: boolean; draft: { tool: string; pts: V2[] } | null; ruler: V2[] | null; hover: V2 | null; rulerSpeed: number;
}) {
  const { map, S, view, drawings, wToS, layers, selectedId, compact, draft, ruler, hover, rulerSpeed } = props;
  const [, tick] = useState(0);
  useEffect(() => { const id = setInterval(() => tick((x) => x + 1), 1000); return () => clearInterval(id); }, []);
  const bt = battleTime();
  const s = getState();
  const ov = !separateMaps() && gridBox('ground').w * (map === 'ground' ? 1 : 0.1875) * S * view.z < 240;
  const sk = symK(view.z), tk = txtK(view.z);
  const fs = Math.max(8, Math.round((compact ? 10 : S >= 600 ? 12 : 11) * tk));
  const hw = (ov ? 3.5 : compact ? 6 : S >= 600 ? 9 : 7.5) * sk;
  const gs = (ov ? 0.5 : compact ? 0.82 : S >= 600 ? 1.12 : 1) * sk;
  const po = (compact ? 22 : 26) * sk + 2;  // point label offset
  const items = useMemo(() => drawings.filter((d) => d.scope === 'both' || d.scope === map), [drawings, map]);
  const text = (x: number, y: number, t: string, col: string, anchor: 'start' | 'middle' | 'end' = 'middle', size = fs, weight = 600) => (
    <text x={x} y={y} textAnchor={anchor} fontSize={size} fontWeight={weight} fill={col} className="halo">{t}</text>
  );

  const els: JSX.Element[] = [];
  if (layers.drawings) for (const d of items) {
    const P = d.pts.map((w) => wToS(w, view));
    const sel = d.id === selectedId;
    const dash = d.status === 'planned' ? '7 5' : undefined;
    const key = d.id;
    const age = bt - d.t;
    const timeTxt = layers.time && d.showTime ? (d.kind === 'point' && d.cat === 'enemy' ? '发现 ' : d.kind === 'source' ? '' : '') + fmtClock(d.t) : null;
    const showLabels = layers.labels && !ov;
    if (d.kind === 'point') {
      const [x, y] = P[0];
      if (x < -30 || y < -30 || x > S + 30 || y > S + 30) continue;
      const fade = d.cat === 'enemy' ? Math.max(0.38, 1 - Math.max(0, age - 30) / 240) : 1;
      els.push(
        <g key={key} transform={`translate(${x.toFixed(1)} ${y.toFixed(1)})`} opacity={fade}>
          {sel && <circle r={20 * gs + 2} fill="rgba(30,34,28,0.08)" stroke={C.ink} strokeWidth={1.4} strokeDasharray="3 3" />}
          <PointGlyph cat={d.cat!} status={d.status} s={gs} />
          {showLabels && d.label && text(0, po, d.label, C.ink)}
          {timeTxt && !ov && text(0, (d.label && showLabels ? po + fs + 1 : po), timeTxt + (age > 120 ? ' · 陈旧' : ''), d.cat === 'enemy' ? C.enemy : C.ink, 'middle', fs - 1, 600)}
        </g>,
      );
    } else if (d.kind === 'route' || d.kind === 'phase' || d.kind === 'arrow') {
      const Q = d.smooth && d.kind !== 'phase' ? sampleSmooth(P) : P;
      if (d.kind === 'arrow') {
        const dd = attackArrowD(Q, hw);
        els.push(<g key={key}>
          {sel && <path d={dd} fill="none" stroke={C.ink} strokeWidth={5} strokeOpacity={0.25} strokeLinejoin="round" />}
          <path d={dd} fill={C.order} fillOpacity={0.18} stroke={C.order} strokeWidth={1.6} strokeDasharray={dash} strokeLinejoin="round" />
          {showLabels && d.label && (() => { const m = Q[Math.floor(Q.length / 2)]; return text(m[0], m[1] - hw - 6, d.label, C.order, 'middle', fs, 700); })()}
          {sel && P.map((p, i) => <rect key={i} x={p[0] - 3.5} y={p[1] - 3.5} width={7} height={7} fill="#fff" stroke={C.ink} strokeWidth={1.2} />)}
        </g>);
      } else if (d.kind === 'route') {
        const body = trimEnd(Q, 6 * sk);
        els.push(<g key={key}>
          <path d={polyD(body)} fill="none" stroke={C.halo} strokeWidth={sel ? 7 : 4} strokeLinejoin="round" strokeLinecap="round" />
          {sel && <path d={polyD(body)} fill="none" stroke={C.ink} strokeOpacity={0.25} strokeWidth={7} strokeLinejoin="round" />}
          <path d={polyD(body)} fill="none" stroke={C.order} strokeWidth={1.9} strokeDasharray={dash} strokeLinejoin="round" strokeLinecap="round" />
          <path d={headD(Q, 13 * sk, 10 * sk)} fill={C.order} stroke={C.halo} strokeWidth={1} />
          {showLabels && d.label && text(Q[0][0], Q[0][1] - 9, d.label, C.order, 'middle', fs, 700)}
          {sel && P.map((p, i) => <rect key={i} x={p[0] - 3.5} y={p[1] - 3.5} width={7} height={7} fill="#fff" stroke={C.ink} strokeWidth={1.2} />)}
        </g>);
      } else {
        els.push(<g key={key}>
          <path d={polyD(Q)} fill="none" stroke={C.halo} strokeWidth={sel ? 7 : 4} strokeLinejoin="round" />
          {sel && <path d={polyD(Q)} fill="none" stroke={C.ink} strokeOpacity={0.25} strokeWidth={7} />}
          <path d={polyD(Q)} fill="none" stroke={C.phase} strokeWidth={1.9} strokeDasharray="8 5" strokeLinejoin="round" />
          {showLabels && text(Q[0][0], Q[0][1] - 8, d.label || 'PL', C.phase, 'middle', fs, 700)}
          {showLabels && text(Q[Q.length - 1][0], Q[Q.length - 1][1] + 15, d.label || 'PL', C.phase, 'middle', fs, 700)}
          {sel && P.map((p, i) => <rect key={i} x={p[0] - 3.5} y={p[1] - 3.5} width={7} height={7} fill="#fff" stroke={C.ink} strokeWidth={1.2} />)}
        </g>);
      }
    } else if (d.kind === 'area') {
      const col = areaColor(d.areaType);
      const x0 = Math.min(P[0][0], P[1][0]), y0 = Math.min(P[0][1], P[1][1]);
      const w = Math.abs(P[1][0] - P[0][0]), h = Math.abs(P[1][1] - P[0][1]);
      els.push(<g key={key}>
        {sel && <rect x={x0 - 3} y={y0 - 3} width={w + 6} height={h + 6} fill="none" stroke={C.ink} strokeOpacity={0.35} strokeWidth={1.5} strokeDasharray="3 3" />}
        <rect x={x0} y={y0} width={w} height={h} fill={d.areaType === 'airstrike' ? `url(#hatch-${map})` : col} fillOpacity={d.areaType === 'airstrike' ? 1 : 0.1} stroke={col} strokeWidth={1.4} strokeDasharray={dash ?? (d.areaType === 'warning' ? '2 0' : undefined)} />
        {showLabels && text(x0 + 4, y0 + fs + 2, d.label || '', col, 'start', fs, 700)}
      </g>);
    } else if (d.kind === 'circle') {
      const col = areaColor(d.areaType);
      const r = Math.hypot(P[1][0] - P[0][0], P[1][1] - P[0][1]);
      const meters = dist(d.pts[0], d.pts[1]) * metersPerCanon(map);
      els.push(<g key={key}>
        {sel && <circle cx={P[0][0]} cy={P[0][1]} r={r + 3} fill="none" stroke={C.ink} strokeOpacity={0.35} strokeWidth={1.5} strokeDasharray="3 3" />}
        <circle cx={P[0][0]} cy={P[0][1]} r={r} fill={d.areaType === 'airstrike' ? `url(#hatch-${map})` : col} fillOpacity={d.areaType === 'airstrike' ? 1 : 0.1} stroke={col} strokeWidth={1.8} strokeDasharray={dash} />
        <circle cx={P[0][0]} cy={P[0][1]} r={2.5} fill={col} />
        {showLabels && text(P[0][0], P[0][1] - r - 5, `${d.label} · r ${fmtDist(meters)}`, col, 'middle', fs, 700)}
      </g>);
    } else if (d.kind === 'source') {
      const o = P[0], q = P[1];
      const brg = bearingDeg(d.pts[0], d.pts[1]);
      const r = Math.max(16, Math.hypot(q[0] - o[0], q[1] - o[1]));
      const xr = 6 * sk;
      const half = ((d.sector ?? 0) / 2) * Math.PI / 180;
      const a = brg * Math.PI / 180;
      // arrow from source toward hit point
      const ux = Math.sin(a), uy = -Math.cos(a);
      const start: V2 = [o[0] + ux * r, o[1] + uy * r];
      const end: V2 = [o[0] + ux * 13 * sk, o[1] + uy * 13 * sk];
      els.push(<g key={key}>
        {half > 0 && <path d={wedgeD(o, a, half, r)} fill={C.enemy} fillOpacity={0.09} stroke={C.enemy} strokeOpacity={0.7} strokeWidth={1.2} strokeDasharray="5 4" />}
        {sel && <circle cx={o[0]} cy={o[1]} r={xr + 8} fill="none" stroke={C.ink} strokeWidth={1.4} strokeDasharray="3 3" />}
        <path d={polyD([start, end])} stroke={C.halo} strokeWidth={4} />
        <path d={polyD([start, trimEnd([start, end], 8 * sk)[1]])} stroke={C.enemy} strokeWidth={1.8} strokeDasharray="6 4" />
        <path d={headD([start, end], 11 * sk, 9 * sk)} fill={C.enemy} />
        <path d={`M${o[0] - xr} ${o[1] - xr}L${o[0] + xr} ${o[1] + xr}M${o[0] + xr} ${o[1] - xr}L${o[0] - xr} ${o[1] + xr}`} stroke={C.halo} strokeWidth={4} />
        <path d={`M${o[0] - xr} ${o[1] - xr}L${o[0] + xr} ${o[1] + xr}M${o[0] + xr} ${o[1] - xr}L${o[0] - xr} ${o[1] + xr}`} stroke={C.enemy} strokeWidth={2} />
        {showLabels && text(start[0], start[1] + (uy > 0 ? 14 : -7), `${d.label || '未确认'} · ${fmtBearing(brg)}`, C.enemy, 'middle', fs, 700)}
        {layers.time && d.showTime && text(o[0], o[1] + xr + fs + 2, fmtClock(d.t), C.enemy, 'middle', fs - 1, 600)}
      </g>);
    }
  }

  // ---- draft preview ----
  if (draft) {
    const P = draft.pts.map((w) => wToS(w, view));
    const lastD = draft.pts[draft.pts.length - 1];
    const H = hover && (!lastD || dist(hover, lastD) > 2) ? wToS(hover, view) : null;
    const st = s;
    if (draft.tool === 'route') {
      const all = H ? [...P, H] : P;
      const Q = st.smooth && st.routeType !== 'phase' ? sampleSmooth(all) : all;
      const col = st.routeType === 'phase' ? C.phase : C.order;
      if (st.routeType === 'arrow' && Q.length >= 2) els.push(<path key="dr-a" d={attackArrowD(Q, hw)} fill={C.order} fillOpacity={0.12} stroke={C.order} strokeWidth={1.6} strokeDasharray="5 4" />);
      else els.push(<path key="dr-l" d={polyD(Q)} fill="none" stroke={col} strokeWidth={2.2} strokeDasharray="6 5" />);
      if (st.routeType === 'route' && Q.length >= 2) els.push(<path key="dr-h" d={headD(Q, 12, 9)} fill={C.order} opacity={0.8} />);
      P.forEach((p, i) => els.push(<circle key={'dv' + i} cx={p[0]} cy={p[1]} r={i === P.length - 1 ? 5 : 4} fill="#fff" stroke={col} strokeWidth={2} />));
    } else if (draft.tool === 'area' && P[0]) {
      const col = areaColor(st.areaType);
      if (H) {
        if (st.areaShape === 'rect') els.push(<rect key="da" x={Math.min(P[0][0], H[0])} y={Math.min(P[0][1], H[1])} width={Math.abs(H[0] - P[0][0])} height={Math.abs(H[1] - P[0][1])} fill={col} fillOpacity={0.08} stroke={col} strokeWidth={1.6} strokeDasharray="5 4" />);
        else els.push(<circle key="da" cx={P[0][0]} cy={P[0][1]} r={Math.hypot(H[0] - P[0][0], H[1] - P[0][1])} fill={col} fillOpacity={0.08} stroke={col} strokeWidth={1.6} strokeDasharray="5 4" />);
      }
      els.push(<circle key="da0" cx={P[0][0]} cy={P[0][1]} r={5} fill="#fff" stroke={col} strokeWidth={2} />);
    } else if (draft.tool === 'source' && P[0]) {
      const o = P[0];
      els.push(<circle key="ds-r" cx={o[0]} cy={o[1]} r={18} fill="none" stroke={C.enemy} strokeWidth={1.2} strokeDasharray="3 3" className="pulse" />);
      els.push(<path key="ds-x" d={`M${o[0] - 6} ${o[1] - 6}L${o[0] + 6} ${o[1] + 6}M${o[0] + 6} ${o[1] - 6}L${o[0] - 6} ${o[1] + 6}`} stroke={C.enemy} strokeWidth={2.4} />);
      if (H) {
        const a = Math.atan2(H[0] - o[0], -(H[1] - o[1]));
        const r = Math.hypot(H[0] - o[0], H[1] - o[1]);
        const half = (st.sourceSector / 2) * Math.PI / 180;
        if (half > 0) els.push(<path key="ds-w" d={wedgeD(o, a, half, r)} fill={C.enemy} fillOpacity={0.07} stroke={C.enemy} strokeOpacity={0.6} strokeDasharray="5 4" />);
        els.push(<path key="ds-l" d={polyD([H, o])} stroke={C.enemy} strokeWidth={1.8} strokeDasharray="6 4" />);
      }
    }
  }

  // ---- ruler (local tool) ----
  if (ruler && ruler.length) {
    const lastR = ruler[ruler.length - 1];
    const all = hover && dist(hover, lastR) > 2 ? [...ruler, hover] : ruler;
    const P = all.map((w) => wToS(w, view));
    const mpc = metersPerCanon(map);
    let total = 0;
    els.push(<path key="ru-h" d={polyD(P)} fill="none" stroke={C.halo} strokeWidth={5} />);
    els.push(<path key="ru" d={polyD(P)} fill="none" stroke="#1f2420" strokeWidth={1.8} />);
    for (let i = 1; i < all.length; i++) {
      const m = dist(all[i - 1], all[i]) * mpc; total += m;
      const mid: V2 = [(P[i - 1][0] + P[i][0]) / 2, (P[i - 1][1] + P[i][1]) / 2];
      els.push(<g key={'rs' + i}>{text(mid[0], mid[1] - 6, `${fmtDist(m)} · ${fmtBearing(bearingDeg(all[i - 1], all[i]))}`, '#1f2420', 'middle', fs - 1, 600)}</g>);
    }
    P.forEach((p, i) => els.push(<circle key={'rv' + i} cx={p[0]} cy={p[1]} r={3.5} fill="#fff" stroke="#1f2420" strokeWidth={1.5} />));
    if (all.length > 1) {
      const last = P[P.length - 1];
      const eta = total / (rulerSpeed / 3.6);
      els.push(<g key="rt">{text(last[0] + 8, last[1] + 16, `共 ${fmtDist(total)} · ETA ${fmtClock(eta)}`, '#1f2420', 'start', fs, 700)}</g>);
    }
  }

  return (
    <svg className="layer" width={S} height={S} style={{ pointerEvents: 'none' }}>
      <defs>
        <pattern id={`hatch-${map}`} width="8" height="8" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">
          <rect width="8" height="8" fill={C.order} fillOpacity="0.06" />
          <line x1="0" y1="0" x2="0" y2="8" stroke={C.order} strokeOpacity="0.35" strokeWidth="1.4" />
        </pattern>
      </defs>
      {els}
    </svg>
  );
}

// ---------- live drawing (canvas) ----------
function drawLive(ctx: CanvasRenderingContext2D, S: number, dpr: number, map: MapId, v: ViewState, compact: boolean) {
  const s = getState();
  const snap = getSnapshot();
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, S, S);
  const toS = (w: V2): V2 => { const n = canonToNorm(map, w); return [(n[0] - v.cx) * S * v.z + S / 2, (n[1] - v.cy) * S * v.z + S / 2]; };
  const inView = (p: V2, m = 24) => p[0] > -m && p[1] > -m && p[0] < S + m && p[1] < S + m;
  const now = performance.now();
  const font = (px: number, w = 600) => `${w} ${px}px "Noto Sans SC","PingFang SC","Microsoft YaHei",Arial,sans-serif`;
  const sk = symK(v.z), tk = txtK(v.z);
  const label = (t: string, x: number, y: number, col: string, px0 = compact ? 10 : 11, align: CanvasTextAlign = 'center') => {
    const px = Math.max(8, Math.round(px0 * tk));
    ctx.font = font(px); ctx.textAlign = align; ctx.textBaseline = 'middle'; ctx.lineJoin = 'round';
    ctx.strokeStyle = 'rgba(246,244,236,0.92)'; ctx.lineWidth = 2.6; ctx.strokeText(t, x, y);
    ctx.fillStyle = col; ctx.fillText(t, x, y);
  };
  const sprite = (sidc: string, size: number, x: number, y: number, opt: Parameters<typeof unitSprite>[2] = {}, alpha = 1) => {
    const sp = unitSprite(sidc, size, opt, dpr);
    if (!sp.c) return sp;
    ctx.globalAlpha = alpha;
    ctx.drawImage(sp.c, x - sp.ax, y - sp.ay, sp.w, sp.h);
    ctx.globalAlpha = 1;
    return sp;
  };
  const cellPx = gridBox(map).stepX * S * v.z;
  const usz = Math.max(9, Math.round((S >= 600 ? 21 : S >= 420 ? 18 : 15) * sk));
  // air overview: the ground battle area is tiny → shrink ground objects, hide their labels (plan §5.3)
  const ov = !separateMaps() && gridBox('ground').w * (map === 'ground' ? 1 : 0.1875) * S * v.z < 240;
  if (!MAPS[map].available) return;

  // static objects: friendly airfield + bombing points
  if (snap.airfield && map === 'air') {
    const a = toS(snap.airfield[0]), b = toS(snap.airfield[1]);
    if (inView(a)) {
      ctx.strokeStyle = 'rgba(246,244,236,0.9)'; ctx.lineWidth = 6; ctx.lineCap = 'round';
      ctx.beginPath(); ctx.moveTo(a[0], a[1]); ctx.lineTo(b[0], b[1]); ctx.stroke();
      ctx.strokeStyle = '#174DFF'; ctx.lineWidth = 3; ctx.stroke(); ctx.lineCap = 'butt';
    }
  }
  if (s.policy !== 'off' && s.layers.enemy && map === 'air') for (const bp of snap.bombing) {
    const p = toS(bp); if (!inView(p)) continue;
    ctx.strokeStyle = C.enemy; ctx.lineWidth = 1.5; ctx.strokeRect(p[0] - 4, p[1] - 4, 8, 8);
  }

  // v2: respawn points (one small flag per spawn area; tank = pennant, air = chevron) + airfield runways
  const mk = snap.marks[map];
  if (mk && s.layers.spawns) {
    const nToS = (x: number, y: number): V2 => [(x - v.cx) * S * v.z + S / 2, (y - v.cy) * S * v.z + S / 2];
    for (const af of mk.airfields) {
      const a = nToS(af.a[0], af.a[1]), b = nToS(af.b[0], af.b[1]);
      if (!inView(a, 60) && !inView(b, 60)) continue;
      const col = af.side === 'enemy' ? 'rgba(200,40,28,0.75)' : 'rgba(43,98,196,0.75)';
      ctx.lineCap = 'round';
      ctx.strokeStyle = 'rgba(246,244,236,0.85)'; ctx.lineWidth = 4.5; ctx.beginPath(); ctx.moveTo(a[0], a[1]); ctx.lineTo(b[0], b[1]); ctx.stroke();
      ctx.strokeStyle = col; ctx.lineWidth = 2.2; ctx.stroke(); ctx.lineCap = 'butt';
    }
    const fs = Math.max(5, 7 * sk + 1);
    for (const sp of mk.spawns) {
      const p = nToS(sp.x, sp.y);
      if (!inView(p, 12)) continue;
      const col = sp.side === 'enemy' ? C.enemy : C.friend;
      ctx.globalAlpha = 0.85;
      ctx.lineJoin = 'round';
      if (sp.k === 'tank') {
        // pennant on a short pole, foot at the spawn point
        ctx.beginPath(); ctx.moveTo(p[0], p[1]); ctx.lineTo(p[0], p[1] - fs * 1.6); ctx.lineTo(p[0] + fs * 1.1, p[1] - fs * 1.15); ctx.lineTo(p[0], p[1] - fs * 0.7);
        ctx.strokeStyle = 'rgba(246,244,236,0.95)'; ctx.lineWidth = 3; ctx.stroke();
        ctx.fillStyle = col; ctx.fill(); ctx.strokeStyle = col; ctx.lineWidth = 1.2;
        ctx.beginPath(); ctx.moveTo(p[0], p[1]); ctx.lineTo(p[0], p[1] - fs * 1.6); ctx.stroke();
      } else {
        // air spawn: small open chevron
        ctx.beginPath(); ctx.moveTo(p[0] - fs * 0.8, p[1] + fs * 0.4); ctx.lineTo(p[0], p[1] - fs * 0.6); ctx.lineTo(p[0] + fs * 0.8, p[1] + fs * 0.4);
        ctx.strokeStyle = 'rgba(246,244,236,0.95)'; ctx.lineWidth = 3.4; ctx.stroke();
        ctx.strokeStyle = col; ctx.lineWidth = 1.6; ctx.stroke();
      }
      ctx.globalAlpha = 1;
    }
  }

  // radio: pings at the target grid cell (uniform, ~30 s) + location calls at the sender (brief)
  for (const pg of snap.pings) {
    if (pg.only !== map) continue;
    const c = toS(pg.w);
    const k = Math.max(0, Math.min(1, pg.remainMs / pg.ttlMs));
    if (pg.area) {
      const a = toS(pg.area[0]), b = toS(pg.area[1]);
      ctx.fillStyle = `rgba(201,120,15,${0.10 + 0.10 * k})`; ctx.fillRect(a[0], a[1], b[0] - a[0], b[1] - a[1]);
      ctx.setLineDash([5, 3]); ctx.strokeStyle = `rgba(201,120,15,${0.5 + 0.4 * k})`; ctx.lineWidth = 1.6; ctx.strokeRect(a[0], a[1], b[0] - a[0], b[1] - a[1]); ctx.setLineDash([]);
    }
    if (!inView(c, 40)) continue;
    const ph = (now % 1400) / 1400;
    const pr = 11 * sk;
    ctx.strokeStyle = `rgba(201,120,15,${0.7 * (1 - ph)})`; ctx.lineWidth = 1.6;
    ctx.beginPath(); ctx.arc(c[0], c[1], pr * 0.7 + ph * 16 * sk, 0, Math.PI * 2); ctx.stroke();
    ctx.strokeStyle = 'rgba(246,244,236,0.95)'; ctx.lineWidth = 4; ctx.beginPath(); ctx.arc(c[0], c[1], pr, 0, Math.PI * 2); ctx.stroke();
    ctx.strokeStyle = C.order; ctx.lineWidth = 2.2; ctx.beginPath(); ctx.arc(c[0], c[1], pr, -Math.PI / 2, -Math.PI / 2 + Math.PI * 2 * k); ctx.stroke();
    ctx.fillStyle = C.order; ctx.beginPath(); ctx.arc(c[0], c[1], 2.6, 0, Math.PI * 2); ctx.fill();
    if (pg.area) { const a = toS(pg.area[0]); label(`${pg.grid ?? ''} 标点 ${Math.ceil(pg.remainMs / 1000)}s`, Math.max(4, a[0] + 3), Math.max(8, a[1] + 9), C.order, compact ? 10 : 11, 'left'); }
    else label(`标点 ${Math.ceil(pg.remainMs / 1000)}s`, c[0], c[1] - pr - 8, C.order, compact ? 10 : 11);
  }
  for (const cl of snap.calls) {
    if (cl.only !== map) continue;
    const c = toS(cl.w); if (!inView(c, 40)) continue;
    const k = Math.max(0, Math.min(1, cl.remainMs / cl.ttlMs));
    ctx.globalAlpha = 0.35 + 0.65 * k;
    const ph = (now % 1000) / 1000;
    const tr = 9 * sk;
    ctx.strokeStyle = `rgba(43,98,196,${0.8 * (1 - ph)})`; ctx.lineWidth = 1.6; ctx.beginPath(); ctx.arc(c[0], c[1], tr + ph * 12 * sk, 0, Math.PI * 2); ctx.stroke();
    ctx.fillStyle = C.friend; ctx.strokeStyle = 'rgba(246,244,236,0.95)'; ctx.lineWidth = 1.6;
    ctx.beginPath(); ctx.moveTo(c[0], c[1] - tr); ctx.lineTo(c[0] + tr * 0.9, c[1] + tr * 0.67); ctx.lineTo(c[0] - tr * 0.9, c[1] + tr * 0.67); ctx.closePath(); ctx.fill(); ctx.stroke();
    ctx.fillStyle = '#fff'; ctx.font = font(Math.round(10 * sk), 800); ctx.textAlign = 'center'; ctx.textBaseline = 'middle'; ctx.fillText('!', c[0], c[1] + 1);
    label(cl.text, c[0], c[1] + tr + 9, C.friend, compact ? 9 : 10);
    ctx.globalAlpha = 1;
  }

  // friendlies (self / squad green / teammates blue)
  if (s.layers.friendly) {
    const rank = (r: string) => (r === 'self' ? 2 : r === 'squad' ? 1 : 0);
    const fr = snap.friends.filter((f) => !f.only || f.only === map).sort((a, b) => rank(a.role) - rank(b.role));
    for (const f of fr) {
      const p = toS(f.w);
      if (!inView(p)) continue;
      const stale = !f.alive || f.ageMs > 2500;
      const squad = f.role === 'squad';
      const tone = f.self ? '#d9a100' : squad ? C.squad : C.friend;
      const col = stale ? C.stale : f.self ? '#b8890a' : tone;
      const small = ov && !f.air;
      if (!stale && f.hdg != null) {
        const a = f.hdg * Math.PI / 180;
        const L = small ? 8 : usz + 3;
        const tip: V2 = [p[0] + Math.sin(a) * L, p[1] - Math.cos(a) * L];
        ctx.strokeStyle = 'rgba(246,244,236,0.95)'; ctx.lineWidth = 3.5; ctx.lineCap = 'round';
        ctx.beginPath(); ctx.moveTo(p[0], p[1]); ctx.lineTo(tip[0], tip[1]); ctx.stroke();
        ctx.strokeStyle = tone; ctx.lineWidth = 1.7;
        ctx.beginPath(); ctx.moveTo(p[0], p[1]); ctx.lineTo(tip[0], tip[1]); ctx.stroke(); ctx.lineCap = 'butt';
        const hx = Math.sin(a), hy = -Math.cos(a);
        ctx.fillStyle = tone;
        const hs = (small ? 3 : 4.5) * Math.max(0.7, sk);
        ctx.beginPath(); ctx.moveTo(tip[0] + hx * hs * 1.1, tip[1] + hy * hs * 1.1); ctx.lineTo(tip[0] - hy * hs, tip[1] + hx * hs); ctx.lineTo(tip[0] + hy * hs, tip[1] - hx * hs); ctx.closePath(); ctx.fill();
      }
      const size = small ? (f.self ? 10 : 8) : f.self ? usz + 2 : squad ? usz : usz - 2;
      const sp = sprite(f.sidc, size, p[0], p[1], { fill: f.self && !stale ? C.self : undefined, squad: squad && !stale, grey: stale }, stale ? 0.75 : 1);
      if (f.self && !f.alive) {
        // dead: cross over the last own position (the Player icon now follows the spectated teammate)
        ctx.strokeStyle = C.enemy; ctx.lineWidth = 2.4; const r = size * 0.45;
        ctx.beginPath(); ctx.moveTo(p[0] - r, p[1] - r); ctx.lineTo(p[0] + r, p[1] + r); ctx.moveTo(p[0] + r, p[1] - r); ctx.lineTo(p[0] - r, p[1] + r); ctx.stroke();
      }
      if (s.layers.labels && (!small || f.self) && (f.self || f.nick)) {
        const t = f.self && !f.alive ? `${f.nick} · 阵亡 ${Math.floor(f.ageMs / 1000)}s` : stale && f.nick ? `${f.nick} · ${Math.floor(f.ageMs / 1000)}s` : f.nick;
        // v2: teammate/squad names only when zoomed in; self always
        if (t && (f.self || (squad && v.z >= 2) || cellPx > 240)) label(t, p[0], p[1] + sp.h - sp.ay + 6, col, f.self ? (S >= 600 ? 11 : compact ? 9 : 10) : 9);
      }
    }
  }

  // enemies: only the occasional live red markers 8111 exports (spawn-proximity reveals → 压家) + short last-seen ghost
  if (s.policy !== 'off' && s.layers.enemy) {
    const fused = s.policy === 'fused';
    const list = snap.contacts.filter((c) => !c.only || c.only === map).sort((a, b) => Number(a.visible) - Number(b.visible) || (a.spawnM ?? 1e9) - (b.spawnM ?? 1e9));
    const placed: [number, number, number, number][] = [];
    for (const c of list) {
      const life = c.air ? (fused ? 15000 : 5000) : (fused ? 30000 : 8000);
      if (!c.visible && c.ageMs > life) continue;
      const p = toS(c.w);
      if (!inView(p, 60)) continue;
      const sidc = ICON_SIDC[c.icon] ?? 'SHGPU-----';
      const sz = c.air ? usz : ov ? 8 : usz;
      const rr = sz * 0.72;
      if (c.visible) {
        const camp = !DEMO && !c.air && (c.spawnM == null || c.spawnM <= 400);
        const ph = (Math.sin(now / 150) + 1) / 2;
        if (camp) {
          ctx.fillStyle = `rgba(240,20,0,${0.10 + 0.10 * ph})`;
          ctx.beginPath(); ctx.arc(p[0], p[1], rr * 2.1, 0, Math.PI * 2); ctx.fill();
          ctx.strokeStyle = `rgba(220,20,0,${0.55 + 0.45 * ph})`; ctx.lineWidth = 2.2;
          ctx.beginPath(); ctx.arc(p[0], p[1], rr * 1.5 + ph * 3, 0, Math.PI * 2); ctx.stroke();
        } else if (c.blink === 2 && c.reveal !== 'air') {
          ctx.strokeStyle = `rgba(240,12,0,${0.35 + 0.5 * ph})`; ctx.lineWidth = ov ? 1.4 : 2;
          ctx.beginPath(); ctx.arc(p[0], p[1], rr + 1 + ph * 3, 0, Math.PI * 2); ctx.stroke();
        }
        if (c.air && c.hdg != null) {
          const a = c.hdg * Math.PI / 180;
          ctx.strokeStyle = C.enemy; ctx.lineWidth = 1.6;
          ctx.beginPath(); ctx.moveTo(p[0], p[1]); ctx.lineTo(p[0] + Math.sin(a) * usz * 0.85, p[1] - Math.cos(a) * usz * 0.85); ctx.stroke();
        }
        sprite(sidc, sz, p[0], p[1], { dashed: true });
        if (camp) {
          const t = `压家 · ${ICON_ZH[c.icon] ?? c.icon}${c.spawnM != null ? ` ${Math.round(c.spawnM)}m` : ''}`;
          const fpx = Math.max(9, Math.round((compact ? 10 : 11) * tk));
          ctx.font = font(fpx, 800); const w = ctx.measureText(t).width + 8, y = p[1] - rr * 1.5 - fpx;
          const box: [number, number, number, number] = [p[0] - w / 2, y - fpx * 0.7, p[0] + w / 2, y + fpx * 0.7];
          // v2: several 压家 enemies close together → only the first label (rings still show each one)
          if (!placed.some((q) => box[0] < q[2] && box[2] > q[0] && box[1] < q[3] && box[3] > q[1])) {
            placed.push(box);
            ctx.fillStyle = 'rgba(200,24,10,0.92)'; ctx.fillRect(box[0], box[1], w, fpx * 1.4);
            ctx.fillStyle = '#fff'; ctx.textAlign = 'center'; ctx.textBaseline = 'middle'; ctx.fillText(t, p[0], y + 0.5);
          }
        } else if (!ov && (cellPx > 150 || !DEMO) && s.layers.labels) label(ICON_ZH[c.icon] ?? c.icon, p[0], p[1] + sz - 1, C.enemy, compact ? 9 : 10);
      } else {
        // ghost / last seen
        const k = 1 - c.ageMs / life;
        if (fused && c.air && c.hdg != null) {
          const a = c.hdg * Math.PI / 180;
          const sp = (c.speed ?? 0);
          const sec = c.ageMs / 1000;
          const pw: V2 = [c.w[0] + Math.sin(a) * sp * sec, c.w[1] - Math.cos(a) * sp * sec];
          const pp = toS(pw);
          const rl = 8 + Math.hypot(pp[0] - p[0], pp[1] - p[1]) * 0.45, rw = 6 + rl * 0.45;
          ctx.save(); ctx.translate(pp[0], pp[1]); ctx.rotate(a);
          ctx.fillStyle = `rgba(200,40,28,${0.10 * k})`; ctx.strokeStyle = `rgba(200,40,28,${0.6 * k})`; ctx.setLineDash([3, 3]); ctx.lineWidth = 1.2;
          ctx.beginPath(); ctx.ellipse(0, 0, rw, rl, 0, 0, Math.PI * 2); ctx.fill(); ctx.stroke(); ctx.restore(); ctx.setLineDash([]);
          ctx.strokeStyle = `rgba(200,40,28,${0.5 * k})`; ctx.setLineDash([2, 3]);
          ctx.beginPath(); ctx.moveTo(p[0], p[1]); ctx.lineTo(pp[0], pp[1]); ctx.stroke(); ctx.setLineDash([]);
        }
        ctx.globalAlpha = Math.max(0.15, k);
        ctx.setLineDash([3, 2.5]); ctx.strokeStyle = C.enemy; ctx.lineWidth = 1.5; ctx.fillStyle = 'rgba(246,244,236,0.55)';
        const r = c.air ? sz * 0.45 : ov ? 5 : sz * 0.5;
        ctx.beginPath(); ctx.moveTo(p[0], p[1] - r); ctx.lineTo(p[0] + r, p[1]); ctx.lineTo(p[0], p[1] + r); ctx.lineTo(p[0] - r, p[1]); ctx.closePath(); ctx.fill(); ctx.stroke(); ctx.setLineDash([]);
        ctx.globalAlpha = 1;
        if (!ov || c.air) label(`${Math.floor(c.ageMs / 1000)}s 前`, p[0], p[1] + r + 8, `rgba(200,40,28,${Math.max(0.4, k)})`, compact ? 9 : 10);
      }
    }
  }
}

// ---------- live overlays ----------
function MapStatus({ map }: { map: MapId }) {
  const snap = useFresh();
  const cfg = MAPS[map];
  if (!cfg.available) {
    const t = snap.conn === 'auth' ? '需要房间密码（右上角设置）' : snap.conn === 'offline' ? '与服务器断开，重连中…' : snap.conn === 'connecting' ? '连接中…'
      : map === 'air' ? '暂无空战视图（驾驶飞机时出现）' : '等待对局数据…';
    return <div className="mapidle"><b>{map === 'ground' ? '陆战' : '空战'}</b><span>{t}</span></div>;
  }
  if (!snap.inBattle) return <div className="mapnote">机库中 · 上一局地图</div>;
  if (!cfg.img) return <div className="mapnote">底图加载中…</div>;
  return null;
}

function FeedOverlay({ compact }: { compact: boolean }) {
  const snap = useFresh();
  const lines = snap.feed.filter((f) => f.ageMs < 8000).slice(-3);
  if (!lines.length) return null;
  return (
    <div className={'killfeed' + (compact ? ' compact' : '')} aria-live="polite">
      {lines.map((f) => <div key={f.id} className={(f.me ? 'me ' : '') + (f.dead ? 'dead' : '')} style={{ opacity: Math.min(1, (8000 - f.ageMs) / 1500) }}>{f.text}</div>)}
    </div>
  );
}
