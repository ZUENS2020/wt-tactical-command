"""Run: cd /workspace/wt-analysis && python3 -m unittest discover -s allies/tests -t ."""
import gzip, json, os, unittest
from allies import classify, parse_chat, cell_bounds, cell_of, GridInfo, FriendlyTracker, PingFuser, find_self
from allies import session
from allies.radio import ChatCursor
from enemy.model import MapObj

FX = os.path.join(os.path.dirname(__file__), "fixtures")


def load(name):
    with gzip.open(os.path.join(FX, name), "rt") as fh:
        return [json.loads(l) for l in fh]


def obj(**k):
    d = {"type": "ground_model", "icon": "MediumTank", "color": "#174DFF", "blink": 0, "x": 0.5, "y": 0.5, "icon_bg": "none"}
    d.update(k)
    return MapObj.from_json(d)


GI_1709_B1 = GridInfo.from_json({"valid": True, "grid_size": [1600.0, 1600.0], "grid_steps": [225.0, 225.0],
                                 "grid_zero": [-884.35, -1107.0], "map_max": [2048.0, 0.0], "map_min": [-2048.0, -4096.0]})


class Classify(unittest.TestCase):
    def test_colors(self):
        c = lambda **k: classify(obj(**k))
        self.assertEqual(c(icon="Player", color="#faC81E").side, "self")
        self.assertEqual(c(color="#67D756").side, "squad")
        self.assertEqual(c(type="aircraft", icon="Fighter", color="#39D921").side, "squad")
        self.assertEqual(c(color="#174DFF").side, "ally")
        s = c(color="#043FFF", blink=2)
        self.assertTrue(s.signal and s.side == "ally" and s.is_unit)
        p = c(icon="Player", color="#043FFF", blink=2)
        self.assertTrue(p.signal and p.side == "self")
        self.assertTrue(c(color="#1839A7").fresh_spawn and c(color="#1839A7").side == "ally")
        self.assertTrue(c(type="aircraft", icon="Fighter", color="#134AFF").fresh_spawn)
        sp = c(icon="Player", color="#174DFF")
        self.assertTrue(sp.spectated and sp.side == "ally")
        self.assertEqual(c(color="#fa0C00").side, "enemy")
        self.assertEqual(c(color="#f00C00", blink=2).side, "enemy")
        self.assertFalse(c(color="#f00C00", blink=2).signal)

    def test_kinds_never_units(self):
        for ty in ("point_of_interest", "some_new_ping_type", "capture_zone", "respawn_base_tank", "bombing_point", "airfield"):
            k = classify(obj(type=ty, icon=ty, color="#174DFF", blink=2))
            self.assertFalse(k.is_unit, ty)
            self.assertFalse(k.signal, ty)
        self.assertEqual(classify(obj(type="point_of_interest")).kind, "map_marker")
        self.assertFalse(classify(obj(type="some_new_ping_type")).known_type)

    def test_find_self_ignores_spectated(self):
        self.assertIsNone(find_self([obj(icon="Player", color="#174DFF")]))
        self.assertIsNotNone(find_self([obj(icon="Player", color="#faC81E")]))


class Radio(unittest.TestCase):
    def test_real_messages(self):
        m = parse_chat({"id": 109, "msg": "注\u200b意\u200b战\u200b术\u200b地\u200b图！<color=#FF00FF50> [c4]</color>", "sender": "RAB_Witesprite", "enemy": False, "mode": "友\u200b军"}, 1)
        self.assertEqual((m.kind, m.subject, m.grid, m.grid_color, m.mode), ("ping_map", "target", "C4", "FF00FF50", "友军"))
        m = parse_chat({"id": 1, "msg": "注\u200b意\u200b该\u200b坐\u200b标\u200b位\u200b置！<color=#FF00FF50> [d4]</color>", "sender": "x"}, 1)
        self.assertEqual((m.kind, m.grid), ("ping_grid", "D4"))
        m = parse_chat({"id": 1, "msg": "注\u200b意\u200b当\u200b前\u200b标\u200b识\u200b区\u200b域！<color=#FF50BE46> [b3]</color>", "sender": "x"}, 1)
        self.assertEqual(m.kind, "ping_marked")
        m = parse_chat({"id": 1, "msg": "请\u200b求\u200b掩\u200b护！<color=#FF00FF50> [e6]</color>", "sender": "x"}, 1)
        self.assertEqual((m.kind, m.subject, m.grid), ("need_cover", "sender", "E6"))
        m = parse_chat({"id": 1, "msg": "请\u200b求\u200b指\u200b示\u200b空\u200b袭\u200b目\u200b标<color=#FF96966E> [a14, 高\u200b度 1000 米]</color>", "sender": "x"}, 1)
        self.assertEqual((m.kind, m.grid, m.alt_m), ("air_target_request", "A14", 1000.0))
        m = parse_chat({"id": 127, "msg": "注\u200b意\u200b战\u200b术\u200b地\u200b图！", "sender": "传奇耐摔王无声铃鹿"}, 1)
        self.assertTrue(m.own and m.is_ping and m.grid is None)
        self.assertEqual(parse_chat({"id": 2, "msg": "Guide on me [C5, alt. 600 m]", "sender": "a"}, 1).alt_m, 600.0)
        self.assertEqual(parse_chat({"id": 2, "msg": "Attention to the map [B4]", "sender": "a"}, 1).kind, "ping_map")
        self.assertEqual(parse_chat({"id": 2, "msg": "装\u200b填\u200b中！ 00:32", "sender": "a"}, 1).kind, "reloading")

    def test_backlog_first_poll(self):
        c = ChatCursor()
        a = c.feed("/gamechat?lastId=0", 1, 10, [{"id": 120, "msg": "x", "sender": "a"}])
        b = c.feed("/gamechat?lastId=120", 1, 20, [{"id": 120, "msg": "x", "sender": "a"}, {"id": 121, "msg": "y", "sender": "a"}])
        self.assertTrue(a[0].backlog)
        self.assertEqual([m.id for m in b], [121])
        self.assertFalse(b[0].backlog)


