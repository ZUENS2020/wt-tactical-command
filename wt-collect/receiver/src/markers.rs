//! Live squad / map-ping / radio-call derivation (port of wt-analysis/allies/{tracker,pings}.py).
//! Fed with every stored record of a session (after the naval filter); raw samples are never modified.
//! Output: events appended to `<session>/markers.jsonl` + a live view for /api/status and /api/markers.
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use wtc_common::mapobj::{classify, Kind, Side};
use wtc_common::radio::{parse_chat, GridInfo, RadioMsg, OWN_NAMES};
use wtc_common::Record;

const MATCH_BEFORE_MS: i64 = 1500;
const MATCH_AFTER_MS: i64 = 1500;
const LOST_MS: i64 = 1500;
const GATE_BASE_M: f64 = 30.0;
pub const PING_TTL_MS: i64 = 30_000;
const RECENT_EVENTS: usize = 200;

#[derive(Debug, Clone)]
struct Track {
    id: u64,
    ty: String,
    icon: String,
    is_self: bool,
    first_seen: i64,
    last_seen: i64,
    x: f64,
    y: f64,
    squad_votes: u32,
    ally_votes: u32,
    fresh_ms: i64,
    spectated: bool,
    sig: Option<u64>,
    squad_announced: bool,
    signal_now: bool,
}

impl Track {
    fn role(&self) -> &'static str {
        if self.is_self {
            "self"
        } else if self.squad_votes > 0 {
            "squad"
        } else if self.ally_votes > 0 {
            "ally"
        } else {
            "unknown"
        }
    }
}

#[derive(Debug, Clone)]
struct Sig {
    id: u64,
    track: u64,
    role: &'static str,
    icon: String,
    t_start: i64,
    t_end: i64,
    x: f64,
    y: f64,
    open: bool,
    claimed_by: Option<String>,
    msgs: Vec<i64>,
    finalized: bool,
}

#[derive(Default)]
pub struct Markers {
    grid: Option<GridInfo>,
    span: Option<[f64; 2]>,
    tracks: Vec<Track>,
    next_track: u64,
    sigs: VecDeque<Sig>,
    next_sig: u64,
    pending: Vec<(i64, RadioMsg, Option<GridInfo>)>,
    chat_seen: HashSet<i64>,
    chat_epochs: HashSet<u32>,
    objs: HashMap<String, (i64, i64, Value)>, // map-marker objects: key -> (first, last, obj)
    pub squad_names: BTreeMap<String, u32>,
    last_t: i64,
    /// events not yet written to markers.jsonl
    pub out: Vec<Value>,
    pub recent: VecDeque<Value>,
    pub counts: BTreeMap<String, u64>,
    /// own in-game nickname (overrides radio::OWN_NAMES for own-message detection)
    pub own: Option<String>,
}

fn vmax(ty: &str) -> f64 {
    if ty == "aircraft" {
        450.0
    } else {
        35.0
    }
}

impl Markers {
    fn emit(&mut self, v: Value) {
        let k = v["ev"].as_str().unwrap_or("?").to_string();
        *self.counts.entry(k).or_default() += 1;
        self.recent.push_back(v.clone());
        while self.recent.len() > RECENT_EVENTS {
            self.recent.pop_front();
        }
        self.out.push(v);
    }

    fn dist_m(&self, a: (f64, f64), b: (f64, f64)) -> f64 {
        match self.span {
            Some(s) => (((a.0 - b.0) * s[0]).powi(2) + ((a.1 - b.1) * s[1]).powi(2)).sqrt(),
            None => ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt() / 0.03 * GATE_BASE_M,
        }
    }

