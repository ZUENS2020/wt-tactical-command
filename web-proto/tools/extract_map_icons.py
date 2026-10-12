"""Extract the real 8111 map glyphs, not Wiki vehicle silhouettes.

Usage: python3 tools/extract_map_icons.py /path/to/icons.ttf /path/to/index.html
Requires fontTools. The input files come from the running game's read-only HTTP API.
"""
import hashlib
import json
import pathlib
import sys
from fontTools.ttLib import TTFont
from fontTools.pens.boundsPen import BoundsPen
from fontTools.pens.svgPathPen import SVGPathPen

ROOT = pathlib.Path(__file__).resolve().parents[1]
ROLES = {
    'Assault': 'A', 'Bomber': 'B', 'Fighter': 'F', 'LightTank': 'L',
    'MediumTank': 'M', 'TankDestroyer': 'T', 'Tracked': 'T', 'Wheeled': 'W',
    'SPAA': 'S', 'SAM': 'S', 'Airdefence': '4', 'Structure': '5',
}

def main():
    font_path, page_path = map(pathlib.Path, sys.argv[1:3])
    page = page_path.read_text()
    if "s = item['icon'][0]" not in page or "s = '4'" not in page:
        raise SystemExit('Game map mapping changed; review the source before extracting.')
    font = TTFont(font_path)
    glyph_set, cmap = font.getGlyphSet(), font.getBestCmap()
    glyphs = {}
    out = ROOT / 'public/icons/map'
    out.mkdir(parents=True, exist_ok=True)
    files = []
    for char in sorted(set(ROLES.values())):
        name = cmap[ord(char)]
        bounds, path = BoundsPen(glyph_set), SVGPathPen(glyph_set)
        glyph_set[name].draw(bounds)
        glyph_set[name].draw(path)
        if not bounds.bounds:
            raise SystemExit(f'Missing native glyph: {char}')
        x0, y0, x1, y1 = bounds.bounds
        glyphs[char] = {'path': path.getCommands(), 'bounds': bounds.bounds}
        # A 30 px sprite corresponds to the game's 18 pt (24 px) icon font.
        em = font['head'].unitsPerEm
        extent = em * 30 / 24
        cx, cy = (x0+x1)/2, (y0+y1)/2
        svg = f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="{-extent/2} {-extent/2} {extent} {extent}"><path fill="#fff" transform="translate({-cx},{cy}) scale(1,-1)" d="{path.getCommands()}"/></svg>\n'
        file = f'glyph_{char}.svg'
        (out/file).write_text(svg)
        files.append({'file': file, 'glyph': char, 'sha256': hashlib.sha256(svg.encode()).hexdigest()})
    data = {'unitsPerEm': font['head'].unitsPerEm, 'fontSize': 24, 'roles': ROLES, 'glyphs': glyphs}
    (ROOT/'src/data/mapGlyphs.json').write_text(json.dumps(data, ensure_ascii=False, indent=2)+'\n')
    manifest = {
        'source': 'War Thunder built-in tactical map HTTP assets',
        'verified_date': '2026-10-11',
        'font_url': 'http://127.0.0.1:8111/icons.ttf',
        'page_url': 'http://127.0.0.1:8111/',
        'font_sha256': hashlib.sha256(font_path.read_bytes()).hexdigest(),
        'page_sha256': hashlib.sha256(page_path.read_bytes()).hexdigest(),
        'mapping': ROLES,
        'unavailable': {'HeavyTank': 'Native H glyph is empty', 'Helicopter': 'Native H glyph is empty', 'naval': 'No verified native category mapping'},
        'fallback': 'Neutral symbol with the received category label; never substitute a silhouette or guess a class',
        'files': files,
    }
    (out/'sources.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2)+'\n')
    print(f'Extracted {len(glyphs)} native map glyphs / {len(ROLES)} category mappings')

if __name__ == '__main__':
    main()
