"""Analysis driver: collector records (dicts) -> FriendlyTracker + PingFuser results for one session."""
import json
from enemy.model import MapObj, MapInfo
from .grid import GridInfo
from .radio import ChatCursor, OWN_NAMES
from .tracker import FriendlyTracker
from .pings import PingFuser


def _body(r):
    b = r.get("body")
    if not isinstance(b, str):
        return None
    try:
        return json.loads(b)
    except ValueError:
        return None


def run(records, own_names=OWN_NAMES):
    tr, fu, cur = FriendlyTracker(), PingFuser(), ChatCursor(own_names)
    msgs = []
    battles = []        # (t_first_valid, map_generation)
    last_gen = None
    for r in sorted(records, key=lambda r: (r["wall_ms"], r.get("seq", 0))):
        if r.get("status") not in (200, None):
            continue
        ep, t = r.get("ep", ""), r["wall_ms"]
        j = _body(r)
        if j is None:
            continue
        if ep.startswith("/map_info"):
            gi = GridInfo.from_json(j)
            if gi is not None:
                tr.set_map_info(MapInfo.from_json(j))
                fu.set_grid(gi)
                g = j.get("map_generation")
                if g != last_gen:
                    battles.append((t, g)); last_gen = g
        elif ep == "/map_obj.json" and isinstance(j, list):
            tr.update(t, [MapObj.from_json(o) for o in j if isinstance(o, dict)])
            fu.add_signals(tr.new_signals)
            fu.flush(t)
        elif ep == "/gamechat":
            ms = cur.feed(r.get("url", ""), r.get("epoch", 0), t, j)
            msgs.extend(ms)
            fu.add_msgs(ms)
    tr.finish()
    fu.flush(10**15, final=True)
    return {"tracker": tr, "pings": fu.out, "msgs": msgs, "battles": battles, "squad_names": fu.squad_names}
