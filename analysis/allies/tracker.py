"""Friendly unit tracking: self / squad / ally roles that survive the #043FFF signal blink, signal events,
and exact map-marker objects (point_of_interest / unknown types). Times are explicit ms, deterministic."""
from dataclasses import dataclass, field
import math
from typing import Dict, List, Optional, Tuple
from .classify import classify

VMAX = {"ground_model": 35.0, "aircraft": 450.0}


@dataclass
class TrackerConfig:
    gate_base_m: float = 30.0
    lost_ms: int = 1500          # unseen longer -> track closed (no reacquire: friendly icons do not hide)
    norm_gate: float = 0.03      # used when no map_info


@dataclass
class SignalEvent:
    track_id: int
    role: str                    # self | squad | ally | unknown (resolved from the track history)
    icon: str
    type: str
    t_start: int
    x: float
    y: float
    t_end: Optional[int] = None
    lost: bool = False           # track disappeared while blinking
    matched: Optional[int] = None  # chat id

    @property
    def dur_ms(self) -> Optional[int]:
        return None if self.t_end is None else self.t_end - self.t_start


@dataclass
class FriendlyTrack:
    id: int
    type: str
    icon: str
    is_self: bool
    first_seen: int
    last_seen: int
    x: float
    y: float
    votes: Dict[str, int] = field(default_factory=dict)   # side counts on non-signal / non-fresh frames
    fresh_ms: int = 0
    spectated: bool = False
    in_signal: Optional[SignalEvent] = None
    signals: List[SignalEvent] = field(default_factory=list)
    _last_t: int = 0

    @property
    def role(self) -> str:
        if self.is_self:
            return "self"
        if self.votes.get("squad", 0) > 0:
            return "squad"
        if self.votes.get("ally", 0) > 0:
            return "ally"
        return "unknown"

    def to_dict(self):
        return {"id": self.id, "role": self.role, "type": self.type, "icon": self.icon,
                "first_seen": self.first_seen, "last_seen": self.last_seen, "x": self.x, "y": self.y,
                "signals": len(self.signals), "spectated": self.spectated, "fresh_ms": self.fresh_ms}


@dataclass
class MapMarker:
    key: Tuple
    type: str
    icon: str
    color: str
    x: float
    y: float
    first_seen: int
    last_seen: int
    closed: bool = False


class FriendlyTracker:
    def __init__(self, cfg: TrackerConfig = None):
        self.cfg = cfg or TrackerConfig()
        self.tracks: List[FriendlyTrack] = []
        self.active: List[FriendlyTrack] = []
        self.signals: List[SignalEvent] = []
        self.new_signals: List[SignalEvent] = []     # opened during the last update()
        self.markers: Dict[Tuple, MapMarker] = {}
        self.marker_log: List[MapMarker] = []
        self.span: Optional[Tuple[float, float]] = None

    def set_map_info(self, mi):
        if mi is not None:
            self.span = (mi.map_max[0] - mi.map_min[0], mi.map_max[1] - mi.map_min[1])

    def _dist_m(self, a, b) -> float:
        if self.span:
            return math.hypot((a[0] - b[0]) * self.span[0], (a[1] - b[1]) * self.span[1])
        return math.hypot(a[0] - b[0], a[1] - b[1]) / self.cfg.norm_gate * self.cfg.gate_base_m

    def update(self, t: int, objs) -> None:
        self.new_signals = []
        units, seen_markers = [], set()
        for o in objs:
            c = classify(o)
            if c.kind == "map_marker":
                if o.x is not None:
                    k = (o.type, o.icon, round(o.x, 4), round(o.y, 4))
                    seen_markers.add(k)
                    m = self.markers.get(k)
                    if m is None:
                        m = self.markers[k] = MapMarker(k, o.type, o.icon, o.color, o.x, o.y, t, t)
                        self.marker_log.append(m)
                    m.last_seen = t
                continue
            if c.is_unit and c.friendly and o.x is not None:
                units.append((o, c))
        for k in list(self.markers):
            if k not in seen_markers:
                self.markers[k].closed = True
                del self.markers[k]
        # greedy global assignment by distance within (type, icon, is_self)
        pairs = []
        for i, (o, c) in enumerate(units):
            is_self = c.side == "self"
            for j, tr in enumerate(self.active):
                if tr.type != o.type or tr.icon != o.icon or tr.is_self != is_self:
                    continue
                d = self._dist_m((o.x, o.y), (tr.x, tr.y))
                gate = self.cfg.gate_base_m + VMAX.get(o.type, 50.0) * max(0, t - tr.last_seen) / 1000.0
                if d <= gate:
                    pairs.append((d, i, j))
        pairs.sort()
        ui, tj, assign = set(), set(), {}
        for d, i, j in pairs:
            if i in ui or j in tj:
                continue
            ui.add(i); tj.add(j); assign[i] = self.active[j]
        nxt = []
        for i, (o, c) in enumerate(units):
            tr = assign.get(i)
            if tr is None:
                tr = FriendlyTrack(len(self.tracks), o.type, o.icon, c.side == "self", t, t, o.x, o.y)
                self.tracks.append(tr)
            self._apply(tr, o, c, t)
            nxt.append(tr)
        for j, tr in enumerate(self.active):
            if j in tj:
                continue
            if t - tr.last_seen <= self.cfg.lost_ms:
                nxt.append(tr)
            else:
                self._close_signal(tr, tr.last_seen, lost=True)
        self.active = nxt

    def _apply(self, tr: FriendlyTrack, o, c, t: int):
        dt = t - tr._last_t if tr._last_t else 0
        tr._last_t = t
        tr.last_seen, tr.x, tr.y = t, o.x, o.y
        if c.spectated:
            tr.spectated = True
        if c.fresh_spawn:
            tr.fresh_ms += dt
        elif not c.signal:
            tr.votes[c.side] = tr.votes.get(c.side, 0) + 1
            if tr.in_signal is not None and tr.in_signal.role == "unknown":
                tr.in_signal.role = tr.role
        if c.signal and tr.in_signal is None:
            ev = SignalEvent(tr.id, tr.role, o.icon, o.type, t, o.x, o.y)
            tr.in_signal = ev
            tr.signals.append(ev)
            self.signals.append(ev)
            self.new_signals.append(ev)
        elif c.signal:
            tr.in_signal.t_end = t
        elif tr.in_signal is not None:
            self._close_signal(tr, tr.in_signal.t_end or tr.in_signal.t_start)
        # late role resolution for closed signals of tracks first seen blinking
        for ev in tr.signals:
            if ev.role == "unknown" and tr.role != "unknown":
                ev.role = tr.role

    def _close_signal(self, tr, t_end, lost=False):
        ev = tr.in_signal
        if ev is None:
            return
        ev.t_end = t_end if ev.t_end is None else ev.t_end
        ev.lost = lost
        tr.in_signal = None

    def finish(self):
        for tr in self.active:
            self._close_signal(tr, tr.last_seen, lost=False)

    # convenience for the web map / receiver live view
    def live(self, t: int):
        return [tr.to_dict() for tr in self.active if t - tr.last_seen <= self.cfg.lost_ms]
