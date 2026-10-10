import { useEffect } from 'react';
import { getState, setState, useStore } from './store';
import { computeLayout, useViewport } from './layout';
import TopBar from './TopBar';
import MapView from './MapView';
import ToolPanel from './ToolPanel';
import Sheet from './Sheet';
import { cancelDraft, finishDraft, undoPoint, undo, selectTool, deleteDrawing } from './actions';
import type { Tool } from './store';
import { MAPS } from './geo';

export default function App() {
  const { W, H } = useViewport();
  const mode = useStore((s) => s.mode);
  const density = useStore((s) => s.density);
  const toasts = useStore((s) => s.toasts);
  useStore((s) => s.mapRev);
  const mk = (m: 'ground' | 'air') => `${m}-${MAPS[m].hash ?? 'none'}-${MAPS[m].min.join(',')}-${MAPS[m].max.join(',')}`;
  const L = computeLayout(W, H, mode, density);

  useEffect(() => {
    const keys: Record<string, Tool> = { v: 'select', p: 'point', r: 'route', l: 'route', a: 'area', s: 'source', m: 'ruler' };
    const onKey = (e: KeyboardEvent) => {
      const tag = (e.target as HTMLElement).tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA') { if (e.key === 'Escape') (e.target as HTMLElement).blur(); return; }
      const s = getState();
      if (e.key === 'Escape') { if (s.draft) cancelDraft(); else if (s.sheet) setState({ sheet: null }); else setState({ selectedId: null, moving: false }); }
      else if (e.key === 'Enter') finishDraft();
      else if (e.key === 'Backspace') { if (s.draft || s.ruler) undoPoint(); else if (s.selectedId) deleteDrawing(s.selectedId); }
      else if (e.key === 'Delete' && s.selectedId) deleteDrawing(s.selectedId);
      else if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') { e.preventDefault(); undo(); }
      else if (!e.ctrlKey && !e.metaKey && !e.altKey && keys[e.key.toLowerCase()] && s.mode !== 'dual') selectTool(keys[e.key.toLowerCase()]);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);

  const single = mode !== 'dual';
  const mainH = H - L.T;
  return (
    <div className={`app ${density}`} style={{ width: W, height: H }}>
      <TopBar h={L.T} W={W} />
      <main className={`main ${L.kind}`} style={{ height: mainH, padding: L.pad / 2, gap: L.gap }} data-layout={L.kind}>
        {single ? (
          <>
            <MapView key={mk(mode as 'ground' | 'air')} map={mode as 'ground' | 'air'} size={L.S} interactive dual={false} feed />
            <ToolPanel orient={L.kind === 'side' ? 'side' : 'bottom'} w={L.panelW} h={L.panelH} />
          </>
        ) : (
          <>
            <MapView key={'d' + mk('ground')} map="ground" size={L.S} interactive={false} dual feed />
            <MapView key={'d' + mk('air')} map="air" size={L.S} interactive={false} dual />
          </>
        )}
      </main>
      <div className="toasts" style={{ top: L.T + 10 }} aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className={'toast ' + (t.tone ?? '')}>
            {t.tone === 'sync' && <i className="sdot" />}{t.text}
            {t.undo && <button onClick={() => undo()}>撤销</button>}
          </div>
        ))}
      </div>
      <Sheet top={L.T + 6} />
    </div>
  );
}
