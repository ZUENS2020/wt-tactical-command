import { defineConfig, type Plugin } from 'vite';
import react from '@vitejs/plugin-react';
import fs from 'fs';
import path from 'path';

// Post-build: classic (non-module) script so dist/index.html also opens from file://,
// plus a tiny cache-first service worker for offline use when served over HTTPS.
function offline(): Plugin {
  return {
    name: 'wt-offline',
    apply: 'build',
    closeBundle() {
      const dist = path.resolve('dist');
      const htmlP = path.join(dist, 'index.html');
      let html = fs.readFileSync(htmlP, 'utf8');
      html = html.replace(/<script type="module" crossorigin src="([^"]+)"><\/script>/, '<script defer src="$1"></script>');
      html = html.replace(/ crossorigin/g, '');
      fs.writeFileSync(htmlP, html);
      const files: string[] = [];
      const walk = (d: string) => fs.readdirSync(d).forEach((f) => { const p = path.join(d, f); fs.statSync(p).isDirectory() ? walk(p) : files.push(path.relative(dist, p)); });
      walk(dist);
      const list = ['./', ...files.filter((f) => f !== 'sw.js').map((f) => './' + f.split(path.sep).join('/'))];
      const ver = Date.now().toString(36);
      fs.writeFileSync(path.join(dist, 'sw.js'), `const C='wt-proto-${ver}';const F=${JSON.stringify(list)};
self.addEventListener('install',e=>{e.waitUntil(caches.open(C).then(c=>c.addAll(F)).then(()=>self.skipWaiting()))});
self.addEventListener('activate',e=>{e.waitUntil(caches.keys().then(k=>Promise.all(k.filter(x=>x!==C).map(x=>caches.delete(x)))).then(()=>self.clients.claim()))});
self.addEventListener('fetch',e=>{const r=e.request;if(r.method!=='GET')return;const u=new URL(r.url);if(u.pathname.startsWith('/api/'))return;
if(r.mode==='navigate'){e.respondWith(fetch(r).then(x=>{const c=x.clone();caches.open(C).then(k=>k.put('./',c));return x}).catch(()=>caches.match('./')));return}
e.respondWith(caches.match(r,{ignoreSearch:true}).then(x=>x||fetch(r)))});
`);
    },
  };
}

const API = process.env.WT_API || 'http://127.0.0.1:18112';
const proxy = { '/api': { target: API, ws: true, changeOrigin: true } };

export default defineConfig({
  base: './',
  server: { proxy },
  preview: { proxy },
  plugins: [react(), offline()],
  build: { outDir: 'dist', assetsInlineLimit: 0, chunkSizeWarningLimit: 900, modulePreload: false, rollupOptions: { output: { format: 'iife' } } },
});
