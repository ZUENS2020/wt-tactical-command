import { useEffect } from 'react';
import { getState, setState, useStore, type Tool } from './store';
import { useViewport } from './layout';
import TopBar from './TopBar';
import MapView from './MapView';
import ToolPanel from './ToolPanel';
import Sheet from './Sheet';
import { cancelDraft, finishDraft, undoPoint, undo, selectTool, deleteDrawing } from './actions';
import { ToolIcon, TOOL_LABEL } from './icons';
const TOOLS: Tool[] = ['select', 'point', 'route', 'area', 'source', 'ruler', 'layers'];
function Toolbar({ mobile }: { mobile: boolean }) {
  const tool = useStore(s => s.tool), more = useStore(s => s.showMore), nUndo = useStore(s => s.undo.length);
  const shown = mobile ? TOOLS.slice(0, 3) : TOOLS;
  const button = (t: Tool) => <button key={t} className={tool === t ? 'on' : ''} aria-label={TOOL_LABEL[t]} aria-pressed={tool === t} data-tool={t} title={TOOL_LABEL[t]} onClick={() => { selectTool(t); setState({ showMore: false }); }}><ToolIcon tool={t} size={22} /><span>{TOOL_LABEL[t]}</span></button>;
  return <nav className={`toolbar ${mobile ? 'mobile' : 'desktop'}`} aria-label="标绘工具">{shown.map(button)}{mobile && <button className={more ? 'on' : ''} aria-label="更多工具" onClick={() => setState({ showMore: !more })}>•••<span>更多</span></button>}<button onClick={undo} disabled={!nUndo} aria-label="撤销" title="撤销">↶<span>撤销</span></button>{mobile && more && <div className="more-tools">{TOOLS.slice(3).map(button)}</div>}</nav>;
}
export default function App() {
  const { W, H } = useViewport();
  const mode = useStore(s => s.mode), density = useStore(s => s.density), panel = useStore(s => s.panelOpen), toasts = useStore(s => s.toasts);
  const mobile = W < 760 || H < 480;
  useEffect(() => {
    const keys: Record<string, Tool> = { v: 'select', p: 'point', r: 'route', l: 'route', a: 'area', s: 'source', m: 'ruler' };
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement).tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT') { if (e.key === 'Escape') (e.target as HTMLElement).blur(); return; }
      const s = getState();
      if (e.key === 'Escape') { if (s.draft) cancelDraft(); else setState({ sheet: null, panelOpen: false, selectedId: null, selectedUnit: null, moving: false, showMore: false }); }
      else if (e.key === 'Enter') finishDraft();
      else if (e.key === 'Backspace' || e.key === 'Delete') { e.preventDefault(); if (s.draft || s.ruler) undoPoint(); else if (s.selectedId) deleteDrawing(s.selectedId); }
      else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') { e.preventDefault(); undo(); }
      else if (!e.ctrlKey && !e.metaKey && !e.altKey && keys[e.key.toLowerCase()]) selectTool(keys[e.key.toLowerCase()]);
    };
    window.addEventListener('keydown', onKey); return () => window.removeEventListener('keydown', onKey);
  }, []);
  return <div className={`app ${mobile ? 'mobile-ui' : 'desktop-ui'} ${density}`} style={{ width: W, height: H }}>
    <TopBar W={W} />
    <div className="workspace">
      {!mobile && <Toolbar mobile={false} />}
      <main className={`maps ${mode === 'dual' ? W >= H ? 'dualH' : 'dualV' : 'single'}`} data-layout={mode === 'dual' ? W >= H ? 'dualH' : 'dualV' : 'single'}>
        {(mode === 'ground' || mode === 'dual') && <MapView map="ground" />}{(mode === 'air' || mode === 'dual') && <MapView map="air" />}
      </main>
      {panel && <aside className="details"><ToolPanel /></aside>}
      {!panel && <button className="details-open" aria-label="展开详情" onClick={() => setState({ panelOpen: true })}>☰</button>}
    </div>
    {mobile && <Toolbar mobile />}
    <div className="toasts" aria-live="polite">{toasts.map(t => <div key={t.id} className={`toast ${t.tone ?? ''}`}>{t.text}{t.undo && <button onClick={undo}>撤销</button>}</div>)}</div>
    <Sheet />
  </div>;
}
