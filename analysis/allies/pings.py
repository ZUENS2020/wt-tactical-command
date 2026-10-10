"""Fuse /gamechat radio messages with #043FFF signal blinks -> MapPing events (who / where / when / lifetime).

category:
  ping      注意战术地图 / 注意该坐标位置 / 注意当前标识区域: target = grid cell (sender is elsewhere; confirmed:
            0/10 ping grids equal the sender's own cell, 9/9 ground location calls do). Own pings carry no grid
            -> location unknown (precision "none"), only time + own position.
  location  请求掩护 / 需要支援 / 掩护我 / 请求指示空袭目标 ...: grid = sender's cell; exact sender_xy from the blink.
  radio     other quick chat (谢谢 / 装填中 / 空袭警报 / 进攻 X 点 ...): sender position only.
  signal    a #043FFF blink without any chat message in the window (owner known only as track/role).
"""
from dataclasses import dataclass, asdict
from typing import List, Optional, Tuple
from .grid import cell_bounds, cell_of, center

MATCH_BEFORE_MS = 1500   # blink onset may precede the chat poll (chat 2 Hz, map_obj 10 Hz)
MATCH_AFTER_MS = 1500
PING_TTL_MS = 30000      # display policy (game does not expose the marker lifetime); observed blink 4.9-5.9 s


@dataclass
class MapPing:
    id: str
    category: str                 # ping | location | radio | signal
    kind: str
    t_ms: int
    sender: Optional[str]
    own: bool
    sender_role: str              # self | squad | ally | unknown
    sender_track: Optional[int]
    sender_xy: Optional[Tuple[float, float]]
    sender_icon: Optional[str]
    grid: Optional[str]
    area: Optional[Tuple[float, float, float, float]]
    x: Optional[float]
    y: Optional[float]
    precision: str                # grid | sender_pos | none
    signal_ms: Optional[int]
    ttl_ms: int
    source: str                   # chat+map | chat | map
    text: Optional[str] = None
    alt_m: Optional[float] = None
    grid_color: Optional[str] = None
    sender_cell: Optional[str] = None

    _sig: object = None           # SignalEvent (blink may still be running when the ping is emitted)

    def resolve(self):
        """Refresh fields that depend on the (possibly still running) blink."""
        s = self._sig
        if s is not None:
            self.signal_ms = s.dur_ms
            self.sender_role = s.role
            if self.category in ("radio", "location", "signal"):
                self.ttl_ms = s.dur_ms or 0
        return self

    def to_dict(self):
        d = asdict(self.resolve())
        d.pop("_sig", None)
        return d


class PingFuser:
    def __init__(self, ttl_ms: int = PING_TTL_MS):
        self.pending_msgs = []        # RadioMsg
        self.pending_sigs = []        # SignalEvent
        self._out: List[MapPing] = []
        self.grid = None
        self.ttl_ms = ttl_ms
        self.squad_names = {}
        self.recent_sigs = []         # matched signals stay claimable by later messages of the same sender
        self.sig_sender = {}          # id(SignalEvent) -> sender         # sender -> count of matches with a squad-role blink

    @property
    def out(self) -> List[MapPing]:
        return [p.resolve() for p in self._out]

    def set_grid(self, gi):
        if gi is not None:
            self.grid = gi

    def add_msgs(self, msgs):
        for m in msgs:
            if m.backlog or m.enemy:
                continue
            self.pending_msgs.append((m, self.grid))

    def add_signals(self, sigs):
        self.pending_sigs.extend((s, self.grid) for s in sigs)
        self.recent_sigs.extend(sigs)
        if len(self.recent_sigs) > 64:
            self.recent_sigs = self.recent_sigs[-64:]

    def flush(self, now: int, final: bool = False):
        # 1) finalize messages whose match window is over
        keep = []
        for m, gi in self.pending_msgs:
            if not final and now < m.t_ms + MATCH_AFTER_MS:
                keep.append((m, gi)); continue
            sig = self._match(m, gi)
            self._out.append(self._from_msg(m, sig, gi))
        self.pending_msgs = keep
        # 2) signals that no message can claim any more
        keep = []
        for s, gi in self.pending_sigs:
            if s.matched is not None:
                continue
            if not final and now < s.t_start + MATCH_BEFORE_MS + MATCH_AFTER_MS:
                keep.append((s, gi)); continue
            self._out.append(self._from_sig(s, gi))
        self.pending_sigs = keep

    def _match(self, m, gi):
        cands = []
        for s, _ in self.pending_sigs:
            if s.matched is not None:
                continue
            dt = s.t_start - m.t_ms
            if dt < -MATCH_BEFORE_MS or dt > MATCH_AFTER_MS:
                continue
            if m.own != (s.role == "self"):
                continue
            score = abs(dt)
            if m.subject == "sender" and m.grid and gi is not None:
                score += 0 if cell_of(s.x, s.y, gi) == m.grid else 5000
            cands.append((score, s))
        # one blink can cover several messages of the same sender (KVASSEXUA1: 2 pings in one 8.2 s blink)
        for s in self.recent_sigs:
            if self.sig_sender.get(id(s)) == m.sender and s.t_start - MATCH_BEFORE_MS <= m.t_ms \
                    and (s.t_end is None or m.t_ms <= s.t_end + 700):
                cands.append((abs(s.t_start - m.t_ms) + 1000, s))
        if not cands:
            return None
        cands.sort(key=lambda c: c[0])
        s = cands[0][1]
        if s.matched is None:
            s.matched = m.id
            self.sig_sender[id(s)] = m.sender
        return s

    def _from_msg(self, m, s, gi) -> MapPing:
        area = cell_bounds(m.grid, gi) if (m.grid and gi is not None) else None
        sxy = (s.x, s.y) if s else None
        cat = "ping" if m.is_ping else ("location" if m.subject == "sender" else "radio")
        if cat == "ping":
            c = center(area)
            x, y = (c if c else (None, None))
            prec = "grid" if c else "none"
        else:
            if sxy:
                (x, y), prec = sxy, "sender_pos"
            elif area:
                (x, y), prec = center(area), "grid"
            else:
                x = y = None; prec = "none"
        role = s.role if s else ("self" if m.own else "unknown")
        if s and role == "squad":
            self.squad_names[m.sender] = self.squad_names.get(m.sender, 0) + 1
        return MapPing(f"chat-{m.id}", cat, m.kind, m.t_ms, m.sender, m.own, role,
                       s.track_id if s else None, sxy, s.icon if s else None, m.grid, area, x, y, prec,
                       s.dur_ms if s else None, self.ttl_ms if cat == "ping" else ((s.dur_ms or 0) if s else 0),
                       "chat+map" if s else "chat", m.text, m.alt_m, m.grid_color,
                       cell_of(s.x, s.y, gi) if (s and gi is not None) else None, s)

    def _from_sig(self, s, gi) -> MapPing:
        return MapPing(f"sig-{s.track_id}-{s.t_start}", "signal", "signal_only", s.t_start, None, s.role == "self",
                       s.role, s.track_id, (s.x, s.y), s.icon, None, None, s.x, s.y, "sender_pos", s.dur_ms,
                       s.dur_ms or 0, "map", None, None, None, cell_of(s.x, s.y, gi) if gi is not None else None, s)
