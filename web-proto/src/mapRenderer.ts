import { MAPS, gridBox, rowLetter, canonToNorm, fmtClock, fmtDist, bearingDeg, CANON, type MapId, type V2 } from './geo';
import { C } from './theme';
import { catColor } from './symbols';
import { attackArrowD, headD, polyD, polyDist, sampleSmooth, wedgeD } from './draw';
import { vehicleSprite, VEHICLE_LABELS } from './vehicleIcons';
import { transform } from './viewport';
import type { Snapshot } from './sim';
import type { Drawing, State, ViewState } from './store';
export interface Hit { id: string; kind: 'drawing' | 'unit'; shape: 'point' | 'line' | 'rect' | 'circle'; pts: V2[]; radius?: number }
export function hitAt(hits: Hit[], p: V2): Hit | null {
  for (const h of [...hits].reverse()) {
    const [a, b = a] = h.pts;
    if (h.shape === 'point' && Math.hypot(p[0] - a[0], p[1] - a[1]) <= (h.radius ?? 22)) return h;
    if (h.shape === 'line' && polyDist(p, h.pts) <= 10) return h;
    if (h.shape === 'rect' && p[0] >= Math.min(a[0], b[0]) - 8 && p[0] <= Math.max(a[0], b[0]) + 8 && p[1] >= Math.min(a[1], b[1]) - 8 && p[1] <= Math.max(a[1], b[1]) + 8) return h;
    if (h.shape === 'circle' && Math.hypot(p[0] - a[0], p[1] - a[1]) <= Math.hypot(b[0] - a[0], b[1] - a[1]) + 8) return h;
  } return null;
}
export function measure(map: MapId, a: V2, b: V2) {
  const c = MAPS[map];
  return Math.hypot((b[0] - a[0]) / CANON * (c.max[0] - c.min[0]), (b[1] - a[1]) / CANON * (c.max[1] - c.min[1]));
}
export function prepare(canvas: HTMLCanvasElement, width: number, height: number, dpr: number) {
  const w = Math.round(width * dpr), h = Math.round(height * dpr);
  if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
  const ctx = canvas.getContext('2d')!; ctx.setTransform(dpr, 0, 0, dpr, 0, 0); ctx.clearRect(0, 0, width, height);
  ctx.lineJoin = 'round'; ctx.lineCap = 'round'; return ctx;
}
function text(ctx: CanvasRenderingContext2D, value: string, x: number, y: number, color = C.ink, align: CanvasTextAlign = 'left', size = 12) {
  ctx.font = `500 ${size}px "Microsoft YaHei", "PingFang SC", sans-serif`; ctx.textAlign = align; ctx.textBaseline = 'middle';
  ctx.strokeStyle = C.halo; ctx.lineWidth = 3; ctx.strokeText(value, x, y); ctx.fillStyle = color; ctx.fillText(value, x, y);
}
function path(ctx: CanvasRenderingContext2D, d: string, color: string, fill = false) {
  if (!d) return; const p = new Path2D(d); ctx.strokeStyle = color; ctx.lineWidth = 2; if (fill) { ctx.fillStyle = color + '28'; ctx.fill(p); } ctx.stroke(p);
}
export function paintBase(ctx: CanvasRenderingContext2D, map: MapId, width: number, height: number, v: ViewState, image: HTMLImageElement | null, grid: boolean, aspect = 1) {
  ctx.fillStyle = C.mapPaper; ctx.fillRect(0, 0, width, height);
  const t = transform(map, width, height, v, aspect), a = t.normToScreen([0, 0]);
  if (image?.naturalWidth) ctx.drawImage(image, a[0], a[1], t.xScale, t.yScale);
  if (!MAPS[map].available) return;
  const g = gridBox(map);
  if (grid && Number.isFinite(g.cols) && Number.isFinite(g.rows)) {
    ctx.strokeStyle = '#39433055'; ctx.lineWidth = .75; ctx.beginPath();
    for (let i = 0; i <= Math.min(g.cols, 200); i++) { const x = t.normToScreen([g.x0 + i * g.stepX, 0])[0]; ctx.moveTo(x, 0); ctx.lineTo(x, height); }
    for (let i = 0; i <= Math.min(g.rows, 200); i++) { const y = t.normToScreen([0, g.y0 + i * g.stepY])[1]; ctx.moveTo(0, y); ctx.lineTo(width, y); }
    ctx.stroke();
    for (let i = 0; i < Math.min(g.cols, 200); i++) { const x = t.normToScreen([g.x0 + (i + .5) * g.stepX, 0])[0]; if (x > 12 && x < width - 12) text(ctx, String(i + 1), x, 42, C.ink, 'center', 11); }
    for (let i = 0; i < Math.min(g.rows, 200); i++) { const y = t.normToScreen([0, g.y0 + (i + .5) * g.stepY])[1]; if (y > 62 && y < height - 44) text(ctx, rowLetter(i), 14, y, C.ink, 'center', 11); }
  }
  const perPx = (MAPS[map].max[0] - MAPS[map].min[0]) / t.xScale;
  const target = perPx * 90; const unit = 10 ** Math.floor(Math.log10(target));
  const metres = [1, 2, 5, 10].map(k => k * unit).find(k => k >= target) ?? unit * 10;
  const px = metres / perPx, y = height - 19;
  ctx.strokeStyle = C.ink; ctx.lineWidth = 2; ctx.beginPath(); ctx.moveTo(18, y - 4); ctx.lineTo(18, y); ctx.lineTo(18 + px, y); ctx.lineTo(18 + px, y - 4); ctx.stroke();
  text(ctx, fmtDist(metres), 18, y - 12, C.ink, 'left', 11);
}
export function paintUnits(ctx: CanvasRenderingContext2D, map: MapId, width: number, height: number, v: ViewState, s: Snapshot, ui: State, dpr: number, aspect = 1): Hit[] {
  const t = transform(map, width, height, v, aspect), hits: Hit[] = [];
  const labels: { name: string; p: V2; color: string; priority: number }[] = [];
  if (ui.layers.spawns) {
    const marks = s.marks[map];
    for (const f of marks?.airfields ?? []) { path(ctx, polyD([t.normToScreen(f.a), t.normToScreen(f.b)]), f.side === 'enemy' ? C.enemy : C.friend); }
    for (const f of marks?.spawns ?? []) { const p = t.normToScreen([f.x, f.y]); ctx.strokeStyle = f.side === 'enemy' ? C.enemy : C.friend; ctx.lineWidth = 1.5; ctx.strokeRect(p[0] - 6, p[1] - 6, 12, 12); }
  }
  const drawUnit = (id: string, w: V2, icon: string, color: string, hdg: number | null, name: string, priority: number) => {
    const p = t.worldToScreen(w); if (p[0] < -30 || p[1] < -30 || p[0] > width + 30 || p[1] > height + 30) return;
    const selected = ui.selectedUnit === id;
    if (selected) { ctx.strokeStyle = color; ctx.lineWidth = 1.5; ctx.strokeRect(p[0] - 21, p[1] - 21, 42, 42); }
    if (icon === 'Player') {
      // Same 14 × 25 arrow as the built-in map; allegiance includes the user's gold.
      ctx.save(); ctx.translate(...p); ctx.rotate((hdg ?? 0) * Math.PI / 180);
      ctx.fillStyle = color; ctx.strokeStyle = '#171b14'; ctx.lineWidth = 1.5;
      ctx.beginPath(); ctx.moveTo(-7, 12.5); ctx.lineTo(7, 12.5); ctx.lineTo(0, -12.5); ctx.closePath(); ctx.fill(); ctx.stroke(); ctx.restore();
    } else {
      const sprite = vehicleSprite(icon, color, 30, dpr);
      if (sprite) ctx.drawImage(sprite, p[0] - 15, p[1] - 15, 30, 30);
      else { ctx.fillStyle = color; ctx.strokeStyle = '#171b14'; ctx.lineWidth = 1; ctx.beginPath(); ctx.arc(p[0], p[1], 4, 0, Math.PI * 2); ctx.fill(); ctx.stroke(); }
      if (hdg != null) { const rad = hdg * Math.PI / 180, tip: V2 = [p[0] + Math.sin(rad) * 19, p[1] - Math.cos(rad) * 19]; path(ctx, headD([p, tip], 5, 4), color); }
    }
    hits.push({ id, kind: 'unit', shape: 'point', pts: [p], radius: 22 });
    if (ui.layers.labels && (name || selected)) labels.push({ name: selected ? name || VEHICLE_LABELS[icon] || icon : [...name].slice(0, 6).join(''), p, color, priority: selected ? -1 : priority });
  };
  if (ui.layers.friendly) for (const f of s.friends) {
    if ((f.only && f.only !== map) || (!f.only && (f.air ? 'air' : 'ground') !== map) || f.ageMs > 2000 || !f.alive) continue;
    const icon = f.self || f.id.startsWith('self:') ? 'Player' : f.icon ?? f.vehicle;
    drawUnit(f.id, f.w, icon, f.self ? C.self : f.role === 'squad' ? C.squad : C.friend, f.hdg, f.nick, f.self ? 0 : f.nick ? 1 : 3);
  }
  if (ui.layers.enemy && ui.policy !== 'off') for (const c of s.contacts) {
    if ((c.only && c.only !== map) || (!c.only && (c.air ? 'air' : 'ground') !== map) || c.ageMs > 2000) continue;
    drawUnit(c.id, c.w, c.icon, C.enemy, c.hdg, ui.policy === 'fused' ? VEHICLE_LABELS[c.icon] ?? '未知' : '', 4);
  }
  ctx.font = `500 12px "Microsoft YaHei", "PingFang SC", sans-serif`;
  const boxes: number[][] = [];
  for (const l of labels.sort((a, b) => a.priority - b.priority)) {
    const w = ctx.measureText(l.name).width + 8;
    for (const [dx, dy] of [[19, 0], [19, 18], [-w - 19, 0], [19, -18]]) {
      const b = [l.p[0] + dx, l.p[1] + dy - 8, w, 16];
      if (b[0] < 2 || b[0] + w > width - 2 || b[1] < 32 || b[1] + 16 > height - 38) continue;
      if (boxes.some(o => b[0] < o[0] + o[2] && b[0] + b[2] > o[0] && b[1] < o[1] + o[3] && b[1] + b[3] > o[1])) continue;
      text(ctx, l.name, b[0] + 4, b[1] + 8, l.color); boxes.push(b); break;
    }
  }
  for (const p of [...s.pings, ...s.calls]) if (p.only === map && p.remainMs > 0) {
    const a = t.worldToScreen(p.w); ctx.strokeStyle = C.order; ctx.lineWidth = 1.5; ctx.beginPath(); ctx.arc(a[0], a[1], 14, 0, Math.PI * 2); ctx.stroke();
    text(ctx, p.grid || '报告', a[0] + 18, a[1], C.order);
  }
  return hits;
}
export function paintDrawings(ctx: CanvasRenderingContext2D, map: MapId, width: number, height: number, v: ViewState, ui: State, battle: number, aspect = 1, layer: 'drawings' | 'preview' = 'drawings'): Hit[] {
  const t = transform(map, width, height, v, aspect), hits: Hit[] = [];
  const paint = (d: Drawing, draft = false) => {
    const P = d.pts.map(t.worldToScreen); if (!P.length) return;
    const selected = d.id === ui.selectedId, color = d.kind === 'point' ? catColor(d.cat ?? 'rally') : d.kind === 'source' ? C.enemy : d.areaType === 'recon' ? C.recon : d.areaType === 'warning' ? C.enemy : d.kind === 'phase' ? C.phase : C.order;
    ctx.setLineDash(d.status === 'planned' || draft ? [6, 4] : []); ctx.lineWidth = selected ? 3 : 2; ctx.strokeStyle = color;
    let hit: Hit = { id: d.id, kind: 'drawing', shape: 'line', pts: P };
    switch (d.kind) {
      case 'point': {
        const [x, y] = P[0]; ctx.fillStyle = C.halo; ctx.beginPath();
        if (d.cat === 'enemy' || d.cat === 'attack') { ctx.moveTo(x, y - 10); ctx.lineTo(x + 10, y); ctx.lineTo(x, y + 10); ctx.lineTo(x - 10, y); ctx.closePath(); }
        else if (d.cat === 'danger' || d.cat === 'observe') { ctx.moveTo(x, y - 10); ctx.lineTo(x + 11, y + 9); ctx.lineTo(x - 11, y + 9); ctx.closePath(); }
        else if (d.cat === 'airstrike') ctx.arc(x, y, 10, 0, Math.PI * 2);
        else ctx.rect(x - 11, y - 8, 22, 16);
        ctx.fill(); ctx.stroke(); text(ctx, d.cat === 'danger' ? '!' : d.cat === 'rally' ? '+' : d.cat === 'defend' ? 'D' : '·', x, y + 1, color, 'center', 12);
        hit = { ...hit, shape: 'point', radius: 22 }; break;
      }
      case 'area': if (P[1]) { const [a, b] = P; ctx.fillStyle = color + '18'; ctx.fillRect(a[0], a[1], b[0] - a[0], b[1] - a[1]); ctx.strokeRect(a[0], a[1], b[0] - a[0], b[1] - a[1]); hit.shape = 'rect'; } break;
      case 'circle': if (P[1]) { ctx.beginPath(); ctx.arc(P[0][0], P[0][1], Math.hypot(P[1][0] - P[0][0], P[1][1] - P[0][1]), 0, Math.PI * 2); ctx.fillStyle = color + '18'; ctx.fill(); ctx.stroke(); hit.shape = 'circle'; } break;
      case 'source': if (P[1]) { const rad = Math.atan2(P[1][0] - P[0][0], -(P[1][1] - P[0][1])); if (d.sector) path(ctx, wedgeD(P[0], rad, d.sector * Math.PI / 360, Math.hypot(P[1][0] - P[0][0], P[1][1] - P[0][1])), color, true); path(ctx, polyD(P), color); path(ctx, headD([...P].reverse()), color); } break;
      default: { const pts = d.smooth ? sampleSmooth(P) : P; path(ctx, d.kind === 'arrow' ? attackArrowD(pts) : polyD(pts), color, d.kind === 'arrow'); if (d.kind === 'route') path(ctx, headD(pts), color); hit.pts = pts; }
    }
    ctx.setLineDash([]);
    if (selected) for (const p of P) { ctx.fillStyle = color; ctx.fillRect(p[0] - 3, p[1] - 3, 6, 6); }
    if (!draft && (d.label || (d.showTime && ui.layers.time))) {
      const label = [d.label, d.showTime && ui.layers.time ? fmtClock(battle - d.t) : ''].filter(Boolean).join(' · ');
      text(ctx, label, P[0][0] + 16, P[0][1] - 18, color);
    }
    if (!draft) hits.push(hit);
  };
  if (layer === 'drawings' && ui.layers.drawings) for (const d of ui.drawings) if ((d.scope === map || d.scope === 'both') && (!d.map_context || d.map_context === MAPS[map].context)) paint(d);
  if (layer === 'preview' && ui.draft?.map === map) {
    const pts = ui.draft.pts.concat(ui.hover?.map === map ? [ui.hover.w] : []);
    paint({ id: 'draft', kind: ui.draft.tool === 'route' ? ui.routeType : ui.draft.tool === 'source' ? 'source' : ui.areaShape === 'circle' ? 'circle' : 'area',
      pts, scope: map, label: '', status: ui.newStatus, t: battle, by: '', v: 0, showTime: false, areaType: ui.areaType, sector: ui.sourceSector }, true);
  }
  if (layer === 'preview' && ui.ruler?.map === map) {
    const points = ui.ruler.pts.concat(ui.hover?.map === map ? [ui.hover.w] : []), P = points.map(t.worldToScreen);
    path(ctx, polyD(P), C.ink);
    for (const p of P) { ctx.fillStyle = C.ink; ctx.fillRect(p[0] - 2, p[1] - 2, 4, 4); }
    if (P.length > 1) { const metres = points.slice(1).reduce((a, p, i) => a + measure(map, points[i], p), 0); const p = P[P.length - 1]; text(ctx, `${fmtDist(metres)} · ${Math.round(bearingDeg(points[0], points[points.length - 1]))}° · ${fmtClock(metres / (ui.rulerSpeed / 3.6))}`, p[0] + 12, p[1] - 12); }
  }
  return hits;
}
