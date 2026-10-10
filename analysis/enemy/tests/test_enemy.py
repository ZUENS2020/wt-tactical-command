"""Unit tests for enemy parsing. Run:  .venv/bin/python -m unittest discover -s enemy/tests -t .
Fixtures are verbatim recorded collector records (session 20261010-182921-shengyuan-b6a7)."""
import gzip, json, math, os, unittest, datetime

from enemy import stream, reveal
from enemy.model import MapInfo, MapObj, Frame, HudEvent
from enemy.tracker import EnemyTracker, TrackerConfig
from enemy.geo import compass, rel, sector_index, heading_from_dir
from enemy.summary import threat_summary
from enemy.classify import is_trackable_enemy, is_enemy_rgb

FX = os.path.join(os.path.dirname(__file__), "fixtures")


def load_fx(name):
    with gzip.open(os.path.join(FX, name), "rt") as f:
        return [json.loads(l) for l in f if l.strip()]


def run(recs, kind=None):
    tr = EnemyTracker()
    frames, huds, snaps = [], [], {}
    for k, t, v in stream.events(recs):
        if k == "map_info":
            tr.set_map_info(v)
        elif k == "hud":
            tr.add_hud_event(v); huds.append(v)
        elif k == "frame":
            frames.append(v)
            snaps[t] = [(c.contact_id, c.icon, c.blink, c.visible) for c in tr.update(v)]
    tr.session_kind = kind or stream.detect_session_kind(frames, huds)[0]
    tr.finish()
    return tr, frames, huds, snaps


def ms(hms):  # '18:31:55.579' on 2026-10-10 UTC+8 -> epoch ms
    d = datetime.datetime.strptime("2026-10-10 " + hms, "%Y-%m-%d %H:%M:%S.%f")
    return int((d - datetime.datetime(1970, 1, 1)).total_seconds() * 1000) - 8 * 3600 * 1000


MI = MapInfo((0.0, 0.0), (4096.0, 4096.0), 1, 1)


def obj(icon="MediumTank", x=0.5, y=0.5, color="#fa0C00", rgb=(250, 12, 0), blink=0, type_="ground_model", dx=None, dy=None):
    return MapObj(type_, icon, color, rgb, blink, x, y, dx, dy)


PLAYER = obj("Player", 0.5, 0.5, "#faC81E", (250, 200, 30), 0, dx=0.0, dy=-1.0)


class TestGeo(unittest.TestCase):
    def test_compass_convention(self):
        self.assertAlmostEqual(compass(0, -1), 0.0)      # up on the map = north
        self.assertAlmostEqual(compass(1, 0), 90.0)
        self.assertAlmostEqual(compass(0, 1), 180.0)
        self.assertAlmostEqual(compass(-1, 0), 270.0)

    def test_rel_uses_world_scale(self):
        mi = MapInfo((-65536, -65536), (65536, 65536))
        d, b = rel(mi, 0.5, 0.5, 0.5, 0.5 - 1000 / 131072)
        self.assertAlmostEqual(d, 1000.0, places=6)
        self.assertAlmostEqual(b, 0.0, places=6)
        mi2 = MapInfo((0, 0), (2000, 1000))           # non-square: must not use normalized deltas
        d, b = rel(mi2, 0.5, 0.5, 0.75, 0.0)          # 500 m east, 500 m north
        self.assertAlmostEqual(b, 45.0, places=6)
        self.assertAlmostEqual(d, math.hypot(500, 500), places=6)

    def test_sectors(self):
        self.assertEqual([sector_index(b) for b in (0, 22.4, 22.6, 90, 180, 270, 337.6, 359.9)],
                         [0, 0, 1, 2, 4, 6, 0, 0])


