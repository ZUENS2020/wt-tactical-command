//! Live team view: per-collector latest in-memory state (fed from every accepted record, never from disk),
//! merged per room into one normalized battle state pushed over WebSocket (`GET /api/live?room=&key=`),
//! plus shared drawings (create/update/delete, last-writer-wins by arrival, persisted per room+battle).
use crate::markers::Markers;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use wtc_common::mapobj::{classify, is_enemy_unit, Kind, Side};
use wtc_common::Record;

pub const PLACEHOLDER_IMG: &str = "cfb99c04947c94c19d4c523c1e0fb3a2d71d403263fefcad5f4c4fc8485d07d0";
const PING_TTL: i64 = 30_000;
const CALL_TTL: i64 = 8_000;
const FEED_TTL: i64 = 9_000;
const TOAST_TTL: i64 = 4_000;
const COL_EXPIRE: i64 = 15 * 60_000;
const GHOST_MAX_GROUND: i64 = 30_000;
const GHOST_MAX_AIR: i64 = 15_000;
const MAX_DRAWINGS: usize = 600;
const MAX_DRAWING_BYTES: usize = 16_384;

fn r6(v: f64) -> f64 {
    (v * 1e5).round() / 1e5
}

#[derive(Clone, Debug, PartialEq)]
pub struct MapInfo {
    pub min: [f64; 2],
    pub max: [f64; 2],
    pub zero: [f64; 2],
    pub steps: [f64; 2],
    pub size: [f64; 2],
    pub hud: i64,
    pub gen: String,
}

impl MapInfo {
    fn parse(v: &Value) -> Option<MapInfo> {
        if v["valid"].as_bool() != Some(true) {
            return None;
        }
        let f = |x: &Value| x.as_f64().or_else(|| x.as_str().and_then(|s| s.trim().parse().ok()));
        let p = |k: &str| -> Option<[f64; 2]> { Some([f(&v[k][0])?, f(&v[k][1])?]) };
        let (min, max) = (p("map_min")?, p("map_max")?);
        if max[0] <= min[0] || max[1] <= min[1] {
            return None;
        }
        let steps = p("grid_steps").map(|s| [s[0].abs(), s[1].abs()]).unwrap_or([max[0] - min[0], max[1] - min[1]]);
        Some(MapInfo {
            min,
            max,
            zero: p("grid_zero").unwrap_or([min[0], max[1]]),
            steps,
            size: p("grid_size").unwrap_or([max[0] - min[0], max[1] - min[1]]),
            hud: f(&v["hud_type"]).map(|x| x as i64).unwrap_or(-1),
            gen: match &v["map_generation"] {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                o => o.to_string(),
            },
        })
    }
    /// "air" (air battle / air view inside a ground battle) or "ground"
    pub fn kind(&self) -> &'static str {
        if self.hud == 0 || self.max[0] - self.min[0] > 12_000.0 {
            "air"
        } else {
            "ground"
        }
    }
    fn sig(&self) -> String {
        format!("{}|{}|{:.0}|{:.0}|{:.0}|{:.0}", self.gen, self.kind(), self.min[0], self.min[1], self.max[0], self.max[1])
    }
    fn json(&self) -> Value {
        json!({"min": self.min, "max": self.max, "grid_zero": self.zero, "grid_steps": self.steps, "grid_size": self.size, "hud_type": self.hud, "gen": self.gen})
    }
    fn span(&self) -> [f64; 2] {
        [self.max[0] - self.min[0], self.max[1] - self.min[1]]
    }
}

#[derive(Clone, Debug)]
struct MapEntry {
    sig: String,
    info: MapInfo,
    img: Option<String>,
    since: i64,
    /// last seen static marks on this map: clustered respawn points + airfields (survive view switches)
    marks: Value,
}

#[derive(Clone, Debug)]
struct LUnit {
    side: &'static str,
    air: bool,
    x: f64,
    y: f64,
    hdg: Option<f64>,
    icon: String,
    signal: bool,
}

#[derive(Default, Clone, Debug)]
struct SelfState {
    pos: Option<(f64, f64)>,
    hdg: Option<f64>,
    air: bool,
    icon: String,
    kind: &'static str,
    seen_t: i64,
    dead: bool,
    dead_t: i64,
    need_gap: bool,
}

#[derive(Default)]
pub struct LiveCol {
    pub session: String,
    pub player: String,
    pub nick: String,
    pub room: String,
    pub replay: bool,
    pub last_recv: i64,
    last_data_ts: i64,
    delays: VecDeque<i64>,
    obj_ts: VecDeque<i64>,
    epoch: u32,
    in_battle: bool,
    maps: Vec<MapEntry>,
    cur: Option<usize>,
    units: Vec<LUnit>,
    /// friendly respawn points (normalized) of the current map
    spawns: Vec<(f64, f64)>,
    frame_t: i64,
    spectating: bool,
    me: SelfState,
    markers: Markers,
    events: VecDeque<Value>,
    hud_seen: HashSet<i64>,
    paused: Option<(i64, bool, String)>,
    aa_dropped: u32,
}

const AIRFIELD_AA_M: f64 = 3000.0;

fn side_of_color(c: &str) -> &'static str {
    match c.to_ascii_lowercase().as_str() {
        "#fa0c00" | "#f00c00" => "enemy",
        _ => "ally",
    }
}

/// distance (m) from point to segment, in map-normalized coords scaled by the map span
fn seg_dist(x: f64, y: f64, sx: f64, sy: f64, ex: f64, ey: f64, span: [f64; 2]) -> f64 {
    let (px, py, ax, ay, bx, by) = (x * span[0], y * span[1], sx * span[0], sy * span[1], ex * span[0], ey * span[1]);
    let (dx, dy) = (bx - ax, by - ay);
    let l2 = dx * dx + dy * dy;
    let k = if l2 > 0.0 { (((px - ax) * dx + (py - ay) * dy) / l2).clamp(0.0, 1.0) } else { 0.0 };
    ((px - ax - k * dx).powi(2) + (py - ay - k * dy).powi(2)).sqrt()
}

