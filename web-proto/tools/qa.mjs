// Headless QA: screenshots + no-scroll + square-map checks.  node tools/qa.mjs [baseUrl]
import { chromium } from 'playwright-core';
const BASE = process.argv[2] || 'http://127.0.0.1:4180/';
const OUT = 'shots/';
const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', args: ['--no-sandbox', '--force-color-profile=srgb'] });
const results = [];
async function check(page, name) {
  const r = await page.evaluate(() => {
    const se = document.scrollingElement;
    const maps = [...document.querySelectorAll('.mapview')].map((m) => { const b = m.getBoundingClientRect(); return { w: b.width, h: b.height, x: b.x, y: b.y, r: b.right, b: b.bottom }; });
    const over = [...document.querySelectorAll('.app *')].filter((e) => { const b = e.getBoundingClientRect(); return b.width > 0 && (b.right > innerWidth + 0.5 || b.bottom > innerHeight + 0.5 || b.left < -0.5 || b.top < -0.5) && !e.closest('.mapview'); }).map((e) => e.className && e.className.baseVal === undefined ? e.className : e.tagName).slice(0, 5);
    const tools = document.querySelector('.tools'); const opts = document.querySelector('.opts');
    return { vw: innerWidth, vh: innerHeight, sw: se.scrollWidth, sh: se.scrollHeight, maps, over, layout: document.querySelector('.main')?.dataset.layout,
      optsOverflow: opts ? opts.scrollHeight - opts.clientHeight : 0, tools: tools ? tools.getBoundingClientRect().toJSON() : null };
  });
  const noScroll = r.sw <= r.vw && r.sh <= r.vh;
  const square = r.maps.every((m) => Math.abs(m.w - m.h) <= 1);
  const inside = r.maps.every((m) => m.r <= r.vw + 0.5 && m.b <= r.vh + 0.5);
  results.push({ name, ...r, noScroll, square, inside });
  console.log(`${noScroll && square && inside && !r.over.length ? 'OK ' : 'BAD'} ${name.padEnd(34)} ${r.vw}x${r.vh} layout=${r.layout} scroll=${r.sw}x${r.sh} maps=${r.maps.map((m) => `${m.w}x${m.h}`).join(',')} optsOverflow=${r.optsOverflow} over=${JSON.stringify(r.over)}`);
}
async function shot(name, vp, q, fn) {
  const ctx = await browser.newContext({ viewport: vp, deviceScaleFactor: vp.dsf ?? (vp.width < 900 ? 2 : 1), hasTouch: !!vp.touch, isMobile: !!vp.touch });
  const page = await ctx.newPage();
  page.on('pageerror', (e) => console.log('PAGEERROR', name, e.message));
  page.on('console', (m) => { if (m.type() === 'error') console.log('CONSOLE', name, m.text()); });
  await page.goto(BASE + '?demo=1&fresh=1&' + q, { waitUntil: 'networkidle' });
  await page.waitForTimeout(2600);
  if (fn) await fn(page);
  await check(page, name);
  await page.screenshot({ path: OUT + name + '.png' });
  await ctx.close();
}
const tapMap = async (page, fx, fy, sel = '.mapview .hit') => {
  const b = await page.locator(sel).first().boundingBox();
  await page.mouse.click(b.x + b.width * fx, b.y + b.height * fy);
  await page.waitForTimeout(120);
};
const only = process.argv[3];
const jobs = [
  ['390x844_single_command', { width: 390, height: 844, touch: true }, 'mode=ground&ui=command'],
  ['390x844_single_member', { width: 390, height: 844, touch: true }, 'mode=ground&ui=member'],
  ['390x844_dual', { width: 390, height: 844, touch: true }, 'mode=dual&ui=member'],
  ['844x390_single_command', { width: 844, height: 390, touch: true }, 'mode=ground&ui=command'],
  ['844x390_dual', { width: 844, height: 390, touch: true }, 'mode=dual'],
  ['320x568_single_command', { width: 320, height: 568, touch: true }, 'mode=ground&ui=command'],
  ['320x568_dual', { width: 320, height: 568, touch: true }, 'mode=dual'],
  ['1280x720_single_command', { width: 1280, height: 720 }, 'mode=ground&ui=command'],
  ['1536x698_single_member', { width: 1536, height: 698 }, 'mode=air&ui=member'],
  ['1440x900_single_command', { width: 1440, height: 900 }, 'mode=ground&ui=command&policy=team_visible'],
  ['1440x900_single_member', { width: 1440, height: 900 }, 'mode=ground&ui=member'],
  ['1440x900_air_command_fused', { width: 1440, height: 900 }, 'mode=air&ui=command&policy=fused', async (p) => { await p.waitForTimeout(19000); }],
  ['1440x900_ground_fused_reveals', { width: 1440, height: 900 }, 'mode=ground&ui=command&policy=fused&collab=0', async (p) => { await p.waitForTimeout(27700); }],
  ['390x844_ground_fused_zoom', { width: 390, height: 844, touch: true }, 'mode=ground&ui=member&policy=fused&collab=0', async (p) => {
    await p.waitForTimeout(26500); await p.click('[aria-label=放大]'); await p.waitForTimeout(300);
  }],
  ['390x844_collab_toast', { width: 390, height: 844, touch: true }, 'mode=ground&ui=command', async (p) => { await p.waitForSelector('.toast', { timeout: 20000 }); await p.waitForTimeout(250); }],
  ['1280x720_stale_indicator', { width: 1280, height: 720 }, 'mode=dual&ui=command&collab=0', async (p) => { await p.waitForSelector('.conn.stale', { timeout: 40000 }); await p.waitForTimeout(900); }],
  ['1440x900_dual_command', { width: 1440, height: 900 }, 'mode=dual&ui=command&policy=fused'],
  ['1440x900_dual_member', { width: 1440, height: 900 }, 'mode=dual&ui=member&policy=off'],
  ['1440x900_drawing_route', { width: 1440, height: 900 }, 'mode=ground&ui=command&collab=0', async (p) => {
    await p.click('[data-tool=route]'); await p.click('text=进攻箭头');
    await tapMap(p, 0.30, 0.40); await tapMap(p, 0.42, 0.33); await tapMap(p, 0.55, 0.36);
    const b = await p.locator('.mapview .hit').boundingBox(); await p.mouse.move(b.x + b.width * 0.66, b.y + b.height * 0.30);
    await p.waitForTimeout(200);
  }],
  ['390x844_drawing_source', { width: 390, height: 844, touch: true }, 'mode=ground&ui=command&collab=0', async (p) => {
    await p.click('[data-tool=source]'); await tapMap(p, 0.40, 0.55);
  }],
  ['1440x900_select_editor', { width: 1440, height: 900 }, 'mode=ground&ui=command&collab=0', async (p) => {
    await p.click('[data-tool=select]');
    // seeded rally point at ground norm (0.402,0.553)
    const v = await p.evaluate(() => JSON.parse(localStorage.getItem('wt-tactical-proto-v1') || '{}').views);
    await p.waitForTimeout(400);
    const pos = await p.evaluate(() => { const b = document.querySelector('.mapview').getBoundingClientRect(); return b; });
    // default view = fit grid box: box x0=0.371 w=0.3906 → norm→screen
    const z = 1 / (0.390625 * 1.06), cx = 0.371 + 0.1953, cy = 0.3903 + 0.1953;
    const sx = (0.402 - cx) * pos.width * z + pos.width / 2, sy = (0.553 - cy) * pos.width * z + pos.width / 2;
    await p.mouse.click(pos.x + sx, pos.y + sy); await p.waitForTimeout(300);
  }],
  ['1440x900_settings_sheet', { width: 1440, height: 900 }, 'mode=ground&ui=command&collab=0', async (p) => { await p.click('.conn'); await p.waitForTimeout(300); }],
  ['390x844_point_tool_member', { width: 390, height: 844, touch: true }, 'mode=ground&ui=member&collab=0', async (p) => { await p.click('[data-tool=point]'); await p.click('[aria-label=敌军]'); await tapMap(p, 0.62, 0.42); }],
  ['1440x900_ruler', { width: 1440, height: 900 }, 'mode=ground&ui=command&collab=0', async (p) => {
    await p.click('[data-tool=ruler]'); await tapMap(p, 0.12, 0.45); await tapMap(p, 0.4, 0.42); await tapMap(p, 0.7, 0.5);
  }],
];
for (const [name, vp, q, fn] of jobs) { if (only && !name.includes(only)) continue; await shot(name, vp, q, fn); }
await browser.close();
const bad = results.filter((r) => !(r.noScroll && r.square && r.inside && !r.over.length));
console.log(bad.length ? `FAILURES: ${bad.length}` : 'ALL CHECKS PASSED');
