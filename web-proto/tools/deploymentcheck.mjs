// Read-only checks against a deployed release. Credentials come from a private local file.
import { chromium } from 'playwright-core';
import assert from 'node:assert/strict';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
const env = JSON.parse(readFileSync(process.env.WT_DEPLOY_CHECK_ENV, 'utf8'));
const base = process.argv[2] || 'https://wt.zuens2020.top';
const room = env.WT_LIVE_ROOM || 'main';
const result = { protocol: null, states: 0, oldClientReadOnly: false, viewports: [] };
await new Promise((resolve, reject) => {
  const url = new URL('/api/live', base);
  url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:';
  url.searchParams.set('room', room);
  url.searchParams.set('key', env.WT_VIEW_TOKEN);
  const ws = new WebSocket(url);
  const timeout = setTimeout(() => { ws.close(); reject(new Error('deployed WebSocket check timed out')); }, 15000);
  let seq = -1;
  ws.onerror = () => reject(new Error('deployed WebSocket transport error'));
  ws.onmessage = e => {
    try {
      const m = JSON.parse(e.data);
      if (m.type === 'hello') {
        assert.equal(m.protocol, 2);
        result.protocol = m.protocol;
        if (!result.oldClientReadOnly) ws.send(JSON.stringify({ op: 'put' }));
      } else if (m.type === 'error') {
        assert.equal(m.error_code, 'upgrade_required');
        assert.equal(m.read_only, true);
        result.oldClientReadOnly = true;
      } else if (m.type === 'state') {
        assert.ok(m.state_seq > seq);
        seq = m.state_seq;
        assert.ok(m.units.every(u => u.age <= 2000));
        const ids = m.units.filter(u => u.id.startsWith('self:')).map(u => u.id);
        assert.equal(new Set(ids).size, ids.length);
        result.states++;
      }
      if (result.states >= 8 && result.oldClientReadOnly) {
        clearTimeout(timeout); ws.close(); resolve();
      }
    } catch (error) { clearTimeout(timeout); ws.close(); reject(error); }
  };
});
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
try {
  for (const [width, height] of [[1440,900], [390,844]]) {
    const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 2 });
    await context.addInitScript(({ key, room }) => {
      localStorage.setItem('wt-room-key', key);
      localStorage.setItem('wt-room', room);
    }, { key: env.WT_VIEW_TOKEN, room });
    const page = await context.newPage(), errors = [], failedAssets = [];
    page.on('pageerror', e => errors.push(e.message));
    page.on('response', r => { if (r.status() >= 400 && !new URL(r.url()).pathname.startsWith('/api/')) failedAssets.push(new URL(r.url()).pathname); });
    await page.goto(base + '/?mode=dual', { waitUntil: 'networkidle' });
    await page.waitForFunction(() => ['机库','已连接'].some(text => document.querySelector('.conn')?.textContent.includes(text)));
    assert.equal(await page.locator('canvas.layer').count(), 8);
    assert.equal(await page.evaluate(() => typeof window.__wtQA), 'undefined');
    await page.getByRole('button', { name: '连接与设置' }).click();
    await page.locator('[data-k=nick]').fill('部署检查');
    if (width < 600) await page.setViewportSize({ width, height: 390 });
    await page.locator('[data-k=nick]').focus();
    const layout = await page.evaluate(() => ({ noOverflow: document.scrollingElement.scrollWidth <= innerWidth, dialog: !!document.querySelector('[role=dialog]') }));
    assert.ok(layout.noOverflow && layout.dialog);
    assert.deepEqual(errors, []); assert.deepEqual(failedAssets, []);
    result.viewports.push({ width, height, passed: true });
    await context.close();
  }
} finally { await browser.close(); }
mkdirSync('shots', { recursive: true });
writeFileSync('shots/deploymentcheck-result.json', JSON.stringify(result, null, 2));
console.log(JSON.stringify(result));
