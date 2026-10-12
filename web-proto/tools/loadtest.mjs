// Real wall-clock soak: 8 independent collectors, 8 WebSocket viewers at 10 Hz.
// Every request uses synthetic credentials and an isolated receiver data directory.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { writeFileSync, readdirSync } from 'node:fs';
const base = process.env.WT_TEST_API || 'http://127.0.0.1:18112';
const duration = Number(process.argv[2] || 1800);
const token = 'synthetic-load-upload', key = 'synthetic-load-view';
const headers = { Authorization: `Bearer ${token}`, 'Content-Type': 'application/json' };
const png = Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aR9sAAAAASUVORK5CYII=', 'base64');
const hash = createHash('sha256').update(png).digest('hex');
const img = await fetch(`${base}/api/image?session=load&hash=${hash}`, { method: 'POST', headers, body: png });
assert.equal(img.status, 200, await img.text());
const started = Date.now(), failures = [], sockets = [], viewers = Array.from({ length: 8 }, () => ({ seq: -1, states: 0, map: new Map() }));
const totals = { accepted: 0, dup: 0, out_of_order: 0, too_old: 0, overflow: 0, context_dropped: 0, states: 0, selfSamples: 0, resyncs: 0, posts: 0 };
const devices = Array.from({ length: 8 }, (_, i) => ({ i, device: `load-device-${i}`, session: `load-${i}-${randomUUID()}`, start: Date.now(), seq: 0, generation: 1, last: null, old: null }));
function viewer(i) {
  const ws = new WebSocket(`${base.replace('http', 'ws')}/api/live?room=load&key=${key}`); sockets[i] = ws;
  ws.onmessage = e => { try {
    const m = JSON.parse(e.data), v = viewers[i];
    if (m.type === 'hello') { v.seq = m.state_seq ?? -1; v.map.clear(); totals.resyncs++; return; }
    if (m.type !== 'state') return;
    assert.ok(m.state_seq > v.seq, 'state sequence must advance'); v.seq = m.state_seq; v.states++; totals.states++;
    totals.selfSamples += m.units.filter(u => u.id.startsWith('self:')).length;
    const selves = m.units.filter(u => u.id.startsWith('self:')); assert.equal(new Set(selves.map(u => u.id)).size, selves.length, 'duplicate device identity');
    for (const u of m.units) { assert.ok(u.age <= 2000, 'expired observation resurrected'); if (!u.id.startsWith('self:')) continue;
      const d = devices.find(d => u.id === `self:${d.device}`); if (!d) continue;
      const prior = v.map.get(u.id); if (prior && prior.generation === d.generation && prior.map === u.map) assert.ok(u.x >= prior.x - .00002, 'position regressed');
      v.map.set(u.id, { x: u.x, generation: d.generation, map: u.map });
    }
    const conflicts = m.collectors.reduce((n, c) => n + (c.context_dropped || 0), 0); totals.context_dropped = Math.max(totals.context_dropped, conflicts);
  } catch (e) { failures.push(String(e)); } };
  ws.onerror = () => { if (!ws.intentional && Date.now() - started < duration * 1000) failures.push(`viewer ${i} transport error`); };
  ws.onclose = () => { if (Date.now() - started < duration * 1000) setTimeout(() => viewer(i), 150); };
}
for (let i = 0; i < 8; i++) viewer(i);
function batch(d, now, records) { return { session: d.session, device_id: d.device, nickname: `成员${d.i}`, player: `load-player-${d.i}`, version: '2.0.0', clock: { synced: true, offset_ms: 0, rtt_ms: 1, sync_age_ms: 0 }, records }; }
function record(d, ep, now, body, extra = {}) { return { session: d.session, seq: ++d.seq, mono_ns: Math.max(1, now - d.start) * 1e6, wall_ms: now, ts_server_ms: now, status: 200, epoch: d.generation, map_context: `${d.i}-${d.generation}`, ep, body: typeof body === 'string' ? body : JSON.stringify(body), ...extra }; }
async function post(b) { const r = await fetch(`${base}/api/ingest`, { method: 'POST', headers, body: JSON.stringify(b) }); assert.equal(r.status, 200, await r.clone().text()); const v = await r.json(); totals.posts++; for (const k of ['accepted', 'dup', 'out_of_order']) totals[k] += v[k] || 0; for (const k of ['too_old', 'overflow']) totals[k] += v.dropped?.[k] || 0; }
let tick = 0, next = Date.now();
while (Date.now() - started < duration * 1000) {
  tick++; const now = Date.now(), seconds = (now - started) / 1000;
  await Promise.all(devices.map(async d => {
    // Short disconnections are repeated without resetting the battle; device 0 is restarted periodically.
    if (d.i === 7 && tick % 600 >= 540) return;
    if (d.i === 0 && tick % 1200 === 0) { d.old = d.last; d.session = `load-${d.i}-${randomUUID()}`; d.start = now; d.seq = 0; d.generation++; }
    const phase = Math.floor(seconds / 60), air = (d.i >= 4) !== (d.i === 3 && phase % 2 === 1);
    if (d.phase !== phase) { d.phase = phase; d.generation++; }
    const span = air ? 32000 : 4096, x = .15 + seconds / 6000 + d.i * .025;
    const records = [record(d, '/map_info.json', now, { valid: true, map_generation: d.generation, hud_type: air ? 0 : 1, map_min: [0, 0], map_max: [span, span], grid_zero: [0, span], grid_steps: [span / 8, span / 8], grid_size: [span, span] }), record(d, '/map.img', now, '', { img_hash: hash }), record(d, '/map_obj.json', now, [{ type: air ? 'aircraft' : 'ground_model', icon: 'Player', color: '#faC81E', x, y: .35 + d.i * .04, dx: 1, dy: 0 }, { type: 'ground_model', icon: 'MediumTank', color: '#fa0c00', x: .75, y: .7 }])];
    const b = batch(d, now, records); await post(b);
    if (tick % 97 === 0) await post(b); // exact duplicate
    if (tick % 113 === 0 && d.last) await post(d.last); // within-TTL reorder
    if (tick % 149 === 0) await post(batch(d, now, [record(d, '/map_obj.json', now - 5000, [])])); // expired
    if (tick % 173 === 0) await post(batch(d, now, [record(d, '/map_obj.json', now, [], { map_context: 'wrong-map' })]));
    if (d.old && tick % 127 === 0) { await post(d.old); d.old = null; }
    d.last = b;
  })).catch(e => failures.push(String(e)));
  if (tick % 300 === 0) console.log(JSON.stringify({ seconds: Math.round(seconds), ...totals, failures: failures.length }));
  if (tick % 911 === 0) (() => { if (sockets[2]) { sockets[2].intentional = true; sockets[2].close(); } })(); // reconnect complete snapshot
  if (failures.length) break;
  next += 100; await new Promise(resolve => setTimeout(resolve, Math.max(0, next - Date.now())));
}
for (const ws of sockets) ws?.close();
const result = { durationSeconds: (Date.now() - started) / 1000, tick, collectors: 8, viewers: 8, totals, viewersStates: viewers.map(v => v.states), failures };
writeFileSync(process.env.WT_LOAD_RESULT || 'shots/loadtest-result.json', JSON.stringify(result, null, 2)); console.log(JSON.stringify(result));
assert.equal(failures.length, 0); assert.ok(totals.selfSamples > duration * 10); assert.ok(totals.too_old > 0); assert.ok(totals.out_of_order > 0); assert.ok(viewers.every(v => v.states > duration * 5));
