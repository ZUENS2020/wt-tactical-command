import { useEffect, useState } from 'react';


function vp() {
  const vv = window.visualViewport;
  const W = Math.floor(vv ? vv.width : window.innerWidth);
  const H = Math.floor(vv ? vv.height : window.innerHeight);
  return { W: Math.min(W, document.documentElement.clientWidth || W), H };
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
