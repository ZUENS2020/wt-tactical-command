import { getState, setState, useStore, type Tool, type PointCat } from './store';
import { selectTool, cancelDraft, undoPoint, finishDraft, deleteDrawing, updateDrawing, undo, kindName, AREA_LABEL, ROUTE_LABEL } from './actions';
import { ToolIcon, TOOL_LABEL } from './icons';
import { CAT_LABEL, CAT_ORDER, GlyphIcon } from './symbols';
import { fmtClock, gridRef, dist, metersPerCanon, fmtDist } from './geo';

const TOOLS: Tool[] = ['select', 'point', 'route', 'area', 'source', 'ruler', 'layers'];

function Seg<T extends string | number>({ value, opts, onChange, small }: { value: T; opts: [T, string][]; onChange: (v: T) => void; small?: boolean }) {
  return (
    <div className={'seg' + (small ? ' small' : '')} role="radiogroup">
      {opts.map(([v, l]) => (
        <button key={String(v)} role="radio" aria-checked={v === value} className={v === value ? 'on' : ''} onClick={() => onChange(v)}>{l}</button>
      ))}
    </div>
  );
}
function Toggle({ on, label, onClick, title }: { on: boolean; label: string; onClick: () => void; title?: string }) {
  return <button className={'tog' + (on ? ' on' : '')} aria-pressed={on} onClick={onClick} title={title}><span className="knob" />{label}</button>;
}

export default function ToolPanel({ orient, w, h }: { orient: 'side' | 'bottom'; w: number; h: number }) {
  const tool = useStore((s) => s.tool);
  const density = useStore((s) => s.density);
  const cmd = density === 'command';
  const narrow = w < 300 || (orient === 'side' && w < 200);
  const tiny = (h < 232 && orient === 'bottom') || (h < 320 && orient === 'side');
  const iconOnly = !cmd || (orient === 'side' && (w < 150 || tiny));
  const opts = (
    <div className="opts">
      {tool === 'select' && <SelectOpts cmd={cmd} />}
      {tool === 'point' && <PointOpts cmd={cmd} narrow={narrow} />}
      {tool === 'route' && <RouteOpts cmd={cmd} />}
      {tool === 'area' && <AreaOpts cmd={cmd} />}
      {tool === 'source' && <SourceOpts cmd={cmd} />}
      {tool === 'ruler' && <RulerOpts cmd={cmd} />}
      {tool === 'layers' && <LayerOpts cmd={cmd} />}
    </div>
  );
  return (
    <div className={`tools ${orient} ${cmd ? 'cmd' : 'mem'} ${tiny ? 'tiny' : ''}`} style={{ width: w, height: h }}>
      {orient === 'bottom' && opts}
      <div className={'toolrow' + (iconOnly ? ' icons' : '')} role="toolbar" aria-label="标绘工具">
        {TOOLS.map((t) => (
          <button key={t} className={'tool' + (tool === t ? ' on' : '')} onClick={() => selectTool(t)} title={TOOL_LABEL[t]} aria-label={TOOL_LABEL[t]} aria-pressed={tool === t} data-tool={t}>
            <ToolIcon tool={t} size={iconOnly ? 21 : 19} />
            {!iconOnly && <span>{TOOL_LABEL[t]}</span>}
          </button>
        ))}
      </div>
      {orient === 'side' && opts}
      <Footer />
    </div>
  );
}

function Footer() {
  const freePan = useStore((s) => s.freePan);
  const nUndo = useStore((s) => s.undo.length);
  return (
    <div className="foot">
      <Toggle on={freePan} label="自由平移" onClick={() => setState({ freePan: !getState().freePan })} title="关闭后绘图时单指不会拖动地图（双指仍可缩放平移）" />
      <span className="sp" />
      <button className="ghostbtn" onClick={undo} disabled={!nUndo} title="撤销我的上一步编辑">
        <svg viewBox="0 0 20 20" width="16" height="16"><path d="M7 5L3 9l4 4" /><path d="M3.5 9H12a4.5 4.5 0 0 1 0 9h-2" /></svg>撤销
      </button>
    </div>
  );
}

function StatusSeg() {
  const st = useStore((s) => s.newStatus);
  return <Seg small value={st} opts={[['present', '现状'], ['planned', '计划']]} onChange={(v) => setState({ newStatus: v })} />;
}

function DraftBar({ n, canFinish = true, hint }: { n: number; canFinish?: boolean; hint: string }) {
  return (
    <div className="draftbar">
      <span className="hint">{hint}</span>
      <div className="acts">
        <button onClick={undoPoint} disabled={!n}>撤销末点</button>
        {canFinish && <button className="pri" onClick={finishDraft} disabled={n < 2}>完成</button>}
        <button onClick={cancelDraft} disabled={!n}>取消</button>
      </div>
    </div>
  );
}

