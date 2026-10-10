import { useState } from 'react';
import { getState, setState, useStore, type EnemyPolicy } from './store';
import { useFresh, POLICY_SHORT } from './TopBar';
import { fmtClock } from './geo';
import { toast } from './store';
import { DEMO } from './mode';
import { cfg, reconnect } from './live';
import { refreshNick } from './actions';

const POL: [EnemyPolicy, string, string][] = [
  ['off', '关闭', '不显示自动敌情，仅人工标绘'],
  ['team_visible', '实时', '仅显示 8111 导出的实时红色标记（压家）+ 8 秒残影'],
  ['fused', '残影', '实时标记 + 30 秒最后位置（空中目标含预测）'],
];
void POLICY_SHORT;

function LiveSettings() {
  const [key, setKey] = useState(cfg.key());
  const [room, setRoom] = useState(cfg.room());
  const [nick, setNick] = useState(cfg.nick());
  const save = () => { cfg.set(key, room, nick); refreshNick(); toast('已保存，正在连接…', { tone: 'info' }); };
  return (
    <div className="sgroup">
      <div className="stitle">连接</div>
      <form className="setin" onSubmit={(e) => { e.preventDefault(); save(); }}>
        <label><span>房间密码</span><input type="password" value={key} onChange={(e) => setKey(e.target.value)} autoComplete="current-password" placeholder="向队长索取" data-k="key" /></label>
        <label><span>房间</span><input value={room} onChange={(e) => setRoom(e.target.value)} data-k="room" /></label>
        <label><span>我的昵称</span><input value={nick} maxLength={16} onChange={(e) => setNick(e.target.value)} data-k="nick" placeholder="显示在你添加的标绘上" /></label>
        <div className="row2"><span className="grow" /><button type="button" className="ghostbtn" onClick={() => reconnect()}>重连</button><button type="submit" className="ghostbtn" data-k="save">保存并连接</button></div>
      </form>
    </div>
  );
}

export default function Sheet({ top }: { top: number }) {
  const sheet = useStore((s) => s.sheet);
  const policy = useStore((s) => s.policy);
  const collab = useStore((s) => s.collab);
  const snap = useFresh();
  if (!sheet) return null;
  const nCol = snap.collectors.filter((c) => c.member).length;
  return (
    <div className="sheetwrap" onPointerDown={(e) => { if (e.target === e.currentTarget) setState({ sheet: null }); }}>
      <div className="sheet" style={{ marginTop: top }} role="dialog" aria-label="设置">
        <div className="shead"><b>房间 {snap.room}</b><span className="meta">{DEMO ? `演示 · 对局 ${fmtClock(snap.battle)}` : snap.inBattle ? `对局 ${fmtClock(snap.battle)} · ${nCol} 台采集端 · ${snap.viewers} 人在看` : `机库 · ${snap.viewers} 人在看`}</span><button className="x" onClick={() => setState({ sheet: null })} aria-label="关闭">×</button></div>
        {!DEMO && <LiveSettings />}
        <div className="sgroup">
          <div className="stitle">敌情显示 <code>enemyPolicy</code></div>
          <div className="polopts">
            {POL.map(([v, l, d]) => (
              <button key={v} className={'polopt' + (policy === v ? ' on' : '')} onClick={() => { if (getState().policy !== v) { setState({ policy: v }); toast(`敌情显示：${l}`, { tone: 'info' }); } }} data-policy={v}>
                <b>{l}</b><span>{d}</span>
              </button>
            ))}
          </div>
        </div>
        <div className="sgroup">
          <div className="stitle">数据</div>
          <div className="members">
            {DEMO ? snap.friends.map((f) => {
              const st = !f.alive || f.ageMs > 2000;
              return (
                <div key={f.id} className={'mrow' + (st ? ' st' : '') + (f.self ? ' me' : '') + (f.role === 'squad' ? ' sq' : '')}>
                  <i />{f.nick}<span>{f.air ? '空' : '陆'} · {f.vehicle}</span><em>{st ? `${Math.floor(f.ageMs / 1000)}s` : `${f.ageMs}ms`}</em>
                </div>
              );
            }) : snap.collectors.length ? snap.collectors.map((c) => {
              const st = c.ageMs > 5000 || (c.inBattle && (c.lagMs ?? 0) > 2000);
              const state = c.paused ? '机库暂停' : !c.inBattle ? '机库' : c.map === 'air' ? '空战视图' : '陆战';
              return (
                <div key={c.id} className={'mrow' + (st ? ' st' : '') + (c.member ? ' me' : '')}>
                  <i />{c.nick || c.player}{c.replay ? '（回放）' : ''}<span>{state}{c.inBattle ? ` · ${c.hz}Hz` : ''}</span>
                  <em>{st ? `${Math.floor(Math.max(c.ageMs, c.inBattle ? c.lagMs ?? 0 : 0) / 1000)}s` : c.inBattle ? `${c.delayMs ?? '-'}ms` : `${Math.floor(c.ageMs / 1000)}s`}</em>
                </div>
              );
            }) : <div className="mrow st"><i />暂无采集端在线<span>队员需运行 wt-collector</span></div>}
          </div>
          <div className="srcline">{DEMO ? '回放：b6a7 真实录制（18:31 试车 / 18:32 试飞）· 其余成员为模拟' : `延迟 = 采集端→服务器 + 往返/2（RTT ${snap.rtt ?? '-'}ms）· 侦察/命中暴露 8111 不导出`}</div>
        </div>
        <div className="sgroup row">
          {DEMO && <button className={'tog' + (collab ? ' on' : '')} onClick={() => setState({ collab: !collab })}><span className="knob" />模拟协同</button>}
          <span className="grow" />
          {DEMO ? <button className="ghostbtn" onClick={() => { localStorage.clear(); location.reload(); }}>重置演示</button>
            : <a className="ghostbtn" href="?demo=1" style={{ textDecoration: 'none' }}>演示模式</a>}
        </div>
      </div>
    </div>
  );
}
