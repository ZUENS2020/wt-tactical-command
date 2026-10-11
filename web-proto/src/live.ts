// Live client: WebSocket to the receiver (/api/live) → Snapshot (same shape as the demo sim) + shared drawings sync.
import { MAPS, CANON, setSeparateMaps, type MapId, type V2 } from './geo';
import { getState, setState, toast, type Drawing } from './store';
import type { Snapshot, Friend, Contact, Ping, Call, FeedLine, Collector, Conn, Marks } from './sim';
import { EXTRA_EMPTY } from './sim';

const LS_KEY = 'wt-room-key', LS_ROOM = 'wt-room', LS_NICK = 'wt-nick';
const q = new URLSearchParams(location.search);
if (q.get('key')) { localStorage.setItem(LS_KEY, q.get('key')!); q.delete('key'); history.replaceState(null, '', location.pathname + (q.toString() ? '?' + q : '')); }
export const cfg = {
  key: () => localStorage.getItem(LS_KEY) ?? '',
  room: () => q.get('room') || localStorage.getItem(LS_ROOM) || 'main',
  nick: () => localStorage.getItem(LS_NICK) || '队员',
  set(key: string, room: string, nick: string) {
    localStorage.setItem(LS_KEY, key.trim());
    localStorage.setItem(LS_ROOM, room.trim() || 'main');
    localStorage.setItem(LS_NICK, nick.trim() || '队员');
    if (q.get('room')) { q.delete('room'); history.replaceState(null, '', location.pathname + (q.toString() ? '?' + q : '')); }
    reconnect();
  },
};

export const FRIEND_SIDC: Record<string, string> = {
  LightTank: 'SFGPUCA---', MediumTank: 'SFGPUCA---', HeavyTank: 'SFGPUCA---', TankDestroyer: 'SFGPUCAA--', Tracked: 'SFGPUCA---',
  SPAA: 'SFGPUCD---', Wheeled: 'SFGPUCAW--', Fighter: 'SFAPMFF---', Bomber: 'SFAPMFB---', Assault: 'SFAPMFA---', Helicopter: 'SFAPMH----',
};

interface Unit { id: string; side: 'self' | 'squad' | 'ally' | 'enemy'; map: MapId; x: number; y: number; hdg: number | null; icon: string; air: boolean; age: number;
  nick?: string; dead?: boolean; since?: number; vx?: number; vy?: number; spawn_m?: number | null; spectating?: boolean; signal?: boolean }
interface MapJ { kind: MapId; hash: string | null; url: string | null; info: { min: V2; max: V2; grid_zero: V2; grid_steps: V2; grid_size: V2; hud_type: number; gen: string }; since: number }
interface StateMsg {
  type: 'state'; t: number; room: string; battle: string | null; battle_started: number | null; in_battle: boolean;
  maps: Partial<Record<MapId, MapJ>> | null; units: Unit[];
  pings: { id: string; t: number; t_end: number; map: MapId; x: number; y: number; area: [number, number, number, number] | null; grid: string | null; text: string }[];
  calls: { id: string; t: number; t_end: number; map: MapId; x: number; y: number; text: string; grid: string | null }[];
  feed: { id: string; t: number; text: string; me: boolean; dead: boolean }[];
  toasts: { id: string; text: string }[];
  collectors: { session: string; player: string; nick: string; replay: boolean; in_battle: boolean; member: boolean; age_ms: number; lag_ms: number | null; delay_ms: number | null; hz: number; paused: boolean; map: string | null }[];
  viewers: number;
  marks?: Partial<Record<MapId, Marks>>;
}

let ws: WebSocket | null = null;
let conn: Conn = 'connecting';
let last: StateMsg | null = null;
let lastAt = 0;              // performance.now() of last state
let offset = 0;              // server_ms - Date.now()
let offsetSet = false;
let rtt: number | null = null;
let me = '';
let battleId: string | null = null;
let battleStarted: number | null = null;
let retry = 0;
let timer: number | undefined;
let pingTimer: number | undefined;
const seenToasts = new Set<string>();
const listeners = new Set<() => void>();
let snapshot: Snapshot = empty();

