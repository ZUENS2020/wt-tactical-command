"""Gated nearest-neighbour tracker for enemy map markers.

The game gives no object IDs, so identity is inferred frame-to-frame:
  * candidates must share (type, icon) – colour/blink are NOT part of the key
    (the same target switches #fa0C00/0 <-> #f00C00/2 while staying in place);
  * gate (world units) = gate_base + vmax(type) * dt, dt since the track's position last changed;
  * assignment = global greedy on ascending distance (deterministic, O(n*m log), easy in Rust);
  * a track missing for > lost_after_ms becomes invisible (stale: last-known position kept);
  * an invisible track can be re-acquired within reacquire_ms using a widened gate;
  * tracks are dropped after expire_ms of invisibility.
"""
from __future__ import annotations
from dataclasses import dataclass, field
from typing import Dict, List, Optional, Tuple
import math

from .model import Contact, Frame, MapInfo, MapObj, Observation, HudEvent
from .classify import is_trackable_enemy, icon_class, find_player
from .geo import to_world, heading_from_dir, rel, compass
from . import reveal


@dataclass
class TrackerConfig:
    gate_base_m: float = 25.0
    vmax_ground_mps: float = 35.0       # ~126 km/h
    vmax_air_mps: float = 450.0
    lost_after_ms: int = 400            # ~3-4 missed polls at 10 Hz
    reacquire_ms: int = 5000
    reacquire_extra_m: float = 60.0
    stale_after_ms: int = 400           # age at which a contact is flagged stale
    expire_ms: int = 60000
    history_max: int = 4000
    vel_window_ms: int = 3000