    /// Feed one stored record (in seq order).
    pub fn feed(&mut self, r: &Record) {
        if r.status != 200 || r.err.is_some() || r.dropped.is_some() {
            return;
        }
        let t = r.ts_server_ms.unwrap_or(r.wall_ms);
        if t < self.last_t - 2000 {
            return; // backlog / out of order: live derivation only
        }
        let Some(b) = r.body.as_deref() else { return };
        match r.ep.as_str() {
            "/map_info.json" => {
                if let Ok(v) = serde_json::from_str::<Value>(b) {
                    if let Some(g) = GridInfo::from_map_info(&v) {
                        self.span = Some([g.map_max[0] - g.map_min[0], g.map_max[1] - g.map_min[1]]);
                        self.grid = Some(g);
                    }
                }
            }
            "/map_obj.json" => {
                if let Ok(Value::Array(a)) = serde_json::from_str::<Value>(b) {
                    self.last_t = self.last_t.max(t);
                    self.frame(t, &a);
                    self.flush(t, false);
                }
            }
            "/gamechat" => {
                if let Ok(Value::Array(a)) = serde_json::from_str::<Value>(b) {
                    let first = self.chat_epochs.insert(r.epoch);
                    let backlog = first && r.url.contains("lastId=0");
                    for m in &a {
                        let id = m["id"].as_i64().unwrap_or(-1);
                        if !self.chat_seen.insert(id) {
                            continue;
                        }
                        let msg = match &self.own {
                            Some(n) => parse_chat(m, &[n.as_str()]),
                            None => parse_chat(m, OWN_NAMES),
                        };
                        if backlog || msg.enemy {
                            *self.counts.entry("chat_backlog_or_enemy".into()).or_default() += 1;
                            continue;
                        }
                        self.pending.push((t, msg, self.grid));
                    }
                    self.last_t = self.last_t.max(t);
                    self.flush(t, false);
                }
            }
            _ => {}
        }
    }

