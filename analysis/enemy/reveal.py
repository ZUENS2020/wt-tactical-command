"""reveal_kind inference. EVERY rule here is a hypothesis until confirmed by the
RB capture protocol in ENEMY_MARKERS_20261010.md. Rules are ordered; first match wins.

Sources for the timing priors:
  * War Thunder Wiki "Scouting": RB scouted target shown on the minimap, position
    "updates periodically", for 30 s.
  * War Thunder Wiki "Realistic Battles": enemies not shown on the minimap; "only a red
    coloured icon is briefly displayed on the minimap when nearby allies scout or hit a vehicle".
Observed in our data (test drive only): ground targets #fa0C00/blink0 static; the same
targets toggle to #f00C00/blink2 and back; enemy aircraft always #f00C00/blink2.
"""
from .classify import red_shade

SCOUT_MIN_MS = 20000
SCOUT_MAX_MS = 40000
HIT_MAX_MS = 15000
ALWAYS_MS = 45000
HUD_PRE_MS = 1500        # hudmsg polled at 2 Hz and only reports kills/fires/crits
HUD_POST_MS = 2500
HUD_KINDS = ("kill", "fire", "crit", "severe")


def _set(c, kind, rule, conf):
    c.reveal_kind, c.reveal_rule, c.reveal_confidence = kind, rule, round(conf, 2)


def _first_span_ms(c):
    if c.visible_spans:
        s, e = c.visible_spans[0]
        return e - s, True
    return c.last_seen_ms - c.first_seen_ms, False


def _hud_near(c, tr):
    t0 = c.first_seen_ms
    for ev in tr.hud:
        if ev.kind in HUD_KINDS and t0 - HUD_PRE_MS <= ev.t_ms <= t0 + HUD_POST_MS:
            return ev
    return None


def _scout_kill_near_end(c, tr):
    """Contact vanished within +-1.5 s of a scout award pair (target destroyed while scouted)."""
    if c.visible:
        return None
    for ev in tr.hud:
        if ev.kind in ("scout_award", "scout_kill") and abs(ev.t_ms - c.last_seen_ms) <= 1500:
            return ev
    return None


def classify(c, tr, final: bool = False):
    span, closed = _first_span_ms(c)
    shade = red_shade(c.color, c.rgb)
    blinks = {b for _, b, _ in c.blink_seq}
    if c.type == "aircraft":
        _set(c, "unknown", "R0 aircraft: air contacts are shown by separate rules (always #f00C00/blink2 in test flight)", 0.3)
        return
    if tr.session_kind == "test_drive":
        if blinks == {0}:
            _set(c, "visible_static", "R1 test drive: AI targets always shown, #fa0C00 blink0", 0.9)
        else:
            _set(c, "visible_static", "R1b test drive: always shown; blink2/#f00C00 phases = highlight (cause unproven)", 0.7)
        return
    sk = _scout_kill_near_end(c, tr)
    if sk is not None:
        _set(c, "scouted", f"R1s vanished within 1.5 s of hudmsg scout award '{sk.kind}' id={sk.id} (scouted target destroyed)", 0.75)
        return
    ev = _hud_near(c, tr)
    if ev is not None and (not closed or span <= HIT_MAX_MS):
        _set(c, "hit_revealed", f"R2 hudmsg '{ev.kind}' id={ev.id} within -{HUD_PRE_MS}/+{HUD_POST_MS} ms of first_seen", 0.6 if ev.involves_player else 0.5)
        return
    if closed and SCOUT_MIN_MS <= span <= SCOUT_MAX_MS:
        conf = 0.55 + (0.15 if c.n_pos_changes >= 2 and c.max_static_ms >= 800 else 0.0)
        _set(c, "scouted", f"R3 first visible span {span/1000:.1f}s in [20,40]s (wiki: 30s scout window)", conf)
        return
    if closed and span <= HIT_MAX_MS:
        conf = 0.5 if c.n_pos_changes == 0 else 0.35
        _set(c, "hit_revealed", f"R4 brief span {span/1000:.1f}s <= 15s, pos changes={c.n_pos_changes}", conf)
        return
    if span > ALWAYS_MS:
        _set(c, "visible_static", f"R5 visible {span/1000:.0f}s > 45s (longer than any reveal window)", 0.5)
        return
    if not closed and span > SCOUT_MAX_MS:
        _set(c, "visible_static", f"R5b open span {span/1000:.0f}s > 40s", 0.4)
        return
    _set(c, "unknown", f"R9 pending/undecided: span {span/1000:.1f}s, closed={closed}, shade={shade}, blinks={sorted(blinks)}", 0.0)
