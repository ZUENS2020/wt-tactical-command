import SYMS from './data/symbols.json';
import type { PointCat, Status } from './store';
import { C } from './theme';

// APP-6 unit symbols pre-rendered by milsymbol at build time (tools/gen_symbols.mjs) → no runtime dependency.
type SymRec = { svg: string; ax: number; ay: number; w: number; h: number };
const LIB = SYMS as Record<string, SymRec>;
const imgs = new Map<string, HTMLImageElement>();
const cache = new Map<string, { c: HTMLCanvasElement; ax: number; ay: number; w: number; h: number }>();
export function unitSprite(sidc: string, size: number, opt: { fill?: string; dashed?: boolean; grey?: boolean } = {}, dpr = 1) {
  const v = opt.grey ? 'g' : opt.fill ? 'self' : opt.dashed ? 'd' : 'n';
  const key = `${sidc}|${v}`;
  const ck = `${key}|${size}|${dpr}`;
  const hit = cache.get(ck);
  if (hit) return hit;
  const rec = LIB[key] ?? LIB[`${sidc}|n`] ?? LIB['SHGPU-----|n'];
  let im = imgs.get(key);
  if (!im) { im = new Image(); im.src = 'data:image/svg+xml;charset=utf-8,' + encodeURIComponent(rec.svg); imgs.set(key, im); }
  const w = rec.w * size, h = rec.h * size;
  if (!im.complete || !im.naturalWidth) return { c: null as unknown as HTMLCanvasElement, ax: rec.ax * size, ay: rec.ay * size, w, h, pending: true };
  const c = document.createElement('canvas');
  c.width = Math.ceil(w * dpr); c.height = Math.ceil(h * dpr);
  c.getContext('2d')!.drawImage(im, 0, 0, c.width, c.height);
  const r = { c, ax: rec.ax * size, ay: rec.ay * size, w, h };
  cache.set(ck, r);
  return r;
}

export const CAT_LABEL: Record<PointCat, string> = {
  rally: '集合', attack: '进攻', defend: '防守', observe: '观察', enemy: '敌军', danger: '危险', airstrike: '空袭', recon: '侦察',
};
export const CAT_ORDER: PointCat[] = ['rally', 'attack', 'defend', 'observe', 'enemy', 'danger', 'airstrike', 'recon'];
export function catColor(cat: PointCat) {
  switch (cat) {
    case 'rally': case 'defend': case 'observe': return C.friend;
    case 'attack': case 'airstrike': return C.order;
    case 'enemy': case 'danger': return C.enemy;
    case 'recon': return C.recon;
  }
}

/** APP-6 style hand-drawn glyphs, centred at (0,0), nominal size ~28px. */
export function PointGlyph({ cat, status = 'present', s = 1 }: { cat: PointCat; status?: Status; s?: number }) {
  const col = catColor(cat);
  const dash = status === 'planned' ? '3.5 2.5' : undefined;
  const sw = 1.9;
  const fr = { stroke: col, strokeWidth: sw, strokeDasharray: dash, fill: 'rgba(250,248,240,0.9)' } as const;
  const txt = (t: string, y = 4, fs = 9.5) => (
    <text x={0} y={y} textAnchor="middle" fontSize={fs} fontWeight={700} fill={col} style={{ fontFamily: 'Arial, sans-serif', letterSpacing: 0.2 }}>{t}</text>
  );
  let body: JSX.Element;
  switch (cat) {
    case 'rally': // friendly frame + RP
      body = <><rect x={-13} y={-9} width={26} height={18} {...fr} fill={C.friendFill} fillOpacity={0.55} />{txt('RP', 3.6)}</>; break;
    case 'defend': // friendly frame + battle position bracket
      body = <><rect x={-13} y={-9} width={26} height={18} {...fr} fill={C.friendFill} fillOpacity={0.55} /><path d="M-7 -4 V4 H7 V-4" fill="none" stroke={col} strokeWidth={sw} />{txt('D', 2.6, 7.5)}</>; break;
    case 'observe': // OP: triangle with dot
      body = <><path d="M0 -12 L12 9 H-12 Z" {...fr} fill={C.friendFill} fillOpacity={0.55} /><circle cx={0} cy={2.5} r={2.6} fill={col} /></>; break;
    case 'attack': // amber objective pin with chevron
      body = <><path d="M0 -13 L11 0 L0 13 L-11 0 Z" {...fr} fill="#f6e2bf" /><path d="M-5 -2.5 L0 -7 L5 -2.5 M-5 3.5 L0 -1 L5 3.5" fill="none" stroke={col} strokeWidth={2} /></>; break;
    case 'enemy': // hostile diamond + armour
      body = <><path d="M0 -13 L13 0 L0 13 L-13 0 Z" {...fr} fill={C.enemyFill} /><ellipse cx={0} cy={0} rx={6.2} ry={3.6} fill="none" stroke={col} strokeWidth={1.7} /></>; break;
    case 'danger': // hazard triangle
      body = <><path d="M0 -12 L12.5 9.5 H-12.5 Z" {...fr} fill="#f8ddd8" />{txt('!', 7, 13)}</>; break;
    case 'airstrike': // target reference: circle + cross
      body = <><circle r={11} {...fr} fill="#f6e2bf" /><path d="M-15 0 H-5 M5 0 H15 M0 -15 V-5 M0 5 V15" stroke={col} strokeWidth={2} /><circle r={2.2} fill={col} /></>; break;
    case 'recon': // recon frame with slash
      body = <><rect x={-13} y={-9} width={26} height={18} {...fr} fill="#bfe4e8" /><path d="M-13 9 L13 -9" stroke={col} strokeWidth={sw} /></>; break;
  }
  return <g transform={s !== 1 ? `scale(${s})` : undefined}>{body}</g>;
}

export function GlyphIcon({ cat, size = 26 }: { cat: PointCat; size?: number }) {
  return <svg width={size} height={size} viewBox="-16 -16 32 32" aria-hidden><PointGlyph cat={cat} /></svg>;
}