    fn frame(&mut self, t: i64, a: &[Value]) {
        // exact map markers (point_of_interest / unknown types): never units
        let mut seen = HashSet::new();
        for o in a {
            let c = classify(o);
            if c.kind != Kind::MapMarker {
                continue;
            }
            let (Some(x), Some(y)) = (o["x"].as_f64().or(o["sx"].as_f64()), o["y"].as_f64().or(o["sy"].as_f64())) else { continue };
            let key = format!("{}|{}|{:.4}|{:.4}", o["type"].as_str().unwrap_or(""), o["icon"].as_str().unwrap_or(""), x, y);
            seen.insert(key.clone());
            match self.objs.get_mut(&key) {
                Some(e) => e.1 = t,
                None => {
                    self.objs.insert(key.clone(), (t, t, o.clone()));
                    self.emit(json!({"ev": "map_marker", "state": "appear", "t": t, "key": key, "type": o["type"], "icon": o["icon"],
                        "color": o["color"], "blink": o["blink"], "x": x, "y": y, "known_type": c.known_type,
                        "cell": self.grid.and_then(|g| g.cell_of(x, y))}));
                }
            }
        }
        let gone: Vec<String> = self.objs.keys().filter(|k| !seen.contains(*k)).cloned().collect();
        for k in gone {
            if let Some((f, l, o)) = self.objs.remove(&k) {
                self.emit(json!({"ev": "map_marker", "state": "gone", "t": l, "key": k, "type": o["type"], "icon": o["icon"], "first_seen": f, "last_seen": l, "lifetime_ms": l - f}));
            }
        }
        // friendly units
        let units: Vec<(&Value, wtc_common::mapobj::ObjClass, f64, f64)> = a
            .iter()
            .filter_map(|o| {
                let c = classify(o);
                if !(c.is_unit() && c.side.friendly()) {
                    return None;
                }
                Some((o, c, o["x"].as_f64()?, o["y"].as_f64()?))
            })
            .collect();
        let mut pairs = Vec::new();
        for (i, (o, c, x, y)) in units.iter().enumerate() {
            let is_self = c.side == Side::Own;
            let (ty, icon) = (o["type"].as_str().unwrap_or(""), o["icon"].as_str().unwrap_or(""));
            for (j, tr) in self.tracks.iter().enumerate() {
                if tr.ty != ty || tr.icon != icon || tr.is_self != is_self {
                    continue;
                }
                let d = self.dist_m((*x, *y), (tr.x, tr.y));
                if d <= GATE_BASE_M + vmax(ty) * ((t - tr.last_seen).max(0) as f64 / 1000.0) {
                    pairs.push((d, i, j));
                }
            }
        }
        pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let (mut ui, mut tj) = (HashSet::new(), HashSet::new());
        let mut assign: HashMap<usize, usize> = HashMap::new();
        for (_, i, j) in pairs {
            if ui.contains(&i) || tj.contains(&j) {
                continue;
            }
            ui.insert(i);
            tj.insert(j);
            assign.insert(i, j);
        }
        let mut new_tracks = Vec::new();
        let mut updated = Vec::new();
        for (i, (o, c, x, y)) in units.iter().enumerate() {
            let mut tr = match assign.get(&i) {
                Some(&j) => self.tracks[j].clone(),
                None => {
                    self.next_track += 1;
                    Track {
                        id: self.next_track,
                        ty: o["type"].as_str().unwrap_or("").into(),
                        icon: o["icon"].as_str().unwrap_or("").into(),
                        is_self: c.side == Side::Own,
                        first_seen: t,
                        last_seen: t,
                        x: *x,
                        y: *y,
                        squad_votes: 0,
                        ally_votes: 0,
                        fresh_ms: 0,
                        spectated: false,
                        sig: None,
                        squad_announced: false,
                        signal_now: false,
                    }
                }
            };
            let dt = t - tr.last_seen;
            tr.last_seen = t;
            tr.x = *x;
            tr.y = *y;
            tr.spectated |= c.spectated;
            tr.signal_now = c.signal;
            if c.fresh_spawn {
                tr.fresh_ms += dt;
            } else if !c.signal {
                match c.side {
                    Side::Squad => tr.squad_votes += 1,
                    Side::Ally => tr.ally_votes += 1,
                    _ => {}
                }
            }
            let role = tr.role();
            if role == "squad" && !tr.squad_announced {
                tr.squad_announced = true;
                self.emit(json!({"ev": "squad", "state": "seen", "t": t, "track": tr.id, "type": tr.ty, "icon": tr.icon, "x": x, "y": y,
                    "cell": self.grid.and_then(|g| g.cell_of(*x, *y))}));
            }
            // signal state machine
            match (c.signal, tr.sig) {
                (true, None) => {
                    self.next_sig += 1;
                    let s = Sig { id: self.next_sig, track: tr.id, role, icon: tr.icon.clone(), t_start: t, t_end: t, x: *x, y: *y, open: true, claimed_by: None, msgs: vec![], finalized: false };
                    tr.sig = Some(s.id);
                    self.emit(json!({"ev": "signal_start", "t": t, "sig": s.id, "track": tr.id, "role": role, "type": tr.ty, "icon": tr.icon, "x": x, "y": y,
                        "cell": self.grid.and_then(|g| g.cell_of(*x, *y))}));
                    self.sigs.push_back(s);
                    while self.sigs.len() > 64 {
                        self.sigs.pop_front();
                    }
                }
                (true, Some(id)) => {
                    if let Some(s) = self.sigs.iter_mut().find(|s| s.id == id) {
                        s.t_end = t;
                    }
                }
                (false, Some(id)) => {
                    tr.sig = None;
                    self.close_sig(id, false, role);
                }
                (false, None) => {
                    if role != "unknown" {
                        for s in self.sigs.iter_mut().filter(|s| s.track == tr.id && s.role == "unknown") {
                            s.role = role;
                        }
                    }
                }
            }
            if assign.contains_key(&i) {
                updated.push(tr);
            } else {
                new_tracks.push(tr);
            }
        }
        // carry over unmatched tracks for LOST_MS, then close
        let matched: HashSet<usize> = assign.values().cloned().collect();
        let old = std::mem::take(&mut self.tracks);
        let mut keep = Vec::new();
        for (j, tr) in old.into_iter().enumerate() {
            if matched.contains(&j) {
                continue;
            }
            if t - tr.last_seen <= LOST_MS {
                keep.push(tr);
            } else {
                if let Some(id) = tr.sig {
                    self.close_sig(id, true, tr.role());
                }
                if tr.role() == "squad" {
                    self.emit(json!({"ev": "squad", "state": "lost", "t": tr.last_seen, "track": tr.id, "type": tr.ty, "icon": tr.icon, "x": tr.x, "y": tr.y,
                        "first_seen": tr.first_seen, "visible_ms": tr.last_seen - tr.first_seen}));
                }
            }
        }
        keep.extend(updated);
        keep.extend(new_tracks);
        self.tracks = keep;
    }

    fn close_sig(&mut self, id: u64, lost: bool, role: &'static str) {
        let Some(s) = self.sigs.iter_mut().find(|s| s.id == id) else { return };
        if !s.open {
            return;
        }
        s.open = false;
        if s.role == "unknown" {
            s.role = role;
        }
        let v = json!({"ev": "signal_end", "t": s.t_end, "sig": s.id, "track": s.track, "role": s.role, "icon": s.icon,
            "t_start": s.t_start, "t_end": s.t_end, "dur_ms": s.t_end - s.t_start, "lost": lost, "msg_ids": s.msgs});
        self.emit(v);
    }