class TestClassify(unittest.TestCase):
    def test_both_red_shades_are_enemy(self):
        self.assertTrue(is_enemy_rgb((250, 12, 0)))
        self.assertTrue(is_enemy_rgb((240, 12, 0)))
        self.assertFalse(is_enemy_rgb((250, 200, 30)))   # player yellow
        self.assertFalse(is_enemy_rgb((23, 77, 255)))    # friendly blue

    def test_exclusions(self):
        self.assertFalse(is_trackable_enemy(obj("BattleShip")))                   # naval (type is ground_model!)
        self.assertFalse(is_trackable_enemy(obj("bombing_point", type_="bombing_point")))
        self.assertFalse(is_trackable_enemy(PLAYER))
        self.assertTrue(is_trackable_enemy(obj("TankDestroyer", color="#f00C00", rgb=(240, 12, 0), blink=2)))


class TestGroundTestDrive(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tr, cls.frames, cls.huds, cls.snaps = run(load_fx("ground_testdrive_b6a7.jsonl.gz"))
        cls.cs = cls.tr.all_contacts()

    def test_session_kind(self):
        self.assertEqual(self.tr.session_kind, "test_drive")

    def test_eight_targets_no_duplicates(self):
        self.assertEqual(len(self.cs), 8)
        self.assertEqual(sorted(c.icon for c in self.cs).count("HeavyTank"), 3)
        self.assertTrue(all(len(c.visible_spans) == 1 for c in self.cs))

    def test_shade_switch_keeps_identity(self):
        td = [c for c in self.cs if c.icon == "TankDestroyer"][0]
        seq = [(b, col) for _, b, col in td.blink_seq]
        self.assertEqual(seq, [(2, "#f00C00"), (0, "#fa0C00")])
        dur = (td.blink_seq[1][0] - td.blink_seq[0][0]) / 1000
        self.assertAlmostEqual(dur, 11.2, delta=0.3)

    def test_shade_and_blink_are_coupled(self):
        for f in self.frames:
            for o in f.objects:
                if o.rgb == (250, 12, 0): self.assertEqual(o.blink, 0)
                if o.rgb == (240, 12, 0): self.assertEqual(o.blink, 2)

    def test_killed_target_removed_immediately(self):
        lt = [c for c in self.cs if c.icon == "LightTank"][0]
        self.assertEqual(lt.last_seen_ms, ms("18:31:55.579"))
        kill = [e for e in self.huds if e.kind == "kill"]
        self.assertEqual(len(kill), 1)
        self.assertIn("T-26", kill[0].msg)
        self.assertTrue(kill[0].involves_player)
        self.assertLess(abs(kill[0].t_ms - lt.last_seen_ms), 600)
        self.assertEqual(lt.blink, 0)          # no blink change on the killing hit

    def test_moving_target_velocity(self):
        mt = max(self.cs, key=lambda c: c.n_pos_changes)
        self.assertEqual(mt.icon, "MediumTank")
        self.assertGreater(mt.n_pos_changes, 50)
        self.assertIsNotNone(mt.speed_mps)

    def test_bearing_distance_and_summary(self):
        td = [c for c in self.cs if c.icon == "TankDestroyer"][0]
        self.assertAlmostEqual(td.distance_m, 94, delta=3)
        self.assertAlmostEqual(td.bearing_deg, 79.6, delta=1.5)
        s = threat_summary(self.cs, self.tr.now_ms, stale_horizon_ms=10 ** 9)
        self.assertEqual(s.dominant_sector, "E")

    def test_player_heading_convention(self):
        p = [o for o in self.frames[-5].objects if o.icon == "Player"][0]
        self.assertAlmostEqual(heading_from_dir(p.dx, p.dy), 70.0, delta=2.0)

    def test_reveal_kind_test_drive(self):
        self.assertTrue(all(c.reveal_kind == "visible_static" for c in self.cs))


class TestAirTestFlightGroundTargets(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tr, cls.frames, _, _ = run(load_fx("air_testflight_b6a7_ground_targets.jsonl.gz"), "test_drive")
        cls.cs = cls.tr.all_contacts()

    def test_aircraft_always_blink2(self):
        air = [c for c in self.cs if c.type == "aircraft"]
        self.assertEqual(len(air), 10)
        self.assertTrue(all({b for _, b, _ in c.blink_seq} == {2} for c in air))
        self.assertTrue(all(c.heading_deg is not None for c in air))

    def test_new_light_tanks_5s_highlight(self):
        hl = [c for c in self.cs if c.type == "ground_model" and any(b == 2 for _, b, _ in c.blink_seq)]
        self.assertEqual(len(hl), 2)
        for c in hl:
            t2 = [t for t, b, _ in c.blink_seq if b == 2][0]
            t0 = [t for t, b, _ in c.blink_seq if b == 0 and t > t2][0]
            self.assertAlmostEqual((t0 - t2) / 1000, 5.0, delta=0.15)


class TestTrackerSynthetic(unittest.TestCase):
    def feed(self, tr, seq):
        for t, objs in seq:
            tr.update(Frame(t, tuple([PLAYER] + objs)))

    def test_reacquire_after_gap(self):
        tr = EnemyTracker(); tr.set_map_info(MI)
        seq = [(t, [obj(x=0.6)]) for t in range(0, 2000, 100)]
        seq += [(t, []) for t in range(2000, 3000, 100)]
        seq += [(t, [obj(x=0.6 + 10 / 4096)]) for t in range(3000, 4000, 100)]
        self.feed(tr, seq); tr.finish()
        cs = tr.all_contacts()
        self.assertEqual(len(cs), 1)
        self.assertEqual(len(cs[0].visible_spans), 2)

    def test_stale_flag_and_last_known(self):
        tr = EnemyTracker(); tr.set_map_info(MI)
        self.feed(tr, [(t, [obj(x=0.6)]) for t in range(0, 1000, 100)] + [(t, []) for t in range(1000, 2000, 100)])
        c = tr.live()[0]
        self.assertFalse(c.visible); self.assertTrue(c.stale)
        self.assertEqual(c.x, 0.6); self.assertGreater(c.age_ms, 900)

    def test_periodic_jump_same_track(self):
        # RB scouting: marker "updates periodically" -> frozen 3 s then jumps 40 m
        tr = EnemyTracker(); tr.set_map_info(MI)
        seq = [(t, [obj(x=0.6)]) for t in range(0, 3000, 100)]
        seq += [(t, [obj(x=0.6 + 40 / 4096)]) for t in range(3000, 4000, 100)]
        self.feed(tr, seq); tr.finish()
        self.assertEqual(len(tr.all_contacts()), 1)

    def test_two_same_icon_targets_do_not_swap(self):
        tr = EnemyTracker(); tr.set_map_info(MI)
        seq = [(t, [obj(x=0.60 + t / 1e6), obj(x=0.62 - t / 1e6)]) for t in range(0, 3000, 100)]
        self.feed(tr, seq); tr.finish()
        cs = tr.all_contacts()
        self.assertEqual(len(cs), 2)
        self.assertLess(cs[0].x, cs[1].x)

    def test_reveal_rules_battle(self):
        tr = EnemyTracker(session_kind="battle"); tr.set_map_info(MI)
        tr.add_hud_event(HudEvent(100_000, 1, "A (T-34) 击毁了 B (Tiger)", True, "kill"))
        seq = [(t, [obj(x=0.6, color="#f00C00", rgb=(240, 12, 0), blink=2)]) for t in range(100_000, 105_000, 100)]
        seq += [(t, []) for t in range(105_000, 106_000, 100)]
        # second target: 30 s, periodic updates every 2 s
        seq += [(t, [obj("LightTank", x=0.3 + (t - 110_000) // 2000 * 0.002, y=0.3)]) for t in range(110_000, 140_000, 100)]
        seq += [(t, []) for t in range(140_000, 141_000, 100)]
        self.feed(tr, seq); tr.finish()
        a, b = tr.all_contacts()
        self.assertEqual(a.reveal_kind, "hit_revealed"); self.assertTrue(a.reveal_rule.startswith("R2"))
        self.assertEqual(b.reveal_kind, "scouted"); self.assertGreaterEqual(b.reveal_confidence, 0.7)



class TestHudKinds(unittest.TestCase):
    def test_award_kinds(self):
        self.assertEqual(stream.hud_kind("собачий_пердёж (M41A1) 获得嘉奖“战场引导×1”"), "scout_award")
        self.assertEqual(stream.hud_kind("=ITFWT= nickpro77 (T-34-85 (STP)) 获得嘉奖“密接支援×1”"), "scout_kill")
        self.assertEqual(stream.hud_kind("A (T-34) 的攻击引燃了 B (M16)"), "fire")

    def test_scout_award_marks_scouted(self):
        tr = EnemyTracker(session_kind="battle"); tr.set_map_info(MI)
        for t in range(0, 8000, 100):
            tr.update(Frame(t, (PLAYER, obj(x=0.7))))
        tr.add_hud_event(HudEvent(8300, 9, "X (M41A1) 获得嘉奖“战场引导×1”", False, "scout_award"))
        for t in range(8000, 9000, 100):
            tr.update(Frame(t, (PLAYER,)))
        tr.finish()
        self.assertEqual(tr.all_contacts()[0].reveal_kind, "scouted")


class TestRBLabelledBattle(unittest.TestCase):
    """Real RB ground battle, session 20261010-203023-shengyuan-df9b (collector 0.2.0, GUI labels,
    times = receiver-synced ts_server_ms). Map objects trimmed to ground_model/aircraft."""

    @classmethod
    def setUpClass(cls):
        cls.R = {n: load_fx(f"rb_df9b_{n}.jsonl.gz") for n in ("red_spaa_kv1_death", "scout_labels_death",
                                                                 "kill_labels", "hitonly_gen35")}
        cls.T = {n: run(r) for n, r in cls.R.items()}

    def labels(self, n):
        return [(t, v["label_id"]) for k, t, v in stream.events(self.R[n]) if k == "label"]

    def test_session_kind_battle(self):
        for n, (tr, *_ ) in self.T.items():
            self.assertEqual(tr.session_kind, "battle", n)

    def test_only_variant_is_fa0C00_blink0(self):
        for n, (_, frames, _, _) in self.T.items():
            for f in frames:
                for o in f.objects:
                    if is_trackable_enemy(o):
                        self.assertEqual((o.color, o.blink, o.has_dir), ("#fa0C00", 0, False), n)

    def test_red_markers_live_and_end(self):
        tr, _, huds, _ = self.T["red_spaa_kv1_death"]
        cs = tr.all_contacts()
        self.assertEqual([c.icon for c in cs], ["SPAA", "HeavyTank"])
        spaa, kv1 = cs
        for c, dur in ((spaa, 14.8), (kv1, 20.8)):
            self.assertEqual(len(c.visible_spans), 1)
            a, b = c.visible_spans[0]
            self.assertAlmostEqual((b - a) / 1000, dur, delta=0.3)
            self.assertTrue(reveal.is_live_tracked(c))          # not frozen, not periodic
            self.assertGreater(c.n_pos_changes / c.n_obs, 0.9)
            self.assertEqual({bl for _, bl, _ in c.blink_seq}, {0})
        self.assertEqual(spaa.end_reason, "timeout")
        self.assertEqual(spaa.reveal_kind, "unknown"); self.assertTrue(spaa.reveal_rule.startswith("R6"))
        self.assertEqual(kv1.end_reason, "killed")
        kill = [e for e in huds if e.id == kv1.end_hud_id][0]
        self.assertIn("(KV-1)", kill.msg); self.assertFalse(kill.involves_player)
        self.assertLess(abs(kill.t_ms - kv1.last_seen_ms), 500)
        self.assertEqual(kv1.reveal_kind, "hit_revealed"); self.assertTrue(kv1.reveal_rule.startswith("R2k"))

    def test_no_markers_at_own_hits_kills_or_scout_labels(self):
        for n in ("scout_labels_death", "kill_labels", "hitonly_gen35"):
            tr = self.T[n][0]
            self.assertEqual(tr.all_contacts(), [], n)
        self.assertEqual([l for _, l in self.labels("scout_labels_death")], ["team_scouted", "team_scouted"])
        self.assertEqual(len(self.labels("hitonly_gen35")), 5)

    def test_kill_labels_lag_hudmsg(self):
        huds = self.T["kill_labels"][2]
        own = [e for e in huds if e.kind == "kill" and e.involves_player and
               e.msg.index("击毁了") > e.msg.index("传奇耐摔王无声铃鹿")]
        for t, lab in self.labels("kill_labels"):
            prev = max(e.t_ms for e in own if e.t_ms <= t)
            self.assertTrue(1500 <= t - prev <= 4000, (lab, t - prev))

    def test_own_icon_turns_blue_after_death(self):
        tr, frames, huds, _ = self.T["scout_labels_death"]
        death = [e for e in huds if e.kind == "kill" and e.msg.endswith("传奇耐摔王无声铃鹿 (东线虎)")][0]
        blue = [f.t_ms for f in frames for o in f.objects if o.icon == "Player" and o.rgb == (23, 77, 255)]
        self.assertTrue(blue)
        self.assertAlmostEqual((blue[0] - death.t_ms) / 1000, 7.3, delta=0.6)
        from enemy.classify import find_player
        for f in frames:                                         # blue Player = spectating, not "self"
            if f.t_ms in blue:
                self.assertIsNone(find_player(f.objects))


class TestRBOwnScoutAndHit(unittest.TestCase):
    """Battle e5:g38 (2026-10-10 21:07-21:12), user in Sd.Kfz. 234/2 (scout vehicle).
    Ground truth: i_scouted 21:09:47.0 (target later killed by a teammate), i_hit 21:11:41.1 with a
    hit marker seen on the minimap, note 'd' pressed ~5-10 s after that marker vanished."""

    @classmethod
    def setUpClass(cls):
        cls.recs = load_fx("rb_df9b_g38_iscout_ihit_note.jsonl.gz")
        cls.tr, cls.frames, cls.huds, _ = run(cls.recs)
        cls.labels = {v["label_id"]: t for k, t, v in stream.events(cls.recs) if k == "label"}

    def test_labels(self):
        self.assertEqual(sorted(self.labels), ["died", "i_hit", "i_scouted", "kill", "note"])
        self.assertAlmostEqual((self.labels["note"] - self.labels["i_hit"]) / 1000, 20.9, delta=0.1)

    def test_no_enemy_unit_ever_exported(self):
        # neither the own-scouted target nor the own hit-revealed target appears in /map_obj.json
        self.assertGreater(len(self.frames), 1500)
        self.assertEqual(self.tr.all_contacts(), [])
        self.assertFalse(any(is_trackable_enemy(o) for f in self.frames for o in f.objects))

    def test_scouted_target_killed_by_teammate(self):
        k = [e for e in self.huds if e.kind == "kill" and e.msg.endswith("dgan9078 (M5A1)")][0]
        self.assertIn("Terrastorm", k.msg); self.assertFalse(k.involves_player)
        self.assertAlmostEqual((k.t_ms - self.labels["i_scouted"]) / 1000, 48.3, delta=0.6)
        self.assertTrue(1000 <= self.labels["kill"] - k.t_ms <= 2500)
        self.assertFalse([e for e in self.huds if e.kind in ("scout_award", "scout_kill")])


if __name__ == "__main__":
    unittest.main()
