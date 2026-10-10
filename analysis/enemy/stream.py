"""Analysis-only: turn collector records (ep, wall_ms, seq, status, body) into tracker inputs."""
import json, re
from .model import Frame, MapInfo, MapObj, HudEvent

ZW = "\u200b"
PLAYER_NAMES = ("传奇耐摔王无声铃鹿",)   # user's in-game nick (from hudmsg history)


# Award names in the zh-CN client. Evidence (hudmsg backlog ids 1275-1278, 1317-1323, 1349-1353):
# "战场引导×N" is always given to a scout-capable vehicle (M41A1, M18, Wiesel 1A4) within 1 s of a
# teammate's kill, paired with "密接支援×N" for the killer -> wiki "Intelligence" / "According to
# Intelligence" awards (scouted target destroyed). Hypothesis, 3/3 pairs.
SCOUT_AWARD = "战场引导"
SCOUT_KILL_AWARD = "密接支援"


def hud_kind(msg: str) -> str:
    if SCOUT_AWARD in msg: return "scout_award"
    if SCOUT_KILL_AWARD in msg: return "scout_kill"
    if "击毁了" in msg or "击落了" in msg: return "kill"
    if "引燃了" in msg: return "fire"
    if "致命攻击" in msg: return "crit"
    if "重创" in msg: return "severe"
    if "获得嘉奖" in msg or "先拔头筹" in msg: return "award"
    if "掉线" in msg or "DISCONNECT" in msg: return "disconnect"
    return "other"


def parse_body(r):
    b = r.get("body")
    if not isinstance(b, str):
        return None
    try:
        return json.loads(b)
    except ValueError:
        return None


def rec_time(r) -> int:
    """Server-synced time when the receiver stamped it (collector >= 0.2.0; same clock as '#label'
    t_server), else the collector wall clock."""
    t = r.get("ts_server_ms")
    return int(t) if isinstance(t, (int, float)) else int(r["wall_ms"])


def events(records, player_names=PLAYER_NAMES):
    """Yield ('map_info', t, MapInfo|None) / ('frame', t, Frame) / ('hud', t, HudEvent) /
    ('label', t, dict) in time order. Labels are ground truth for analysis only, never tracker input."""
    seen_ids = set()
    for r in sorted(records, key=lambda r: (rec_time(r), r.get("seq", 0))):
        ep, t = r.get("ep", ""), rec_time(r)
        if ep == "#label":
            j = parse_body(r)
            if isinstance(j, dict):
                yield ("label", int(j.get("t_server") or t), j)
            continue
        if r.get("status") not in (200, None):
            continue
        j = parse_body(r)
        if j is None:
            continue
        if ep.startswith("/map_info"):
            yield ("map_info", t, MapInfo.from_json(j))
        elif ep == "/map_obj.json" and isinstance(j, list):
            yield ("frame", t, Frame(t, tuple(MapObj.from_json(o) for o in j if isinstance(o, dict))))
        elif ep == "/hudmsg" and isinstance(j, dict):
            for d in j.get("damage", []) or []:
                i = d.get("id")
                if i in seen_ids:
                    continue
                seen_ids.add(i)
                msg = str(d.get("msg", "")).replace(ZW, "")
                if "lastDmg=0" in str(r.get("url", "")):
                    continue    # backlog for cursor 0 = history from earlier battles, not live events
                yield ("hud", t, HudEvent(t, int(i), msg, any(n in msg for n in player_names), hud_kind(msg)))


FRIENDLY_TYPES = ("ground_model", "aircraft")


def is_friendly_unit(o) -> bool:
    """Teammate / squadmate unit incl. fresh-spawn #1839A7 and signal #043FFF (see allies/classify.py)."""
    if o.icon == "Player" or o.type not in FRIENDLY_TYPES or o.x is None:
        return False
    from allies.classify import classify
    return classify(o).side in ("ally", "squad")


def detect_session_kind(frames, hud_events):
    """Return (kind, reasons). kind in battle | test_drive | unknown.
    battle    : friendly units, capture zones or respawn bases present, or hudmsg names other players.
    test_drive: none of the above, and kills/targets have bare vehicle names (AI targets)."""
    max_friendly = 0
    obj_types = set()
    for f in frames:
        max_friendly = max(max_friendly, sum(1 for o in f.objects if is_friendly_unit(o)))
        obj_types.update(o.type for o in f.objects)
    zones = sorted(t for t in obj_types if t == "capture_zone" or t.startswith("respawn_base"))
    pvp = [e for e in hud_events if e.kind in ("kill", "fire", "crit", "severe") and
           re.search(r"击[毁落]了 .+\(.+\)\s*$", e.msg)]
    ai = [e for e in hud_events if e.kind == "kill" and not re.search(r"\)\s*$", e.msg)]
    reasons = [f"max friendly units/frame={max_friendly}", f"zones/respawn objs={zones or 'none'}",
               f"live hud PvP lines={len(pvp)}", f"live hud kills of bare-named AI targets={len(ai)}"]
    if max_friendly > 0 or zones or pvp:
        return "battle", reasons
    if frames:
        return "test_drive", reasons
    return "unknown", reasons
