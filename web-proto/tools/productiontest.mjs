// Exercise the shipped bundle and packaged icons without the development-only QA hooks.
import { chromium } from 'playwright-core';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
const root = new URL('../', import.meta.url);
const icons = JSON.parse(readFileSync(new URL('public/icons/map/sources.json', root), 'utf8'));
const browser = await chromium.launch({ executablePath: process.env.CHROME_PATH || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', headless: true });
try {
  const page = await browser.newPage({ viewport: { width: 1280, height: 720 } });
  const errors = [];
  page.on('pageerror', error => errors.push(error.message));
  await page.goto(new URL('dist/index.html?demo=1&fresh=1&collab=0&mode=dual', root).href);
  await page.waitForFunction(() => Object.values(window.__wtRenderMetrics ?? {}).some(m => m.iconCache > 0));
  assert.equal(await page.locator('canvas.layer').count(), 8);
  assert.equal(await page.evaluate(() => typeof window.__wtQA), 'undefined');
  const loaded = await page.evaluate(files => Promise.all(files.map(file => new Promise(resolve => {
    const img = new Image();
    img.onload = () => resolve(img.naturalWidth > 0 && img.naturalHeight > 0);
    img.onerror = () => resolve(false);
    img.src = `./icons/map/${file}`;
  }))), icons.files.map(i => i.file));
  assert.ok(loaded.every(Boolean), 'every packaged native map glyph must load from the standalone build');
  assert.deepEqual(errors, []);
  console.log(`PASS production bundle, standalone file URL, ${loaded.length} native map glyphs, development hooks excluded`);
} finally {
  await browser.close();
}
