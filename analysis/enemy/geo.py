"""Coordinates. Spec §5: world = map_min + norm * (map_max - map_min); y grows downward (south).
Compass bearing / heading: atan2(east, -south) i.e. atan2(dx, -dy), 0 = north, clockwise."""
import math
from typing import Tuple, Optional

SECTORS = ("N", "NE", "E", "SE", "S", "SW", "W", "NW")


def to_world(mi, x: float, y: float) -> Tuple[float, float]:
    w, h = mi.size
    return (mi.map_min[0] + x * w, mi.map_min[1] + y * h)


def compass(dx: float, dy: float) -> float:
    """dx east-positive, dy south-positive (map/image convention)."""
    return (math.degrees(math.atan2(dx, -dy)) + 360.0) % 360.0


def heading_from_dir(dx: Optional[float], dy: Optional[float]) -> Optional[float]:
    if dx is None or dy is None or (dx == 0 and dy == 0):
        return None
    return compass(dx, dy)


def rel(mi, px: float, py: float, tx: float, ty: float) -> Tuple[float, float]:
    """distance (world units) and compass bearing from player (px,py) to target (tx,ty), normalized inputs."""
    w, h = mi.size
    ex, sy = (tx - px) * w, (ty - py) * h
    return (math.hypot(ex, sy), compass(ex, sy))


def sector_index(bearing_deg: float) -> int:
    return int(((bearing_deg + 22.5) % 360.0) // 45.0)


def ang_diff(a: float, b: float) -> float:
    return abs((a - b + 180.0) % 360.0 - 180.0)
