// opts-panel overflow sweep for every tool state at small viewports
import { chromium } from 'playwright-core';
const BASE = process.argv[2] || 'http://127.0.0.1:4180/';
const b = await chromium.launch({ executablePath: '/usr/bin/google-chrome', args: ['--no-sandbox'] });
for (const vp of [[320, 568], [390, 844], [844, 390], [568, 320], [1536, 698]]) for (const ui of ['command', 'member']) {
  const p = await b.newPage({ viewport: { width: vp[0], height: vp[1] } });
  await p.goto(BASE + `?fresh=1&ui=${ui}&collab=0`); await p.waitForTimeout(700);
  const out = [];
  for (const t of ['select', 'point', 'route', 'area', 'source', 'ruler', 'layers']) {
    await p.click(`[data-tool=${t}]`);
    if (t === 'point') { const bb = await p.locator('.mapview .hit').boundingBox(); await p.mouse.click(bb.x + bb.width * 0.5, bb.y + bb.height * 0.5); }
    if (t === 'route') { const bb = await p.locator('.mapview .hit').boundingBox(); await p.mouse.click(bb.x + 20, bb.y + 20); await p.mouse.click(bb.x + 60, bb.y + 40); }
    await p.waitForTimeout(80);
    const r = await p.evaluate(() => { const o = document.querySelector('.opts'); const se = document.scrollingElement; return { ov: o.scrollHeight - o.clientHeight, sw: se.scrollWidth - innerWidth, sh: se.scrollHeight - innerHeight }; });
    out.push(`${t}:${r.ov}${r.sw > 0 || r.sh > 0 ? '!SCROLL' : ''}`);
    if (t === 'route') await p.keyboard.press('Escape');
  }
  // select editor
  await p.click('[data-tool=select]');
  const bb = await p.locator('.mapview .hit').boundingBox(); await p.mouse.click(bb.x + bb.width * 0.5, bb.y + bb.height * 0.5); await p.waitForTimeout(80);
  const r = await p.evaluate(() => { const o = document.querySelector('.opts'); return o.scrollHeight - o.clientHeight + (document.querySelector('.editor') ? '' : '(no-sel)'); });
  out.push('editor:' + r);
  console.log(`${vp[0]}x${vp[1]} ${ui.padEnd(7)} ${out.join(' ')}`);
  await p.close();
}
await b.close();
