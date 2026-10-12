import {chromium} from 'playwright-core';import assert from 'node:assert/strict';
const browser=await chromium.launch({executablePath:process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'}),ctx=await browser.newContext({viewport:{width:390,height:844},deviceScaleFactor:2,hasTouch:true,isMobile:true}),p=await ctx.newPage();await p.goto('http://127.0.0.1:4180/?demo=1&fresh=1&mode=ground&ui=member&collab=0');await p.waitForTimeout(500);
const cdp=await ctx.newCDPSession(p),box=await p.locator('.mapview').boundingBox(),cx=box.x+box.width*.5,cy=box.y+box.height*.25;
const state=()=>p.evaluate(async()=>{const s=(window.__wtQA.store).getState();return {view:structuredClone(s.views.ground),draws:s.drawings.length};});
const touch=(type,pts)=>cdp.send('Input.dispatchTouchEvent',{type,touchPoints:pts.map(([x,y],id)=>({x,y,id}))});
async function drag(){await touch('touchStart',[[cx,cy]]);for(let i=1;i<=8;i++)await touch('touchMove',[[cx+i*8,cy+i*4]]);await touch('touchEnd',[]);await p.waitForTimeout(100);}
async function pinch(a,b){await touch('touchStart',[[cx-a,cy],[cx+a,cy]]);for(let i=1;i<=8;i++){const d=a+(b-a)*i/8;await touch('touchMove',[[cx-d,cy],[cx+d,cy]]);}await touch('touchEnd',[]);await p.waitForTimeout(100);}
let before=await state();await drag();let after=await state();assert.notDeepEqual(after.view,before.view);assert.equal(after.draws,before.draws);
before=after;await pinch(35,85);after=await state();assert.ok(after.view.z>before.view.z);
await p.locator('[data-tool=point]').click();await p.locator('.foot .tog').click();before=await state();await drag();after=await state();assert.deepEqual(after,before,'locked drawing drag cannot pan or place a point');
await touch('touchStart',[[cx,cy]]);await touch('touchEnd',[]);await p.waitForTimeout(150);assert.equal((await state()).draws,before.draws+1);before=await state();await pinch(85,35);after=await state();assert.ok(after.view.z<before.view.z);assert.equal(after.draws,before.draws);
await browser.close();console.log('PASS pan, anchored pinch, locked drawing drag, tap, pinch while drawing');