export const serverNow = () => Date.now() + offset;
export const battleTime = () => (battleStarted ? Math.max(0, (serverNow() - battleStarted) / 1000) : 0);
export const getSnapshot = () => snapshot;
export const onSnapshot = (l: () => void) => { listeners.add(l); return () => { listeners.delete(l); }; };
export const currentBattle = () => battleId;
export const clientId = () => me;
const emit = () => listeners.forEach((l) => l());

function empty(): Snapshot {
  return { t: 0, battle: 0, friends: [], contacts: [], airfield: null, bombing: [], stale: false, ageMs: 0, latency: 0, hz: 0, ...EXTRA_EMPTY,
    conn, inBattle: false, viewers: 0, room: cfg.room(), battleId, rtt, marks: {} };
}

export function apiUrl(path: string) {
  return path + (path.includes('?') ? '&' : '?') + 'key=' + encodeURIComponent(cfg.key());
}

// ------------------------------------------------------------------ maps
function applyMaps(maps: StateMsg['maps']) {
  for (const k of ['ground', 'air'] as MapId[]) {
    const m = maps?.[k];
    const cur = MAPS[k];
    if (!m) { if (cur.available) { MAPS[k] = { ...cur, available: false, hash: null }; setState((s) => ({ views: { ...s.views, [k]: null } })); } continue; }
    const i = m.info;
    const sig = (m.hash ?? 'nohash') + '|' + i.gen + '|' + i.min.join(',') + i.max.join(',');
    const prevSig = (cur.hash ?? 'nohash') + '|' + (cur as unknown as { gen?: string }).gen + '|' + cur.min.join(',') + cur.max.join(',');
    const gridSame = cur.gridZero[0] === i.grid_zero[0] && cur.gridZero[1] === i.grid_zero[1] && cur.gridSteps[0] === i.grid_steps[0] && cur.gridSize[0] === i.grid_size[0];
    if (cur.available && sig === prevSig && gridSame) continue;
    const span = i.max[0] - i.min[0];
    const newMap = !cur.available || cur.min[0] !== i.min[0] || cur.max[0] !== i.max[0] || (cur.hash && m.hash && cur.hash !== m.hash);
    MAPS[k] = {
      id: k, title: k === 'ground' ? '陆战' : '空战', sub: `${(span / 1000).toFixed(span >= 10000 ? 0 : 1)} km · ${m.hash ? m.hash.slice(0, 6) : '底图加载中'}`,
      img: m.url ? apiUrl(m.url) : '', min: i.min, max: i.max, gridZero: i.grid_zero, gridSteps: i.grid_steps, gridSize: i.grid_size,
      hudType: i.hud_type, imgPx: 2048, available: true, hash: m.hash, ...({ gen: i.gen } as object),
    };
    if (newMap) setState((s) => ({ views: { ...s.views, [k]: null }, mapRev: s.mapRev + 1 }));
    else setState((s) => ({ mapRev: s.mapRev + 1 }));
  }
}

