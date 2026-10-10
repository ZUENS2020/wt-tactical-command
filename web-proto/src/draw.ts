// geometry helpers for screen-space drawing
import type { V2 } from './geo';

export function sampleSmooth(P: V2[], per = 10): V2[] {
  if (P.length < 3) return P;
  const out: V2[] = [P[0]];
  for (let i = 0; i < P.length - 1; i++) {
    const p0 = P[i - 1] ?? P[i], p1 = P[i], p2 = P[i + 1], p3 = P[i + 2] ?? p2;
    const c1: V2 = [p1[0] + (p2[0] - p0[0]) / 6, p1[1] + (p2[1] - p0[1]) / 6];
    const c2: V2 = [p2[0] - (p3[0] - p1[0]) / 6, p2[1] - (p3[1] - p1[1]) / 6];
    for (let k = 1; k <= per; k++) {
      const t = k / per, u = 1 - t;
      out.push([
        u * u * u * p1[0] + 3 * u * u * t * c1[0] + 3 * u * t * t * c2[0] + t * t * t * p2[0],
        u * u * u * p1[1] + 3 * u * u * t * c1[1] + 3 * u * t * t * c2[1] + t * t * t * p2[1],
      ]);
    }
  }
  return out;
}

export function polyD(P: V2[]) {
  return P.map((p, i) => `${i ? 'L' : 'M'}${p[0].toFixed(1)} ${p[1].toFixed(1)}`).join('');
}

/** arrowhead triangle at end of polyline */
export function headD(P: V2[], len = 11, wid = 8) {
  const n = P.length;
  if (n < 2) return '';
  const b = P[n - 1];
  let a = P[n - 2];
  for (let i = n - 2; i >= 0; i--) { a = P[i]; if (Math.hypot(b[0] - a[0], b[1] - a[1]) > 2) break; }
  const dx = b[0] - a[0], dy = b[1] - a[1], L = Math.hypot(dx, dy) || 1;
  const ux = dx / L, uy = dy / L;
  const bx = b[0] - ux * len, by = b[1] - uy * len;
  return `M${b[0]} ${b[1]}L${bx - uy * wid / 2} ${by + ux * wid / 2}L${bx + uy * wid / 2} ${by - ux * wid / 2}Z`;
}

/** trim polyline end by d px */
export function trimEnd(P: V2[], d: number): V2[] {
  const out = P.slice();
  let rem = d;
  while (out.length >= 2 && rem > 0) {
    const b = out[out.length - 1], a = out[out.length - 2];
    const L = Math.hypot(b[0] - a[0], b[1] - a[1]);
    if (L > rem) { const f = (L - rem) / L; out[out.length - 1] = [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f]; return out; }
    out.pop(); rem -= L;
  }
  return out;
}

/** TacOps-like broad attack arrow outline (screen space) */
export function attackArrowD(P: V2[], hw = 7) {
  if (P.length < 2) return '';
  const tip = P[P.length - 1];
  const hl = hw * 2.6;
  const body = trimEnd(P, hl);
  if (body.length < 2) return '';
  const n = body.length;
  const L: V2[] = [], R: V2[] = [];
  const norm = (a: V2, b: V2): V2 => { const dx = b[0] - a[0], dy = b[1] - a[1], l = Math.hypot(dx, dy) || 1; return [-dy / l, dx / l]; };
  for (let i = 0; i < n; i++) {
    let nx: V2;
    if (i === 0) nx = norm(body[0], body[1]);
    else if (i === n - 1) nx = norm(body[n - 2], body[n - 1]);
    else {
      const a = norm(body[i - 1], body[i]), b = norm(body[i], body[i + 1]);
      const m: V2 = [a[0] + b[0], a[1] + b[1]]; const ml = Math.hypot(m[0], m[1]) || 1;
      const cos = (m[0] / ml) * a[0] + (m[1] / ml) * a[1];
      const k = 1 / Math.max(0.35, cos);
      nx = [(m[0] / ml) * k, (m[1] / ml) * k];
    }
    // taper slightly from tail to head
    const w = hw * (0.75 + 0.25 * (i / Math.max(1, n - 1)));
    L.push([body[i][0] + nx[0] * w, body[i][1] + nx[1] * w]);
    R.push([body[i][0] - nx[0] * w, body[i][1] - nx[1] * w]);
  }
  const e = body[n - 1];
  const nE = norm(body[n - 2], body[n - 1]);
  const wl: V2 = [e[0] + nE[0] * hw * 2, e[1] + nE[1] * hw * 2];
  const wr: V2 = [e[0] - nE[0] * hw * 2, e[1] - nE[1] * hw * 2];
  const pts = [...L, wl, tip, wr, ...R.reverse()];
  return polyD(pts) + 'Z';
}

export function segDist(p: V2, a: V2, b: V2) {
  const dx = b[0] - a[0], dy = b[1] - a[1];
  const l2 = dx * dx + dy * dy;
  let t = l2 ? ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2 : 0;
  t = Math.max(0, Math.min(1, t));
  return Math.hypot(p[0] - (a[0] + t * dx), p[1] - (a[1] + t * dy));
}
export function polyDist(p: V2, P: V2[]) {
  let m = Infinity;
  for (let i = 0; i < P.length - 1; i++) m = Math.min(m, segDist(p, P[i], P[i + 1]));
  return m;
}
export function wedgeD(o: V2, bearingRad: number, half: number, r: number) {
  // bearing measured from north (screen -y), clockwise
  const pt = (a: number): V2 => [o[0] + Math.sin(a) * r, o[1] - Math.cos(a) * r];
  const a0 = pt(bearingRad - half), a1 = pt(bearingRad + half);
  return `M${o[0]} ${o[1]}L${a0[0]} ${a0[1]}A${r} ${r} 0 0 1 ${a1[0]} ${a1[1]}Z`;
}
