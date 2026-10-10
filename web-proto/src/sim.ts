// Live layer simulation. Enemy contacts + own track + one air member are driven by the
// real 8111 recording (b6a7, 2026-10-10); the other 6 squadron members are synthetic.
import replay from './data/replay.json';
import { normToCanon, type V2 } from './geo';

type Smp = [number, number, number, number, number | null];
interface Track { id: number; icon: string; type: string; s: Smp[]; end: number; }
interface Seg { t0: number; dur: number; self: [number, number, number, number | null, string][]; tracks: Track[]; airfield: number[] | null; bombing: [number, number, number][]; }
const R = replay as unknown as { ground: Seg; air: Seg };

export type Role = 'self' | 'squad' | 'ally';
export interface Friend {
  id: string; nick: string; air: boolean; sidc: string; w: V2; hdg: number | null;
  self: boolean; alive: boolean; ageMs: number; real: boolean; vehicle: string;
  role: Role; only?: 'ground' | 'air'; spectating?: boolean;
}
export type Reveal = 'static' | 'highlight' | 'air';
export interface Contact {
  id: string; icon: string; air: boolean; w: V2; hdg: number | null; blink: number;
  visible: boolean; reveal: Reveal; ageMs: number; remainMs?: number; sinceMs: number;
  speed?: number; vel?: V2; real: boolean; only?: 'ground' | 'air';
  /** distance (m) to the nearest own tank respawn point: live red markers are spawn-proximity reveals (压家) */
  spawnM?: number | null;
}
export interface Ping { id: string; only: 'ground' | 'air'; w: V2; area: [V2, V2] | null; grid: string | null; remainMs: number; ttlMs: number; text: string; }
export interface Call { id: string; only: 'ground' | 'air'; w: V2; text: string; grid: string | null; remainMs: number; ttlMs: number; }
export interface FeedLine { id: string; text: string; me: boolean; dead: boolean; ageMs: number; }
export interface Collector { id: string; nick: string; player: string; inBattle: boolean; member: boolean; ageMs: number; lagMs: number | null; delayMs: number | null; hz: number; paused: boolean; replay: boolean; map: string | null; }
export type Conn = 'demo' | 'connecting' | 'live' | 'idle' | 'offline' | 'auth';
export interface Snapshot {
  t: number;              // sim ms
  battle: number;         // battle seconds
  friends: Friend[];
  contacts: Contact[];    // visible + ghosts (policy filtering happens at render time)
  airfield: [V2, V2] | null;
  bombing: V2[];
  stale: boolean; ageMs: number; latency: number; hz: number;
  pings: Ping[]; calls: Call[]; feed: FeedLine[];
  conn: Conn; inBattle: boolean; collectors: Collector[]; viewers: number; room: string; battleId: string | null;
  rtt: number | null;
}
export const EXTRA_EMPTY = { pings: [] as Ping[], calls: [] as Call[], feed: [] as FeedLine[], collectors: [] as Collector[] };

const BATTLE_T0 = 372; // battle clock starts at 06:12
const start = performance.now();
export const simNow = () => performance.now() - start;
export const battleTime = () => BATTLE_T0 + simNow() / 1000;

function sampleAt(s: Smp[], t: number): Smp {
  let lo = 0, hi = s.length - 1;
  if (t <= s[0][0]) return s[0];
  while (lo < hi) { const m = (lo + hi + 1) >> 1; if (s[m][0] <= t) lo = m; else hi = m - 1; }
  return s[lo];
}

// --------- synthetic squadron members (ground norm loops) ----------
interface Loop { pts: V2[]; speed: number; len: number; segs: number[]; }
function mkLoop(ptsN: V2[], speed: number): Loop {
  const pts = ptsN.map((p) => [p[0] * 4096, p[1] * 4096] as V2);
  const segs: number[] = []; let len = 0;
  for (let i = 0; i < pts.length; i++) { const a = pts[i], b = pts[(i + 1) % pts.length]; const d = Math.hypot(b[0] - a[0], b[1] - a[1]); segs.push(d); len += d; }
  return { pts, speed, len, segs };
}
function onLoop(l: Loop, tSec: number, phase = 0): { w: V2; hdg: number } {
  let d = ((tSec * l.speed + phase * l.len) % l.len + l.len) % l.len;
  for (let i = 0; i < l.pts.length; i++) {
    if (d <= l.segs[i]) {
      const a = l.pts[i], b = l.pts[(i + 1) % l.pts.length], f = d / l.segs[i];
      const hdg = (Math.atan2(b[0] - a[0], -(b[1] - a[1])) * 180 / Math.PI + 360) % 360;
      return { w: [a[0] + (b[0] - a[0]) * f, a[1] + (b[1] - a[1]) * f], hdg };
    }
    d -= l.segs[i];
  }
  return { w: l.pts[0], hdg: 0 };
}
const LOOPS = {
  sishen: mkLoop([[0.395, 0.505], [0.452, 0.474], [0.474, 0.522], [0.432, 0.552]], 7.5),
  tiezhen: mkLoop([[0.392, 0.604], [0.441, 0.618], [0.458, 0.662], [0.405, 0.676]], 6),
  yexiao: mkLoop([[0.498, 0.425], [0.548, 0.438], [0.536, 0.482], [0.49, 0.468]], 9),
  baihua: mkLoop([[0.455, 0.585], [0.488, 0.598], [0.47, 0.63]], 5),
};