// ------------------------------------------------------------------ snapshot
const n2c = (x: number, y: number): V2 => [x * CANON, y * CANON];
function build(): Snapshot {
  const now = performance.now();
  const s = last;
  if (!s) return { ...empty(), conn };
  const el = now - lastAt;                       // ms since the state arrived
  const sNow = s.t + el;
  const friends: Friend[] = [];
  const contacts: Contact[] = [];
  for (const u of s.units) {
    const age = u.age + el;
    if (u.side === 'enemy') {
      contacts.push({ id: u.id, icon: u.icon, air: u.air, w: n2c(u.x, u.y), hdg: u.hdg, blink: 0, visible: age < 1500, reveal: u.air ? 'air' : 'static',
        ageMs: age, sinceMs: u.since ?? 0, vel: [(u.vx ?? 0) * CANON, (u.vy ?? 0) * CANON], speed: Math.hypot(u.vx ?? 0, u.vy ?? 0) * CANON, real: true, only: u.map, spawnM: u.spawn_m });
    } else {
      const self = u.side === 'self';
      friends.push({ id: u.id, nick: self ? (u.nick ?? '我') : u.side === 'squad' ? '小队' : '', air: u.air, sidc: FRIEND_SIDC[u.icon] ?? (u.air ? 'SFAPMFF---' : 'SFGPUCA---'),
        w: n2c(u.x, u.y), hdg: u.hdg, self, alive: !u.dead, ageMs: age, real: true, vehicle: u.icon, role: u.side, only: u.map, spectating: u.spectating });
    }
  }
  const pings: Ping[] = s.pings.map((p) => ({ id: p.id, only: p.map, w: n2c(p.x, p.y), area: p.area ? [n2c(p.area[0], p.area[1]), n2c(p.area[2], p.area[3])] as [V2, V2] : null,
    grid: p.grid, remainMs: p.t_end - sNow, ttlMs: p.t_end - p.t, text: p.text })).filter((p) => p.remainMs > 0);
  const calls: Call[] = s.calls.map((c) => ({ id: c.id, only: c.map, w: n2c(c.x, c.y), text: c.text, grid: c.grid, remainMs: c.t_end - sNow, ttlMs: c.t_end - c.t })).filter((c) => c.remainMs > 0);
  const feed: FeedLine[] = s.feed.map((f) => ({ id: f.id, text: f.text, me: f.me, dead: f.dead, ageMs: sNow - f.t }));
  const collectors: Collector[] = s.collectors.map((c) => ({ id: c.session, nick: c.nick, player: c.player, inBattle: c.in_battle, member: c.member,
    ageMs: c.age_ms + el, lagMs: c.lag_ms == null ? null : c.lag_ms + el, delayMs: c.delay_ms, hz: c.hz, paused: c.paused, replay: c.replay, map: c.map }));
  const act = collectors.filter((c) => c.member);
  const best = act.length ? act.reduce((a, b) => ((a.lagMs ?? 9e9) < (b.lagMs ?? 9e9) ? a : b)) : null;
  const dataAge = best?.lagMs ?? 0;
  const socketAge = el;
  const stale = conn !== 'live' || socketAge > 2500 || (s.in_battle && dataAge > 2000);
  const latency = Math.round((best?.delayMs ?? 0) + (rtt ?? 0) / 2);
  return { t: sNow, battle: battleTime(), friends, contacts, airfield: null, bombing: [], stale, ageMs: s.in_battle ? Math.max(dataAge, socketAge) : socketAge,
    latency, hz: best ? Math.round(best.hz) : 0, pings, calls, feed, collectors, conn: conn === 'live' && !s.in_battle ? 'idle' : conn,
    inBattle: s.in_battle, viewers: s.viewers, room: s.room, battleId: s.battle, rtt, marks: s.marks ?? {} };
}
setInterval(() => { snapshot = build(); emit(); }, 100);

// ------------------------------------------------------------------ socket
function setConn(c: Conn) { conn = c; snapshot = build(); emit(); }

export function reconnect() {
  clearTimeout(timer);
  retry = 0;
  if (ws) { ws.onclose = null; ws.close(); ws = null; }
  last = null;
  connect();
}

function connect() {
  const key = cfg.key();
  if (!key) { setConn('auth'); setState({ sheet: 'settings' }); return; }
  setConn('connecting');
  const proto = location.protocol === 'https:' ? 'wss:' : 'ws:';
  const url = `${proto}//${location.host}/api/live?room=${encodeURIComponent(cfg.room())}&key=${encodeURIComponent(key)}&nick=${encodeURIComponent(cfg.nick())}`;
  const sock = new WebSocket(url);
  ws = sock;
  let opened = false;
  sock.onopen = () => { opened = true; retry = 0; setConn('live'); sendPing(); };
  sock.onmessage = (ev) => { try { onMsg(JSON.parse(ev.data)); } catch (e) { console.warn('bad live msg', e); } };
  sock.onclose = () => {
    if (ws !== sock) return;
    ws = null;
    if (!opened) {
      // handshake refused: check whether the password is wrong
      fetch(apiUrl('/api/live/check')).then((r) => { if (r.status === 401) { setConn('auth'); setState({ sheet: 'settings' }); toast('房间密码错误', { tone: 'warn' }); } else schedule(); }).catch(() => schedule());
      return;
    }
    schedule();
  };
}
function schedule() {
  setConn('offline');
  const d = Math.min(10000, 600 * 2 ** retry++);
  timer = window.setTimeout(connect, d);
}
function sendPing() {
  clearInterval(pingTimer);
  const p = () => { if (ws?.readyState === 1) ws.send(JSON.stringify({ op: 'ping', c: performance.now() })); };
  p();
  pingTimer = window.setInterval(p, 3000);
}

