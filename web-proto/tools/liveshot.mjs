// Live-mode screenshot helper: node tools/liveshot.mjs <url> <out.png> [waitMs] [WxH] [touch]
import { chromium } from 'playwright-core';
const [url, out, wait = '8000', vp = '1440x900', touch] = process.argv.slice(2);
const [width, height] = vp.split('x').map(Number);
const browser = await chromium.launch({ executablePath: '/usr/bin/google-chrome', args: ['--no-sandbox', '--force-color-profile=srgb'] });
const ctx = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: width < 900 ? 2 : 1, hasTouch: !!touch, isMobile: !!touch });
const page = await ctx.newPage();
page.on('pageerror', (e) => console.log('PAGEERROR', e.message));
page.on('console', (m) => { if (m.type() === 'error' || m.type() === 'warning') console.log('CONSOLE', m.text()); });
await page.goto(url, { waitUntil: 'domcontentloaded' });
await page.waitForTimeout(Number(wait));
const info = await page.evaluate(() => ({ conn: document.querySelector('.conn')?.textContent, se: [document.scrollingElement.scrollWidth, document.scrollingElement.scrollHeight, innerWidth, innerHeight], maps: [...document.querySelectorAll('.mapview')].map((m) => { const b = m.getBoundingClientRect(); return `${b.width}x${b.height}`; }), idle: [...document.querySelectorAll('.mapidle,.mapnote')].map((e) => e.textContent), feed: document.querySelector('.killfeed')?.textContent }));
console.log(JSON.stringify(info));
await page.screenshot({ path: out });
await browser.close();