    fn match_msg(&mut self, t_msg: i64, m: &RadioMsg, g: Option<GridInfo>) -> Option<usize> {
        let mut best: Option<(i64, usize)> = None;
        for (i, s) in self.sigs.iter().enumerate() {
            if (s.role == "self") != m.own {
                continue;
            }
            let dt = s.t_start - t_msg;
            let mut score = if s.claimed_by.is_none() && (-MATCH_BEFORE_MS..=MATCH_AFTER_MS).contains(&dt) {
                dt.abs()
            } else if s.claimed_by.as_deref() == Some(m.sender.as_str()) && s.t_start - MATCH_BEFORE_MS <= t_msg && (s.open || t_msg <= s.t_end + 700) {
                dt.abs() + 1000 // same sender, one blink covers several messages
            } else {
                continue;
            };
            if m.subject == Some("sender") {
                if let (Some(gi), Some(gr)) = (g, m.grid.as_deref()) {
                    if gi.cell_of(s.x, s.y).as_deref() != Some(gr) {
                        score += 5000;
                    }
                }
            }
            if best.map(|b| score < b.0).unwrap_or(true) {
                best = Some((score, i));
            }
        }
        best.map(|b| b.1)
    }

    /// Emit ping/location/radio events whose match window is over, and unclaimed signals.
    pub fn flush(&mut self, now: i64, all: bool) {
        let pend = std::mem::take(&mut self.pending);
        for (t_msg, m, g) in pend {
            if !all && now < t_msg + MATCH_AFTER_MS {
                self.pending.push((t_msg, m, g));
                continue;
            }
            let si = self.match_msg(t_msg, &m, g);
            let sig = si.map(|i| {
                let s = &mut self.sigs[i];
                if s.claimed_by.is_none() {
                    s.claimed_by = Some(m.sender.clone());
                }
                s.msgs.push(m.id);
                s.clone()
            });
            let cat = m.category();
            let area = match (g, m.grid.as_deref()) {
                (Some(gi), Some(gr)) => gi.cell_bounds(gr),
                _ => None,
            };
            let center = area.map(|a| ((a[0] + a[2]) / 2.0, (a[1] + a[3]) / 2.0));
            let (xy, precision) = if cat == "ping" {
                (center, if center.is_some() { "grid" } else { "none" })
            } else if let Some(s) = &sig {
                (Some((s.x, s.y)), "sender_pos")
            } else {
                (center, if center.is_some() { "grid" } else { "none" })
            };
            let role = sig.as_ref().map(|s| s.role).unwrap_or(if m.own { "self" } else { "unknown" });
            if role == "squad" {
                *self.squad_names.entry(m.sender.clone()).or_default() += 1;
            }
            if cat == "ping" {
                // pings are uniform: who placed it (self / squad / ally) is optional metadata only, never branched on
                let v = json!({"ev": "ping", "id": format!("p{}", m.id), "t": t_msg, "t_end": t_msg + PING_TTL_MS, "ttl_ms": PING_TTL_MS,
                    "grid": m.grid, "area": area, "x": xy.map(|p| p.0), "y": xy.map(|p| p.1), "precision": precision,
                    "kind": m.kind, "text": m.text, "source": if sig.is_some() { "chat+map" } else { "chat" },
                    "meta": {"msg_id": m.id, "sender": m.sender, "own": m.own, "sender_role": role, "sender_track": sig.as_ref().map(|s| s.track),
                        "sig": sig.as_ref().map(|s| s.id), "sender_xy": sig.as_ref().map(|s| [s.x, s.y]), "sender_icon": sig.as_ref().map(|s| s.icon.clone()),
                        "sender_cell": sig.as_ref().and_then(|s| g.and_then(|gi| gi.cell_of(s.x, s.y))), "grid_color": m.grid_color, "mode": m.mode}});
                self.emit(v);
                continue;
            }
            let v = json!({"ev": cat, "t": t_msg, "msg_id": m.id, "kind": m.kind, "sender": m.sender, "own": m.own,
                "sender_role": role, "sender_track": sig.as_ref().map(|s| s.track), "sig": sig.as_ref().map(|s| s.id),
                "sender_xy": sig.as_ref().map(|s| [s.x, s.y]), "sender_icon": sig.as_ref().map(|s| s.icon.clone()),
                "sender_cell": sig.as_ref().and_then(|s| g.and_then(|gi| gi.cell_of(s.x, s.y))),
                "grid": m.grid, "area": area, "x": xy.map(|p| p.0), "y": xy.map(|p| p.1), "precision": precision,
                "ttl_ms": if cat == "ping" { PING_TTL_MS } else { 0 }, "source": if sig.is_some() { "chat+map" } else { "chat" },
                "text": m.text, "alt_m": m.alt_m, "grid_color": m.grid_color, "mode": m.mode});
            self.emit(v);
        }
        // signals nobody claimed
        let mut lone = Vec::new();
        for s in self.sigs.iter_mut() {
            if s.finalized || s.claimed_by.is_some() {
                continue;
            }
            if all || now >= s.t_start + MATCH_BEFORE_MS + MATCH_AFTER_MS {
                s.finalized = true;
                lone.push(s.clone());
            }
        }
        for s in lone {
            let cell = self.grid.and_then(|g| g.cell_of(s.x, s.y));
            self.emit(json!({"ev": "signal_only", "t": s.t_start, "sig": s.id, "track": s.track, "role": s.role, "icon": s.icon,
                "x": s.x, "y": s.y, "cell": cell, "source": "map"}));
        }
    }

