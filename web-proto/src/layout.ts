import { useEffect, useState } from 'react';
import type { Density, ViewMode } from './store';

export interface Layout {
  W: number; H: number; T: number; pad: number; gap: number;
  kind: 'side' | 'bottom' | 'dualH' | 'dualV';
  S: number; panelW: number; panelH: number;
}

function vp() {
  const vv = window.visualViewport;
  const W = Math.floor(vv ? vv.width : window.innerWidth);
  const H = Math.floor(vv ? vv.height : window.innerHeight);
  return { W: Math.min(W, document.documentElement.clientWidth || W), H };
}

/** Plan v1.2 §3.2 forced layout algorithm */
export function computeLayout(W: number, H: number, mode: ViewMode, density: Density): Layout {
  const T = H < 430 ? 38 : W < 420 ? 44 : 46;
  const small = W < 600 || H < 500;
  const pad = small ? 8 : 16;          // total (both sides)
  const gap = small ? 6 : 10;
  const avH = H - T - pad;
  if (mode === 'dual') {
    const sH = Math.floor(Math.min(avH, (W - gap - pad) / 2));
    const sV = Math.floor(Math.min(W - pad, (H - T - gap - pad) / 2));
    return sH >= sV ? { W, H, T, pad, gap, kind: 'dualH', S: sH, panelW: 0, panelH: 0 } : { W, H, T, pad, gap, kind: 'dualV', S: sV, panelW: 0, panelH: 0 };
  }
  const cmd = density === 'command';
  const sideMin = cmd ? 236 : 176;
  const sideMax = cmd ? 360 : 268;
  const toolsMin = cmd ? 190 : 140;
  const sSide = Math.floor(Math.min(avH, W - sideMin - gap - pad));
  const sBot = Math.floor(Math.min(W - pad, H - T - toolsMin - gap - pad));
  if (sSide >= sBot) {
    const panelW = Math.floor(Math.min(sideMax, W - sSide - gap - pad));
    return { W, H, T, pad, gap, kind: 'side', S: sSide, panelW, panelH: sSide };
  }
  const panelH = Math.floor(H - T - sBot - gap - pad);
  return { W, H, T, pad, gap, kind: 'bottom', S: sBot, panelW: Math.floor(W - pad), panelH };
}

export function useViewport() {
  const [v, setV] = useState(vp);
  useEffect(() => {
    let raf = 0;
    const upd = () => { cancelAnimationFrame(raf); raf = requestAnimationFrame(() => setV(vp())); };
    const ro = new ResizeObserver(upd);
    ro.observe(document.documentElement);
    window.visualViewport?.addEventListener('resize', upd);
    window.addEventListener('orientationchange', upd);
    window.addEventListener('resize', upd);
    return () => { ro.disconnect(); window.visualViewport?.removeEventListener('resize', upd); window.removeEventListener('orientationchange', upd); window.removeEventListener('resize', upd); };
  }, []);
  return v;
}
