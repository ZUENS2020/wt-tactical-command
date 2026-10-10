import { useEffect, useState } from 'react';
import { getState, setState, useStore } from './store';
import { setMode } from './actions';
import { getSnapshot, onSnapshot } from './sim';

export const POLICY_SHORT = { off: '关', team_visible: '可见', fused: '融合' } as const;

export function useFresh() {
  const [v, setV] = useState(() => getSnapshot());
  useEffect(() => {
    let last = 0;
    return onSnapshot(() => { const n = performance.now(); if (n - last > 240) { last = n; setV(getSnapshot()); } });
  }, []);
  return v;
}

export default function TopBar({ h, W }: { h: number; W: number }) {
  const mode = useStore((s) => s.mode);
  const density = useStore((s) => s.density);
  const policy = useStore((s) => s.policy);
  const snap = useFresh();
  const narrow = W < 520;
  const xn = W < 430;
  const tiny = W < 360;
  const stale = snap.stale;
  return (
    <header className="topbar" style={{ height: h }}>
      <div className="brand" title="WT Tactical">
        <svg width="20" height="20" viewBox="0 0 20 20" aria-hidden><path d="M10 1.5L18.5 10 10 18.5 1.5 10z" fill="none" stroke="#3d4a37" strokeWidth="1.8" /><path d="M10 5.5v9M5.5 10h9" stroke="#c9780f" strokeWidth="1.8" /></svg>
        {!xn && <b>{narrow ? 'WT' : 'WT Tactical'}</b>}
      </div>
      <div className="seg top" role="radiogroup" aria-label="地图模式">
        {([['ground', '陆战'], ['air', '空战'], ['dual', narrow ? '双图' : '双地图']] as const).map(([m, l]) => (
          <button key={m} role="radio" aria-checked={mode === m} className={mode === m ? 'on' : ''} onClick={() => setMode(m)} data-mode={m}>{l}</button>
        ))}
      </div>
      <div className="seg top dens" role="radiogroup" aria-label="界面密度">
        {([['command', '指挥'], ['member', '成员']] as const).map(([d, l]) => (
          <button key={d} role="radio" aria-checked={density === d} className={density === d ? 'on' : ''} onClick={() => setState({ density: d, showMore: false })} data-density={d}>{l}</button>
        ))}
      </div>
      <span className="grow" />
      <button className={'conn' + (stale ? ' stale' : '')} onClick={() => setState({ sheet: getState().sheet ? null : 'settings' })} title="连接与敌情策略" aria-label="连接与设置">
        <i className="dot" />
        {(!tiny || stale) && <span className="ctext">{stale ? (tiny ? `${(snap.ageMs / 1000).toFixed(1)}s` : `陈旧 ${(snap.ageMs / 1000).toFixed(1)}s`) : narrow ? `${snap.latency}ms` : `${snap.hz}Hz · ${snap.latency}ms`}</span>}
        <span className={'pol ' + policy} title={`敌情策略：${policy}`}>{narrow ? POLICY_SHORT[policy][0] : '敌情 ' + POLICY_SHORT[policy]}</span>
      </button>
    </header>
  );
}