function onMsg(m: { type: string; [k: string]: unknown }) {
  switch (m.type) {
    case 'hello': {
      const h = m as unknown as { you?: string; battle: string | null; battle_started: number | null; drawings: Drawing[]; server_ms: number };
      if (h.you) me = h.you;
      if (!offsetSet) { offset = h.server_ms - Date.now(); offsetSet = true; }
      const changed = battleId !== null && battleId !== h.battle;
      battleId = h.battle; battleStarted = h.battle_started;
      setState({ drawings: h.drawings ?? [], undo: [], selectedId: null, moving: false });
      if (changed) toast('新的对局：标绘已切换', { tone: 'info' });
      break;
    }
    case 'state': {
      const s = m as unknown as StateMsg;
      last = s; lastAt = performance.now();
      if (s.battle !== battleId) { battleId = s.battle; battleStarted = s.battle_started; }
      applyMaps(s.maps);
      for (const t of s.toasts) if (!seenToasts.has(t.id)) { seenToasts.add(t.id); toast(t.text, { tone: 'info' }); }
      break;
    }
    case 'pong': {
      const p = m as unknown as { c: number; server_ms: number };
      rtt = Math.round(performance.now() - p.c);
      offset = p.server_ms + rtt / 2 - Date.now();
      offsetSet = true;
      break;
    }
    case 'draw': {
      const d = m as unknown as { op: 'put' | 'del'; battle: string; d?: Drawing; id?: string; old?: Drawing; by?: string; by_client: string; created?: boolean };
      if (d.battle !== battleId) break;
      const mine = d.by_client === me;
      if (d.op === 'put' && d.d) {
        const dd = d.d;
        setState((s) => ({ drawings: s.drawings.some((x) => x.id === dd.id) ? s.drawings.map((x) => (x.id === dd.id ? (mine ? { ...x, sv: dd.sv } as Drawing : dd) : x)) : [...s.drawings, dd] }));
        if (!mine && d.created) remoteNote?.('add', dd);
      } else if (d.op === 'del' && d.id) {
        const id = d.id;
        setState((s) => ({ drawings: s.drawings.filter((x) => x.id !== id), selectedId: s.selectedId === id ? null : s.selectedId }));
        if (!mine && d.old) remoteNote?.('del', d.old);
      }
      break;
    }
    case 'error': {
      const e = m as unknown as { error: string };
      if (e.error !== 'battle_changed' && e.error !== 'not found') toast('同步失败：' + e.error, { tone: 'warn' });
      break;
    }
  }
}

let remoteNote: ((k: 'add' | 'del', d: Drawing) => void) | null = null;
export function onRemote(f: (k: 'add' | 'del', d: Drawing) => void) { remoteNote = f; }

export function syncPut(d: Drawing) {
  if (ws?.readyState !== 1) { toast('未连接：标绘仅保存在本地，重连后请重新绘制', { tone: 'warn' }); return; }
  ws.send(JSON.stringify({ op: 'put', battle: battleId, d }));
}
export function syncDel(id: string, by: string) {
  if (ws?.readyState !== 1) { toast('未连接：删除未同步', { tone: 'warn' }); return; }
  ws.send(JSON.stringify({ op: 'del', battle: battleId, id, by }));
}

export function startLive() {
  setSeparateMaps(true);
  for (const k of ['ground', 'air'] as MapId[]) MAPS[k] = { ...MAPS[k], available: false, hash: null, img: '' };
  // stored views belong to the demo maps
  setState({ views: { ground: null, air: null }, drawings: [] });
  connect();
  void getState;
}
