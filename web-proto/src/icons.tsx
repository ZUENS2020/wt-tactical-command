import type { Tool } from './store';
const P: Record<Tool, JSX.Element> = {
  select: <path d="M5 3l11 7-5 1.2 3 5.3-2.2 1.2-3-5.3L5 16z" />,
  point: <><path d="M10 18s-5.5-5.4-5.5-9.3a5.5 5.5 0 0 1 11 0C15.5 12.6 10 18 10 18z" /><circle cx="10" cy="8.6" r="2" /></>,
  route: <><path d="M3 16l4.5-6 4 3L16 5" /><path d="M12.6 4.6L16.4 4.4 16.2 8.2" /></>,
  area: <><rect x="3.5" y="4.5" width="13" height="11" strokeDasharray="2.6 2" /></>,
  source: <><path d="M5.5 5.5l9 9M14.5 5.5l-9 9" opacity=".0" /><circle cx="6.5" cy="13.5" r="2.4" /><path d="M17 3l-8.5 8.5" strokeDasharray="2.4 1.8" /><path d="M8.8 7.4l-.4 4.3 4.3-.4" /></>,
  ruler: <><path d="M2.8 13.6L13.6 2.8l3.6 3.6L6.4 17.2z" /><path d="M6 10.4l1.6 1.6M8.4 8l2.4 2.4M10.8 5.6l1.6 1.6" /></>,
  layers: <><path d="M10 3l7 3.6-7 3.6-7-3.6z" /><path d="M3 10.2l7 3.6 7-3.6M3 13.6l7 3.6 7-3.6" /></>,
};
export function ToolIcon({ tool, size = 20 }: { tool: Tool; size?: number }) {
  return <svg className="ticon" width={size} height={size} viewBox="0 0 20 20" aria-hidden>{P[tool]}</svg>;
}
export const TOOL_LABEL: Record<Tool, string> = { select: '选择', point: '标点', route: '路线', area: '区域', source: '来源', ruler: '测距', layers: '图层' };
