import { getState, setState, useStore, type EnemyPolicy } from './store';
import { useFresh, POLICY_SHORT } from './TopBar';
import { fmtClock } from './geo';
import { toast } from './store';

const POL: [EnemyPolicy, string, string][] = [
  ['off', '关闭', '不下发自动敌情，仅人工标绘'],
  ['team_visible', '队伍可见', '仅当前任一成员 8111 可见的敌方'],
  ['fused', '融合', '融合轨迹 + 消失后残影 / 预测'],
];

export default function Sheet({ top }: { top: number }) {
  const sheet = useStore((s) => s.sheet);
  const policy = useStore((s) => s.policy);
  const collab = useStore((s) => s.collab);
  const snap = useFresh();
  if (!sheet) return null;
  return (
    <div className="sheetwrap" onPointerDown={(e) => { if (e.target === e.currentTarget) setState({ sheet: null }); }}>
      <div className="sheet" style={{ marginTop: top }} role="dialog" aria-label="设置">
        <div className="shead"><b>房间 TEAM-07</b><span className="meta">对局 {fmtClock(snap.battle)} · 8 名成员</span><button className="x" onClick={() => setState({ sheet: null })} aria-label="关闭">×</button></div>
        <div className="sgroup">
          <div className="stitle">敌情显示策略 <code>enemyPolicy</code></div>
          <div className="polopts">
            {POL.map(([v, l, d]) => (
              <button key={v} className={'polopt' + (policy === v ? ' on' : '')} onClick={() => { if (getState().policy !== v) { setState({ policy: v }); toast(`朔风 将敌情策略改为「${l}」`, { tone: 'info' }); } }} data-policy={v}>
                <b>{l}</b><span>{d}</span>
              </button>
            ))}
          </div>
        </div>
        <div className="sgroup">
          <div className="stitle">数据</div>
          <div className="members">
            {snap.friends.map((f) => {
              const st = !f.alive || f.ageMs > 2000;
              return (
                <div key={f.id} className={'mrow' + (st ? ' st' : '') + (f.self ? ' me' : '')}>
                  <i />{f.nick}<span>{f.air ? '空' : '陆'} · {f.vehicle}</span><em>{st ? `${Math.floor(f.ageMs / 1000)}s` : `${f.ageMs}ms`}</em>
                </div>
              );
            })}
          </div>
          <div className="srcline">回放：b6a7 真实录制（18:31 试车 / 18:32 试飞）· 其余成员为模拟</div>
        </div>
        <div className="sgroup row">
          <button className={'tog' + (collab ? ' on' : '')} onClick={() => setState({ collab: !collab })}><span className="knob" />模拟协同</button>
          <span className="grow" />
          <button className="ghostbtn" onClick={() => { localStorage.clear(); location.reload(); }}>重置演示</button>
        </div>
      </div>
    </div>
  );
}
