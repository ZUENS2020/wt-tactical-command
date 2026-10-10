#!/usr/bin/env python3
"""SYNTHETIC mock of the War Thunder 8111 API for end-to-end testing of wt-collector.

All data here is synthetic (shapes modelled on spec §4 and the wt-modern-8111 / Neo fixtures);
it is NOT a real-game capture. Scenario loops through:
  hangar -> ground match (tank) -> same match, player takes off in aircraft (new map_generation,
  new map image + range) -> dead/spectating (no Player) -> respawn tank -> mission fail -> hangar
  -> air RB match (+-32768 map, 128x128 placeholder image) -> loop.
Chat/HUD ids continue across matches (as the real game does).
Usage: python3 mock8111.py [--port 8111] [--speed 1.0]
"""
import json, math, random, struct, sys, threading, time, zlib, argparse
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler
from urllib.parse import urlparse, parse_qs

ap = argparse.ArgumentParser()
ap.add_argument("--port", type=int, default=8111)
ap.add_argument("--speed", type=float, default=1.0)
ap.add_argument("--hangar-s", type=float, default=None, help="override hangar phase length (gate/pause tests)")
args = ap.parse_args()

def png(w, h, fn):
    raw = b"".join(b"\x00" + bytes(c for x in range(w) for c in fn(x, y)) for y in range(h))
    chunk = lambda t, d: struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)) + chunk(b"IDAT", zlib.compress(raw, 6)) + chunk(b"IEND", b"")