class EnemyTracker:
    def __init__(self, cfg: Optional[TrackerConfig] = None, session_kind: str = "unknown"):
        self.cfg = cfg or TrackerConfig()
        self.session_kind = session_kind          # battle | test_drive | unknown
        self.map_info: Optional[MapInfo] = None
        self.contacts: Dict[int, Contact] = {}
        self.closed: List[Contact] = []           # expired contacts (kept for analysis)
        self.hud: List[HudEvent] = []
        self.player: Optional[MapObj] = None
        self.now_ms: int = 0
        self.player_seen_ms: Optional[int] = None   # last frame with our own (alive) Player icon
        self._next_id = 1

    # ---------------------------------------------------------------- inputs
    def set_map_info(self, mi: Optional[MapInfo]):
        if mi is None:
            return
        if self.map_info and mi.map_generation != self.map_info.map_generation:
            self.reset()
        self.map_info = mi

    def add_hud_event(self, ev: HudEvent):
        self.hud.append(ev)

    def reset(self):
        self.closed.extend(self.contacts.values())
        self.contacts.clear()
        self.hud.clear()

    def update(self, frame: Frame) -> List[Contact]:
        """Feed one /map_obj.json frame. Returns the live contact list (visible + stale)."""
        self.now_ms = frame.t_ms
        mi = self.map_info
        if mi is None:
            return []
        p = find_player(frame.objects)
        if p is not None:
            self.player, self.player_seen_ms = p, frame.t_ms
        dets = [o for o in frame.objects if is_trackable_enemy(o)]
        self._associate(dets, frame.t_ms)
        self._age(frame.t_ms)
        if self.player is not None:
            for c in self.contacts.values():
                c.distance_m, c.bearing_deg = rel(mi, self.player.x, self.player.y, c.x, c.y)
        for c in self.contacts.values():
            reveal.classify(c, self)
        return self.live()

    def live(self) -> List[Contact]:
        return sorted(self.contacts.values(), key=lambda c: c.contact_id)

    def all_contacts(self) -> List[Contact]:
        return sorted(list(self.closed) + list(self.contacts.values()), key=lambda c: c.contact_id)

    def finish(self):
        """End of stream: close spans and run final classification."""
        for c in self.contacts.values():
            if c.visible:
                c.visible_spans.append((c._span_start_ms, c.last_seen_ms))
                c.visible = False
                c.end_reason = "stream_end"
            reveal.end_reason(c, self)
            reveal.classify(c, self, final=True)
        self.closed.extend(self.contacts.values())
        self.contacts.clear()

    # ------------------------------------------------------------- internals
    def _gate(self, c: Contact, t_ms: int) -> float:
        # time since the reported position last changed: frozen/periodically-updated
        # markers (RB scouting "updates periodically") jump by speed * that interval
        dt = max(0, t_ms - min(c.last_seen_ms, c._static_since_ms)) / 1000.0
        vmax = self.cfg.vmax_air_mps if c.type == "aircraft" else self.cfg.vmax_ground_mps
        g = self.cfg.gate_base_m + vmax * dt
        if not c.visible:
            g += self.cfg.reacquire_extra_m
        return g

    def _associate(self, dets: List[MapObj], t_ms: int):
        mi = self.map_info
        cands: List[Tuple[float, int, int]] = []
        for di, o in enumerate(dets):
            wx, wy = to_world(mi, o.x, o.y)
            for cid, c in self.contacts.items():
                if c.type != o.type or c.icon != o.icon:
                    continue
                if not c.visible and t_ms - c.last_seen_ms > self.cfg.reacquire_ms:
                    continue
                d = math.hypot(wx - c.wx, wy - c.wy)
                if d <= self._gate(c, t_ms):
                    cands.append((d, cid, di))
        cands.sort()
        used_c, used_d = set(), set()
        for d, cid, di in cands:
            if cid in used_c or di in used_d:
                continue
            used_c.add(cid); used_d.add(di)
            self._observe(self.contacts[cid], dets[di], t_ms)
        for di, o in enumerate(dets):
            if di not in used_d:
                self._spawn(o, t_ms)

    def _spawn(self, o: MapObj, t_ms: int):
        cid = self._next_id; self._next_id += 1
        c = Contact(contact_id=cid, type=o.type, icon=o.icon, cls=icon_class(o.type, o.icon),
                    first_seen_ms=t_ms, last_seen_ms=t_ms)
        c._span_start_ms = t_ms
        c._static_since_ms = t_ms
        self.contacts[cid] = c
        self._observe(c, o, t_ms, new=True)

    def _observe(self, c: Contact, o: MapObj, t_ms: int, new: bool = False):
        mi = self.map_info
        if not c.visible:                      # re-acquired after a gap
            c.visible = True
            c._span_start_ms = t_ms
            c._static_since_ms = t_ms
        moved = new or (o.x != c.x or o.y != c.y)
        if not new and moved:
            c.n_pos_changes += 1
            c._static_since_ms = t_ms
        else:
            c.max_static_ms = max(c.max_static_ms, t_ms - c._static_since_ms)
        if new or o.blink != c.blink or o.color != c.color:
            c.blink_seq.append((t_ms, o.blink, o.color))
        c.color, c.rgb, c.blink = o.color, o.rgb, o.blink
        c.x, c.y = o.x, o.y
        c.wx, c.wy = to_world(mi, o.x, o.y)
        c.heading_deg = heading_from_dir(o.dx, o.dy)
        c.last_seen_ms = t_ms
        c.n_obs += 1
        c.stale = False
        c.age_ms = 0
        c.history.append(Observation(t_ms, o.x, o.y, o.color, o.rgb, o.blink, c.heading_deg))
        if len(c.history) > self.cfg.history_max:
            del c.history[0]
        self._velocity(c)

    def _velocity(self, c: Contact):
        """Least-effort estimate: displacement over the last vel_window_ms of *distinct* positions."""
        mi = self.map_info
        h = c.history
        t1 = h[-1].t_ms
        old = None
        for ob in reversed(h):
            if t1 - ob.t_ms > self.cfg.vel_window_ms:
                break
            old = ob
        if old is None or old.t_ms == t1:
            return
        w, hh = mi.size
        ex, sy = (h[-1].x - old.x) * w, (h[-1].y - old.y) * hh
        dt = (t1 - old.t_ms) / 1000.0
        if dt < 0.3:
            return
        vx, vy = ex / dt, sy / dt
        sp = math.hypot(vx, vy)
        c.vel_mps = (vx, vy)
        c.speed_mps = sp
        c.course_deg = compass(vx, vy) if sp > 0.5 else None

    def _age(self, t_ms: int):
        drop = []
        for cid, c in self.contacts.items():
            c.age_ms = t_ms - c.last_seen_ms
            c.stale = c.age_ms > self.cfg.stale_after_ms
            if c.visible and c.age_ms > self.cfg.lost_after_ms:
                c.visible = False
                c.visible_spans.append((c._span_start_ms, c.last_seen_ms))
                # own icon missing around the disappearance -> view change (death, respawn screen)
                gone = self.player_seen_ms is None or self.player_seen_ms < c.last_seen_ms + 150
                c.end_reason = "player_gone" if gone else "timeout"
                reveal.end_reason(c, self)
            if not c.visible and c.age_ms > self.cfg.expire_ms:
                drop.append(cid)
        for cid in drop:
            c = self.contacts.pop(cid)
            reveal.classify(c, self, final=True)
            self.closed.append(c)
