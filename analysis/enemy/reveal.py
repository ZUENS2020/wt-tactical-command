"""reveal_kind inference. EVERY rule here is a hypothesis until confirmed by the
RB capture protocol in ENEMY_MARKERS_20261010.md. Rules are ordered; first match wins.

Sources for the timing priors:
  * War Thunder Wiki "Scouting": RB scouted target shown on the minimap, position
    "updates periodically", for 30 s.
  * War Thunder Wiki "Realistic Battles": enemies not shown on the minimap; "only a red
    coloured icon is briefly displayed on the minimap when nearby allies scout or hit a vehicle".
Observed in our data (test drive only): ground targets #fa0C00/blink0 static; the same
targets toggle to #f00C00/blink2 and back; enemy aircraft always #f00C00/blink2.

reveal_kind values: scouted | hit_revealed | spawn_proximity | visible_static | unknown.

RB ground evidence (2026-10-10, sessions 1709 + df9b with GUI labels, ~6 battles):
  * only ONE enemy variant ever appears in a real battle: ground_model #fa0C00 blink 0, no dx/dy;
    #f00C00 / blink 2 never (0 obs) -> test-drive/AI-only.
  * markers are LIVE: position changes on ~95-100 % of 10 Hz polls (not a frozen last-known
    position, not "periodic" jumps); visible 2-41 s; end by kill (hud kill line <=0.5 s) or timeout.
  * none of the user's own hits/kills (14 ground kills, 2 i_hit labels) produced a marker; the
    labelled hit-only battle (gen 35) had 0 markers; the 2 team_scouted labels had 0 markers +-15 s.
  * every marker was 330-470 m from the player and 53-180 m from the nearest teammate.
So for live-tracked RB markers the scout-vs-hit cause is NOT separable from map_obj fields (R6).
Battle e5:g38 (user in a scout vehicle, own i_scouted + own i_hit with an on-screen hit marker):
  0 enemy units in 2,990 frames -> the game does NOT export own scouting or hit-reveal markers to
  /map_obj.json. On-screen hit reveal lasted ~6-16 s (i_hit -> note 'd' 20.9 s, 'd' pressed 5-15 s late).
  Practical consequence: hit_revealed/scouted can only come from hudmsg/labels, never from map_obj alone.
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


SPAWN_REVEAL_MAX_M = 300   # observed trigger radius 150-250 m (map dependent); first fix must be inside
SPAWN_EXIT_SLACK_M = 15    # "left the zone": last fix within this of the track's max spawn distance
KILL_PRE_MS = 1000      # kill line may be polled slightly before the marker disappears
KILL_POST_MS = 1500     # hudmsg 2 Hz + receive jitter
LIVE_RATIO = 0.6        # share of polls with a new position
LIVE_MAX_STATIC_MS = 1500


def is_live_tracked(c) -> bool:
    """RB 2026-10-10: reveal markers follow the target every poll (moves on ~all 10 Hz frames).
    The wiki's 'updates periodically' scout pattern would show long frozen stretches + jumps."""
    if c.n_obs < 5:
        return False
    return c.n_pos_changes / max(1, c.n_obs - 1) >= LIVE_RATIO and c.max_static_ms <= LIVE_MAX_STATIC_MS


def end_reason(c, tr):
    """Refine c.end_reason once the contact is invisible: a kill line near last_seen wins
    (hud arrives up to ~1.5 s later, so this is re-run on every update)."""
    if c.visible or c.end_reason == "killed":
        return
    for ev in tr.hud:
        if ev.kind == "kill" and c.last_seen_ms - KILL_PRE_MS <= ev.t_ms <= c.last_seen_ms + KILL_POST_MS:
            c.end_reason, c.end_hud_id = "killed", ev.id
            return
    if not c.end_reason:
        c.end_reason = "timeout"
    if (c.end_reason == "timeout" and c.spawn_dist_m is not None and c.spawn_dist_max_m is not None
            and c.spawn_dist_first_m is not None and c.spawn_dist_first_m <= SPAWN_REVEAL_MAX_M
            and c.spawn_dist_max_m - c.spawn_dist_m <= SPAWN_EXIT_SLACK_M):
        c.end_reason = "left_spawn_zone"


def _kill_at_end(c, tr):
    return next((ev for ev in tr.hud if c.end_hud_id is not None and ev.id == c.end_hud_id), None)


def classify(c, tr, final: bool = False):
    if not c.visible:
        end_reason(c, tr)
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
    live = is_live_tracked(c)
    if live and c.spawn_dist_first_m is not None and c.spawn_dist_first_m <= SPAWN_REVEAL_MAX_M:
        _set(c, "spawn_proximity", f"R7 live marker appeared {c.spawn_dist_first_m:.0f} m from our respawn base "
                                   f"(<= {SPAWN_REVEAL_MAX_M} m; spawn-camping reveal, 5/5 RB segments 2026-10-10)", 0.8)
        return
    kv = _kill_at_end(c, tr) if c.end_reason == "killed" else None
    if kv is not None and live and not kv.involves_player:
        _set(c, "hit_revealed", f"R2k live marker ended by a teammate kill (hud id={kv.id}) -> target was under fire; "
                                "scouting not excluded (no scout award)", 0.45)
        return
    ev = _hud_near(c, tr)
    if ev is not None and (not closed or span <= HIT_MAX_MS):
        _set(c, "hit_revealed", f"R2 hudmsg '{ev.kind}' id={ev.id} within -{HUD_PRE_MS}/+{HUD_POST_MS} ms of first_seen", 0.6 if ev.involves_player else 0.5)
        return
    if live and shade == "fa0C00" and blinks == {0}:
        _set(c, "unknown", f"R6 RB live marker #fa0C00/blink0 ({span/1000:.1f}s, moved on "
                           f"{c.n_pos_changes}/{max(1, c.n_obs - 1)} polls): scout vs teammate-hit not separable "
                           "from map_obj fields (2026-10-10 labelled battles)", 0.0)
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