// --------- air member 青鸟 = real IA-58 track + Hermite closing curve ----------
const AIR = R.air, GND = R.ground;
const AIR_RET = 26000; // ms closing curve
const AIR_PERIOD = AIR.dur + AIR_RET;
const GND_HOLD = 20000;
const GND_PERIOD = GND.dur + GND_HOLD;
function airSelfAt(tau: number): { n: V2; hdg: number; real: boolean } {
  const s = AIR.self;
  if (tau <= AIR.dur) {
    let i = 0; while (i < s.length - 1 && s[i + 1][0] <= tau) i++;
    const a = s[i], b = s[Math.min(i + 1, s.length - 1)];
    const f = b[0] > a[0] ? Math.min(1, Math.max(0, (tau - a[0]) / (b[0] - a[0]))) : 0;
    return { n: [a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f], hdg: a[3] ?? 0, real: true };
  }
  const u = (tau - AIR.dur) / AIR_RET;
  const e = s[s.length - 1], b0 = s[0];
  const v = 7.55e-4 * AIR_RET / 1000 * 0.9; // ~99 m/s in air-norm units, scaled to curve time
  const dir = (h: number) => [Math.sin(h * Math.PI / 180), -Math.cos(h * Math.PI / 180)];
  const d1 = dir(e[3] ?? 0), d0 = dir(b0[3] ?? 0);
  const P0: V2 = [e[1], e[2]], P1: V2 = [b0[1], b0[2]];
  const m0 = [d1[0] * v, d1[1] * v], m1 = [d0[0] * v, d0[1] * v];
  const h00 = 2 * u ** 3 - 3 * u ** 2 + 1, h10 = u ** 3 - 2 * u ** 2 + u, h01 = -2 * u ** 3 + 3 * u ** 2, h11 = u ** 3 - u ** 2;
  const x = h00 * P0[0] + h10 * m0[0] + h01 * P1[0] + h11 * m1[0];
  const y = h00 * P0[1] + h10 * m0[1] + h01 * P1[1] + h11 * m1[1];
  const dh00 = 6 * u ** 2 - 6 * u, dh10 = 3 * u ** 2 - 4 * u + 1, dh01 = -6 * u ** 2 + 6 * u, dh11 = 3 * u ** 2 - 2 * u;
  const dx = dh00 * P0[0] + dh10 * m0[0] + dh01 * P1[0] + dh11 * m1[0];
  const dy = dh00 * P0[1] + dh10 * m0[1] + dh01 * P1[1] + dh11 * m1[1];
  return { n: [x, y], hdg: (Math.atan2(dx, -dy) * 180 / Math.PI + 360) % 360, real: false };
}

const SIDC = {
  tank: 'SFGPUCA---', spaa: 'SFGPUCD---', fighter: 'SFAPMFF---', attack: 'SFAPMFA---',
};

// ghost memory
interface Mem { w: V2; t: number; icon: string; air: boolean; hdg: number | null; speed: number; reveal: Reveal; real: boolean; only?: 'ground' | 'air'; }
const mem = new Map<string, Mem>();
const prevPos = new Map<string, { w: V2; t: number }>();

let snapshot: Snapshot = build(0);
const listeners = new Set<() => void>();
export const getSnapshot = () => snapshot;
export const onSnapshot = (l: () => void) => { listeners.add(l); return () => { listeners.delete(l); }; };

