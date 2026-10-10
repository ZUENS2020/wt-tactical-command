"""Plain data types. Every field is a primitive / tuple so the Rust port is mechanical."""
from __future__ import annotations
from dataclasses import dataclass, field
from typing import Optional, List, Tuple


@dataclass(frozen=True)
class MapInfo:
    map_min: Tuple[float, float]
    map_max: Tuple[float, float]
    map_generation: int = -1
    hud_type: int = -1

    @property
    def size(self) -> Tuple[float, float]:
        return (self.map_max[0] - self.map_min[0], self.map_max[1] - self.map_min[1])

    @staticmethod
    def from_json(j: dict) -> Optional["MapInfo"]:
        if not j or not j.get("valid"):
            return None
        try:
            mn = (float(j["map_min"][0]), float(j["map_min"][1]))
            mx = (float(j["map_max"][0]), float(j["map_max"][1]))
        except (KeyError, IndexError, TypeError, ValueError):
            return None
        if mx[0] <= mn[0] or mx[1] <= mn[1]:
            return None
        return MapInfo(mn, mx, int(j.get("map_generation", -1)), int(j.get("hud_type", -1)))


@dataclass(frozen=True)
class MapObj:
    """One raw entry of /map_obj.json (point objects only; airfields keep sx..ey)."""
    type: str
    icon: str
    color: str                      # raw hex string, e.g. "#fa0C00"
    rgb: Tuple[int, int, int]       # color[]
    blink: int
    x: Optional[float] = None
    y: Optional[float] = None
    dx: Optional[float] = None
    dy: Optional[float] = None
    icon_bg: str = "none"

    @staticmethod
    def from_json(o: dict) -> "MapObj":
        rgb = o.get("color[]")
        if not (isinstance(rgb, list) and len(rgb) >= 3):
            rgb = _hex_to_rgb(o.get("color", ""))
        def f(k):
            v = o.get(k)
            return float(v) if isinstance(v, (int, float)) else None
        return MapObj(
            type=str(o.get("type", "")), icon=str(o.get("icon", "")),
            color=str(o.get("color", "")), rgb=(int(rgb[0]), int(rgb[1]), int(rgb[2])),
            blink=int(o.get("blink", 0) or 0), x=f("x"), y=f("y"), dx=f("dx"), dy=f("dy"),
            icon_bg=str(o.get("icon_bg", "none")))

    @property
    def has_dir(self) -> bool:
        return self.dx is not None and self.dy is not None


def _hex_to_rgb(s: str) -> Tuple[int, int, int]:
    s = s.lstrip("#")
    try:
        return (int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16))
    except (ValueError, IndexError):
        return (0, 0, 0)


@dataclass(frozen=True)
class Frame:
    t_ms: int
    objects: Tuple[MapObj, ...]


@dataclass(frozen=True)
class HudEvent:
    """A /hudmsg damage line (already de-duplicated by id)."""
    t_ms: int          # receive wall time (game 'time' field is mission seconds, not usable)
    id: int
    msg: str           # U+200B removed
    involves_player: bool = False
    kind: str = "other"  # kill | fire | crit | severe | award | disconnect | other


@dataclass
class Observation:
    t_ms: int
    x: float
    y: float
    color: str
    rgb: Tuple[int, int, int]
    blink: int
    heading_deg: Optional[float]


@dataclass
class Contact:
    contact_id: int
    type: str                       # ground_model | aircraft
    icon: str                       # raw icon
    cls: str                        # tank_light | tank_medium | tank_heavy | td | spaa | wheeled | air_fighter | ...
    first_seen_ms: int
    last_seen_ms: int
    # current/last raw state
    color: str = ""
    rgb: Tuple[int, int, int] = (0, 0, 0)
    blink: int = 0
    x: float = 0.0                  # normalized, last seen
    y: float = 0.0
    wx: float = 0.0                 # world (map units ~ metres), last seen
    wy: float = 0.0
    heading_deg: Optional[float] = None   # from dx/dy if present
    vel_mps: Optional[Tuple[float, float]] = None   # world units/s (east, south)
    speed_mps: Optional[float] = None
    course_deg: Optional[float] = None    # movement direction (compass)
    visible: bool = True
    stale: bool = False
    age_ms: int = 0                 # now - last_seen
    # relative to player (last computed)
    bearing_deg: Optional[float] = None
    distance_m: Optional[float] = None
    # reveal inference
    reveal_kind: str = "unknown"
    reveal_rule: str = ""
    reveal_confidence: float = 0.0
    # why the marker went away (set when it becomes invisible / at finish):
    #   killed      : a hudmsg kill line arrived within -1.0/+1.5 s of last_seen (RB 2026-10-10: KV-1 marker
    #                 vanished 0.11 s before "... 击毁了 x2ded#2 (KV-1)")
    #   player_gone : own Player icon absent at that moment (death / respawn screen / vehicle switch)
    #   timeout     : reveal simply ended
    end_reason: str = ""
    end_hud_id: Optional[int] = None
    # distance (world m) to the nearest OWN respawn_base_* point (blue). RB 2026-10-10: every live
    # red marker appeared when the enemy crossed ~150-250 m of our spawn and vanished when it left.
    spawn_dist_m: Optional[float] = None
    spawn_dist_first_m: Optional[float] = None
    spawn_dist_max_m: Optional[float] = None
    # bookkeeping
    n_obs: int = 0
    n_pos_changes: int = 0
    max_static_ms: int = 0          # longest interval with an identical position while visible
    blink_seq: List[Tuple[int, int, str]] = field(default_factory=list)  # (t_ms, blink, color) transitions
    visible_spans: List[Tuple[int, int]] = field(default_factory=list)
    history: List[Observation] = field(default_factory=list)
    _static_since_ms: int = 0
    _span_start_ms: int = 0


@dataclass
class ThreatSummary:
    t_ms: int
    n_visible: int
    n_stale: int
    sectors: Tuple[int, ...]               # 8 bins N,NE,E,SE,S,SW,W,NW (visible + stale-not-expired)
    sectors_visible: Tuple[int, ...]
    nearest_id: Optional[int]
    nearest_distance_m: Optional[float]
    nearest_bearing_deg: Optional[float]
    nearest_sector: Optional[str]
    dominant_sector: Optional[str]
