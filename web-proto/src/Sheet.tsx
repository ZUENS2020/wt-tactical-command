import { useState } from 'react';
import { getState, setState, useStore, type EnemyPolicy } from './store';
import { useFresh, POLICY_SHORT } from './TopBar';
import { fmtClock } from './geo';
import { toast } from './store';
import { DEMO } from './mode';
import { cfg, reconnect, newRound, canEdit } from './live';
import { refreshNick } from './actions';

const POL: [EnemyPolicy, string, string][] = [
  ['off', '关闭', '不显示自动敌情，仅人工标绘'],
  ['team_visible', '实时', '仅显示当前有效敌情，有效期 2 秒'],
  ['fused', '类别', '当前有效敌情附类别标签，有效期 2 秒'],
];
void POLICY_SHORT;

function LiveSettings() {
  const [key, setKey] = useState(cfg.key());
  const [room, setRoom] = useState(cfg.room());
  const [nick, setNick] = useState(cfg.nick());
  const [selfSource, setSelfSource] = useState(cfg.selfSource());
  const sources = useFresh().collectors;
  const save = () => { cfg.set(key, room, nick, selfSource); refreshNick(); toast('已保存，正在连接…', { tone: 'info' }); };
  return (
    <div className="sgroup">
      <div className="stitle">连接</div>
      <form className="setin" onSubmit={(e) => { e.preventDefault(); save(); }}>
        <label><span>房间密码</span><input type="password" value={key} onChange={(e) => setKey(e.target.value)} autoComplete="current-password" placeholder="向队长索取" data-k="key" /></label>
        <label><span>房间</span><input value={room} onChange={(e) => { setRoom(e.target.value); setSelfSource(''); }} data-k="room" /></label>
        <label><span>我的昵称</span><input value={nick} maxLength={16} onChange={(e) => setNick(e.target.value)} data-k="nick" placeholder="显示在你添加的标绘上" /></label>
        <label><span>金色本人标记</span><select value={selfSource} onChange={e => setSelfSource(e.target.value)} data-k="self-source">
          <option value="">自动识别本人</option><option value="observer">仅观看，不标记本人</option>
          {selfSource && selfSource !== 'observer' && !sources.some(c => c.sourceId === selfSource) && <option value={selfSource}>已绑定的采集端（离线）</option>}
          {sources.map(c => <option key={c.id} value={c.sourceId}>{c.nick || c.player}{c.self ? ' · 本人' : ''}</option>)}
        </select></label>
        <p className="hint">本人为金色。多人回传时可选择自己的采集端，重启后仍保持绑定。</p>
        <div className="row2"><span className="grow" /><button type="button" className="ghostbtn" onClick={() => reconnect()}>重连</button><button type="submit" className="ghostbtn" data-k="save">保存并连接</button></div>
      </form>
    </div>
  );
}

export default function Sheet() {
  const sheet = useStore((s) => s.sheet);
  const policy = useStore((s) => s.policy);
  const density = useStore(s => s.density);
  const [confirmRound, setConfirmRound] = useState(false);
  const collab = useStore((s) => s.collab);
  const snap = useFresh();
  if (!sheet) return null;
  const nCol = snap.collectors.filter((c) => c.member).length;
  return (
    <div className="sheetwrap" onPointerDown={(e) => { if (e.target === e.currentTarget) setState({ sheet: null }); }}>
      <div className="sheet" role="dialog" aria-label="设置">
        <div className="shead"><b>房间 {snap.room}</b><span className="meta">{DEMO ? `演示 · 对局 ${fmtClock(snap.battle)}` : snap.inBattle ? `对局 ${fmtClock(snap.battle)} · ${nCol} 台采集端 · ${snap.viewers} 人在看` : `机库 · ${snap.viewers} 人在看`}</span><button className="x" onClick={() => setState({ sheet: null })} aria-label="关闭">×</button></div>
        {!DEMO && <LiveSettings />}
        <div className="sgroup">
          <div className="stitle">敌情显示</div>
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
              const st = c.ageMs > 2000 || (c.inBattle && (c.lagMs ?? 0) > 2000);
              const state = c.paused ? '机库暂停' : !c.inBattle ? '机库' : c.map === 'air' ? '空战视图' : '陆战';
              return (
                <div key={c.id} className={'mrow' + (st ? ' st' : '') + (c.self ? ' me' : c.member ? ' sq' : '')}>
                  <i />{c.nick || c.player}{c.replay ? '（回放）' : ''}<span>{state}{c.inBattle ? ` · ${c.hz}Hz` : ''}</span>
                  <em>{st ? `${Math.floor(Math.max(c.ageMs, c.inBattle ? c.lagMs ?? 0 : 0) / 1000)}s` : c.inBattle ? `${c.delayMs ?? '-'}ms` : `${Math.floor(c.ageMs / 1000)}s`}</em>
                </div>
              );
            }) : <div className="mrow st"><i />暂无采集端在线<span>队员需运行 wt-collector</span></div>}
          </div>
          <div className="srcline">{DEMO ? '回放：b6a7 真实录制（18:31 试车 / 18:32 试飞）· 其余成员为模拟' : `延迟 = 采集端→服务器 + 往返/2（RTT ${snap.rtt ?? '-'}ms）· 侦察/命中暴露 8111 不导出`}</div>
        </div>
        <div className="sgroup"><div className="stitle">界面密度</div><div className="seg" role="radiogroup" aria-label="界面密度">{(['command', 'member'] as const).map(d => <button key={d} className={density === d ? 'on' : ''} aria-checked={density === d} role="radio" data-density={d} onClick={() => setState({ density: d })}>{d === 'command' ? '指挥' : '成员'}</button>)}</div><p className="hint">过期观测在 2 秒后丢弃。陆战与空战分别判断数据新鲜度。</p></div>
        {!DEMO && <div className="sgroup"><div className="stitle">对局纠正</div>{confirmRound ? <><p>开始新一局会清空当前共享标绘。所有成员会同时切换。</p><div className="acts"><button className="pri" onClick={() => { newRound(); setConfirmRound(false); }}>确认开始</button><button onClick={() => setConfirmRound(false)}>取消</button></div></> : <button disabled={!canEdit()} onClick={() => setConfirmRound(true)}>开始新一局</button>}<p className="hint">重生和切换载具不会自动开始新一局。{!canEdit() && '共享标绘为只读；连接恢复后会自动同步，旧协议网页请刷新。'}</p></div>}
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