IMG_TANK = png(256, 256, lambda x, y: (60 + (x * 7 + y * 3) % 40, 90 + (x ^ y) % 50, 50))
IMG_AIRVIEW = png(384, 384, lambda x, y: (40, 70 + (x // 16 + y // 16) % 2 * 30, 110))
IMG_NOMAP = png(128, 128, lambda x, y: (64, 64, 64) if (x - 64) ** 2 + (y - 64) ** 2 > 400 else (200, 200, 200))

PHASES = [  # name, seconds
    ("hangar", 6), ("ground", 20), ("ground_air", 14), ("dead", 6), ("ground_respawn", 8), ("end_fail", 4),
    ("hangar", 5), ("air", 16), ("end_air", 3),
]
if args.hangar_s is not None:
    PHASES = [(n, args.hangar_s if n == "hangar" else d) for n, d in PHASES]
TOTAL = sum(p[1] for p in PHASES)
T0 = time.time()
lock = threading.Lock()
chat_log, dmg_log = [], []
counters = {"chat": 0, "dmg": 0, "gen": 0}
state = {"phase": None, "gen": 0}

def phase_at(t):
    t = t % TOTAL
    acc = 0
    for name, d in PHASES:
        if t < acc + d:
            return name, t - acc
        acc += d
    return PHASES[-1][0], 0

def tick():
    """advance scenario: generation counter, chat/hud producers"""
    t = (time.time() - T0) * args.speed
    name, pt = phase_at(t)
    with lock:
        if name != state["phase"]:
            prev = state["phase"]
            state["phase"] = name
            if name in ("ground", "ground_air", "ground_respawn", "air"):
                counters["gen"] += 1
                state["gen"] = counters["gen"]
            if name == "ground_air":
                add_chat("Squadmate", "I'm taking off [C5, alt. 600 m] 起飞 Взлёт", "Team")
            if name == "dead":
                add_dmg("EnemyTank (T-72) destroyed MockPlayer (F-4C)")
        if name in ("ground", "ground_air", "ground_respawn", "air") and random.random() < 0.006:
            add_dmg(random.choice(["A (T-80U) shot down B (Su-25)", "C (Leopard 2A4) destroyed D (M1)", "E set afire F (MiG-21)", "玩家甲 (T-80U) 摧毁了 Игрок (M1)"]))
        if name in ("ground", "air") and random.random() < 0.002:
            add_chat(random.choice(["Ally1", "Союзник", "队友甲"]), random.choice(["go B", "去 B 点！", "Вперёд на A", "spaa left [D4]"]), "Team")
    return t, name, pt

def add_chat(sender, msg, mode):
    counters["chat"] += 1
    chat_log.append({"id": counters["chat"], "msg": msg, "sender": sender, "enemy": False, "mode": mode, "time": round(time.time() - T0, 1)})

def add_dmg(msg):
    counters["dmg"] += 1
    dmg_log.append({"id": counters["dmg"], "msg": msg, "sender": "", "enemy": False, "mode": "", "time": round(time.time() - T0, 1)})

def in_match(name):
    return name in ("ground", "ground_air", "dead", "ground_respawn", "air")

def map_info(name):
    if not in_match(name):
        return {"valid": False}
    g = state["gen"]
    if name == "air":
        return {"grid_size": [57584.11, 64194.2], "grid_steps": [6400.0, 6400.0], "grid_zero": [-24816.11, 31426.2], "hud_type": 0,
                "map_generation": g, "map_max": [32768.0, 32768.0], "map_min": [-32768.0, -32768.0], "valid": True}
    if name == "ground_air":
        return {"grid_size": [8192.0, 8192.0], "grid_steps": [800.0, 800.0], "grid_zero": [-4000.0, 4000.0], "hud_type": 0,
                "map_generation": g, "map_max": [8192.0, 8192.0], "map_min": [-8192.0, -8192.0], "valid": True}
    return {"grid_size": [1600.0, 1600.0], "grid_steps": [225.0, 225.0], "grid_zero": [1519.45, 2497.25], "hud_type": 1,
            "map_generation": g, "map_max": [4096.0, 4096.0], "map_min": [0.0, 0.0], "valid": True}

def q(v):  # quantize like a ~20 Hz game update
    return round(v, 6)

def map_obj(name, t, pt):
    if not in_match(name):
        return []
    tq = math.floor(t * 20) / 20.0
    objs = []
    air_view = name == "ground_air"
    def g2a(x, y):  # tank-map normalized -> air-view normalized (affine, like WTP fit)
        return (0.25 + 0.5 * x, 0.25 + 0.5 * y) if air_view else (x, y)
    if name in ("ground", "ground_respawn"):
        moving = (pt % 10) < 6  # moves 6 s, stands still 4 s
        a = (tq * 0.05) if moving else (math.floor(t / 10) * 10 * 0.05)
        px, py = 0.3 + 0.1 * math.cos(a), 0.7 + 0.1 * math.sin(a)
        objs.append({"type": "ground_model", "color": "#faC81E", "color[]": [250, 200, 30], "blink": 0, "icon": "Player", "icon_bg": "none",
                     "x": q(px), "y": q(py), "dx": q(-math.sin(a)), "dy": q(math.cos(a))})
    elif name == "ground_air":
        a = tq * 0.3
        px, py = 0.45 + 0.08 * math.cos(a), 0.55 + 0.08 * math.sin(a)
        objs.append({"type": "aircraft", "color": "#faC81E", "color[]": [250, 200, 30], "blink": 0, "icon": "Player", "icon_bg": "none",
                     "x": q(px), "y": q(py), "dx": q(-math.sin(a)), "dy": q(math.cos(a))})
    elif name == "air":
        a = tq * 0.2
        objs.append({"type": "aircraft", "color": "#faC81E", "color[]": [250, 200, 30], "blink": 0, "icon": "Player", "icon_bg": "none",
                     "x": q(0.5 + 0.2 * math.cos(a)), "y": q(0.5 + 0.2 * math.sin(a)), "dx": q(-math.sin(a)), "dy": q(math.cos(a))})
        for i in range(4):
            b = tq * 0.25 + i
            objs.append({"type": "aircraft", "color": "#f00C00", "color[]": [240, 12, 0], "blink": 0, "icon": "Fighter", "icon_bg": "none",
                         "x": q(0.6 + 0.1 * math.cos(b)), "y": q(0.3 + 0.1 * math.sin(b)), "dx": q(-math.sin(b)), "dy": q(math.cos(b))})
        for i in range(2):
            b = tq * 0.2 + i * 2
            objs.append({"type": "aircraft", "color": "#185AFF", "color[]": [24, 90, 255], "blink": 0, "icon": "Fighter", "icon_bg": "none",
                         "x": q(0.4 + 0.1 * math.cos(b)), "y": q(0.6 + 0.1 * math.sin(b)), "dx": q(-math.sin(b)), "dy": q(math.cos(b))})
        objs.append({"type": "ground_model", "color": "#fa0C00", "color[]": [250, 12, 0], "blink": 0, "icon": "Structure", "icon_bg": "none", "x": 0.7, "y": 0.2})
        objs.append({"type": "airfield", "color": "#174DFF", "color[]": [23, 77, 255], "blink": 0, "icon": "none", "icon_bg": "none", "sx": 0.35, "sy": 0.56, "ex": 0.35, "ey": 0.51})
        objs.append({"type": "airfield", "color": "#fa3200", "color[]": [250, 50, 0], "blink": 0, "icon": "none", "icon_bg": "none", "sx": 0.75, "sy": 0.16, "ex": 0.79, "ey": 0.16})
        return objs
    # ground match objects (also shown in air view / while dead), positions transformed in air view
    for i, icon in enumerate(["MediumTank", "HeavyTank", "SPAA", "LightTank"]):
        x, y = g2a(0.25 + 0.04 * i + 0.01 * math.sin(tq * 0.1 + i), 0.65 + 0.02 * i)
        objs.append({"type": "ground_model", "color": "#174DFF", "color[]": [23, 77, 255], "blink": 0, "icon": icon, "icon_bg": "none", "x": q(x), "y": q(y)})
    x, y = g2a(0.33, 0.62)
    objs.append({"type": "ground_model", "color": "#39d921", "color[]": [57, 217, 33], "blink": 0, "icon": "MediumTank", "icon_bg": "none", "x": q(x), "y": q(y)})
    if (t % 8) < 5:  # spotted enemies come and go
        for i, icon in enumerate(["MediumTank", "MediumTank", "SPAA"]):
            x, y = g2a(0.65 + 0.03 * i, 0.35 + 0.01 * math.cos(tq * 0.2 + i))
            objs.append({"type": "ground_model", "color": "#fa0C00", "color[]": [250, 12, 0], "blink": 1 if i == 2 else 0, "icon": icon, "icon_bg": "none", "x": q(x), "y": q(y)})
    for zi, (zx, zy) in enumerate([(0.48, 0.48), (0.72, 0.54)]):
        x, y = g2a(zx, zy)
        objs.append({"type": "capture_zone", "color": "#f4b13d", "color[]": [244, 177, 61], "blink": 0, "icon": "capture_zone", "icon_bg": "none", "x": x, "y": y})
    for (bx, by, c) in [(0.14, 0.86, "#174DFF"), (0.86, 0.14, "#fa0C00")]:
        x, y = g2a(bx, by)
        objs.append({"type": "respawn_base_tank", "color": c, "blink": 0, "icon": "respawn_base_tank", "icon_bg": "none", "x": x, "y": y})
    if name == "ground_air":
        objs.append({"type": "aircraft", "color": "#fa3200", "color[]": [250, 50, 0], "blink": 0, "icon": "Assault", "icon_bg": "none", "x": 0.7, "y": 0.3, "dx": 0.0, "dy": 1.0})
    return objs

def indicators(name, t):
    if name in ("ground", "ground_respawn"):
        return {"valid": True, "army": "tank", "type": "tankModels/ussr_t_80u", "stabilizer": 1.0, "gear": 1.0, "speed": round(8 + math.sin(t), 2),
                "rpm": 1500.0 + round(200 * math.sin(t), 1), "lws": -1.0, "ircm": -1.0, "crew_total": 3.0, "crew_current": 3.0}
    if name in ("ground_air", "air"):
        return {"valid": True, "army": "air", "type": "f_4c" if name == "ground_air" else "jh_7", "speed": round(189 + 5 * math.sin(t), 2),
                "altitude_10k": 4936 + int(100 * math.sin(t)), "compass": round((t * 10) % 360, 2), "throttle": 0.9, "g_meter": 0.92}
    return {"valid": False}

def state_ep(name, t):
    if name in ("ground_air", "air"):
        return {"valid": True, "H, m": int(1500 + 100 * math.sin(t)), "TAS, km/h": int(700 + 10 * math.sin(t * 3)), "IAS, km/h": 650, "M": 0.6,
                "AoA, deg": round(2 + math.sin(t * 5), 1), "Ny": 1.0, "Vy, m/s": round(math.cos(t), 1), "Mfuel, kg": 3000 - int(t) % 1000, "throttle 1, %": 90}
    return {"valid": False}

def mission(name):
    if name in ("end_fail",):
        return {"status": "fail", "objectives": [{"primary": True, "status": "failed", "text": "占领并守住据点 / Захватите точки"}]}
    if name == "end_air":
        return {"status": "success", "objectives": [{"primary": True, "status": "completed", "text": "Destroy enemy bombers"}]}
    if not in_match(name):
        return {"status": "running", "objectives": None}
    return {"status": "running", "objectives": [{"primary": True, "status": "in_progress", "text": "Capture and hold the points"}]}

class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    disable_nagle_algorithm = True  # avoid 40 ms Nagle/delayed-ACK artifacts in latency numbers
    wbufsize = 1 << 16
    def log_message(self, *a):
        pass
    def send(self, code, body, ctype="application/json"):
        if isinstance(body, str):
            body = body.encode()
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Access-Control-Allow-Origin", "*")
        self.end_headers()
        self.wfile.write(body)
    def do_GET(self):
        t, name, pt = tick()
        time.sleep(random.uniform(0.0005, 0.003))
        u = urlparse(self.path)
        qs = parse_qs(u.query)
        p = u.path
        j = lambda o: self.send(200, json.dumps(o, separators=(", ", ": "), ensure_ascii=False).encode("utf-8"))
        if p == "/map_info.json": return j(map_info(name))
        if p == "/map_obj.json": return j(map_obj(name, t, pt))
        if p == "/indicators": return j(indicators(name, t))
        if p == "/state": return j(state_ep(name, t))
        if p == "/mission.json": return j(mission(name))
        if p == "/gamechat":
            last = int(qs.get("lastId", ["0"])[0] or 0)
            with lock: return j([m for m in chat_log if m["id"] > last])
        if p == "/hudmsg":
            le = int(qs.get("lastEvt", ["0"])[0] or 0); ld = int(qs.get("lastDmg", ["0"])[0] or 0)
            with lock: return j({"events": [], "damage": [m for m in dmg_log if m["id"] > ld]})
        if p == "/map.img":
            if not in_match(name): return self.send(200, IMG_NOMAP, "image/png")
            img = IMG_NOMAP if name == "air" else (IMG_AIRVIEW if name == "ground_air" else IMG_TANK)
            return self.send(200, img, "image/png")
        if p == "/loc/map/primary_objectives": return self.send(200, 'primary_objectives = "PRIMARY GOALS";', "text/javascript")
        if p == "/loc/map/secondary_objectives": return self.send(200, 'secondary_objectives = "SECONDARY GOALS";', "text/javascript")
        return self.send(404, "not found", "text/plain")

if __name__ == "__main__":
    srv = ThreadingHTTPServer(("127.0.0.1", args.port), H)
    srv.daemon_threads = True
    print(f"SYNTHETIC mock 8111 on 127.0.0.1:{args.port}, scenario loop {TOTAL}s / speed {args.speed}", flush=True)
    srv.serve_forever()
