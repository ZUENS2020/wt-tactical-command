import native from './data/mapGlyphs.json';
/** Exact outlines from the game's 8111 map font, at its native relative sizes. */
export const VEHICLE_ICONS: Record<string, string> = native.roles;
export const VEHICLE_LABELS: Record<string, string> = {
  LightTank: '轻坦', MediumTank: '中坦', HeavyTank: '重坦', TankDestroyer: '坦歼', SPAA: '防空车', SAM: '防空导弹',
  Airdefence: '防空', Fighter: '战斗机', Bomber: '轰炸机', Assault: '攻击机', Helicopter: '直升机',
  Jet: '喷气机', UCAV: '无人机', UtilityHelicopter: '通用直升机', Battleship: '战列舰', BattleCruiser: '战巡', LightCruiser: '轻巡', Frigate: '护卫舰', Barge: '驳船', SubmarineChaser: '猎潜艇', Unknown: '未知',
  Tracked: '履带载具', Wheeled: '轮式载具', BattleShip: '战列舰', Destroyer: '驱逐舰', HeavyCruiser: '重巡', Boat: '艇', Player: '本人',
};
const glyphs: Record<string, { path: string; bounds: number[] }> = native.glyphs;
const paths = new Map<string, Path2D>();
const sprites = new Map<string, HTMLCanvasElement>();
export function vehicleSprite(icon: string, color: string, size = 30, dpr = 1): HTMLCanvasElement | null {
  const role = VEHICLE_ICONS[icon]; if (!role) return null;
  const ratio = Math.min(3, Math.max(1, Math.round(dpr * 2) / 2));
  const key = `${role}/${color}/${size}/${ratio}`;
  const hit = sprites.get(key); if (hit) { sprites.delete(key); sprites.set(key, hit); return hit; }
  const glyph = glyphs[role];
  let path = paths.get(role);
  if (!path) { path = new Path2D(glyph.path); paths.set(role, path); }
  const c = document.createElement('canvas'); c.width = c.height = Math.ceil(size * ratio);
  const ctx = c.getContext('2d')!;
  const scale = native.fontSize / native.unitsPerEm * c.width / 30;
  const [x0, y0, x1, y1] = glyph.bounds;
  ctx.translate(c.width / 2, c.height / 2);
  ctx.scale(scale, -scale); ctx.translate(-(x0 + x1) / 2, -(y0 + y1) / 2);
  ctx.fillStyle = color; ctx.fill(path);
  ctx.strokeStyle = '#171b14'; ctx.lineWidth = c.width / 30 / scale; ctx.stroke(path);
  sprites.set(key, c); if (sprites.size > 128) sprites.delete(sprites.keys().next().value!);
  return c;
}
export function iconCacheSize() { return sprites.size; }