function SelectOpts({ cmd }: { cmd: boolean }) {
  const sel = useStore((s) => s.selectedId);
  const d = useStore((s) => s.drawings.find((x) => x.id === s.selectedId));
  const moving = useStore((s) => s.moving);
  const more = useStore((s) => s.showMore);
  const lock = useStore((s) => s.layers.lockDrawings);
  if (!sel || !d) return (
    <>
      <div className="empty">{lock ? '标绘已锁定' : '点选地图上的标绘进行编辑'}</div>
      <QuickPalette cmd={cmd} />
    </>
  );
  return <Editor cmd={cmd} more={more} moving={moving} />;
}

function Editor({ cmd, more, moving }: { cmd: boolean; more: boolean; moving: boolean }) {
  const d = useStore((s) => s.drawings.find((x) => x.id === s.selectedId))!;
  const extra = cmd || more;
  const len = d.kind !== 'point' && d.pts.length > 1 ? d.pts.slice(1).reduce((a, p, i) => a + dist(d.pts[i], p), 0) * metersPerCanon('ground') : 0;
  return (
    <div className="editor">
      <div className="ehead">
        {d.kind === 'point' && <GlyphIcon cat={d.cat!} size={24} />}
        <b>{kindName(d)}</b>
        <span className="meta">{gridRef(d.scope === 'air' ? 'air' : 'ground', d.pts[0])} · {d.by} · {fmtClock(d.t)}{len ? ` · ${fmtDist(len)}` : ''}</span>
        <button className="x" onClick={() => setState({ selectedId: null, moving: false })} aria-label="关闭">×</button>
      </div>
      <div className="erow nrow">
        <input className="name" value={d.label} maxLength={16} placeholder="名称（可选）" onChange={(e) => updateDrawing(d.id, { label: e.target.value }, false)} />
        <Seg small value={d.status} opts={[['present', '现状'], ['planned', '计划']]} onChange={(v) => updateDrawing(d.id, { status: v })} />
      </div>
      {(extra || d.kind === 'source' || d.kind === 'route' || d.kind === 'arrow') && (
        <div className={'erow' + (more ? '' : ' extra')}>
          {extra && <Seg small value={d.scope} opts={[['both', '双图'], ['ground', '仅陆'], ['air', '仅空']]} onChange={(v) => updateDrawing(d.id, { scope: v })} />}
          {(d.kind === 'route' || d.kind === 'arrow') && <Toggle on={!!d.smooth} label="曲线" onClick={() => updateDrawing(d.id, { smooth: !d.smooth })} />}
          {d.kind === 'source' && <Seg small value={d.sector ?? 0} opts={[[0, '直线'], [30, '±15°'], [60, '±30°']]} onChange={(v) => updateDrawing(d.id, { sector: v })} />}
        </div>
      )}
      <div className="erow acts">
        <button className={moving ? 'pri' : ''} onClick={() => setState({ moving: !moving })}>{moving ? '点击新位置…' : '移动'}</button>
        <button className="danger" onClick={() => deleteDrawing(d.id)}>删除</button>
        <Toggle on={d.showTime} label="时间" onClick={() => updateDrawing(d.id, { showTime: !d.showTime })} />
        <button className="ghostbtn more" onClick={() => setState({ showMore: !more })}>{more ? '收起' : '更多'}</button>
      </div>
    </div>
  );
}

const FAVS: PointCat[] = ['rally', 'attack', 'enemy', 'danger'];
/** command: full symbol library; member: favourites. Tapping explicitly switches to the point tool. */
function QuickPalette({ cmd }: { cmd: boolean }) {
  const cats = cmd ? CAT_ORDER : FAVS;
  return (
    <div className="quick">
      <div className="stitle">{cmd ? '军标库' : '常用'}</div>
      <div className={'palette' + (cmd ? ' labelled' : ' favs')}>
        {cats.map((c) => (
          <button key={c} onClick={() => { selectTool('point'); setState({ pointCat: c }); }} title={CAT_LABEL[c]} aria-label={'标点：' + CAT_LABEL[c]}>
            <GlyphIcon cat={c} size={cmd ? 26 : 24} />{cmd && <span>{CAT_LABEL[c]}</span>}
          </button>
        ))}
      </div>
    </div>
  );
}

function PointOpts({ cmd, narrow }: { cmd: boolean; narrow: boolean }) {
  const cat = useStore((s) => s.pointCat);
  const sel = useStore((s) => s.selectedId);
  const just = useStore((s) => s.drawings.find((x) => x.id === s.selectedId && x.kind === 'point'));
  return (
    <>
      <div className={'palette' + (cmd ? ' labelled' : '')}>
        {CAT_ORDER.map((c: PointCat) => (
          <button key={c} className={c === cat ? 'on' : ''} onClick={() => setState({ pointCat: c })} title={CAT_LABEL[c]} aria-label={CAT_LABEL[c]} aria-pressed={c === cat}>
            <GlyphIcon cat={c} size={cmd ? 26 : 24} />
            {cmd && <span>{CAT_LABEL[c]}</span>}
          </button>
        ))}
      </div>
      <div className="erow"><StatusSeg />{cmd && <span className="hint">点击地图放置{CAT_LABEL[cat]}</span>}</div>
      {sel && just && (
        <div className="justadded">
          <input className="name" value={just.label} maxLength={16} placeholder={`命名刚放置的${CAT_LABEL[just.cat!]}点`} onChange={(e) => updateDrawing(just.id, { label: e.target.value }, false)} />
          <button className="danger" onClick={() => deleteDrawing(just.id)}>删除</button>
        </div>
      )}
    </>
  );
}