class Grid(unittest.TestCase):
    def test_roundtrip_and_zero(self):
        b = cell_bounds("E6", GI_1709_B1)
        cx, cy = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2
        self.assertEqual(cell_of(cx, cy, GI_1709_B1), "E6")
        self.assertEqual(cell_of(b[0] + 1e-6, b[1] + 1e-6, GI_1709_B1), "E6")
        self.assertAlmostEqual(b[2] - b[0], 225.0 / 4096.0)
        self.assertIsNone(cell_bounds("??", GI_1709_B1))


class RealFixtures(unittest.TestCase):
    def test_b1_location_calls_grid_equals_sender_cell(self):
        out = session.run(load("b1_location_calls_1709.jsonl.gz"))
        loc = [p for p in out["pings"] if p.category == "location" and p.source == "chat+map"]
        self.assertGreaterEqual(len(loc), 4)
        for p in loc:
            self.assertEqual(p.sender, "charlieliang520")
            self.assertEqual(p.grid, "E6")
            self.assertEqual(p.sender_cell, "E6")
            self.assertEqual(p.sender_role, "ally")
            self.assertTrue(5800 <= p.signal_ms <= 11000, p.signal_ms)
        thanks = [p for p in out["pings"] if p.kind == "thanks" and p.sender == "Dakrimon"]
        self.assertEqual(len(thanks), 1)
        self.assertTrue(1800 <= thanks[0].signal_ms <= 2000)
        # the dead-player spectator camera (Player in #174DFF) is not self
        self.assertTrue(any(t.spectated and not t.is_self for t in out["tracker"].tracks))

    def test_b2_squad_and_fresh_spawn(self):
        tr = session.run(load("b2_squad_spawn_1709.jsonl.gz"))["tracker"]
        sq = [t for t in tr.tracks if t.role == "squad"]
        self.assertEqual(len(sq), 1)
        self.assertEqual(sq[0].icon, "MediumTank")
        fresh = [t for t in tr.tracks if t.fresh_ms > 0]
        self.assertGreaterEqual(len(fresh), 8)
        self.assertTrue(all(t.role == "ally" for t in fresh))
        self.assertEqual(sum(1 for t in tr.tracks if t.role == "self"), 1)

    def test_b3_spectate_not_self(self):
        tr = session.run(load("b3_spectate_1709.jsonl.gz"))["tracker"]
        spect = [t for t in tr.tracks if t.spectated]
        self.assertTrue(spect)
        self.assertTrue(all(t.role != "self" for t in spect))

    def test_b4_pings_squad_and_own(self):
        out = session.run(load("b4_pings_squad_air_1709.jsonl.gz"))
        P = {p.id: p for p in out["pings"]}
        # squadmate (green Fighter, HuWangTK) radio call: blink keeps squad identity
        hu = [p for p in P.values() if p.sender == "HuWangTK"]
        self.assertEqual(len(hu), 2)
        self.assertTrue(all(p.sender_role == "squad" and p.sender_icon == "Fighter" for p in hu))
        self.assertEqual(out["squad_names"].get("HuWangTK"), 2)
        # teammate pings: target cell differs from the sender's cell
        pings = [p for p in P.values() if p.category == "ping" and not p.own]
        self.assertGreaterEqual(len(pings), 4)
        for p in pings:
            self.assertEqual(p.precision, "grid")
            self.assertIsNotNone(p.area)
            if p.sender_cell:
                self.assertNotEqual(p.grid, p.sender_cell)
        # two pings of KVASSEXUA1 share one 8.2 s blink
        kv = sorted((p for p in pings if p.sender == "KVASSEXUA1"), key=lambda p: p.t_ms)
        self.assertEqual(kv[0].sender_track, kv[1].sender_track)
        # own ping: no grid in the chat echo -> location unknown, own position known
        own = [p for p in P.values() if p.own and p.category == "ping"]
        self.assertEqual(len(own), 1)
        self.assertEqual((own[0].sender_role, own[0].precision, own[0].grid), ("self", "none", None))
        self.assertIsNotNone(own[0].sender_xy)
        self.assertTrue(4800 <= own[0].signal_ms <= 5000)
        # pings / signals never become units: no map_marker objects in RB data, and no ping track
        self.assertEqual(out["tracker"].marker_log, [])

    def test_map_marker_objects_tracked_not_units(self):
        tr = FriendlyTracker()
        f = lambda t, objs: tr.update(t, [MapObj.from_json(o) for o in objs])
        poi = {"type": "point_of_interest", "icon": "none", "color": "#174DFF", "blink": 2, "x": 0.2, "y": 0.43, "icon_bg": "none"}
        me = {"type": "ground_model", "icon": "Player", "color": "#faC81E", "blink": 0, "x": 0.5, "y": 0.5, "icon_bg": "none"}
        f(0, [me, poi]); f(100, [me, poi]); f(200, [me])
        self.assertEqual(len(tr.marker_log), 1)
        m = tr.marker_log[0]
        self.assertEqual((m.first_seen, m.last_seen, m.closed), (0, 100, True))
        self.assertEqual(len(tr.tracks), 1)


if __name__ == "__main__":
    unittest.main()
