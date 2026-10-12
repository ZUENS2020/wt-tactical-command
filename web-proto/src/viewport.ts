import { canonToNorm, normToCanon, gridBox, type MapId, type V2 } from './geo';
import type { ViewState } from './store';
export function defaultView(map: MapId): ViewState {
  const g = gridBox(map);
  return { cx: g.x0 + g.w / 2, cy: g.y0 + g.h / 2, z: Math.max(1, 1 / (Math.max(g.w, g.h) * 1.18)) };
}
export function transform(map: MapId, width: number, height: number, view: ViewState, aspect = 1) {
  const full = Math.min(width, height) * view.z;
  const xScale = full, yScale = full / aspect;
  const normToScreen = (n: V2): V2 => [(n[0] - view.cx) * xScale + width / 2, (n[1] - view.cy) * yScale + height / 2];
  const screenToNorm = (p: V2): V2 => [(p[0] - width / 2) / xScale + view.cx, (p[1] - height / 2) / yScale + view.cy];
  return { xScale, yScale, normToScreen, screenToNorm,
    worldToScreen: (w: V2) => normToScreen(canonToNorm(map, w)),
    screenToWorld: (p: V2) => normToCanon(map, screenToNorm(p)),
  };
}
export function zoomAt(map: MapId, width: number, height: number, view: ViewState, point: V2, factor: number, aspect = 1): ViewState {
  const n = transform(map, width, height, view, aspect).screenToNorm(point);
  const z = Math.max(0.5, Math.min(64, view.z * factor));
  const t = transform(map, width, height, { ...view, z }, aspect);
  return { z, cx: n[0] - (point[0] - width / 2) / t.xScale, cy: n[1] - (point[1] - height / 2) / t.yScale };
}
