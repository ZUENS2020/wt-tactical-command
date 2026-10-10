// Two browsers in the same live room: draw in A, see it in B; delete in B, gone in A; reload A keeps drawings.
import { chromium } from 'playwright-core';
const BASE = process.argv[2] || 'http://127.0.0.1:4181/';
const Q = process.argv[3] || 'room=test&key=devview123';
const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', args: ['--no-sandbox'] });
const open = async (nick) => {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 760 } });
  await ctx.addInitScript((n) => localStorage.setItem('wt-nick', n), nick);
  const p = await ctx.newPage(); p.on('pageerror', (e) => console.log('PAGEERROR', nick, e.message));
  await p.goto(BASE + '?mode=ground&' + Q); await p.waitForTimeout(2500); return p;
};
const a = await open('甲'), b = await open('乙');
const count = (p) => p.evaluate(() => document.querySelectorAll('.mapview svg.layer g').length);
const before = [await count(a), await count(b)];
await a.click('[data-tool=point]').catch(() => a.keyboard.press('p'));
const box = await a.locator('.mapview .hit').boundingBox();
await a.mouse.click(box.x + box.width * 0.42, box.y + box.height * 0.37);
await a.waitForTimeout(1200);
const afterAdd = [await count(a), await count(b)];
const toastB = await b.evaluate(() => [...document.querySelectorAll('.toast')].map((t) => t.textContent));
// B selects + deletes it
await b.keyboard.press('v');
const bb = await b.locator('.mapview .hit').boundingBox();
await b.mouse.click(bb.x + bb.width * 0.42, bb.y + bb.height * 0.37);
await b.waitForTimeout(300);
await b.keyboard.press('Delete');
await b.waitForTimeout(1200);
const afterDel = [await count(a), await count(b)];
// A adds a route, then reloads
await a.keyboard.press('r');
for (const [fx, fy] of [[0.3, 0.6], [0.45, 0.55], [0.55, 0.62]]) { await a.mouse.click(box.x + box.width * fx, box.y + box.height * fy); await a.waitForTimeout(150); }
await a.keyboard.press('Enter'); await a.waitForTimeout(1200);
const afterRoute = [await count(a), await count(b)];
await a.reload(); await a.waitForTimeout(2500);
const afterReload = await count(a);
console.log(JSON.stringify({ before, afterAdd, toastB, afterDel, afterRoute, afterReload }));
await browser.close();