    /// Role of the friendly track sitting exactly at (x, y) in the last frame (self / squad / ally / unknown).
    pub fn role_at(&self, x: f64, y: f64, icon: &str) -> Option<&'static str> {
        self.tracks.iter().find(|t| t.icon == icon && (t.x - x).abs() < 1e-9 && (t.y - y).abs() < 1e-9).map(|t| t.role())
    }

    /// Live view for /api/status: current friendly units by role + pings of the last PING_TTL_MS.
    pub fn live(&self, now_t: i64) -> Value {
        let units: Vec<Value> = self
            .tracks
            .iter()
            .filter(|t| self.last_t - t.last_seen <= LOST_MS)
            .filter(|t| t.role() != "ally" || t.signal_now)
            .map(|t| json!({"track": t.id, "role": t.role(), "type": t.ty, "icon": t.icon, "x": t.x, "y": t.y, "signal": t.signal_now, "spectated": t.spectated}))
            .collect();
        let horizon = now_t.max(self.last_t) - PING_TTL_MS;
        // all pings look the same on the map (no owner/role in the live view)
        let pings: Vec<Value> = self
            .recent
            .iter()
            .filter(|v| v["ev"] == "ping" && v["t_end"].as_i64().unwrap_or(0) >= now_t.max(self.last_t))
            .map(|v| json!({"id": v["id"], "t": v["t"], "t_end": v["t_end"], "ttl_ms": v["ttl_ms"], "grid": v["grid"], "area": v["area"],
                "x": v["x"], "y": v["y"], "precision": v["precision"], "kind": v["kind"]}))
            .collect();
        let calls: Vec<&Value> = self.recent.iter().filter(|v| matches!(v["ev"].as_str(), Some("location") | Some("map_marker")) && v["t"].as_i64().unwrap_or(0) >= horizon).collect();
        json!({"squad_and_signalling": units, "recent_pings": pings, "recent_calls": calls, "allies_tracked": self.tracks.iter().filter(|t| t.role() == "ally").count(),
            "squad_names": self.squad_names, "counts": self.counts, "spectating": self.tracks.iter().any(|t| t.spectated && self.last_t - t.last_seen <= LOST_MS)})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rec(seq: u64, ep: &str, t: i64, body: &str) -> Record {
        Record { session: "s".into(), seq, ep: ep.into(), url: format!("{ep}?lastId=5"), wall_ms: t, status: 200, body: Some(body.into()), ..Default::default() }
    }
    const MI: &str = r#"{"valid":true,"grid_steps":[225.0,225.0],"grid_zero":[-884.35,-1107.0],"map_max":[2048.0,0.0],"map_min":[-2048.0,-4096.0]}"#;
    fn unit(icon: &str, color: &str, blink: i64, x: f64, y: f64) -> String {
        format!(r#"{{"type":"ground_model","icon":"{icon}","color":"{color}","blink":{blink},"x":{x},"y":{y},"icon_bg":"none"}}"#)
    }

    #[test]
    fn squad_blink_keeps_identity_and_fuses_with_chat() {
        let mut m = Markers::default();
        m.feed(&rec(1, "/map_info.json", 0, MI));
        let me = unit("Player", "#faC81E", 0, 0.5, 0.5);
        let mut seq = 2;
        for i in 0..100i64 {
            let t = 1000 + i * 100;
            let (c, b) = if (30..80).contains(&i) { ("#043FFF", 2) } else { ("#67D756", 0) };
            let body = format!("[{me},{},{}]", unit("HeavyTank", c, b, 0.30, 0.40), unit("MediumTank", "#174DFF", 0, 0.7, 0.7));
            m.feed(&rec(seq, "/map_obj.json", t, &body));
            seq += 1;
            if i == 32 {
                m.feed(&rec(seq, "/gamechat", t, "[{\"id\":7,\"msg\":\"注\u{200b}意\u{200b}该\u{200b}坐\u{200b}标\u{200b}位\u{200b}置！<color=#FF00FF50> [d4]</color>\",\"sender\":\"HuWangTK\",\"enemy\":false,\"mode\":\"友\u{200b}军\"}]"));
                seq += 1;
            }
        }
        m.flush(i64::MAX / 2, true);
        let ev = |k: &str| m.out.iter().filter(|v| v["ev"] == k).cloned().collect::<Vec<_>>();
        assert_eq!(ev("squad").len(), 1);
        let p = &ev("ping")[0];
        assert_eq!(p["meta"]["sender_role"], "squad");
        assert_eq!(p["grid"], "D4");
        assert_eq!(p["t_end"].as_i64().unwrap() - p["t"].as_i64().unwrap(), PING_TTL_MS);
        assert_eq!(p["precision"], "grid");
        assert_eq!(p["source"], "chat+map");
        assert!(p["area"].is_array());
        let e = &ev("signal_end")[0];
        assert_eq!(e["dur_ms"], 4900);
        assert_eq!(e["role"], "squad");
        assert_eq!(m.squad_names.get("HuWangTK"), Some(&1));
        assert!(ev("signal_only").is_empty());
        // the ally never signalled and is not a squad -> no events for it
        assert_eq!(m.tracks.iter().filter(|t| t.role() == "ally").count(), 1);
    }

    #[test]
    fn own_ping_without_grid_and_marker_objects() {
        let mut m = Markers::default();
        m.feed(&rec(1, "/map_info.json", 0, MI));
        for i in 0..40i64 {
            let t = 1000 + i * 100;
            let (c, b) = if (10..30).contains(&i) { ("#043FFF", 2) } else { ("#faC81E", 0) };
            let poi = if (5..15).contains(&i) { r##",{"type":"point_of_interest","icon":"none","color":"#174DFF","blink":0,"x":0.2,"y":0.43,"icon_bg":"none"}"## } else { "" };
            m.feed(&rec(2 + i as u64 * 2, "/map_obj.json", t, &format!("[{}{poi}]", unit("Player", c, b, 0.5, 0.5))));
            if i == 11 {
                m.feed(&rec(3 + i as u64 * 2, "/gamechat", t, "[{\"id\":9,\"msg\":\"注\u{200b}意\u{200b}战\u{200b}术\u{200b}地\u{200b}图！\",\"sender\":\"┾UMADF┿ 传奇耐摔王无声铃鹿\",\"enemy\":false,\"mode\":\"\"}]"));
            }
        }
        m.flush(i64::MAX / 2, true);
        let p = m.out.iter().find(|v| v["ev"] == "ping").unwrap();
        assert_eq!((p["meta"]["own"].as_bool(), p["meta"]["sender_role"].as_str(), p["precision"].as_str()), (Some(true), Some("self"), Some("none")));
        assert!(p["meta"]["sender_xy"].is_array());
        // uniform shape: own ping has exactly the same top-level keys as a teammate ping
        let keys: std::collections::BTreeSet<&str> = p.as_object().unwrap().keys().map(|k| k.as_str()).collect();
        let want: std::collections::BTreeSet<&str> = ["area", "ev", "grid", "id", "kind", "meta", "precision", "source", "t", "t_end", "text", "ttl_ms", "x", "y"].into_iter().collect();
        assert_eq!(keys, want);
        let live = m.live(1000);
        let lp = &live["recent_pings"][0];
        assert!(lp.get("meta").is_none() && lp.get("own").is_none() && lp["precision"] == "none");
        let mk: Vec<_> = m.out.iter().filter(|v| v["ev"] == "map_marker").collect();
        assert_eq!(mk.len(), 2);
        assert_eq!(mk[1]["lifetime_ms"], 900);
        assert!(m.tracks.iter().all(|t| t.is_self)); // marker never became a unit track
    }

    #[test]
    fn backlog_first_poll_ignored() {
        let mut m = Markers::default();
        let mut r = rec(1, "/gamechat", 10, "[{\"id\":120,\"msg\":\"注\u{200b}意\u{200b}战\u{200b}术\u{200b}地\u{200b}图！ [c4]\",\"sender\":\"x\"}]");
        r.url = "/gamechat?lastId=0".into();
        m.feed(&r);
        m.flush(i64::MAX / 2, true);
        assert!(m.out.is_empty());
    }
}