function RouteOpts({ cmd }: { cmd: boolean }) {
  const rt = useStore((s) => s.routeType);
  const smooth = useStore((s) => s.smooth);
  const n = useStore((s) => s.draft?.pts.length ?? 0);
  return (
    <>
      <div className="erow">
        <Seg value={rt} opts={[['route', ROUTE_LABEL.route], ['arrow', cmd ? ROUTE_LABEL.arrow : '箭头'], ['phase', '阶段线']]} onChange={(v) => setState({ routeType: v })} />
      </div>
      <div className="erow"><StatusSeg />{rt !== 'phase' && <Toggle on={smooth} label="曲线" onClick={() => setState({ smooth: !smooth })} />}</div>
      <DraftBar n={n} hint={n ? `${n} 个点 · 双击或“完成”结束` : '逐点点击地图'} />
    </>
  );
}

function AreaOpts({ cmd }: { cmd: boolean }) {
  const at = useStore((s) => s.areaType);
  const sh = useStore((s) => s.areaShape);
  const n = useStore((s) => s.draft?.pts.length ?? 0);
  return (
    <>
      <div className="erow"><Seg value={at} opts={[['airstrike', AREA_LABEL.airstrike], ['recon', AREA_LABEL.recon], ['warning', AREA_LABEL.warning]]} onChange={(v) => setState({ areaType: v })} /></div>
      <div className="erow"><Seg small value={sh} opts={[['rect', '矩形'], ['circle', '圆形']]} onChange={(v) => setState({ areaShape: v })} /><StatusSeg /></div>
      <DraftBar n={n} canFinish={false} hint={n ? (sh === 'rect' ? '点击对角' : '点击半径终点') : (sh === 'rect' ? '点击第一个角' : '点击圆心')} />
    </>
  );
}

function SourceOpts({ cmd }: { cmd: boolean }) {
  const sec = useStore((s) => s.sourceSector);
  const n = useStore((s) => s.draft?.pts.length ?? 0);
  return (
    <>
      <div className="steps">
        <span className={n === 0 ? 'cur' : 'done'}>① 受击位置</span>
        <span className={n === 1 ? 'cur' : ''}>② 来袭方向</span>
      </div>
      <div className="erow"><Seg small value={sec} opts={[[0, '直线'], [30, '±15°'], [60, '±30°']]} onChange={(v) => setState({ sourceSector: v })} />{cmd && <span className="hint">人工报告 · 未确认</span>}</div>
      <DraftBar n={n} canFinish={false} hint={n ? '点击疑似来源方向' : '点击你被击中的位置'} />
    </>
  );
}

function RulerOpts({ cmd }: { cmd: boolean }) {
  const r = useStore((s) => s.ruler);
  const sp = useStore((s) => s.rulerSpeed);
  const total = r && r.pts.length > 1 ? r.pts.slice(1).reduce((a, p, i) => a + dist(r.pts[i], p), 0) * metersPerCanon(r.map) : 0;
  return (
    <>
      <div className="erow"><Seg small value={sp} opts={[[30, '30 km/h'], [60, '60'], [450, '450 空']]} onChange={(v) => setState({ rulerSpeed: v })} /></div>
      <div className="readout">{total ? <><b>{fmtDist(total)}</b><span>ETA {fmtClock(total / (sp / 3.6))}</span><span>{r!.pts.length} 点</span></> : <span className="hint">逐点点击测量 · 仅本地</span>}</div>
      <div className="draftbar"><span className="hint" /><div className="acts">
        <button onClick={undoPoint} disabled={!r}>撤销末点</button>
        <button onClick={() => setState({ ruler: null })} disabled={!r}>清除</button>
      </div></div>
    </>
  );
}

function LayerOpts({ cmd }: { cmd: boolean }) {
  const L = useStore((s) => s.layers);
  const set = (k: keyof typeof L) => setState((s) => ({ layers: { ...s.layers, [k]: !s.layers[k] } }));
  const rows: [keyof typeof L, string][] = [['grid', '网格'], ['friendly', '友军'], ['spawns', '重生点/机场'], ['enemy', '自动敌情'], ['drawings', '标绘'], ['labels', '名称'], ['time', '时间标签'], ['lockDrawings', '锁定标绘']];
  return (
    <div className="layers">
      {rows.map(([k, l]) => <Toggle key={k} on={L[k]} label={l} onClick={() => set(k)} />)}
    </div>
  );
}
