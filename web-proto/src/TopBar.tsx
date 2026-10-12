import { useEffect, useState } from 'react';
import { getState, setState, useStore } from './store';
import { setMode } from './actions';
import { getSnapshot, onSnapshot } from './data';
export const POLICY_SHORT = { off: '关闭', team_visible: '实时', fused: '类别' } as const;
export function useFresh() {
  const [v, setV] = useState(() => getSnapshot());
  useEffect(() => { let last = 0; return onSnapshot(() => { const now = performance.now(); if (now - last >= 190) { last = now; setV(getSnapshot()); } }); }, []);
  return v;
}
export default function TopBar({ W }: { W: number }) {
  const mode = useStore(s => s.mode), snap = useFresh();
  const status = ({ connecting: '连接中', offline: '离线', auth: '连接', idle: '机库', live: '已连接' } as Record<string, string>)[snap.conn] ?? '已连接';
  return <header className="topbar">
    <div className="brand"><span className="brand-cross">✛</span>{W >= 540 && <b>WT TACTICAL</b>}<span className="room">{snap.room || '战术地图'}</span></div>
    <div className="seg top" role="radiogroup" aria-label="地图模式">{([['ground', '陆战'], ['air', '空战'], ['dual', '双图']] as const).map(([m, l]) => <button key={m} role="radio" aria-checked={mode === m} className={mode === m ? 'on' : ''} data-mode={m} onClick={() => setMode(m)}>{l}</button>)}</div>
    <span className="grow" /><button className={`conn ${snap.conn === 'offline' || snap.conn === 'auth' ? 'stale' : ''}`} aria-label="连接与设置" onClick={() => setState({ sheet: getState().sheet ? null : 'settings' })}><i className="dot" />{W >= 360 ? status : '⚙'}{W >= 800 && snap.rtt != null && <span className="meta">{snap.rtt} ms</span>}</button>
  </header>;
}
