//! Live team view: per-collector latest in-memory state (fed from every accepted record, never from disk),
//! merged per room into one normalized battle state pushed over WebSocket (`GET /api/live?room=&key=`),
//! plus shared drawings (create/update/delete, version-checked operations, persisted per room+battle).
use crate::markers::Markers;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use wtc_common::mapobj::{classify, is_enemy_unit, Kind, Side};
use wtc_common::{Record, LIVE_MAX_AGE_MS};

pub const PLACEHOLDER_IMG: &str = "cfb99c04947c94c19d4c523c1e0fb3a2d71d403263fefcad5f4c4fc8485d07d0";
const PING_TTL: i64 = LIVE_MAX_AGE_MS;
const CALL_TTL: i64 = LIVE_MAX_AGE_MS;
const FEED_TTL: i64 = LIVE_MAX_AGE_MS;
const TOAST_TTL: i64 = 4_000;
const COL_EXPIRE: i64 = 15 * 60_000;
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
        if !min.iter().chain(max.iter()).all(|x| x.is_finite()) || max[0] <= min[0] || max[1] <= min[1] {
            return None;
        }
        let zero = p("grid_zero").unwrap_or([min[0], max[1]]);
        let size = p("grid_size").unwrap_or([max[0]-min[0],max[1]-min[1]]);
        if !zero.iter().all(|x| x.is_finite()) || !size.iter().all(|x| x.is_finite() && *x > 0.0) { return None; }
        let steps = p("grid_steps").map(|s| [s[0].abs(), s[1].abs()]).unwrap_or([max[0] - min[0], max[1] - min[1]]);
        if !steps.iter().all(|x| x.is_finite() && *x > 0.0) { return None; }
        Some(MapInfo {
            min,
            max,
            zero,
            steps,
            size,
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
        format!("{}|{:?}|{:?}|{:?}|{:?}", self.kind(), self.min, self.max, self.zero, self.steps)
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
    pub device_id: String,
    context: Option<String>,
    watermarks: HashMap<String, (u64, u64)>,
    invalid_since: Option<i64>,
    pub out_of_order: u64,
    pub context_dropped: u64,
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

    #[cfg(test)]
    pub fn feed(&mut self, r: &Record, now: i64) { let parsed=r.body.as_deref().and_then(|b| serde_json::from_str::<Value>(b).ok()); self.feed_parsed(r,now,parsed.as_ref()); }
    pub fn feed_parsed(&mut self, r: &Record, now: i64, parsed: Option<&Value>) {
        self.last_recv = now;
        if r.status != 200 || r.err.is_some() || r.dropped.is_some() {
            return;
        }
        let t = r.ts_server_ms.unwrap_or(r.wall_ms);
        if now - t > LIVE_MAX_AGE_MS {
            return; // backlog after a reconnect: live view only
        }
        if r.epoch < self.epoch { self.context_dropped += 1; return; }
        let order = (if r.mono_ns > 0 { r.mono_ns } else { t.max(0) as u64 * 1_000_000 }, r.seq);
        if self.watermarks.get(&r.ep).map(|o| *o >= order).unwrap_or(false) { self.out_of_order += 1; return; }
        if r.ep != "/map_info.json" && !r.ep.starts_with('#') && r.map_context.is_some() && r.map_context != self.context {
            self.context_dropped += 1; return;
        }
        self.watermarks.insert(r.ep.clone(), order);
        if r.epoch > self.epoch {
            self.epoch = r.epoch;
            self.me = SelfState::default();
            self.hud_seen.clear();
        }
        match r.ep.as_str() {
            "#state" | "#heartbeat" => {
                if let Some(v) = parsed {
                    let paused = v["paused"].as_bool().unwrap_or(false);
                    self.paused = Some((now, paused, v["reason"].as_str().unwrap_or("").to_string()));
                    if paused && v["map_valid"].as_bool() == Some(false) && v["reason"].as_str() == Some("hangar") {
                        self.invalid_since.get_or_insert(t); self.in_battle = false; self.units.clear();
                    }
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
        self.markers.feed_parsed(r, parsed);
        let Some(v) = parsed else { return };
        match r.ep.as_str() {
            "/map_info.json" => {

                match MapInfo::parse(&v) {
                    Some(mi) => {
                        self.invalid_since = None;
                        if self.context != r.map_context { self.units.clear(); self.me = SelfState::default(); }
                        self.context = r.map_context.clone();
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
                                self.maps[i].since = t;
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
                            self.invalid_since.get_or_insert(t);
                            self.in_battle = false;
                            self.units.clear();
                        }
                    }
                }
            }
            "/map_obj.json" => {
                let Value::Array(a) = v else { return };
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
                            "x": e["x"], "y": e["y"], "area": e["area"], "grid": e["grid"], "sender": e["meta"]["sender"], "msg_id": e["meta"]["msg_id"], "context": self.cur_map().map(|m| format!("{}|{}", m.img.as_deref().unwrap_or(""), m.sig)), "text": e["text"]}));
                    } else if e["meta"]["own"].as_bool() == Some(true) {
                        self.push_ev(json!({"k": "toast", "id": e["id"], "t": te, "t_end": te + TOAST_TTL, "text": format!("你发出了地图标记（{}）", e["text"].as_str().unwrap_or("无坐标"))}));
                    }
                }
                Some("location") => {
                    let te = e["t"].as_i64().unwrap_or(t);
                    if e["x"].is_number() && e["own"].as_bool() != Some(true) {
                        self.push_ev(json!({"k": "call", "id": format!("c{}", e["msg_id"]), "t": te, "t_end": te + CALL_TTL, "map": kind,
                            "x": e["x"], "y": e["y"], "kind": e["kind"], "sender": e["sender"], "msg_id":e["msg_id"], "context":self.cur_map().map(|m| format!("{}|{}", m.img.as_deref().unwrap_or(""), m.sig)), "text": e["text"], "grid": e["grid"]}));
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
            if !x.is_finite() || !y.is_finite() || !(-0.05..=1.05).contains(&x) || !(-0.05..=1.05).contains(&y) { continue; }
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
            units.push(LUnit { side, air, x, y, hdg: hdg_of(o), icon: if icon == "Player" { "Unknown".into() } else { icon.to_string() }, signal: c.signal });
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
            me.icon = "Unknown".into();
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
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub tombstones: BTreeMap<String, u64>,
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
    side: &'static str,
    signal: bool,
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
    state_seq: u64,
    ended: bool,
    op_cache: HashMap<String, String>,
    op_order: VecDeque<String>,
}

impl Room {
    fn new(name: &str, dir: &PathBuf) -> Room {
        let path = dir.join(format!("room-{name}.json"));
        let raw = std::fs::read(&path).ok();
        let mut f: RoomFile = raw.as_deref().and_then(|b| serde_json::from_slice(b).ok()).unwrap_or_default();
        // Preserve pre-existing history before migrating the live file to one current battle.
        if f.battles.len() > 1 {
            let backup = dir.join(format!("room-{name}.legacy.json"));
            if let Ok(mut file) = std::fs::OpenOptions::new().write(true).create_new(true).open(backup) {
                use std::io::Write; if let Some(bytes)=raw.as_deref() { let _=file.write_all(bytes); let _=file.sync_all(); }
            }
        }
        for battle in &mut f.battles { battle.revision = battle.revision.max(battle.drawings.values().filter_map(|d| d["sv"].as_u64()).max().unwrap_or(0)); }
        Room { name: name.into(), tx: broadcast::channel(256).0, battles: f.battles, current: f.current, dirty: false, tracks: vec![], next_eid: 1, last_json: String::new(), last_sent: 0, state_seq: 0, ended: false, op_cache: HashMap::new(), op_order: VecDeque::new() }
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
        let id = format!("b{}-{:016x}", radix36(now as u64), rand::random::<u64>());
        self.battles.clear();
        self.ended = false;
        self.op_cache.clear(); self.op_order.clear();
        self.battles.push(BattleRec { id: id.clone(), started: now, last_active: now, ..Default::default() });
        while self.battles.len() > 40 {
            self.battles.remove(0);
        }
        self.current = Some(id);
        self.tracks.clear();
        self.dirty = true;
        self.battles.last_mut().unwrap()
    }
    pub fn hello(&self, now: i64) -> String {
        let b = self.battle();
        json!({"type": "hello", "protocol": 2, "state_seq": self.state_seq, "draw_revision": b.map(|b| b.revision).unwrap_or(0), "state": serde_json::from_str::<Value>(&self.last_json).ok(), "room": self.name, "server_ms": now, "battle": b.map(|b| &b.id), "battle_started": b.map(|b| b.started),
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
    pub ready_images: HashSet<String>,
    device_sessions: HashMap<String, (String, i64)>,
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
        let nicks = kv("WT_PLAYER_NICKS");
        Hub {
            cols: HashMap::new(),
            rooms: HashMap::new(),
            dir,
            default_room: std::env::var("WT_LIVE_ROOM").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "main".into()),
            room_map: kv("WT_LIVE_ROOM_MAP"),
            nicks,
            view_key: std::env::var("WT_VIEW_TOKEN").unwrap_or_default(),
            ready_images: HashSet::new(),
            device_sessions: HashMap::new(),
        }
    }

    pub fn room(&mut self, name: &str) -> &mut Room {
        let dir = self.dir.clone();
        self.rooms.entry(name.to_string()).or_insert_with(|| Room::new(name, &dir))
    }

    /// Feed one accepted record of a collector session (or a replay pseudo-collector).
    #[cfg(test)]
    pub fn feed(&mut self, session: &str, player: &str, room: Option<&str>, r: &Record, now: i64) {
        let parsed = r.body.as_deref().and_then(|b| serde_json::from_str::<Value>(b).ok());
        self.feed_parsed(session, player, room, r, now, parsed.as_ref());
    }
    fn feed_parsed(&mut self, session: &str, player: &str, room: Option<&str>, r: &Record, now: i64, parsed: Option<&Value>) {
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
        c.feed_parsed(r, now, parsed);
    }

    #[cfg(test)]
    pub fn feed_source(&mut self, session: &str, player: &str, device: &str, nick: &str, room: &str, r: &Record, now: i64) {
        let parsed = r.body.as_deref().and_then(|b| serde_json::from_str::<Value>(b).ok());
        self.feed_source_parsed(session,player,device,nick,room,r,now,parsed.as_ref());
    }
    pub fn feed_source_parsed(&mut self, session: &str, player: &str, device: &str, nick: &str, room: &str, r: &Record, now: i64, parsed: Option<&Value>) {
        let device = if device.is_empty() { session } else { device };
        let started = r.ts_server_ms.unwrap_or(r.wall_ms) - (r.mono_ns / 1_000_000) as i64;
        if let Some((current, previous_start)) = self.device_sessions.get(device) {
            if current != session {
                if started <= *previous_start { return; }
                let old = current.clone(); self.cols.remove(&old);
            }
        }
        self.device_sessions.entry(device.into()).and_modify(|v| { if v.0 != session { *v = (session.into(), started); } })
            .or_insert_with(|| (session.into(), started));
        self.feed_parsed(session, player, Some(room), r, now, parsed);
        if let Some(c) = self.cols.get_mut(session) {
            c.device_id = device.into();
            if !nick.is_empty() { c.nick = nick.into(); c.markers.own = Some(nick.into()); }
        }
    }

    pub fn new_round(&mut self, name: &str, now: i64) {
        let room = self.room(name);
        // The browser may correct the round between fresh samples. Keep ready static
        // resources; fresh contexts replace them normally. Never carry live units,
        // membership or drawings into the new round.
        let maps = room.battle().map(|b| b.maps.clone()).unwrap_or_default();
        room.new_battle(now).maps = maps;
        self.tick(now);
    }

    pub fn tick(&mut self, now: i64) {
        self.cols.retain(|_, c| now - c.last_recv < COL_EXPIRE);
        self.device_sessions.retain(|_, (session, _)| self.cols.contains_key(session));
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
        // A room is one team in one match. An epoch switch / reconnect cannot reset shared drawings.
        let active: Vec<_> = cols.iter().filter(|c| c.in_battle && now - c.last_recv <= LIVE_MAX_AGE_MS).collect();
        let all_left = !cols.is_empty() && cols.iter().all(|c| c.invalid_since.map(|t| now - t >= 20_000).unwrap_or(false));
        if all_left { room.ended = true; }
        if !active.is_empty() {
            if room.battle().is_none() || room.ended { room.new_battle(now); }
            let b = room.battle_mut().unwrap();
            for c in active { b.members.insert(c.session.clone(), c.epoch); }
        }
        let (members, bstart): (Vec<&LiveCol>, i64) = match room.battle() {
            Some(b) => (cols.iter().filter(|c| c.in_battle && now - c.last_recv <= LIVE_MAX_AGE_MS && b.members.get(&c.session) == Some(&c.epoch)).cloned().collect(), b.started),
            None => (vec![], 0),
        };
        let in_battle = !members.is_empty();
        // ---- maps: latest entry of each kind among members
        let mut best: BTreeMap<&'static str, (&MapEntry, &LiveCol)> = BTreeMap::new();
        for c in &members {
            for m in &c.maps {
                if !m.img.as_ref().map(|h| self.ready_images.contains(h)).unwrap_or(false) { continue; }
                let k = m.info.kind();
                if best.get(k).map(|(e, _)| m.since > e.since || (m.since == e.since && m.img.is_some() && e.img.is_none())).unwrap_or(true) {
                    best.insert(k, (m, c));
                }
            }
        }
        let mut changed = false;
        let mut reset_maps = vec![];
        if let Some(b) = room.battle_mut() {
            if in_battle {
                b.last_active = now;
            }
            for (k, (m, _)) in &best {
                let v = json!({"kind": k, "hash": m.img, "url": m.img.as_ref().map(|h| format!("/api/map/{h}")), "info": m.info.json(), "context": format!("{}|{}", m.img.as_deref().unwrap_or(""), m.sig), "since": m.since});
                if b.maps.get(*k).map(|old| old["context"] != v["context"]).unwrap_or(false) { reset_maps.push(*k); }
                if b.maps.get(*k).map(|old| old["context"] != v["context"] || old["info"] != v["info"]).unwrap_or(true) {
                    b.maps.insert(k.to_string(), v);
                    changed = true;
                }
            }
        }
        room.tracks.retain(|tr| !reset_maps.contains(&tr.map));
        room.dirty |= changed;
        // ---- units
        let mut units: Vec<Value> = Vec::new();
        let mut enemy_obs: Vec<(&'static str, &LUnit, i64, Option<f64>, &str)> = Vec::new();
        let mut selves: Vec<(&'static str, f64, f64)> = Vec::new();
        for c in &members {
            let Some(cm) = c.cur_map() else { continue };
            let k = cm.info.kind();
            let on_room_map = best.get(k).map(|(m, _)| m.sig == cm.sig && m.img.is_some() && m.img == cm.img).unwrap_or(false);
            let age = now - c.frame_t;
            if !on_room_map || age > LIVE_MAX_AGE_MS { continue; }
            if c.me.pos.is_some() && now - c.me.seen_t <= LIVE_MAX_AGE_MS {
                let (x, y) = c.me.pos.unwrap();
                if c.me.kind == k || c.me.dead {
                    selves.push((c.me.kind, x, y));
                    units.push(json!({"id": format!("self:{}", if c.device_id.is_empty() { &c.session } else { &c.device_id }), "side": "self", "map": c.me.kind, "x": r6(x), "y": r6(y), "hdg": c.me.hdg,
                        "icon": c.me.icon, "air": c.me.air, "nick": if c.nick.is_empty() { c.player.clone() } else { c.nick.clone() },
                        "dead": c.me.dead, "age": if c.me.dead { now - c.me.dead_t } else { age }, "by": c.player, "spectating": c.me.dead && c.spectating}));
                }
            }
            if !on_room_map {
                continue;
            }
            let span = cm.info.span();
            for u in &c.units {
                if u.side == "enemy" {
                    let sp = if u.air {
                        None
                    } else {
                        c.spawns.iter().map(|s| (((s.0 - u.x) * span[0]).powi(2) + ((s.1 - u.y) * span[1]).powi(2)).sqrt()).fold(None, |a: Option<f64>, d| Some(a.map_or(d, |a| a.min(d))))
                    };
                    enemy_obs.push((k, u, c.frame_t, sp, &c.session));
                    continue;
                }
                enemy_obs.push((k, u, c.frame_t, None, &c.session));
            }
        }
        // drop friendlies that are another member's own Player (same unit seen by two collectors)
        if selves.len() > 0 && members.len() > 1 {
            units.retain(|u| {
                u["side"] == "self"
                    || !selves.iter().any(|(k, x, y)| u["map"] == *k && best.get(k).map(|(m, _)| (((u["x"].as_f64().unwrap_or(9.0) - x) * m.info.span()[0]).powi(2) + ((u["y"].as_f64().unwrap_or(9.0) - y) * m.info.span()[1]).powi(2)).sqrt() < 8.0).unwrap_or(false))
            });
        }
        // ---- enemy tracks (live + short last-seen ghost)
        enemy_obs.retain(|(k, u, _, _, _)| u.side == "enemy" || selves.iter().filter(|(mk, x, y)| {
            if mk != k { return false; }
            let span = best.get(k).map(|(m, _)| m.info.span()).unwrap_or([4096.0; 2]);
            (((u.x - x) * span[0]).powi(2) + ((u.y - y) * span[1]).powi(2)).sqrt() < if u.air { 50.0 } else { 8.0 }
        }).count() != 1);
        self.track_enemies(room, &enemy_obs, now);
        for tr in &room.tracks {
            let age = now - tr.last;
            units.push(json!({"id": format!("e{}", tr.id), "side": tr.side, "signal": tr.signal, "map": tr.map, "x": r6(tr.x), "y": r6(tr.y), "hdg": tr.hdg, "icon": tr.icon,
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
                let key = format!("{}|{}|{}|{}|{}|{}", e["context"], e["sender"], e["k"], e["msg_id"], e["text"], e["grid"]);
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
                json!({"session": c.session, "device_id": c.device_id, "out_of_order": c.out_of_order, "context_dropped": c.context_dropped, "player": c.player, "nick": c.nick, "replay": c.replay, "in_battle": c.in_battle,
                "member": members.iter().any(|m| m.session == c.session), "age_ms": now - c.last_recv,
                "lag_ms": if c.last_data_ts > 0 { Some(now - c.last_data_ts) } else { None }, "delay_ms": c.delay_p50(), "hz": c.hz(),
                "paused": c.paused.as_ref().filter(|p| now - p.0 < 75_000).map(|p| p.1).unwrap_or(false),
                "map": c.cur_map().map(|m| m.info.kind()), "spectating": c.spectating, "aa_filtered": c.aa_dropped})
            })
            .collect();
        room.state_seq += 1;
        let b = room.battle();
        let marks: BTreeMap<&str, &Value> = best.iter().filter(|(_, (m, _))| !m.marks.is_null()).map(|(k, (m, _))| (*k, &m.marks)).collect();
        let map_age: BTreeMap<_, _> = ["ground", "air"].into_iter().map(|k| (k, members.iter().filter(|c| c.cur_kind() == k).map(|c| now - c.frame_t).min())).collect();
        let state = json!({
            "marks": marks,
            "type": "state", "protocol": 2, "state_seq": room.state_seq, "draw_revision": b.map(|b| b.revision).unwrap_or(0), "map_age": map_age, "room": room.name, "battle": b.map(|b| &b.id), "battle_started": b.map(|b| b.started), "in_battle": in_battle,
            "maps": b.map(|b| &b.maps), "units": units, "pings": pick("ping"), "calls": pick("call"), "feed": pick("feed"), "toasts": pick("toast"),
            "collectors": collectors, "viewers": room.tx.receiver_count(),
        });
        let s = state.to_string();
        if room.tx.receiver_count() > 0 && (s != room.last_json || now - room.last_sent >= 1000) {
            let with_t = format!("{{\"t\":{now},{}", &s[1..]);
            let _ = room.tx.send(Arc::from(with_t.as_str()));
            room.last_sent = now;
        }
        room.last_json = format!("{{\"t\":{now},{}", &s[1..]);
    }

    fn track_enemies(&self, room: &mut Room, obs: &[(&'static str, &LUnit, i64, Option<f64>, &str)], now: i64) {
        let span = |k: &str| room.battle().and_then(|b| b.maps.get(k)).and_then(|v| Some([
            v["info"]["max"][0].as_f64()? - v["info"]["min"][0].as_f64()?,
            v["info"]["max"][1].as_f64()? - v["info"]["min"][1].as_f64()?])).unwrap_or([4096.0; 2]);
        let mut sorted = obs.to_vec(); sorted.sort_by_key(|o| std::cmp::Reverse(o.2));
        let mut merged: Vec<(&'static str, &LUnit, i64, Option<f64>, &str)> = vec![];
        let mut sources: Vec<HashSet<&str>> = vec![];
        for o in sorted {
            let matches: Vec<_> = merged.iter().enumerate().filter(|(i, m)| m.0 == o.0 && m.1.side == o.1.side && m.1.icon == o.1.icon
                && !sources[*i].contains(o.4) && (m.2 - o.2).abs() <= 250
                && (((m.1.x - o.1.x) * span(o.0)[0]).powi(2) + ((m.1.y - o.1.y) * span(o.0)[1]).powi(2)).sqrt() < if o.1.air { 50.0 } else { 8.0 })
                .map(|(i, _)| i).collect();
            if matches.len() == 1 { sources[matches[0]].insert(o.4); }
            else { merged.push(o); sources.push(HashSet::from([o.4])); }
        }
        let mut pairs = vec![];
        for (i, o) in merged.iter().enumerate() {
            for (j, tr) in room.tracks.iter().enumerate() {
                if tr.map != o.0 || tr.icon != o.1.icon || tr.side != o.1.side || o.2 < tr.last { continue; }
                let dt = (o.2 - tr.last) as f64 / 1000.0;
                let gate = if o.1.air { 50.0 + 400.0 * dt } else { 8.0 + 35.0 * dt };
                let d = (((o.1.x - tr.x) * span(o.0)[0]).powi(2) + ((o.1.y - tr.y) * span(o.0)[1]).powi(2)).sqrt();
                if d <= gate { pairs.push((d, i, j)); }
            }
        }
        // Ambiguous associations create separate tracks instead of merging nearby vehicles.
        let ic: Vec<_> = (0..merged.len()).map(|i| pairs.iter().filter(|p| p.1 == i).count()).collect();
        let jc: Vec<_> = (0..room.tracks.len()).map(|j| pairs.iter().filter(|p| p.2 == j).count()).collect();
        let mut matched = HashSet::new();
        let mut kept_tracks = HashSet::new();
        for (_, i, j) in pairs {
            if ic[i] != 1 || jc[j] != 1 { continue; }
            matched.insert(i); kept_tracks.insert(j);
            let (_, u, t, sp, _) = merged[i]; let tr = &mut room.tracks[j];
            let dt = (t - tr.last) as f64 / 1000.0;
            if dt > 0.05 { tr.vx = (u.x - tr.x) / dt; tr.vy = (u.y - tr.y) / dt; }
            tr.x = u.x; tr.y = u.y; tr.hdg = u.hdg.or(tr.hdg); tr.last = t; tr.spawn_m = sp; tr.signal = u.signal;
        }
        // Ambiguous current observations replace possible old representations; they do not multiply every frame.
        let old_ids: HashSet<_> = room.tracks.iter().enumerate().filter(|(j, _)| jc[*j] > 0 && !kept_tracks.contains(j)).map(|(_, tr)| tr.id).collect();
        room.tracks.retain(|tr| !old_ids.contains(&tr.id));
        for (i, (k, u, t, sp, _)) in merged.iter().enumerate() {
            if matched.contains(&i) { continue; }
            room.next_eid += 1;
            room.tracks.push(ETrack { id: room.next_eid, map: k, icon: u.icon.clone(), side: u.side, signal: u.signal,
                air: u.air, x: u.x, y: u.y, vx: 0.0, vy: 0.0, hdg: u.hdg, first: *t, last: *t, spawn_m: *sp });
        }
        room.tracks.retain(|t| now - t.last <= LIVE_MAX_AGE_MS);
    }

    /// Apply a drawing op from a browser. Returns the broadcast message (or an error string for the sender).
    pub fn draw_op(&mut self, room_name: &str, op: &Value, client: &str, now: i64) -> Result<String, String> {
        let room = self.room(room_name);
        let op_id = op["op_id"].as_str().filter(|s| !s.is_empty() && s.len() <= 64).ok_or("upgrade_required")?.to_string();
        if let Some(ack) = room.op_cache.get(&op_id) { return Ok(ack.clone()); }
        if room.battle().is_none() { room.new_battle(now); }
        if op["battle"].as_str() != room.current.as_deref() { return Err("battle_changed".into()); }
        let base = op["base_version"].as_u64().ok_or("upgrade_required")?;
        let bid = room.current.clone().unwrap_or_default();
        let b = room.battle_mut().unwrap();
        let id = op["id"].as_str().or(op["d"]["id"].as_str()).unwrap_or("").to_string();
        if id.is_empty() || id.len() > 64 { return Err("bad drawing id".into()); }
        let version = b.drawings.get(&id).and_then(|d| d["sv"].as_u64()).or_else(|| b.tombstones.get(&id).copied()).unwrap_or(0);
        if base != version { return Err("version_conflict".into()); }
        let mut msg = match op["op"].as_str() {
            Some("put") => {
                let mut d = op["d"].clone();
                let pts = d["pts"].as_array().ok_or("bad pts")?;
                if pts.is_empty() || pts.len() > 400 || pts.iter().any(|p| p.as_array().map(|a| a.len() != 2 || a.iter().any(|v| v.as_f64().map(|n| !n.is_finite() || n.abs() > 1e7).unwrap_or(true))).unwrap_or(true)) { return Err("bad pts".into()); }
                if !["point", "route", "arrow", "phase", "area", "circle", "source"].contains(&d["kind"].as_str().unwrap_or("")) { return Err("bad kind".into()); }
                let map = d["scope"].as_str().filter(|s| *s == "ground" || *s == "air").ok_or("bad scope")?;
                if d["map_context"] != b.maps.get(map).map(|m| &m["context"]).cloned().unwrap_or(Value::Null) { return Err("map_changed".into()); }
                if d.to_string().len() > MAX_DRAWING_BYTES || d["label"].as_str().map(|s| s.chars().count() > 512).unwrap_or(false) { return Err("drawing too large".into()); }
                if !b.drawings.contains_key(&id) && b.drawings.len() >= MAX_DRAWINGS { return Err("too many drawings".into()); }
                if !b.drawings.contains_key(&id) && !b.tombstones.contains_key(&id) && b.drawings.len() + b.tombstones.len() >= 4096 { return Err("drawing capacity reached; start a new round".into()); }
                let created = !b.drawings.contains_key(&id);
                b.revision += 1; d["sv"] = json!(b.revision); b.tombstones.remove(&id); b.drawings.insert(id.clone(), d.clone());
                json!({"type": "draw", "op": "put", "battle": bid, "d": d, "by_client": client, "created": created})
            }
            Some("del") => {
                let old = b.drawings.remove(&id).ok_or("not found")?;
                b.revision += 1; b.tombstones.insert(id.clone(), b.revision);
                json!({"type": "draw", "op": "del", "battle": bid, "id": id, "old": old, "by": op["by"], "by_client": client})
            }
            _ => return Err("bad op".into()),
        };
        msg["draw_revision"] = json!(b.revision); msg["op_id"] = json!(op_id);
        room.dirty = true;
        let ack = msg.to_string();
        room.op_cache.insert(op_id.clone(), ack.clone()); room.op_order.push_back(op_id);
        while room.op_order.len() > 512 { if let Some(id) = room.op_order.pop_front() { room.op_cache.remove(&id); } }
        let _ = room.tx.send(Arc::from(ack.as_str()));
        Ok(ack)
    }

    pub fn mark_dirty(&mut self) { for room in self.rooms.values_mut() { room.dirty = true; } }

    pub fn take_persistence(&mut self) -> Vec<(PathBuf, Vec<u8>)> {
        let mut out = vec![];
        for r in self.rooms.values_mut().filter(|r| r.dirty) {
            let f = RoomFile { current: r.current.clone(), battles: r.battle().cloned().into_iter().collect() };
            if let Ok(bytes) = serde_json::to_vec(&f) { out.push((self.dir.join(format!("room-{}.json", r.name)), bytes)); r.dirty = false; }
        }
        out
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
        let mut h = Hub { cols: HashMap::new(), rooms: HashMap::new(), dir: std::env::temp_dir().join("wt-live-test"), default_room: "t".into(), room_map: HashMap::new(), nicks: [("p".to_string(), "Me".to_string())].into(), view_key: String::new(), ready_images: ["testhash".into()].into(), device_sessions: HashMap::new() };
        let _ = std::fs::create_dir_all(&h.dir);
        h.feed("s1", "p", None, &rec(1, "/map_info.json", now - 500, MI), now);
        let me = |c: &str, x: f64| format!(r##"[{{"type":"ground_model","icon":"Player","color":"{c}","blink":0,"x":{x},"y":0.5,"dx":1.0,"dy":0.0}},{{"type":"ground_model","icon":"HeavyTank","color":"#fa0C00","blink":0,"x":0.3,"y":0.3}},{{"type":"respawn_base_tank","icon":"respawn_base_tank","color":"#174DFF","blink":0,"x":0.31,"y":0.31}},{{"type":"respawn_base_tank","icon":"respawn_base_tank","color":"#174DFF","blink":0,"x":0.32,"y":0.31}},{{"type":"respawn_base_tank","icon":"respawn_base_tank","color":"#fa0C00","blink":0,"x":0.8,"y":0.8}},{{"type":"airfield","color":"#174DFF","blink":0,"icon":"none","sx":0.9,"sy":0.1,"ex":0.95,"ey":0.1}},{{"type":"ground_model","icon":"Airdefence","color":"#174DFF","blink":0,"x":0.93,"y":0.12}},{{"type":"respawn_base_fighter","icon":"respawn_base_fighter","color":"#174DFF","blink":0,"x":-2.5,"y":0.5}}]"##);
        h.feed("s1", "p", None, &rec(2, "/map_obj.json", now - 400, &me("#faC81E", 0.5)), now);
        h.feed("s1", "p", None, &rec(3, "/hudmsg", now - 390, r#"{"events":[],"damage":[{"id":9,"msg":"X (T-34) 击毁了 Me (Tiger)"}]}"#), now);
        h.feed("s1", "p", None, &rec(4, "/map_obj.json", now - 380, &me("#faC81E", 0.6)), now);
        let c = &h.cols["s1"];
        assert!(c.me.dead && c.me.pos == Some((0.5, 0.5)));
        h.feed("s1", "p", None, &rec(5, "/map_obj.json", now - 300, &me("#174DFF", 0.7)), now); // spectating teammate
        assert!(h.cols["s1"].me.dead && h.cols["s1"].spectating);
        h.feed("s1", "p", None, &rec(6, "/map_obj.json", now - 200, &me("#faC81E", 0.2)), now); // respawn
        assert!(!h.cols["s1"].me.dead && h.cols["s1"].me.pos == Some((0.2, 0.5)));
        let mut image = rec(7, "/map.img", now - 100, ""); image.img_hash = Some("testhash".into()); h.feed("s1", "p", None, &image, now);
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
        let battle = h.room("t").current.clone().unwrap();
        let context = s["maps"]["ground"]["context"].clone();
        let r = h.draw_op("t", &json!({"op": "put", "op_id":"add1", "battle":battle, "base_version":0, "d": {"id": "a1", "kind": "point", "scope":"ground", "map_context": context, "pts": [[1, 2]]}}), "c1", now).unwrap();
        assert!(r.contains("\"created\":true"));
        assert!(h.room("t").hello(now).contains("a1"));
        assert!(h.draw_op("t", &json!({"op": "del", "op_id":"delete1", "battle":battle, "base_version":1, "id": "a1"}), "c1", now).is_ok());
    }
    fn test_hub() -> Hub {
        Hub { cols: HashMap::new(), rooms: HashMap::new(), dir: std::env::temp_dir().join(format!("wt-live-{:x}", rand::random::<u64>())), default_room: "t".into(), room_map: HashMap::new(), nicks: HashMap::new(), view_key: String::new(), ready_images: ["testhash".into()].into(), device_sessions: HashMap::new() }
    }
    fn source(h: &mut Hub, session: &str, device: &str, now: i64, mono_ms: u64, x: f64) {
        let mut mi = rec(mono_ms * 10 + 1, "/map_info.json", now, MI); mi.mono_ns = mono_ms * 1_000_000; mi.map_context = Some("map-a".into());
        h.feed_source(session, "p", device, device, "t", &mi, now);
        let mut image = mi.clone(); image.seq += 1; image.ep = "/map.img".into(); image.img_hash = Some("testhash".into()); h.feed_source(session,"p",device,device,"t",&image,now);
        let mut unit = image.clone(); unit.seq += 1; unit.ep = "/map_obj.json".into(); unit.body = Some(format!(r##"[{{"type":"ground_model","icon":"Player","color":"#faC81E","x":{x},"y":0.5}}]"##));
        h.feed_source(session,"p",device,device,"t",&unit,now);
    }
    #[test]
    fn latest_watermark_context_and_expiry() {
        let mut h = test_hub(); let now = 100_000;
        source(&mut h,"s","device",now,1000,0.5); h.tick(now);
        let original = h.cols["s"].me.pos;
        let mut old = rec(2,"/map_obj.json",now-500,r##"[{"type":"ground_model","icon":"Player","color":"#faC81E","x":0.1,"y":0.5}]"##); old.mono_ns = 500_000_000; old.map_context = Some("map-a".into());
        h.feed_source("s","p","device","device","t",&old,now); assert_eq!(h.cols["s"].me.pos,original); assert_eq!(h.cols["s"].out_of_order,1);
        let mut wrong = old.clone(); wrong.seq=99999;wrong.mono_ns=2000_000_000;wrong.map_context=Some("map-b".into()); h.feed_source("s","p","device","device","t",&wrong,now); assert_eq!(h.cols["s"].context_dropped,1); assert_eq!(h.cols["s"].me.pos,original);
        h.tick(now+2001); let state:Value=serde_json::from_str(&h.room("t").last_json).unwrap();assert_eq!(state["units"].as_array().unwrap().len(),0); assert!(state["maps"]["ground"].is_object(),"static map survives expiry");
        old.wall_ms=now-5000;old.ts_server_ms=Some(now-5000);old.mono_ns=9999_000_000;h.feed_source("s","p","device","device","t",&old,now+2001);h.tick(now+2001);let state:Value=serde_json::from_str(&h.room("t").last_json).unwrap();assert!(state["units"].as_array().unwrap().is_empty());
    }
    #[test]
    fn device_restart_and_transient_dropout_keep_battle() {
        let mut h=test_hub();source(&mut h,"old","device",100_000,1000,0.5);h.tick(100_000);let battle=h.room("t").current.clone();
        source(&mut h,"new","device",100_100,1,0.6);h.tick(100_100);assert!(!h.cols.contains_key("old"));assert_eq!(h.room("t").current,battle);
        source(&mut h,"old","device",100_200,1200,0.1);h.tick(100_200);assert_eq!(h.cols.len(),1);assert_eq!(h.cols["new"].me.pos,Some((0.6,0.5)));
        h.tick(110_000);assert_eq!(h.room("t").current,battle,"network silence is not a confirmed exit");
        source(&mut h,"new","device",110_100,10001,0.7);h.tick(110_100);assert_eq!(h.room("t").current,battle);
        let mut invalid=rec(999999,"/map_info.json",110_200,r#"{"valid":false}"#);invalid.mono_ns=10101_000_000;h.feed_source("new","p","device","device","t",&invalid,110_200);
        h.tick(130_199);assert!(!h.room("t").ended);h.tick(130_201);assert!(h.room("t").ended);
        source(&mut h,"new","device",130_300,30201,0.8);h.tick(130_300);assert_ne!(h.room("t").current,battle);
    }
    #[test]
    fn drawing_conflicts_idempotence_tombstones_and_round() {
        let mut h=test_hub();h.new_round("t",100_000);let battle=h.room("t").current.clone().unwrap();
        let mut op=json!({"op":"put","op_id":"create","battle":battle,"base_version":0,"d":{"id":"draw","kind":"point","scope":"ground","pts":[[100,200]],"label":"A"}});
        let ack=h.draw_op("t",&op,"A",100_001).unwrap();assert_eq!(h.draw_op("t",&op,"A",100_002).unwrap(),ack);assert_eq!(h.room("t").battle().unwrap().revision,1);
        op["op_id"]=json!("edit-a");op["base_version"]=json!(1);op["d"]["label"]=json!("A edit");h.draw_op("t",&op,"A",100_003).unwrap();
        op["op_id"]=json!("edit-b");op["d"]["label"]=json!("B edit");assert_eq!(h.draw_op("t",&op,"B",100_004).unwrap_err(),"version_conflict");
        let del=json!({"op":"del","op_id":"delete","battle":battle,"base_version":2,"id":"draw"});h.draw_op("t",&del,"B",100_005).unwrap();
        op["op_id"]=json!("stale-undo");op["base_version"]=json!(2);assert_eq!(h.draw_op("t",&op,"A",100_006).unwrap_err(),"version_conflict");
        op["op_id"]=json!("undo-delete");op["base_version"]=json!(3);h.draw_op("t",&op,"B",100_007).unwrap();assert_eq!(h.room("t").battle().unwrap().revision,4);
        op["op_id"]=json!("bad-map");op["base_version"]=json!(4);op["d"]["map_context"]=json!("other");assert_eq!(h.draw_op("t",&op,"A",100_008).unwrap_err(),"map_changed");
        h.new_round("t",100_000);assert_ne!(h.room("t").current.as_deref(),Some(battle.as_str()),"manual round ids cannot collide within a millisecond");assert_eq!(h.draw_op("t",&op,"A",100_009).unwrap_err(),"battle_changed");
        assert_eq!(h.draw_op("t",&json!({"op":"put"}),"old-browser",100_010).unwrap_err(),"upgrade_required");
    }
    #[test]
    fn manual_round_keeps_ready_static_map_without_reviving_positions() {
        let mut h = test_hub(); source(&mut h, "s", "device", 100_000, 1000, 0.5); h.tick(100_000);
        let before: Value = serde_json::from_str(&h.room("t").last_json).unwrap();
        assert!(!before["units"].as_array().unwrap().is_empty());
        // No fresh collector sample at the time the browser starts another round.
        h.new_round("t", 105_000);
        let after: Value = serde_json::from_str(&h.room("t").last_json).unwrap();
        assert_ne!(before["battle"], after["battle"]);
        assert_eq!(before["maps"], after["maps"], "ready static map remains available without another image upload");
        assert!(after["units"].as_array().unwrap().is_empty());
        assert_eq!(after["in_battle"], false);
        assert_eq!(after["map_age"]["ground"], Value::Null);
        // A fresh sample rejoins the manual round and uses the already cached image.
        source(&mut h, "s", "device", 105_100, 6100, 0.6); h.tick(105_100);
        let fresh: Value = serde_json::from_str(&h.room("t").last_json).unwrap();
        assert_eq!(after["battle"], fresh["battle"]);
        assert_eq!(fresh["units"][0]["x"], 0.6);
        assert_eq!(fresh["maps"]["ground"]["hash"], "testhash");
    }
    #[test]
    fn spectated_player_does_not_invent_vehicle_class() {
        let mut h = test_hub(); let now = 150_000;
        source(&mut h, "s", "device", now, 1000, 0.5);
        let mut r = rec(10_004, "/map_obj.json", now, r##"[{"type":"ground_model","icon":"Player","color":"#174DFF","x":0.4,"y":0.4}]"##);
        r.mono_ns = 1000_000_000; r.map_context = Some("map-a".into());
        h.feed_source("s", "p", "device", "测试", "t", &r, now);
        assert_eq!(h.cols["s"].units[0].icon, "Unknown");
    }

    #[test]
    fn anonymous_dense_units_are_one_to_one_without_accumulation() {
        let h=test_hub();let mut room=Room::new("t",&h.dir);room.new_battle(100_000);
        room.battle_mut().unwrap().maps.insert("ground".into(),json!({"info":{"min":[0,0],"max":[4096,4096]}}));
        let unit=|x,side,icon:&str|LUnit{side,air:false,x,y:0.5,hdg:None,icon:icon.into(),signal:false};
        let a=unit(0.5,"enemy","MediumTank");let b=unit(0.5001,"enemy","MediumTank");let c=unit(0.8,"ally","MediumTank");
        for t in (100_000..102_000).step_by(100) { h.track_enemies(&mut room,&[("ground",&a,t,None,"one"),("ground",&b,t,None,"one"),("ground",&c,t,None,"two")],t);assert_eq!(room.tracks.len(),3,"ambiguous vehicles stay distinct without frame duplicates"); }
        h.track_enemies(&mut room,&[],104_001);assert!(room.tracks.is_empty());
        h.track_enemies(&mut room,&[("ground",&a,105_000,None,"one"),("ground",&a,105_000,None,"two")],105_000);assert_eq!(room.tracks.len(),1,"unambiguous cross-source duplicates fuse");
        let ally=unit(0.5,"ally","MediumTank");let heavy=unit(0.5,"enemy","HeavyTank");h.track_enemies(&mut room,&[("ground",&a,105_100,None,"one"),("ground",&ally,105_100,None,"two"),("ground",&heavy,105_100,None,"three")],105_100);assert_eq!(room.tracks.len(),3,"allegiance and category are independent");
    }

}