/// One marker per spawn area: greedy clustering of respawn points of the same kind + side
/// (tank points within 350 m, air spawns within 2 km); points far outside the map are dropped.
fn cluster_spawns(pts: &[(String, &'static str, f64, f64)], span: [f64; 2]) -> Vec<Value> {
    let mut cl: Vec<(String, &'static str, f64, f64, u32)> = Vec::new();
    for (k, side, x, y) in pts {
        if *x < -0.05 || *y < -0.05 || *x > 1.05 || *y > 1.05 {
            continue;
        }
        let r = if k == "tank" { 350.0 } else { 2000.0 };
        match cl.iter_mut().find(|c| &c.0 == k && c.1 == *side && (((c.2 - x) * span[0]).powi(2) + ((c.3 - y) * span[1]).powi(2)).sqrt() < r) {
            Some(c) => {
                let n = c.4 as f64;
                c.2 = (c.2 * n + x) / (n + 1.0);
                c.3 = (c.3 * n + y) / (n + 1.0);
                c.4 += 1;
            }
            None => cl.push((k.clone(), side, *x, *y, 1)),
        }
    }
    cl.into_iter().map(|(k, side, x, y, n)| json!({"k": k, "side": side, "x": r6(x), "y": r6(y), "n": n})).collect()
}

fn hdg_of(o: &Value) -> Option<f64> {
    let (dx, dy) = (o["dx"].as_f64()?, o["dy"].as_f64()?);
    if dx == 0.0 && dy == 0.0 {
        return None;
    }
    Some(((dx.atan2(-dy).to_degrees() + 360.0) % 360.0 * 10.0).round() / 10.0)
}

impl LiveCol {
    fn cur_map(&self) -> Option<&MapEntry> {
        self.cur.and_then(|i| self.maps.get(i))
    }
    fn cur_kind(&self) -> &'static str {
        self.cur_map().map(|m| m.info.kind()).unwrap_or("ground")
    }
    fn push_ev(&mut self, v: Value) {
        self.events.push_back(v);
        while self.events.len() > 200 {
            self.events.pop_front();
        }
    }

    pub fn feed(&mut self, r: &Record, now: i64) {
        self.last_recv = now;
        if r.status != 200 || r.err.is_some() || r.dropped.is_some() {
            return;
        }
        let t = r.ts_server_ms.unwrap_or(r.wall_ms);
        if now - t > 15_000 {
            return; // backlog after a reconnect: live view only
        }
        if r.epoch > self.epoch {
            self.epoch = r.epoch;
            self.me = SelfState::default();
            self.hud_seen.clear();
        }
        match r.ep.as_str() {
            "#state" | "#heartbeat" => {
                if let Some(v) = r.body.as_deref().and_then(|b| serde_json::from_str::<Value>(b).ok()) {
                    self.paused = Some((now, v["paused"].as_bool().unwrap_or(false), v["reason"].as_str().unwrap_or("").to_string()));
                }
                return;
            }
            "/map.img" => {
                if let Some(h) = &r.img_hash {
                    if h != PLACEHOLDER_IMG && self.in_battle {
                        if let Some(i) = self.cur {
                            self.maps[i].img = Some(h.clone());
                        }
                    }
                }
                return;
            }
            _ => {}
        }
        self.markers.feed(r);
        let Some(b) = r.body.as_deref() else { return };
        match r.ep.as_str() {
            "/map_info.json" => {
                let Ok(v) = serde_json::from_str::<Value>(b) else { return };
                match MapInfo::parse(&v) {
                    Some(mi) => {
                        if !self.in_battle {
                            self.in_battle = true;
                            self.maps.clear();
                            self.cur = None;
                            self.me = SelfState::default();
                        }
                        let sig = mi.sig();
                        match self.maps.iter().position(|m| m.sig == sig) {
                            Some(i) => {
                                self.maps[i].info = mi; // provisional grid during the first ~1.3 s
                                if self.cur != Some(i) {
                                    self.maps[i].since = t;
                                }
                                self.cur = Some(i);
                            }
                            None => {
                                // same gen + kind but changed range => replace that entry
                                self.maps.retain(|m| !(m.info.gen == mi.gen && m.info.kind() == mi.kind()));
                                self.maps.push(MapEntry { sig, info: mi, img: None, since: t, marks: Value::Null });
                                self.cur = Some(self.maps.len() - 1);
                            }
                        }
                    }
                    None => {
                        if v["valid"].as_bool() == Some(false) && self.in_battle {
                            self.in_battle = false;
                            self.units.clear();
                        }
                    }
                }
            }
            "/map_obj.json" => {
                let Ok(Value::Array(a)) = serde_json::from_str::<Value>(b) else { return };
                if let Some(ts) = r.ts_server_ms {
                    self.delays.push_back(r.recv_ms.unwrap_or(now) - ts);
                    while self.delays.len() > 60 {
                        self.delays.pop_front();
                    }
                }
                self.last_data_ts = self.last_data_ts.max(t);
                self.obj_ts.push_back(t);
                while self.obj_ts.front().map(|f| *f < t - 5000).unwrap_or(false) {
                    self.obj_ts.pop_front();
                }
                self.frame(t, &a);
            }
            "/hudmsg" => {
                let backlog = r.url.contains("lastDmg=0&") || r.url.ends_with("lastDmg=0");
                let Ok(v) = serde_json::from_str::<Value>(b) else { return };
                let Some(arr) = v["damage"].as_array() else { return };
                for d in arr {
                    let id = d["id"].as_i64().unwrap_or(-1);
                    if !self.hud_seen.insert(id) || backlog {
                        continue;
                    }
                    let msg: String = d["msg"].as_str().unwrap_or("").chars().filter(|c| *c != '\u{200b}').collect();
                    self.hud_line(t, id, &msg);
                }
            }
            _ => {}
        }
        // radio derivations (pings / location calls) of the live tracker
        let out: Vec<Value> = self.markers.out.drain(..).collect();
        let kind = self.cur_kind();
        for e in out {
            match e["ev"].as_str() {
                Some("ping") => {
                    let te = e["t"].as_i64().unwrap_or(t);
                    if e["x"].is_number() {
                        self.push_ev(json!({"k": "ping", "id": e["id"], "t": te, "t_end": te + PING_TTL, "map": kind,
                            "x": e["x"], "y": e["y"], "area": e["area"], "grid": e["grid"], "text": e["text"]}));
                    } else if e["meta"]["own"].as_bool() == Some(true) {
                        self.push_ev(json!({"k": "toast", "id": e["id"], "t": te, "t_end": te + TOAST_TTL, "text": format!("你发出了地图标记（{}）", e["text"].as_str().unwrap_or("无坐标"))}));
                    }
                }
                Some("location") => {
                    let te = e["t"].as_i64().unwrap_or(t);
                    if e["x"].is_number() && e["own"].as_bool() != Some(true) {
                        self.push_ev(json!({"k": "call", "id": format!("c{}", e["msg_id"]), "t": te, "t_end": te + CALL_TTL, "map": kind,
                            "x": e["x"], "y": e["y"], "kind": e["kind"], "text": e["text"], "grid": e["grid"]}));
                    }
                }
                _ => {}
            }
        }
    }

    fn hud_line(&mut self, t: i64, id: i64, msg: &str) {
        const VERBS: &[&str] = &[" 击毁了 ", " 击落了 ", " destroyed ", " shot down "];
        let Some((verb, pos)) = VERBS.iter().find_map(|v| msg.find(v).map(|p| (*v, p))) else { return };
        let (killer, victim) = (&msg[..pos], &msg[pos + verb.len()..]);
        let nick = self.nick.clone();
        let me_k = !nick.is_empty() && killer.contains(&nick);
        let me_v = !nick.is_empty() && victim.contains(&nick);
        if me_v && !self.me.dead {
            self.me.dead = true;
            self.me.dead_t = t;
            self.me.need_gap = true; // Player stays yellow ~7 s after death (R7): ignore until it disappears
        }
        self.push_ev(json!({"k": "feed", "id": format!("h{id}"), "t": t, "t_end": t + FEED_TTL, "text": msg, "me": me_k || me_v, "dead": me_v}));
    }

    fn frame(&mut self, t: i64, a: &[Value]) {
        self.frame_t = t;
        let mut units = Vec::new();
        let mut spawns = Vec::new();
        let mut own: Option<&Value> = None;
        let mut spect = false;
        let span = self.cur_map().map(|m| m.info.span()).unwrap_or([4096.0, 4096.0]);
        // airfields (runway segments) and all respawn points of this frame
        let mut airfields: Vec<(f64, f64, f64, f64, &'static str)> = Vec::new();
        let mut rpts: Vec<(String, &'static str, f64, f64)> = Vec::new();
        for o in a {
            let side = match o["color"].as_str().map(side_of_color) { Some(s) => s, None => continue };
            match o["type"].as_str() {
                Some("airfield") => {
                    if let (Some(sx), Some(sy), Some(ex), Some(ey)) = (o["sx"].as_f64(), o["sy"].as_f64(), o["ex"].as_f64(), o["ey"].as_f64()) {
                        airfields.push((sx, sy, ex, ey, side));
                    }
                }
                Some(ty) if ty.starts_with("respawn_base") => {
                    if let (Some(x), Some(y)) = (o["x"].as_f64(), o["y"].as_f64()) {
                        rpts.push((ty.trim_start_matches("respawn_base").trim_start_matches('_').to_string(), side, x, y));
                    }
                }
                _ => {}
            }
        }
        let near_airfield = |x: f64, y: f64| {
            airfields.iter().any(|(sx, sy, ex, ey, _)| {
                let d = seg_dist(x, y, *sx, *sy, *ex, *ey, span);
                d < AIRFIELD_AA_M
            })
        };
        let mut aa_dropped = 0u32;
        for o in a {
            let c = classify(o);
            let icon = o["icon"].as_str().unwrap_or("");
            if c.kind == Kind::RespawnBase && c.side.friendly() && o["type"].as_str().map(|s| !s.contains("bomber") && !s.contains("fighter")).unwrap_or(true) {
                if let (Some(x), Some(y)) = (o["x"].as_f64(), o["y"].as_f64()) {
                    spawns.push((x, y));
                }
                continue;
            }
            if !c.is_unit() {
                continue;
            }
            // airfield AA defences (ground_model "Airdefence", always #174DFF in recordings, 97 % within 3 km of a runway): clutter, drop
            if icon == "Airdefence" {
                if let (Some(x), Some(y)) = (o["x"].as_f64(), o["y"].as_f64()) {
                    if near_airfield(x, y) {
                        aa_dropped += 1;
                        continue;
                    }
                }
            }
            let (Some(x), Some(y)) = (o["x"].as_f64(), o["y"].as_f64()) else { continue };
            let air = o["type"].as_str() == Some("aircraft");
            if icon == "Player" && c.side == Side::Own {
                own = Some(o);
                continue;
            }
            let side = if is_enemy_unit(o) {
                "enemy"
            } else if c.spectated {
                spect = true;
                "ally"
            } else if c.side == Side::Squad {
                "squad"
            } else if c.side == Side::Ally {
                match self.markers.role_at(x, y, icon) {
                    Some("squad") => "squad",
                    _ => "ally",
                }
            } else {
                continue;
            };
            units.push(LUnit { side, air, x, y, hdg: hdg_of(o), icon: if icon == "Player" { if air { "Fighter".into() } else { "MediumTank".into() } } else { icon.to_string() }, signal: c.signal });
        }
        self.units = units;
        self.spawns = spawns;
        self.aa_dropped = aa_dropped;
        if let Some(i) = self.cur {
            let marks = json!({
                "spawns": cluster_spawns(&rpts, span),
                "airfields": airfields.iter().map(|(sx, sy, ex, ey, side)| json!({"side": side, "a": [r6(*sx), r6(*sy)], "b": [r6(*ex), r6(*ey)]})).collect::<Vec<_>>(),
            });
            self.maps[i].marks = marks;
        }
        self.spectating = spect;
        let kind = self.cur_kind();
        let me = &mut self.me;
        let upd = |me: &mut SelfState, o: &Value| {
            me.pos = Some((o["x"].as_f64().unwrap_or(0.0), o["y"].as_f64().unwrap_or(0.0)));
            me.hdg = hdg_of(o).or(me.hdg);
            me.air = o["type"].as_str() == Some("aircraft");
            me.icon = if me.air { "Fighter".into() } else { "MediumTank".into() };
            me.kind = kind;
            me.seen_t = t;
        };
        if me.dead {
            match own {
                None => me.need_gap = false,
                Some(_) if spect => me.need_gap = false,
                Some(o) if !me.need_gap => {
                    me.dead = false;
                    upd(me, o);
                }
                Some(_) => {}
            }
        } else if let Some(o) = own {
            upd(me, o);
        } else if me.pos.is_some() && t - me.seen_t > 1500 {
            me.dead = true;
            me.dead_t = me.seen_t;
            me.need_gap = false;
        }
    }

    fn hz(&self) -> f64 {
        if self.obj_ts.len() < 2 {
            return 0.0;
        }
        let span = (self.obj_ts.back().unwrap() - self.obj_ts.front().unwrap()) as f64 / 1000.0;
        if span > 0.0 {
            ((self.obj_ts.len() - 1) as f64 / span * 10.0).round() / 10.0
        } else {
            0.0
        }
    }
    fn delay_p50(&self) -> Option<i64> {
        if self.delays.is_empty() {
            return None;
        }
        let mut v: Vec<i64> = self.delays.iter().cloned().collect();
        v.sort();
        Some(v[v.len() / 2])
    }
}

// ---------------------------------------------------------------- rooms / battles / drawings

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct BattleRec {
    pub id: String,
    pub started: i64,
    pub last_active: i64,
    /// collector session -> epoch that belongs to this battle
    pub members: BTreeMap<String, u32>,
    pub maps: BTreeMap<String, Value>,
    pub drawings: BTreeMap<String, Value>,
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct RoomFile {
    current: Option<String>,
    battles: Vec<BattleRec>,
}

#[derive(Clone, Debug)]
struct ETrack {
    id: u64,
    map: &'static str,
    icon: String,
    air: bool,
    x: f64,
    y: f64,
    vx: f64,
    vy: f64,
    hdg: Option<f64>,
    first: i64,
    last: i64,
    spawn_m: Option<f64>,
}

pub struct Room {
    pub name: String,
    pub tx: broadcast::Sender<Arc<str>>,
    battles: Vec<BattleRec>,
    current: Option<String>,
    dirty: bool,
    tracks: Vec<ETrack>,
    next_eid: u64,
    last_json: String,
    last_sent: i64,
}

impl Room {
    fn new(name: &str, dir: &PathBuf) -> Room {
        let f: RoomFile = std::fs::read(dir.join(format!("room-{name}.json"))).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Room { name: name.into(), tx: broadcast::channel(256).0, battles: f.battles, current: f.current, dirty: false, tracks: vec![], next_eid: 1, last_json: String::new(), last_sent: 0 }
    }
    fn battle(&self) -> Option<&BattleRec> {
        let id = self.current.as_ref()?;
        self.battles.iter().find(|b| &b.id == id)
    }
    fn battle_mut(&mut self) -> Option<&mut BattleRec> {
        let id = self.current.clone()?;
        self.battles.iter_mut().find(|b| b.id == id)
    }
    fn new_battle(&mut self, now: i64) -> &mut BattleRec {
        let id = format!("b{}", radix36(now as u64));
        self.battles.push(BattleRec { id: id.clone(), started: now, last_active: now, ..Default::default() });
        while self.battles.len() > 40 {
            self.battles.remove(0);
        }
        self.current = Some(id);
        self.tracks.clear();
        self.dirty = true;
        self.battles.last_mut().unwrap()
    }
    fn save(&mut self, dir: &PathBuf) {
        if !self.dirty {
            return;
        }
        let f = RoomFile { current: self.current.clone(), battles: self.battles.clone() };
        if let Ok(b) = serde_json::to_vec(&f) {
            let tmp = dir.join(format!("room-{}.json.tmp", self.name));
            if std::fs::write(&tmp, b).is_ok() && std::fs::rename(&tmp, dir.join(format!("room-{}.json", self.name))).is_ok() {
                self.dirty = false;
            }
        }
    }
    pub fn hello(&self, now: i64) -> String {
        let b = self.battle();
        json!({"type": "hello", "room": self.name, "server_ms": now, "battle": b.map(|b| &b.id), "battle_started": b.map(|b| b.started),
            "drawings": b.map(|b| b.drawings.values().collect::<Vec<_>>()).unwrap_or_default()})
        .to_string()
    }
}

fn radix36(mut n: u64) -> String {
    let d = b"0123456789abcdefghijklmnopqrstuvwxyz";
    let mut s = Vec::new();
    while n > 0 {
        s.push(d[(n % 36) as usize]);
        n /= 36;
    }
    s.reverse();
    String::from_utf8(s).unwrap_or_default()
}

pub struct Hub {
    pub cols: HashMap<String, LiveCol>,
    pub rooms: HashMap<String, Room>,
    pub dir: PathBuf,
    pub default_room: String,
    pub room_map: HashMap<String, String>,
    pub nicks: HashMap<String, String>,
    pub view_key: String,
}

impl Hub {
    pub fn from_env(data_root: &std::path::Path) -> Hub {
        let dir = std::env::var("WT_LIVE_DIR").map(PathBuf::from).unwrap_or_else(|_| data_root.parent().unwrap_or(data_root).join("live"));
        let _ = std::fs::create_dir_all(&dir);
        let kv = |k: &str| -> HashMap<String, String> {
            std::env::var(k)
                .unwrap_or_default()
                .split(',')
                .filter_map(|p| {
                    let (a, b) = p.split_once('=')?;
                    Some((a.trim().to_string(), b.trim().to_string()))
                })
                .filter(|(a, b)| !a.is_empty() && !b.is_empty())
                .collect()
        };
        let mut nicks = kv("WT_PLAYER_NICKS");
        nicks.entry("shengyuan".into()).or_insert_with(|| wtc_common::radio::OWN_NAMES[0].to_string());
        Hub {
            cols: HashMap::new(),
            rooms: HashMap::new(),
            dir,
            default_room: std::env::var("WT_LIVE_ROOM").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "main".into()),
            room_map: kv("WT_LIVE_ROOM_MAP"),
            nicks,
            view_key: std::env::var("WT_VIEW_TOKEN").unwrap_or_default(),
        }
    }

    pub fn room(&mut self, name: &str) -> &mut Room {
        let dir = self.dir.clone();
        self.rooms.entry(name.to_string()).or_insert_with(|| Room::new(name, &dir))
    }

    /// Feed one accepted record of a collector session (or a replay pseudo-collector).
    pub fn feed(&mut self, session: &str, player: &str, room: Option<&str>, r: &Record, now: i64) {
        if !self.cols.contains_key(session) {
            let nick = self.nicks.get(player).cloned().unwrap_or_default();
            let room = room.map(|s| s.to_string()).or_else(|| self.room_map.get(player).cloned()).unwrap_or_else(|| self.default_room.clone());
            let mut c = LiveCol { session: session.into(), player: player.into(), room, replay: session.starts_with("replay:"), ..Default::default() };
            if !nick.is_empty() {
                c.markers.own = Some(nick.clone());
            }
            c.nick = nick;
            self.cols.insert(session.to_string(), c);
        }
        let c = self.cols.get_mut(session).unwrap();
        if c.player.is_empty() && !player.is_empty() {
            c.player = player.into();
        }
        c.feed(r, now);
    }

    pub fn remove_col(&mut self, session: &str) {
        self.cols.remove(session);
    }

    /// 10 Hz: battle tracking + state build + broadcast. Returns nothing; sends on room channels.
    pub fn tick(&mut self, now: i64) {
        self.cols.retain(|_, c| now - c.last_recv < COL_EXPIRE);
        let names: HashSet<String> = self.cols.values().map(|c| c.room.clone()).chain(self.rooms.keys().cloned()).collect();
        for n in names {
            self.room(&n);
            let mut room = self.rooms.remove(&n).unwrap();
            self.tick_room(&mut room, now);
            self.rooms.insert(n, room);
        }
    }

    fn tick_room(&self, room: &mut Room, now: i64) {
        let mut cols: Vec<&LiveCol> = self.cols.values().filter(|c| c.room == room.name).collect();
        cols.sort_by(|a, b| a.session.cmp(&b.session));
        // ---- battle tracking: a collector entering a match (new epoch) joins the running battle if another
        // member is in it right now, otherwise it starts a new battle.
        for c in cols.iter().filter(|c| c.in_battle && now - c.last_recv < 10_000) {
            let in_cur = room.battle().map(|b| b.members.get(&c.session) == Some(&c.epoch)).unwrap_or(false);
            if in_cur {
                continue;
            }
            let running = room
                .battle()
                .map(|b| cols.iter().any(|o| o.session != c.session && o.in_battle && now - o.last_recv < 10_000 && b.members.get(&o.session) == Some(&o.epoch)))
                .unwrap_or(false);
            if !running {
                room.new_battle(now);
            }
            if let Some(b) = room.battle_mut() {
                b.members.insert(c.session.clone(), c.epoch);
            }
            room.dirty = true;
        }
        let (members, bstart): (Vec<&LiveCol>, i64) = match room.battle() {
            Some(b) => (cols.iter().filter(|c| c.in_battle && b.members.get(&c.session) == Some(&c.epoch)).cloned().collect(), b.started),
            None => (vec![], 0),
        };
        let in_battle = !members.is_empty();
        // ---- maps: latest entry of each kind among members
        let mut best: BTreeMap<&'static str, (&MapEntry, &LiveCol)> = BTreeMap::new();
        for c in &members {
            for m in &c.maps {
                let k = m.info.kind();
                if best.get(k).map(|(e, _)| m.since > e.since || (m.since == e.since && m.img.is_some() && e.img.is_none())).unwrap_or(true) {
                    best.insert(k, (m, c));
                }
            }
        }
        let mut changed = false;
        if let Some(b) = room.battle_mut() {
            if in_battle {
                b.last_active = now;
            }
            for (k, (m, _)) in &best {
                let v = json!({"kind": k, "hash": m.img, "url": m.img.as_ref().map(|h| format!("/api/map/{h}")), "info": m.info.json(), "since": m.since});
                if b.maps.get(*k) != Some(&v) {
                    b.maps.insert(k.to_string(), v);
                    changed = true;
                }
            }
        }
        room.dirty |= changed;
        // ---- units
        let mut units: Vec<Value> = Vec::new();
        let mut enemy_obs: Vec<(&'static str, &LUnit, i64, Option<f64>)> = Vec::new();
        let mut selves: Vec<(&'static str, f64, f64)> = Vec::new();
        for c in &members {
            let Some(cm) = c.cur_map() else { continue };
            let k = cm.info.kind();
            let on_room_map = best.get(k).map(|(m, _)| m.sig == cm.sig || m.img.is_none() || cm.img.is_none() || m.img == cm.img).unwrap_or(false);
            let age = now - c.frame_t;
            if c.me.pos.is_some() {
                let (x, y) = c.me.pos.unwrap();
                if c.me.kind == k || c.me.dead {
                    selves.push((c.me.kind, x, y));
                    units.push(json!({"id": format!("self:{}", c.session), "side": "self", "map": c.me.kind, "x": r6(x), "y": r6(y), "hdg": c.me.hdg,
                        "icon": c.me.icon, "air": c.me.air, "nick": if c.nick.is_empty() { c.player.clone() } else { c.nick.clone() },
                        "dead": c.me.dead, "age": if c.me.dead { now - c.me.dead_t } else { age }, "by": c.player, "spectating": c.me.dead && c.spectating}));
                }
            }
            if !on_room_map {
                continue;
            }
            let span = cm.info.span();
            for (i, u) in c.units.iter().enumerate() {
                if u.side == "enemy" {
                    let sp = if u.air {
                        None
                    } else {
                        c.spawns.iter().map(|s| (((s.0 - u.x) * span[0]).powi(2) + ((s.1 - u.y) * span[1]).powi(2)).sqrt()).fold(None, |a: Option<f64>, d| Some(a.map_or(d, |a| a.min(d))))
                    };
                    enemy_obs.push((k, u, c.frame_t, sp));
                    continue;
                }
                units.push(json!({"id": format!("{}:{i}", &c.session[c.session.len().saturating_sub(4)..]), "side": u.side, "map": k, "x": r6(u.x), "y": r6(u.y),
                    "hdg": u.hdg, "icon": u.icon, "air": u.air, "age": age, "signal": u.signal}));
            }
        }
        // drop friendlies that are another member's own Player (same unit seen by two collectors)
        if selves.len() > 0 && members.len() > 1 {
            units.retain(|u| {
                u["side"] == "self"
                    || !selves.iter().any(|(k, x, y)| u["map"] == *k && (u["x"].as_f64().unwrap_or(9.0) - x).abs() < 0.003 && (u["y"].as_f64().unwrap_or(9.0) - y).abs() < 0.003)
            });
        }
        // ---- enemy tracks (live + short last-seen ghost)
        self.track_enemies(room, &enemy_obs, now);
        for tr in &room.tracks {
            let age = now - tr.last;
            units.push(json!({"id": format!("e{}", tr.id), "side": "enemy", "map": tr.map, "x": r6(tr.x), "y": r6(tr.y), "hdg": tr.hdg, "icon": tr.icon,
                "air": tr.air, "age": age, "since": now - tr.first, "vx": r6(tr.vx), "vy": r6(tr.vy), "spawn_m": tr.spawn_m.map(|d| d.round())}));
        }
        // ---- events
        let mut evs: Vec<&Value> = Vec::new();
        let mut keys = HashSet::new();
        for c in &members {
            for e in c.events.iter().rev() {
                if e["t_end"].as_i64().unwrap_or(0) < now || e["t"].as_i64().unwrap_or(0) < bstart - 5000 {
                    continue;
                }
                let key = format!("{}|{}|{}|{}", e["k"], e["grid"], e["text"], e["t"].as_i64().unwrap_or(0) / 3000);
                if keys.insert(key) {
                    evs.push(e);
                }
            }
        }
        evs.sort_by_key(|e| e["t"].as_i64().unwrap_or(0));
        let pick = |k: &str| evs.iter().filter(|e| e["k"] == k).cloned().cloned().collect::<Vec<Value>>();
        let collectors: Vec<Value> = cols
            .iter()
            .map(|c| {
                json!({"session": c.session, "player": c.player, "nick": c.nick, "replay": c.replay, "in_battle": c.in_battle,
                "member": members.iter().any(|m| m.session == c.session), "age_ms": now - c.last_recv,
                "lag_ms": if c.last_data_ts > 0 { Some(now - c.last_data_ts) } else { None }, "delay_ms": c.delay_p50(), "hz": c.hz(),
                "paused": c.paused.as_ref().filter(|p| now - p.0 < 75_000).map(|p| p.1).unwrap_or(false),
                "map": c.cur_map().map(|m| m.info.kind()), "spectating": c.spectating, "aa_filtered": c.aa_dropped})
            })
            .collect();
        let b = room.battle();
        let marks: BTreeMap<&str, &Value> = best.iter().filter(|(_, (m, _))| !m.marks.is_null()).map(|(k, (m, _))| (*k, &m.marks)).collect();
        let state = json!({
            "marks": marks,
            "type": "state", "room": room.name, "battle": b.map(|b| &b.id), "battle_started": b.map(|b| b.started), "in_battle": in_battle,
            "maps": b.map(|b| &b.maps), "units": units, "pings": pick("ping"), "calls": pick("call"), "feed": pick("feed"), "toasts": pick("toast"),
            "collectors": collectors, "viewers": room.tx.receiver_count(),
        });
        let s = state.to_string();
        if room.tx.receiver_count() > 0 && (s != room.last_json || now - room.last_sent >= 1000) {
            let with_t = format!("{{\"t\":{now},{}", &s[1..]);
            let _ = room.tx.send(Arc::from(with_t.as_str()));
            room.last_sent = now;
        }
        room.last_json = s;
    }

    fn track_enemies(&self, room: &mut Room, obs: &[(&'static str, &LUnit, i64, Option<f64>)], now: i64) {
        // merge duplicates (same enemy seen by two collectors)
        let mut merged: Vec<(&'static str, &LUnit, i64, Option<f64>)> = Vec::new();
        for o in obs {
            if !merged.iter().any(|m| m.0 == o.0 && m.1.icon == o.1.icon && (m.1.x - o.1.x).abs() < 0.004 && (m.1.y - o.1.y).abs() < 0.004) {
                merged.push(*o);
            }
        }
        let mut pairs = Vec::new();
        for (i, o) in merged.iter().enumerate() {
            for (j, tr) in room.tracks.iter().enumerate() {
                if tr.map != o.0 || tr.icon != o.1.icon {
                    continue;
                }
                let dt = ((o.2 - tr.last).max(0) as f64 / 1000.0).min(30.0);
                let gate = if o.1.air { 0.02 + 0.01 * dt } else { 0.01 + 0.004 * dt };
                let d = ((o.1.x - tr.x).powi(2) + (o.1.y - tr.y).powi(2)).sqrt();
                if d <= gate {
                    pairs.push((d, i, j));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let (mut ui, mut tj) = (HashSet::new(), HashSet::new());
        for (_, i, j) in pairs {
            if ui.contains(&i) || tj.contains(&j) {
                continue;
            }
            ui.insert(i);
            tj.insert(j);
            let (_, u, t, sp) = merged[i];
            let tr = &mut room.tracks[j];
            let dt = (t - tr.last) as f64 / 1000.0;
            if dt > 0.05 {
                let (vx, vy) = ((u.x - tr.x) / dt, (u.y - tr.y) / dt);
                tr.vx = tr.vx * 0.6 + vx * 0.4;
                tr.vy = tr.vy * 0.6 + vy * 0.4;
            }
            tr.x = u.x;
            tr.y = u.y;
            tr.hdg = u.hdg.or(tr.hdg);
            tr.last = tr.last.max(t);
            tr.spawn_m = sp;
        }
        for (i, (k, u, t, sp)) in merged.iter().enumerate() {
            if ui.contains(&i) {
                continue;
            }
            room.next_eid += 1;
            room.tracks.push(ETrack { id: room.next_eid, map: k, icon: u.icon.clone(), air: u.air, x: u.x, y: u.y, vx: 0.0, vy: 0.0, hdg: u.hdg, first: *t, last: *t, spawn_m: *sp });
        }
        room.tracks.retain(|t| now - t.last < if t.air { GHOST_MAX_AIR } else { GHOST_MAX_GROUND });
    }

    /// Apply a drawing op from a browser. Returns the broadcast message (or an error string for the sender).
    pub fn draw_op(&mut self, room_name: &str, op: &Value, client: &str, now: i64) -> Result<String, String> {
        let room = self.room(room_name);
        if room.battle().is_none() {
            room.new_battle(now);
        }
        if let Some(want) = op["battle"].as_str() {
            if Some(want) != room.current.as_deref() {
                return Err("battle_changed".into());
            }
        }
        let bid = room.current.clone().unwrap_or_default();
        let b = room.battle_mut().unwrap();
        let msg = match op["op"].as_str() {
            Some("put") => {
                let mut d = op["d"].clone();
                let id = d["id"].as_str().unwrap_or("").to_string();
                if id.is_empty() || id.len() > 64 || !d.is_object() {
                    return Err("bad drawing".into());
                }
                if d["pts"].as_array().map(|a| a.len() > 400 || a.is_empty()).unwrap_or(true) {
                    return Err("bad pts".into());
                }
                if d.to_string().len() > MAX_DRAWING_BYTES {
                    return Err("drawing too large".into());
                }
                if !b.drawings.contains_key(&id) && b.drawings.len() >= MAX_DRAWINGS {
                    return Err("too many drawings".into());
                }
                let created = !b.drawings.contains_key(&id);
                d["sv"] = json!(now);
                b.drawings.insert(id, d.clone());
                json!({"type": "draw", "op": "put", "battle": bid, "d": d, "by_client": client, "created": created})
            }
            Some("del") => {
                let id = op["id"].as_str().unwrap_or("").to_string();
                let Some(old) = b.drawings.remove(&id) else { return Err("not found".into()) };
                json!({"type": "draw", "op": "del", "battle": bid, "id": id, "old": old, "by": op["by"], "by_client": client})
            }
            _ => return Err("bad op".into()),
        };
        room.dirty = true;
        let s = msg.to_string();
        let _ = room.tx.send(Arc::from(s.as_str()));
        Ok(s)
    }

    pub fn save_all(&mut self) {
        let dir = self.dir.clone();
        for r in self.rooms.values_mut() {
            r.save(&dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore]
    fn debug_session() {
        let dir = std::env::var("WT_DEBUG_SESSION").unwrap();
        let recs = wtc_common::read_session(std::path::Path::new(&dir)).unwrap();
        let mut c = LiveCol::default();
        let base = recs[0].ts_server_ms.unwrap_or(recs[0].wall_ms);
        let now0 = wtc_common::now_ms();
        let mut n = 0;
        for r0 in &recs {
            let mut r = r0.clone();
            let t = now0 + (r.ts_server_ms.unwrap_or(r.wall_ms) - base);
            r.ts_server_ms = Some(t);
            c.feed(&r, t);
            n += 1;
            if n % 3000 == 0 {
                eprintln!("{n} ep{} inb {} maps {} cur {:?} units {} me {:?} dead {}", c.epoch, c.in_battle, c.maps.len(), c.cur, c.units.len(), c.me.pos, c.me.dead);
            }
        }
    }
    fn rec(seq: u64, ep: &str, t: i64, body: &str) -> Record {
        Record { session: "s".into(), seq, ep: ep.into(), url: format!("{ep}?lastId=5&lastDmg=5"), wall_ms: t, ts_server_ms: Some(t), status: 200, body: Some(body.into()), epoch: 1, ..Default::default() }
    }
    const MI: &str = r#"{"valid":true,"grid_steps":[225.0,225.0],"grid_zero":[-884.35,-1107.0],"grid_size":[1600,1600],"map_max":[2048.0,0.0],"map_min":[-2048.0,-4096.0],"hud_type":1,"map_generation":5}"#;

    #[test]
    fn self_dead_after_hud_and_spectate() {
        let now = wtc_common::now_ms();
        let mut h = Hub { cols: HashMap::new(), rooms: HashMap::new(), dir: std::env::temp_dir().join("wt-live-test"), default_room: "t".into(), room_map: HashMap::new(), nicks: [("p".to_string(), "Me".to_string())].into(), view_key: String::new() };
        let _ = std::fs::create_dir_all(&h.dir);
        h.feed("s1", "p", None, &rec(1, "/map_info.json", now - 5000, MI), now);
        let me = |c: &str, x: f64| format!(r##"[{{"type":"ground_model","icon":"Player","color":"{c}","blink":0,"x":{x},"y":0.5,"dx":1.0,"dy":0.0}},{{"type":"ground_model","icon":"HeavyTank","color":"#fa0C00","blink":0,"x":0.3,"y":0.3}},{{"type":"respawn_base_tank","icon":"respawn_base_tank","color":"#174DFF","blink":0,"x":0.31,"y":0.31}},{{"type":"respawn_base_tank","icon":"respawn_base_tank","color":"#174DFF","blink":0,"x":0.32,"y":0.31}},{{"type":"respawn_base_tank","icon":"respawn_base_tank","color":"#fa0C00","blink":0,"x":0.8,"y":0.8}},{{"type":"airfield","color":"#174DFF","blink":0,"icon":"none","sx":0.9,"sy":0.1,"ex":0.95,"ey":0.1}},{{"type":"ground_model","icon":"Airdefence","color":"#174DFF","blink":0,"x":0.93,"y":0.12}},{{"type":"respawn_base_fighter","icon":"respawn_base_fighter","color":"#174DFF","blink":0,"x":-2.5,"y":0.5}}]"##);
        h.feed("s1", "p", None, &rec(2, "/map_obj.json", now - 4000, &me("#faC81E", 0.5)), now);
        h.feed("s1", "p", None, &rec(3, "/hudmsg", now - 3900, r#"{"events":[],"damage":[{"id":9,"msg":"X (T-34) 击毁了 Me (Tiger)"}]}"#), now);
        h.feed("s1", "p", None, &rec(4, "/map_obj.json", now - 3800, &me("#faC81E", 0.6)), now);
        let c = &h.cols["s1"];
        assert!(c.me.dead && c.me.pos == Some((0.5, 0.5)));
        h.feed("s1", "p", None, &rec(5, "/map_obj.json", now - 3000, &me("#174DFF", 0.7)), now); // spectating teammate
        assert!(h.cols["s1"].me.dead && h.cols["s1"].spectating);
        h.feed("s1", "p", None, &rec(6, "/map_obj.json", now - 2000, &me("#faC81E", 0.2)), now); // respawn
        assert!(!h.cols["s1"].me.dead && h.cols["s1"].me.pos == Some((0.2, 0.5)));
        let mut rx = h.room("t").tx.subscribe();
        h.tick(now);
        let s: Value = serde_json::from_str(&rx.try_recv().unwrap()).unwrap();
        assert_eq!(s["in_battle"], true);
        assert_eq!(s["maps"]["ground"]["info"]["grid_steps"][0], 225.0);
        let en: Vec<&Value> = s["units"].as_array().unwrap().iter().filter(|u| u["side"] == "enemy").collect();
        assert_eq!(en.len(), 1);
        assert!(en[0]["spawn_m"].as_f64().unwrap() < 100.0);
        assert_eq!(s["feed"][0]["dead"], true);
        assert!(!s["units"].as_array().unwrap().iter().any(|u| u["icon"] == "Airdefence"), "airfield AA filtered");
        let sp = s["marks"]["ground"]["spawns"].as_array().unwrap();
        assert_eq!(sp.len(), 2, "two tank spawn clusters, off-map fighter spawn dropped: {sp:?}");
        assert_eq!(sp.iter().find(|x| x["side"] == "ally").unwrap()["n"], 2);
        assert_eq!(s["marks"]["ground"]["airfields"].as_array().unwrap().len(), 1);
        // drawings
        let r = h.draw_op("t", &json!({"op": "put", "d": {"id": "a1", "kind": "point", "pts": [[1, 2]]}}), "c1", now).unwrap();
        assert!(r.contains("\"created\":true"));
        assert!(h.room("t").hello(now).contains("a1"));
        assert!(h.draw_op("t", &json!({"op": "del", "id": "a1"}), "c1", now).is_ok());
    }
}