// stall simulation (real data: every map load stalls the 8111 HTTP for ~3 s)
const STALL_EVERY = 47000, STALL_LEN = 3200, STALL_FIRST = 31000;
function inStall(t: number) {
  if (t < STALL_FIRST) return false;
  return ((t - STALL_FIRST) % STALL_EVERY) < STALL_LEN;
}
let lastGood = 0;

function build(t: number): Snapshot {
  const battle = BATTLE_T0 + t / 1000;
  const ts = t / 1000;
  const friends: Friend[] = [];
  const jitter = (k: number) => 70 + ((Math.sin(t / 900 + k * 7.3) + 1) * 60) | 0;

  // self — real ground replay (Tiger H1, b6a7)
  const tg = t % GND_PERIOD;
  const gs = GND.self;
  const tt = Math.min(tg, GND.dur);
  let sIdx = 0; while (sIdx < gs.length - 1 && gs[sIdx + 1][0] <= tt) sIdx++;
  const so = gs[sIdx];
  friends.push({ id: 'm1', nick: '朔风', air: false, sidc: SIDC.tank, w: [so[1] * 4096, so[2] * 4096], hdg: so[3] ?? 70, self: true, alive: true, ageMs: jitter(1), real: true, vehicle: '虎 H1', role: 'self' });

  const L = (id: string, nick: string, loop: Loop, phase: number, sidc: string, veh: string) => {
    const p = onLoop(loop, ts, phase);
    friends.push({ id, nick, air: false, sidc, w: p.w, hdg: p.hdg, self: false, alive: true, ageMs: jitter(id.charCodeAt(1)), real: false, vehicle: veh, role: id === 'm2' || id === 'm3' ? 'squad' : 'ally' });
  };
  L('m2', '死神', LOOPS.sishen, 0.1, SIDC.tank, '豹 1');
  L('m3', '铁砧', LOOPS.tiezhen, 0.6, SIDC.tank, 'T-72A');
  L('m4', '夜枭', LOOPS.yexiao, 0.3, SIDC.tank, 'M41A1');
  friends.push({ id: 'm5', nick: '雷鸣', air: false, sidc: SIDC.spaa, w: [0.405 * 4096, 0.586 * 4096], hdg: (80 + Math.sin(ts / 8) * 25 + 360) % 360, self: false, alive: true, ageMs: jitter(5), real: false, vehicle: '猎豹' , role: 'ally' });
  // 白桦: dead for 70 s of each 150 s cycle → greyed last known position + age
  {
    const cyc = ts % 150;
    if (cyc < 70) {
      friends.push({ id: 'm6', nick: '白桦', air: false, sidc: SIDC.tank, w: [0.488 * 4096, 0.598 * 4096], hdg: 95, self: false, alive: false, ageMs: (cyc + 12) * 1000, real: false, vehicle: 'M18' , role: 'ally' });
    } else {
      const p = onLoop(LOOPS.baihua, cyc - 70, 0.33);
      friends.push({ id: 'm6', nick: '白桦', air: false, sidc: SIDC.tank, w: p.w, hdg: p.hdg, self: false, alive: true, ageMs: jitter(6), real: false, vehicle: 'M18' , role: 'ally' });
    }
  }
  // 青鸟 — real IA-58 air track
  {
    const a = airSelfAt(t % AIR_PERIOD);
    friends.push({ id: 'm7', nick: '青鸟', air: true, sidc: SIDC.attack, w: normToCanon('air', a.n), hdg: a.hdg, self: false, alive: true, ageMs: jitter(7), real: a.real, vehicle: 'IA-58A' , role: 'ally' });
  }
  // 孤狼 — synthetic fighter orbit (air norm circle)
  {
    const ang = ts * 0.085;
    const c: V2 = [0.672, 0.503], r = 0.011;
    const n: V2 = [c[0] + Math.cos(ang) * r, c[1] + Math.sin(ang) * r];
    const hdg = (Math.atan2(-Math.sin(ang), -Math.cos(ang)) * 180 / Math.PI + 360) % 360;
    friends.push({ id: 'm8', nick: '孤狼', air: true, sidc: SIDC.fighter, w: normToCanon('air', n), hdg, self: false, alive: true, ageMs: jitter(8), real: false, vehicle: 'F-86F' , role: 'ally' });
  }

  // ---------- contacts ----------
  const contacts: Contact[] = [];
  const seen = new Set<string>();
  const push = (id: string, icon: string, air: boolean, w: V2, hdg: number | null, blink: number, reveal: Reveal, real: boolean, sinceMs: number, remainMs?: number, only?: 'ground' | 'air') => {
    seen.add(id);
    const pp = prevPos.get(id);
    let speed = 0;
    if (pp && t - pp.t > 0) speed = Math.hypot(w[0] - pp.w[0], w[1] - pp.w[1]) / ((t - pp.t) / 1000);
    prevPos.set(id, { w, t });
    mem.set(id, { w, t, icon, air, hdg, speed: speed || (mem.get(id)?.speed ?? 0), reveal, real, only });
    contacts.push({ id, icon, air, w, hdg, blink, visible: true, reveal, ageMs: 0, sinceMs, remainMs, real, only });
  };
  // ground replay targets
  const gtau = Math.min(tg, GND.dur);
  for (const tr of GND.tracks) {
    const first = tr.s[0][0];
    const ended = tr.end < GND.dur && tg > tr.end;
    if (tg < first || ended) continue;
    const s = sampleAt(tr.s, gtau);
    push('g' + tr.id, tr.icon, false, [s[1] * 4096, s[2] * 4096], s[4], s[3], s[3] === 2 ? 'highlight' : 'static', true, tg - first);
  }
  // air replay contacts
  const ta = t % AIR_PERIOD;
  for (const tr of AIR.tracks) {
    const first = tr.s[0][0];
    const isAir = tr.type === 'aircraft';
    if (ta < first) continue;
    if (isAir && ta > AIR.dur) continue; // lost during the closing curve → ghost/prediction
    const s = sampleAt(tr.s, Math.min(ta, AIR.dur));
    push('a' + tr.id, tr.icon, isAir, normToCanon('air', [s[1], s[2]]), s[4], s[3], isAir ? 'air' : (s[3] === 2 ? 'highlight' : 'static'), true, ta - first, undefined, isAir ? undefined : 'air');
  }
  // ghosts
  for (const [id, m] of mem) {
    if (seen.has(id)) continue;
    const age = t - m.t;
    const life = m.air ? 15000 : 30000;
    if (age > life) { mem.delete(id); prevPos.delete(id); continue; }
    contacts.push({ id, icon: m.icon, air: m.air, w: m.w, hdg: m.hdg, blink: 0, visible: false, reveal: m.reveal, ageMs: age, sinceMs: 0, speed: m.speed, real: m.real, only: m.only });
  }

  const af = AIR.airfield ? [normToCanon('air', [AIR.airfield[0], AIR.airfield[1]]), normToCanon('air', [AIR.airfield[2], AIR.airfield[3]])] as [V2, V2] : null;
  const bombing = AIR.bombing.filter((b) => (t % AIR_PERIOD) >= b[2]).map((b) => normToCanon('air', [b[0], b[1]]));
  return { t, battle, friends, contacts, airfield: af, bombing, stale: false, ageMs: 0, latency: 96 + ((Math.sin(t / 2300) + 1) * 18) | 0, hz: 10,
    ...EXTRA_EMPTY, conn: 'demo', inBattle: true, viewers: 1, room: 'DEMO', battleId: 'demo', rtt: null };
}

