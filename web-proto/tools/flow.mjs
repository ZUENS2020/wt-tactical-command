// functional smoke test: create / select / rename / move / delete / undo / mode switch persistence
import { chromium } from 'playwright-core';
const BASE = process.argv[2] || 'http://127.0.0.1:4180/';
const b = await chromium.launch({ executablePath: '/usr/bin/google-chrome', args: ['--no-sandbox'] });
const p = await b.newPage({ viewport: { width: 1440, height: 900 } });
p.on('pageerror', (e) => console.log('PAGEERROR', e.message));
await p.goto(BASE + '?fresh=1&collab=0'); await p.waitForTimeout(900);
const st = () => p.evaluate(() => new Promise((r) => setTimeout(() => r(JSON.parse(localStorage.getItem('wt-tactical-proto-v1')).drawings), 400)));
const bb = await p.locator('.mapview .hit').boundingBox();
const at = (fx, fy) => p.mouse.click(bb.x + bb.width * fx, bb.y + bb.height * fy);
const n0 = (await st()).length;
await p.click('[data-tool=point]'); await p.click('[aria-label=空袭]'); await at(0.7, 0.7);
await p.fill('.justadded .name', '目标 1');
let d = await st(); const pt = d.find((x) => x.label === '目标 1');
console.log('point created+named:', d.length === n0 + 1 && !!pt && pt.cat === 'airstrike');
await p.click('[data-tool=select]'); await at(0.7, 0.7); await p.waitForTimeout(150);
console.log('selected editor shows:', await p.locator('.editor .name').inputValue());
await p.click('text=移动'); await at(0.8, 0.8);
d = await st(); const moved = d.find((x) => x.id === pt.id);
console.log('moved:', moved.pts[0][0] > pt.pts[0][0] && moved.pts[0][1] > pt.pts[0][1]);
await p.click('.editor button.danger'); d = await st();
console.log('deleted:', !d.find((x) => x.id === pt.id), 'toast:', await p.locator('.toast').first().innerText());
await p.click('.toast button'); d = await st();
console.log('undo delete restores:', !!d.find((x) => x.id === pt.id));
// route via keyboard Enter, area, source
await p.click('[data-tool=route]'); await at(0.2, 0.2); await at(0.3, 0.25); await at(0.35, 0.35); await p.keyboard.press('Enter');
await p.click('[data-tool=area]'); await at(0.5, 0.2); await at(0.6, 0.3);
await p.click('[data-tool=source]'); await at(0.4, 0.8); await at(0.55, 0.75);
d = await st();
console.log('route/area/source created:', ['route', 'area', 'source'].every((k) => d.some((x) => x.kind === k && x.by === '朔风')));
// mode switch persistence
await p.click('[data-mode=air]'); await p.waitForTimeout(200); await p.click('[data-mode=dual]'); await p.waitForTimeout(200); await p.click('[data-mode=ground]'); await p.waitForTimeout(200);
console.log('drawings persist across modes:', (await st()).length === d.length, 'tool panel hidden in dual: checked by layout');
await p.click('[data-mode=dual]'); console.log('dual has no tool panel:', (await p.locator('.tools').count()) === 0);
await b.close();
