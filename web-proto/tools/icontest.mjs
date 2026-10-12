// Compare our Canvas glyphs with the source game font, not with the old silhouettes.
import { chromium } from 'playwright-core';
import assert from 'node:assert/strict';
import { readFileSync, mkdirSync, writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
const font = readFileSync(process.env.WT_NATIVE_FONT || '/tmp/wt-native-icons/icons.ttf');
const sources = JSON.parse(readFileSync(new URL('../public/icons/map/sources.json', import.meta.url), 'utf8'));
assert.equal(createHash('sha256').update(font).digest('hex'), sources.font_sha256);
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
try {
  const page = await browser.newPage();
  await page.goto((process.argv[2] || 'http://127.0.0.1:4180/') + '?demo=1&fresh=1&collab=0');
  await page.waitForFunction(() => !!window.__wtQA);
  const result = await page.evaluate(async ({ font, roles }) => {
    const f = new FontFace('GameReference', `url(data:font/ttf;base64,${font})`); await f.load(); document.fonts.add(f);
    const { vehicleSprite, iconCacheSize } = window.__wtQA.vehicleIcons;
    const results = [];
    for (const [role, char] of Object.entries(roles)) for (const dpr of [1, 1.5, 2, 3]) {
      const sprite = vehicleSprite(role, '#fac81e', 30, dpr), c = document.createElement('canvas'); c.width = c.height = sprite.width;
      const ctx = c.getContext('2d'); ctx.scale(c.width / 30, c.height / 30); ctx.font = '24px GameReference';
      const m = ctx.measureText(char), x = 15 + (m.actualBoundingBoxLeft - m.actualBoundingBoxRight) / 2, y = 15 + (m.actualBoundingBoxAscent - m.actualBoundingBoxDescent) / 2;
      ctx.fillStyle = '#fac81e'; ctx.strokeStyle = '#171b14'; ctx.lineWidth = 1; ctx.fillText(char, x, y); ctx.strokeText(char, x, y);
      const expected = ctx.getImageData(0, 0, c.width, c.height).data, actual = sprite.getContext('2d').getImageData(0, 0, c.width, c.height).data;
      let diff = 0, energy = 0; for (let i = 0; i < expected.length; i++) { diff += Math.abs(actual[i] - expected[i]); energy += expected[i]; }
      // TrueType hinting snaps outlines at small sizes; vector paths remain subpixel.
      // Require matching shape coverage within one device pixel, plus filled color.
      const occupied = data => Array.from({ length: c.width * c.height }, (_, i) => i).filter(i => data[i * 4 + 3] > 127);
      const a = occupied(actual), e = occupied(expected);
      const close = (i, other) => other.some(j => Math.abs(i % c.width - j % c.width) <= 1 && Math.abs(Math.floor(i / c.width) - Math.floor(j / c.width)) <= 1);
      const mismatches = a.filter(i => !close(i, e)).length + e.filter(i => !close(i, a)).length;
      results.push({ role, dpr, difference: diff / Math.max(1, energy), shapeMismatch: mismatches / Math.max(1, a.length + e.length) });
    }
    const unavailable = ['Unknown', 'HeavyTank', 'Helicopter', 'Jet', 'Destroyer'].every(role => vehicleSprite(role, '#fac81e') === null);
    for (let i = 0; i < 160; i++) vehicleSprite('Assault', `rgb(${i},20,30)`);
    return { results, unavailable, cache: iconCacheSize() };
  }, { font: font.toString('base64'), roles: sources.mapping });
  mkdirSync('shots', { recursive: true }); writeFileSync('shots/icontest-result.json', JSON.stringify(result, null, 2));
  for (const r of result.results) assert.ok(r.shapeMismatch < .04, `${r.role} DPR ${r.dpr} differs from the original font: ${r.shapeMismatch}`);
  assert.ok(result.unavailable, 'unverified or blank glyphs must use a neutral fallback'); assert.ok(result.cache <= 128);
  console.log(`PASS ${result.results.length} source-font comparisons, neutral fallbacks, bounded icon cache`);
} finally { await browser.close(); }
