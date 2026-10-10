"""Grid labels <-> normalized map coordinates.

Confirmed on real data (8 sender-location calls "请求掩护 [e6]" etc.: chat grid == grid of the sender's
blinking icon, 9/9 ground calls): columns are numbers, rows letters, both counted from grid_zero
(world top-left of the grid) in steps of grid_steps. Not from map_min (that gives J11 instead of E6).
World y grows northwards; normalized y grows downwards: ny = (map_max.y - wy) / span_y.
"""
from dataclasses import dataclass
import math, re
from typing import Optional, Tuple

_LABEL = re.compile(r"^([A-Za-z]{1,2})(\d{1,2})$")


@dataclass(frozen=True)
class GridInfo:
    map_min: Tuple[float, float]
    map_max: Tuple[float, float]
    grid_steps: Tuple[float, float]
    grid_zero: Optional[Tuple[float, float]] = None
    grid_size: Optional[Tuple[float, float]] = None

    @staticmethod
    def from_json(j: dict) -> Optional["GridInfo"]:
        if not j or not j.get("valid"):
            return None
        try:
            mn = (float(j["map_min"][0]), float(j["map_min"][1]))
            mx = (float(j["map_max"][0]), float(j["map_max"][1]))
            st = (abs(float(j["grid_steps"][0])), abs(float(j["grid_steps"][1])))
        except (KeyError, IndexError, TypeError, ValueError):
            return None
        if mx[0] <= mn[0] or mx[1] <= mn[1] or st[0] <= 0 or st[1] <= 0:
            return None
        z = j.get("grid_zero")
        gz = (float(z[0]), float(z[1])) if isinstance(z, list) and len(z) >= 2 else None
        s = j.get("grid_size")
        gs = (float(s[0]), float(s[1])) if isinstance(s, list) and len(s) >= 2 else None
        return GridInfo(mn, mx, st, gz, gs)

    @property
    def zero(self) -> Tuple[float, float]:
        return self.grid_zero if self.grid_zero is not None else (self.map_min[0], self.map_max[1])


def row_index(letters: str) -> int:
    n = 0
    for ch in letters.upper():
        n = n * 26 + (ord(ch) - 64)
    return n - 1


def row_letters(i: int) -> str:
    s, i = "", i + 1
    while i > 0:
        i, rem = divmod(i - 1, 26)
        s = chr(65 + rem) + s
    return s


def cell_bounds(label: str, gi: GridInfo):
    """'E6' -> (x0, y0, x1, y1) normalized (may lie partly outside 0..1), or None."""
    m = _LABEL.match(label.strip())
    if not m or gi is None:
        return None
    row, col = row_index(m.group(1)), int(m.group(2)) - 1
    if col < 0:
        return None
    zx, zy = gi.zero
    spx, spy = gi.map_max[0] - gi.map_min[0], gi.map_max[1] - gi.map_min[1]
    wx0 = zx + col * gi.grid_steps[0]
    wy_top = zy - row * gi.grid_steps[1]
    x0 = (wx0 - gi.map_min[0]) / spx
    y0 = (gi.map_max[1] - wy_top) / spy
    return (x0, y0, x0 + gi.grid_steps[0] / spx, y0 + gi.grid_steps[1] / spy)


def cell_of(x: float, y: float, gi: GridInfo) -> Optional[str]:
    if gi is None or x is None or y is None:
        return None
    wx = gi.map_min[0] + x * (gi.map_max[0] - gi.map_min[0])
    wy = gi.map_max[1] - y * (gi.map_max[1] - gi.map_min[1])
    zx, zy = gi.zero
    col = math.floor((wx - zx) / gi.grid_steps[0]) + 1
    row = math.floor((zy - wy) / gi.grid_steps[1])
    if row < 0 or col < 1:
        return None
    return f"{row_letters(row)}{col}"


def center(b):
    return None if b is None else ((b[0] + b[2]) / 2, (b[1] + b[3]) / 2)
