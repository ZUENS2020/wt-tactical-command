// Pre-render the APP-6 unit symbols we need with milsymbol (build-time only) → src/data/symbols.json
import ms from 'milsymbol';
import fs from 'fs';
ms.setStandard('APP6');
const SIDCS = ['SFGPUCA---', 'SFGPUCD---', 'SFAPMFF---', 'SFAPMFA---',
  'SHGPUCA---', 'SHGPUCAA--', 'SHGPUCD---', 'SHGPUCAW--', 'SHAPMFF---', 'SHAPMFB---', 'SHAPMFA---', 'SHAPMH----', 'SHGPU-----'];
const FRIEND = '#a8c3ea', HOSTILE = '#f2aaa0', SELF = '#faC81E';
const grey = '#80857d';
const cm = (f, h) => ({ Friend: f, Hostile: h, Neutral: '#aadaa0', Unknown: '#f6e79c', Civilian: '#e0c0e8', Suspect: '#f6c8a0' });
const out = {};
for (const sidc of SIDCS) {
  const variants = { n: {}, d: {} };
  if (sidc[1] === 'F') { variants.self = { fillColor: SELF }; variants.g = { fillColor: '#d3d5cf', frameColor: cm(grey, grey), iconColor: grey }; }
  for (const [k, o] of Object.entries(variants)) {
    const code = k === 'd' ? sidc.slice(0, 3) + 'A' + sidc.slice(4) : sidc;
    const sym = new ms.Symbol(code, { size: 40, outlineWidth: 3, outlineColor: 'rgba(250,248,240,0.95)', colorMode: cm(FRIEND, HOSTILE), ...o });
    const a = sym.getAnchor(), s = sym.getSize();
    out[`${sidc}|${k}`] = { svg: sym.asSVG(), ax: a.x / 40, ay: a.y / 40, w: s.width / 40, h: s.height / 40 };
  }
}
fs.writeFileSync('src/data/symbols.json', JSON.stringify(out));
console.log(Object.keys(out).length, 'symbols', fs.statSync('src/data/symbols.json').size, 'bytes');
