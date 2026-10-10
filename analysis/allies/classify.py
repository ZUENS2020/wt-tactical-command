"""Map object classification: who (side/role) and what (kind). Keep in sync with common/src/mapobj.rs.

Confirmed on real RB ground data (session 1709/df9b, 2026-10-10; see ALLY_MARKERS_20261010.md):
  #faC81E  Player icon                      -> self
  #174DFF  units                            -> teammate (ally)
  #67D756  ground units, #39D921 aircraft   -> squadmate (auto-squad; one per battle, matches "团队协作" awards / deaths)
  #043FFF blink 2 on a unit / Player        -> "radio signal": the unit's owner just sent a radio/quick-chat
                                              message (incl. map pings); 1.9 s (thanks) .. 5.9 s (location / ping).
                                              Colour hides squad/self identity -> resolved by the tracker.
  #1839A7 ground, #134AFF aircraft blink 0  -> teammate that just spawned (2-20 s, then #174DFF)
  Player icon in #174DFF                    -> NOT self: after own death the Player icon follows the spectated ally
  #fa0C00 / #f00C00                         -> enemy (unchanged, see enemy/)
No separate ping / point_of_interest object was observed (0 in 2 RB battles with 20+ pings); the type is
handled anyway (kind "map_marker") because other tools document it.
"""
from dataclasses import dataclass
from typing import Optional, Tuple

SELF_RGB = (250, 200, 30)                       # #faC81E
SIGNAL_RGB = (4, 63, 255)                       # #043FFF (blink 2)
ALLY_RGB = (23, 77, 255)                        # #174DFF
SQUAD_COLORS = {(103, 215, 86): "ground", (57, 217, 33): "air"}        # #67D756, #39D921
FRESH_SPAWN_COLORS = {(24, 57, 167): "ground", (19, 74, 255): "air"}   # #1839A7, #134AFF

UNIT_TYPES = frozenset({"ground_model", "aircraft"})
KIND_BY_TYPE = {
    "ground_model": "unit", "aircraft": "unit", "capture_zone": "capture_zone", "airfield": "airfield",
    "bombing_point": "bombing_point", "defending_point": "defending_point",
    "respawn_base_tank": "respawn_base", "respawn_base_fighter": "respawn_base", "respawn_base_bomber": "respawn_base",
    "point_of_interest": "map_marker",
}


@dataclass(frozen=True)
class ObjClass:
    kind: str            # unit | capture_zone | respawn_base | airfield | bombing_point | defending_point | map_marker
    side: str            # self | squad | ally | enemy | neutral | unknown
    signal: bool = False         # #043FFF blink 2 (owner sent a radio message / ping)
    fresh_spawn: bool = False    # #1839A7 / #134AFF
    spectated: bool = False      # Player icon that is not self (spectator camera after death)
    known_type: bool = True      # False: type not in KIND_BY_TYPE -> treated as map_marker candidate

    @property
    def is_unit(self) -> bool:
        return self.kind == "unit"

    @property
    def friendly(self) -> bool:
        return self.side in ("self", "squad", "ally")


def rgb_of(o) -> Tuple[int, int, int]:
    return tuple(o.rgb) if hasattr(o, "rgb") else _rgb_dict(o)


def _rgb_dict(o: dict) -> Tuple[int, int, int]:
    c = o.get("color[]")
    if isinstance(c, list) and len(c) >= 3:
        return (int(c[0]), int(c[1]), int(c[2]))
    s = str(o.get("color", "")).lstrip("#")
    try:
        return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16))
    except ValueError:
        return (0, 0, 0)


def _get(o, k, d=None):
    return getattr(o, k, d) if not isinstance(o, dict) else o.get(k, d)


def side_of_rgb(rgb, blink: int = 0) -> str:
    r, g, b = rgb
    if rgb == SELF_RGB:
        return "self"
    if rgb in SQUAD_COLORS or (g >= 180 and r < 150 and b < 150):
        return "squad"
    if r >= 200 and g < 130 and b < 130:
        return "enemy"
    if b >= 150 and b > r + 60 and b > g + 40:
        return "ally"
    if r >= 200 and g >= 200 and b >= 200:
        return "neutral"
    return "unknown"


def classify(o) -> ObjClass:
    """o: enemy.model.MapObj or a raw map_obj dict."""
    ty = str(_get(o, "type", "") or "")
    icon = str(_get(o, "icon", "") or "")
    blink = int(_get(o, "blink", 0) or 0)
    rgb = rgb_of(o)
    kind = KIND_BY_TYPE.get(ty)
    known = kind is not None
    if kind is None:
        kind = "map_marker"            # unknown type: never a unit, never dropped
    signal = rgb == SIGNAL_RGB and blink == 2 and kind == "unit"
    fresh = rgb in FRESH_SPAWN_COLORS and kind == "unit"
    if kind == "unit" and icon == "Player":
        if rgb == SELF_RGB or signal:
            return ObjClass(kind, "self", signal=signal)
        side = side_of_rgb(rgb, blink)
        return ObjClass(kind, side if side != "self" else "ally", spectated=True)
    if signal:
        return ObjClass(kind, "ally", signal=True)          # squad or ally: colour hides it, see tracker
    if fresh:
        return ObjClass(kind, "ally", fresh_spawn=True)
    side = side_of_rgb(rgb, blink)
    if side == "self":
        side = "unknown"                                    # yellow non-Player object: not seen so far
    if kind != "unit" and side == "squad":
        side = "ally"
    return ObjClass(kind, side, known_type=known)


def find_self(objs):
    """Own Player object, or None (also None while spectating: the Player icon is then a blue ally)."""
    for o in objs:
        if _get(o, "icon") == "Player" and _get(o, "x") is not None and classify(o).side == "self":
            return o
    return None
