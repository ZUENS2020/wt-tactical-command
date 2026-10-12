// Real frontend socket/image/rendering paths against a controlled current-state server.
import { chromium } from 'playwright-core';
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
const base = process.argv[2] || 'http://127.0.0.1:4180/';
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const results = [];
try {
  for (const viewport of [{ width: 1440, height: 900 }, { width: 390, height: 844 }]) {
    const context = await browser.newContext({ viewport });
    await context.addInitScript(() => {
      localStorage.clear(); localStorage.setItem('wt-room-key', 'first-key'); localStorage.setItem('wt-room', 'regression');
      let seq = 0;
      const state = () => ({ type: 'state', protocol: 2, state_seq: ++seq, draw_revision: 0, t: Date.now(), room: 'regression', battle: 'round-1', battle_started: Date.now(), in_battle: true, maps: {}, units: [], pings: [], calls: [], feed: [], toasts: [], collectors: [], viewers: 1, marks: {} });
      window.__current = state(); window.__sockets = [];
      const hello = s => ({ type: 'hello', protocol: 2, you: 'viewer', server_ms: Date.now(), battle: s.battle, battle_started: s.battle_started, draw_revision: 0, drawings: [], state: s });
      window.__push = patch => {
        const s = window.__current = { ...window.__current, ...patch, state_seq: ++seq, t: Date.now() };
        window.__sockets.at(-1)?.onmessage?.({ data: JSON.stringify(s) });
      };
      window.WebSocket = class {
        constructor() { this.readyState = 0; window.__sockets.push(this); setTimeout(() => { this.readyState = 1; this.onopen?.({}); this.onmessage?.({ data: JSON.stringify(hello({ ...window.__current, state_seq: ++seq, t: Date.now() })) }); }, 0); }
        send(data) {
          const m = JSON.parse(data);
          if (m.op === 'sync') this.onmessage?.({ data: JSON.stringify(hello({ ...window.__current, state_seq: ++seq, t: Date.now() })) });
          if (m.op === 'ping') this.onmessage?.({ data: JSON.stringify({ type: 'pong', c: m.c, server_ms: Date.now() }) });
          if (m.op === 'new_round') {
            window.__current = { ...state(), battle: 'manual-round', maps: {} };
            this.onmessage?.({ data: JSON.stringify(hello(window.__current)) });
          }
        }
        close() { this.readyState = 3; this.onclose?.({}); }
      };
    });
    const page = await context.newPage(), errors = [], requests = [];
    let flaky = 0;
    page.on('pageerror', e => errors.push(e.message));
    await page.route('**/api/map/**', async route => {
      const url = new URL(route.request().url()); requests.push(url.pathname + url.search);
      if (url.pathname.endsWith('/flaky') && flaky++ === 0) return route.fulfill({ status: 503 });
      const color = url.pathname.endsWith('/blue') ? '#4268a0' : '#507050';
      await route.fulfill({ contentType: 'image/svg+xml', body: `<svg xmlns="http://www.w3.org/2000/svg" width="128" height="128"><path fill="${color}" d="M0 0H128V128H0Z"/></svg>` });
    });
    await page.goto(`${base}?mode=ground`);
    await page.waitForFunction(() => !!window.__wtQA && window.__wtQA.live.canEdit());
    await page.evaluate(() => window.__wtQA.store.setState(s => ({ layers: { ...s.layers, grid: false, labels: false } })));
    const map = { kind: 'ground', hash: 'same-hash', url: null, context: 'context-1', since: Date.now(), info: { min: [0, 0], max: [4096, 4096], grid_zero: [0, 4096], grid_steps: [512, 512], grid_size: [4096, 4096], hud_type: 1, gen: '1' } };
    const collectors = [{ session: 'session-a', device_id: 'device-a', player: 'a', nick: '游戏玩家', replay: false, in_battle: true, member: true, age_ms: 0, lag_ms: 0, delay_ms: 0, hz: 10, paused: false, map: 'ground' }];
    const units = [{ id: 'self:device-a', side: 'self', map: 'ground', x: .5, y: .5, hdg: 0, icon: 'Unknown', air: false, age: 0, nick: '游戏玩家' }];
    const push = patch => page.evaluate(patch => window.__push(patch), patch);
    const pixel = layer => page.evaluate(layer => {
      const c = document.querySelector(`canvas[data-layer=${layer}]`); return [...c.getContext('2d').getImageData(Math.floor(c.width / 2), Math.floor(c.height / 2), 1, 1).data];
    }, layer);
    const waitColor = async (layer, color) => {
      await page.waitForFunction(({ layer, color }) => {
        const c = document.querySelector(`canvas[data-layer=${layer}]`), p = c.getContext('2d').getImageData(Math.floor(c.width / 2), Math.floor(c.height / 2), 1, 1).data;
        return color.every((v, i) => p[i] === v);
      }, { layer, color });
    };
    await push({ maps: { ground: map }, collectors, units });
    await page.waitForFunction(() => window.__wtQA.live.getSnapshot().friends[0]?.self);
    await waitColor('units', [250, 200, 30]);
    await push({ maps: { ground: { ...map, url: '/api/map/green' } }, collectors, units });
    await waitColor('base', [80, 112, 80]); // Late image URL with unchanged hash/gen must load.
    assert.ok(requests.some(r => r.includes('/green?key=first-key')));
    await page.evaluate(() => window.__wtQA.live.cfg.set('second-key', 'regression', '队员'));
    await page.waitForFunction(() => window.__wtQA.geo.MAPS.ground.img.includes('second-key'));
    await page.waitForTimeout(150);
    await waitColor('base', [80, 112, 80]);
    assert.ok(requests.some(r => r.includes('/green?key=second-key')), 'reconnection must refresh the authenticated image URL');
    // Manual round enters an empty state, then receives a new image of the same dimensions.
    await page.evaluate(() => window.__wtQA.live.newRound());
    await page.waitForFunction(() => !window.__wtQA.geo.MAPS.ground.available);
    await waitColor('base', [222, 222, 209]);
    assert.equal((await pixel('units'))[3], 0, 'old positions cleared with the new snapshot');
    const blue = { ...map, url: '/api/map/blue' };
    await push({ maps: { ground: blue }, collectors, units });
    await waitColor('base', [66, 104, 160]);
    await waitColor('units', [250, 200, 30]);
    // Reset a panned view on a context-only transition (same hash, URL and generation).
    await page.evaluate(() => window.__wtQA.store.setState({ views: { ground: { cx: .3, cy: .4, z: 2 }, air: null } }));
    await push({ maps: { ground: { ...blue, context: 'context-2' } }, collectors, units });
    assert.equal(await page.evaluate(() => window.__wtQA.store.getState().views.ground), null);
    await push({ maps: { ground: { ...blue, url: '/api/map/flaky' } }, collectors, units });
    await waitColor('base', [80, 112, 80]);
    assert.ok(flaky >= 2, 'temporary image failure automatically retries');
    // A second sender and a restart cannot steal the remembered gold identity.
    const b = { ...collectors[0], session: 'session-b', device_id: 'device-b' };
    const two = [...units, { ...units[0], id: 'self:device-b', x: .7 }];
    await push({ collectors: [{ ...collectors[0], session: 'restart-a' }, b], units: two });
    await page.waitForFunction(() => window.__wtQA.live.getSnapshot().friends.length === 2);
    assert.deepEqual(await page.evaluate(() => window.__wtQA.live.getSnapshot().friends.filter(f => f.self).map(f => f.id)), ['self:device-a']);
    await page.evaluate(() => window.__wtQA.live.cfg.set('second-key', 'regression', '队员', 'self:device-b'));
    await page.waitForFunction(() => window.__wtQA.live.getSnapshot().friends.find(f => f.id === 'self:device-b')?.self);
    await page.evaluate(() => window.__wtQA.live.cfg.set('second-key', 'regression', '队员', 'observer'));
    await page.waitForFunction(() => window.__wtQA.live.getSnapshot().friends.every(f => !f.self));
    assert.deepEqual(errors, []);
    results.push({ viewport, passed: true, requests, cases: ['gold without matching nick', 'late image URL', 'changed authentication', 'manual new round', 'same-size new image', 'context-only transition', 'image retry', 'stable device identity', 'explicit self binding', 'observer mode'] });
    console.log(`PASS map lifecycle and gold identity ${viewport.width}×${viewport.height}`);
    await context.close();
  }
  mkdirSync('shots', { recursive: true }); writeFileSync('shots/mapregression-result.json', JSON.stringify(results, null, 2));
} finally { await browser.close(); }