function tick() {
  const t = simNow();
  if (inStall(t)) {
    const age = t - lastGood;
    snapshot = { ...snapshot, stale: age > 1000, ageMs: age, battle: BATTLE_T0 + t / 1000,
      friends: snapshot.friends.map((f) => ({ ...f, ageMs: f.alive ? f.ageMs + 100 : f.ageMs + 100 })) };
  } else {
    snapshot = build(t);
    lastGood = t;
  }
  listeners.forEach((l) => l());
}
let started = false;
export function startSim() { if (!started) { started = true; setInterval(tick, 100); } }

export const ICON_ZH: Record<string, string> = {
  LightTank: '轻坦', MediumTank: '中坦', HeavyTank: '重坦', TankDestroyer: '坦歼', SPAA: '防空', Wheeled: '轮式',
  Fighter: '战斗机', Bomber: '轰炸机', Assault: '攻击机', Helicopter: '直升机',
};
export const ICON_SIDC: Record<string, string> = {
  LightTank: 'SHGPUCA---', MediumTank: 'SHGPUCA---', HeavyTank: 'SHGPUCA---', TankDestroyer: 'SHGPUCAA--',
  SPAA: 'SHGPUCD---', Wheeled: 'SHGPUCAW--', Fighter: 'SHAPMFF---', Bomber: 'SHAPMFB---', Assault: 'SHAPMFA---', Helicopter: 'SHAPMH----',
};
