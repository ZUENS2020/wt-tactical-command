// Two real browsers: conflict rejection, unknown acknowledgements, omitted broadcasts, offline edits, undo, stale callbacks.
import { chromium } from 'playwright-core';
import assert from 'node:assert/strict';
import { writeFileSync,mkdirSync } from 'node:fs';
const BASE=process.argv[2]||'http://127.0.0.1:4180/';
const browser=await chromium.launch({executablePath:process.env.CHROME_PATH||'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'});
const errors=[];
async function open(nick){const ctx=await browser.newContext({viewport:{width:1280,height:760}});await ctx.addInitScript(n=>{
  localStorage.setItem('wt-nick',n);window.__dropDraw=0;window.__syncCount=0;window.__conflicts=0;window.__liveSockets=[];
  const Native=window.WebSocket;window.WebSocket=class extends Native{constructor(...a){super(...a);if(String(a[0]).includes('/api/live'))window.__liveSockets.push(this);}set onmessage(cb){this.__callback=cb;super.onmessage=e=>{if(window.__offlineSockets)return;const m=JSON.parse(e.data);if(m.type==='draw'&&window.__holdDraw)return;if(m.type==='error'&&m.error==='version_conflict')window.__conflicts++;if(m.type==='draw'&&window.__dropDraw>0){window.__dropDraw--;return;}cb(e);};}send(value){try{if(JSON.parse(value).op==='sync')window.__syncCount++;}catch{}return super.send(value);}};
},nick);const p=await ctx.newPage();p.on('pageerror',e=>errors.push(e.message));await p.goto(`${BASE}?room=load&key=synthetic-load-view&mode=ground`);await p.waitForFunction(()=>!!window.__wtQA);await p.waitForFunction(async()=>{const{canEdit}=window.__wtQA.live;return canEdit();});return{ctx,p};}
const {p:a,ctx:ac}=await open('甲'),{p:b,ctx:bc}=await open('乙');
const state=p=>p.evaluate(async()=>structuredClone((window.__wtQA.store).getState().drawings));
const add=async(p,label)=>p.evaluate(async label=>{const{addDrawing}=window.__wtQA.actions;return addDrawing({kind:'point',scope:'ground',cat:'rally',pts:[[1700,1900]],label,status:'present',showTime:false}).id;},label);
const edit=(p,id,label)=>p.evaluate(async({id,label})=>(window.__wtQA.actions).updateDrawing(id,{label}),{id,label});
const equal=async()=>{await a.waitForTimeout(500);const x=(await state(a)).sort((a,b)=>a.id.localeCompare(b.id)),y=(await state(b)).sort((a,b)=>a.id.localeCompare(b.id));assert.deepEqual(x,y);};
let id=await add(a,'创建');await b.waitForFunction(async id=>(window.__wtQA.store).getState().drawings.some(d=>d.id===id),id);await equal();
await Promise.all([a.evaluate(()=>window.__holdDraw=true),b.evaluate(()=>window.__holdDraw=true)]);await Promise.all([edit(a,id,'甲改'),edit(b,id,'乙改')]);await a.waitForTimeout(200);await Promise.all([a.evaluate(async()=>{window.__holdDraw=false;(window.__wtQA.live).requestSync();}),b.evaluate(async()=>{window.__holdDraw=false;(window.__wtQA.live).requestSync();})]);await equal();const conflicts=await a.evaluate(()=>window.__conflicts)+await b.evaluate(()=>window.__conflicts);assert.ok(conflicts>=1,'simultaneous changes conflict');
await Promise.all([a.evaluate(async id=>(window.__wtQA.actions).deleteDrawing(id),id),edit(b,id,'删除竞态')]);await equal();
// The server may send both the broadcast and direct acknowledgement. Drop both in A.
await a.evaluate(()=>window.__dropDraw=2);id=await add(a,'确认丢失');await a.waitForTimeout(3500);await equal();assert.ok((await state(a)).some(d=>d.id===id));assert.ok(await a.evaluate(()=>window.__syncCount)>0);
await b.evaluate(()=>window.__dropDraw=1);const missed=await add(a,'广播遗漏');await a.waitForTimeout(800);await equal();assert.ok((await state(b)).some(d=>d.id===missed));assert.ok(await b.evaluate(()=>window.__syncCount)>0);
const stale=await add(a,'撤销冲突');await a.waitForTimeout(300);await edit(b,stale,'队友的新版本');await a.waitForTimeout(400);await a.evaluate(async()=>(window.__wtQA.actions).undo());await equal();assert.equal((await state(a)).find(d=>d.id===stale).label,'队友的新版本');
// An update can be undone using its current acknowledged version, not its old version.
const own=await add(a,'原名');await a.waitForTimeout(300);await edit(a,own,'新名');await a.waitForTimeout(300);await a.evaluate(async()=>(window.__wtQA.actions).undo());await equal();assert.equal((await state(a)).find(d=>d.id===own).label,'原名');
await b.evaluate(async()=>{window.__qaLive=window.__wtQA.live;window.__qaActions=window.__wtQA.actions;window.__qaStore=window.__wtQA.store;});
await bc.setOffline(true);await b.evaluate(()=>{window.__offlineSockets=true;window.__qaLive.reconnect();});await b.waitForFunction(()=>!window.__qaLive.canEdit());await b.waitForTimeout(300);const before=await b.evaluate(()=>window.__qaStore.getState().drawings.length);await b.evaluate(()=>window.__qaActions.addDrawing({kind:'point',scope:'ground',cat:'rally',pts:[[1700,1900]],label:'离线操作',status:'present',showTime:false}));assert.equal(await b.evaluate(()=>window.__qaStore.getState().drawings.length),before,'offline shared edits disabled');
await bc.setOffline(false);await b.evaluate(()=>{window.__offlineSockets=false;window.__qaLive.reconnect();});await b.waitForFunction(async()=>(window.__wtQA.live).canEdit());await equal();
await b.evaluate(()=>{const old=window.__liveSockets[0];old.__callback?.({data:JSON.stringify({type:'state',state_seq:99999999,battle:'old-connection',maps:{},units:[]})});});await equal();
// Complete snapshot from the server must agree with both browsers.
const server=await a.evaluate(async()=>await new Promise(resolve=>{const ws=new WebSocket(`${location.protocol==='https:'?'wss':'ws'}://${location.host}/api/live?room=load&key=synthetic-load-view`);ws.onmessage=e=>{const m=JSON.parse(e.data);if(m.type==='hello'){resolve(m.drawings);ws.close();}};}));assert.deepEqual((await state(a)).sort((a,b)=>a.id.localeCompare(b.id)),server.sort((a,b)=>a.id.localeCompare(b.id)));
assert.deepEqual(errors,[]);const result={passed:true,conflicts,resyncA:await a.evaluate(()=>window.__syncCount),resyncB:await b.evaluate(()=>window.__syncCount),cases:['create','concurrent edit','delete race','ack loss','broadcast omission','remote edit undo rejection','own update undo','offline edit rejection','reconnect','old callback','server snapshot convergence']};mkdirSync('shots',{recursive:true});writeFileSync('shots/synctest-result.json',JSON.stringify(result,null,2));console.log(JSON.stringify(result));await browser.close();
