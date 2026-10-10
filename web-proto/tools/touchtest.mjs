import { chromium } from 'playwright-core';
const BASE = process.argv[2] || 'http://127.0.0.1:4180/';
const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', args: ['--no-sandbox'] });
const ctx = await browser.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, hasTouch: true, isMobile: true });
const page = await ctx.newPage();
page.on('pageerror', (e) => console.log('PAGEERROR', e.message));
await page.goto(BASE + '?fresh=1&mode=ground&ui=member&collab=0', { waitUntil: 'networkidle' });
await page.waitForTimeout(1200);
const cdp = await ctx.newCDPSession(page);
const b = await page.locator('.mapview .hit').boundingBox();
const view = () => page.evaluate(() => JSON.parse(localStorage.getItem('wt-tactical-proto-v1') || '{}').views?.ground);
const nDraw = () => page.evaluate(() => JSON.parse(localStorage.getItem('wt-tactical-proto-v1') || '{}').drawings?.length);
const touch = async (type, pts) => cdp.send('Input.dispatchTouchEvent', { type, touchPoints: pts.map(([x, y], i) => ({ x, y, id: i })) });
async function drag(x0, y0, x1, y1) {
  await touch('touchStart', [[x0, y0]]);
  for (let i = 1; i <= 8; i++) await touch('touchMove', [[x0 + (x1 - x0) * i / 8, y0 + (y1 - y0) * i / 8]]);
  await touch('touchEnd', []); await page.waitForTimeout(400);
}
async function pinch(cx, cy, d0, d1) {
  await touch('touchStart', [[cx - d0, cy], [cx + d0, cy]]);
  for (let i = 1; i <= 8; i++) { const d = d0 + (d1 - d0) * i / 8; await touch('touchMove', [[cx - d, cy], [cx + d, cy]]); }
  await touch('touchEnd', []); await page.waitForTimeout(400);
}
const cx = b.x + b.width / 2, cy = b.y + b.height / 2;
await page.waitForTimeout(500);
const v0 = await view(); const n0 = await nDraw();
await drag(cx, cy, cx + 80, cy + 40);
const v1 = await view();
console.log('select tool one-finger drag pans:', JSON.stringify(v0) !== JSON.stringify(v1), 'drawings unchanged:', (await nDraw()) === n0);
await pinch(cx, cy, 40, 110);
const v2 = await view();
console.log('pinch zoom in: z', v1?.z?.toFixed(2), '->', v2?.z?.toFixed(2));
// point tool, lock pan
await page.click('[data-tool=point]');
await page.click('.foot .tog'); // 自由平移 off
await drag(cx - 30, cy, cx + 60, cy + 30);
const v3 = await view();
console.log('point tool + pan locked: drag does not pan:', JSON.stringify(v2) === JSON.stringify(v3), 'no accidental point:', (await nDraw()) === n0);
await touch('touchStart', [[cx, cy]]); await touch('touchEnd', []); await page.waitForTimeout(500);
console.log('tap places point:', (await nDraw()) === n0 + 1);
await pinch(cx, cy, 100, 50);
const v4 = await view();
console.log('two-finger pinch still works while locked: z', v3?.z?.toFixed(2), '->', v4?.z?.toFixed(2), 'no extra point:', (await nDraw()) === n0 + 1);
await browser.close();
