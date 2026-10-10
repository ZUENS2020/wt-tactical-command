import { chromium } from 'playwright-core';
const b = await chromium.launch({ executablePath: '/usr/bin/google-chrome', args: ['--no-sandbox'] });
const p = await (await b.newContext({ viewport: { width: 390, height: 844 }, deviceScaleFactor: 2, hasTouch: true, isMobile: true })).newPage();
await p.goto('https://wt.zuens2020.top/'); await p.waitForTimeout(3000);
await p.screenshot({ path: '/workspace/wt-proto/shots/live-phone-password.png' });
await p.fill('[data-k=key]', process.env.WT_KEY || 'devview123'); await p.fill('[data-k=nick]', '测试'); await p.click('[data-k=save]'); await p.waitForTimeout(3000);
console.log(await p.evaluate(() => [document.querySelector('.conn')?.textContent, localStorage.getItem('wt-room-key')?.length, document.scrollingElement.scrollHeight, innerHeight]));
await p.screenshot({ path: '/workspace/wt-proto/shots/live-phone-connected.png' });
await b.close();
