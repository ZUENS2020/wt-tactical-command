// Manual round through the real UI and isolated receiver after collector samples expire.
import { chromium } from 'playwright-core';
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
const base = process.argv[2] || 'http://127.0.0.1:4180/';
try {
  const pages = [], errors = [];
  for (const nick of ['成员0', '成员1']) {
    const context = await browser.newContext();
    await context.addInitScript(nick => localStorage.setItem('wt-nick', nick), nick);
    const page = await context.newPage(); page.on('pageerror', e => errors.push(e.message));
    await page.goto(base+'?room=load&key=synthetic-load-view&mode=dual');
    await page.waitForFunction(() => window.__wtQA.live.canEdit() && window.__wtQA.geo.MAPS.ground.available && window.__wtQA.geo.MAPS.air.available);
    pages.push(page);
  }
  const [a, b] = pages;
  const state = p => p.evaluate(() => ({ battle: window.__wtQA.live.currentBattle(), maps: structuredClone(window.__wtQA.geo.MAPS), snapshot: window.__wtQA.live.getSnapshot() }));
  const before = await state(a); assert.equal(before.snapshot.friends.length, 0, 'test starts with expired positions');
  await a.getByRole('button', { name: '连接与设置' }).click();
  await a.getByRole('button', { name: '开始新一局', exact: true }).click();
  await a.getByRole('button', { name: '确认开始', exact: true }).click();
  await a.waitForFunction(old => window.__wtQA.live.currentBattle() !== old, before.battle);
  const after = await state(a);
  await b.waitForFunction(battle => window.__wtQA.live.currentBattle() === battle, after.battle);
  const other = await state(b);
  for (const s of [after, other]) {
    for (const map of ['ground', 'air']) { assert.ok(s.maps[map].available); assert.equal(s.maps[map].hash, before.maps[map].hash); assert.equal(s.maps[map].img, before.maps[map].img); }
    assert.equal(s.snapshot.friends.length, 0); assert.equal(s.snapshot.contacts.length, 0);
  }
  assert.deepEqual(errors, []);
  const result = { passed: true, viewers: 2, staticMapsRetained: ['ground', 'air'], expiredPositions: 0, cases: ['real settings button', 'shared round change', 'cached maps without another upload', 'expired positions remain absent'] };
  mkdirSync('shots', { recursive: true }); writeFileSync('shots/roundtest-result.json', JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result));
} finally { await browser.close(); }
