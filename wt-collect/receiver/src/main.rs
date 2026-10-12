//! wt-receiver: ingests raw 8111 samples from wt-collector over the tailnet.
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path as AxPath, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use wtc_common::{now_ms, Batch, Record};

mod guard;
#[allow(dead_code)] // Historical exports remain readable; live label writes are disabled.
mod labels;
mod live;
mod runtime;
mod markers;
mod naval;

struct App {
    root: PathBuf,
    token: String,
    max_bytes: u64,
    min_free: u64,
    started_ms: i64,
    inner: Mutex<Inner>,
    guard_cfg: guard::GuardCfg,
    /// Room actors own live state; network locks never enclose room updates.
    hub: runtime::Runtime,
    img_index: Mutex<(i64, HashMap<String, PathBuf>)>,
}

#[derive(Default)]
struct Inner {
    sessions: HashMap<String, Sess>,
    known_dirs: HashMap<String, PathBuf>,
    data_bytes: u64,
}

#[derive(Default)]
struct EpLive {
    recent: VecDeque<(i64, bool, u64, bool)>, // wall_ms, changed, lat_us, error
    total: u64,
    changed: u64,
    errors: u64,
    last_wall: i64,
    last_recv: i64,
    last_status: u16,
    last_bytes: u64,
}

#[derive(Default)]
struct Sess {
    id: String,
    dir: PathBuf,
    player: String,
    version: String,
    watermarks: HashMap<String, (u64, u64)>,
    records: u64,
    dups: u64,
    raw_bytes: u64,
    last_recv: i64,
    eps: BTreeMap<String, EpLive>,
    map_valid: Option<bool>,
    map_gen: Option<String>,
    hud_type: Option<String>,
    map_range: Option<String>,
    objs: Vec<(f64, f64, String, String, String)>,
    player_pos: Option<(f64, f64, String)>,
    latest_img: Option<String>,
    mission: Option<String>,
    army: Option<String>,
    vehicle: Option<String>,
    notes: Vec<(i64, String)>,
    naval_objs: u64,
    naval_ticks: u64,
    guard: guard::GuardStats,
    /// latest #state / #heartbeat of a 0.2.0 collector: (recv_ms, paused, reason)
    gate: Option<(i64, bool, String)>,
    labels: u64,
    /// In-memory radio derivations for diagnostics
    markers: markers::Markers,
}


fn env(k: &str, d: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| d.to_string())
}

fn free_bytes(p: &Path) -> u64 {
    use std::os::unix::ffi::OsStrExt;
    let c = match std::ffi::CString::new(p.as_os_str().as_bytes()) {
        Ok(c) => c,
        Err(_) => return u64::MAX,
    };
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0 {
        s.f_bavail as u64 * s.f_frsize as u64
    } else {
        u64::MAX
    }
}

fn dir_size(p: &Path) -> u64 {
    let mut t = 0;
    if let Ok(rd) = std::fs::read_dir(p) {
        for e in rd.flatten() {
            let m = match e.metadata() {
                Ok(m) => m,
                Err(_) => continue,
            };
            if m.is_dir() {
                t += dir_size(&e.path());
            } else {
                t += m.len();
            }
        }
    }
    t
}

fn val_str(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        o => Some(o.to_string()),
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn safe_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 96 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn fmt_local(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(ms).single().map(|t| t.format("%m-%d %H:%M:%S").to_string()).unwrap_or_default()
}

impl App {
    fn authed(&self, h: &HeaderMap) -> bool {
        let tok = h
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .or_else(|| h.get("x-token").and_then(|v| v.to_str().ok()));
        match tok {
            Some(t) => !self.token.is_empty() && constant_eq(t.as_bytes(), self.token.as_bytes()),
            None => false,
        }
    }

    fn guard(&self, inner: &Inner, incoming: u64) -> Result<(), (StatusCode, String)> {
        let free = free_bytes(&self.root);
        if free < self.min_free {
            return Err((StatusCode::INSUFFICIENT_STORAGE, format!("disk free {} MB below guard", free / 1_048_576)));
        }
        if inner.data_bytes + incoming > self.max_bytes {
            return Err((StatusCode::INSUFFICIENT_STORAGE, format!("data dir cap {} GB reached", self.max_bytes >> 30)));
        }
        Ok(())
    }

    fn sess<'a>(&self, inner: &'a mut Inner, id: &str, player: &str, version: &str) -> std::io::Result<&'a mut Sess> {
        let s = inner.sessions.entry(id.into()).or_insert_with(|| Sess {
            id: id.into(), dir: self.root.join("_maps"), player: player.into(), version: version.into(),
            ..Default::default()
        });
        if !player.is_empty() { s.player = player.into(); }
        if !version.is_empty() { s.version = version.into(); }
        Ok(s)
    }

    fn ingest(&self, batch: Batch) -> Result<serde_json::Value, (StatusCode, String)> {
        if !safe_id(&batch.session) || (!batch.device_id.is_empty() && !safe_id(&batch.device_id)) {
            return Err((StatusCode::BAD_REQUEST, "bad identity".into()));
        }
        if batch.records.len() > 128 { return Err((StatusCode::PAYLOAD_TOO_LARGE, "too many records".into())); }
        let mode = guard::batch_mode(&self.guard_cfg, &batch);
        let now = now_ms();
        let room = self.hub.for_player(&batch.player);
        let mut accepted = Vec::new();
        let (mut acc, mut dup, mut foreign, mut too_old, mut future, mut no_sync, mut reordered) = (0u64, 0u64, 0u64, 0u64, 0u64, 0u64, 0u64);
        let parsed_records: HashMap<u64, serde_json::Value> = batch.records.iter().filter_map(|r| r.body.as_deref().and_then(|b| serde_json::from_str(b).ok()).map(|v| (r.seq,v))).collect();
        {
            let mut inner = self.inner.lock().unwrap();
            inner.sessions.retain(|_, s| now - s.last_recv < 15 * 60_000);
            if inner.sessions.len() >= 256 && !inner.sessions.contains_key(&batch.session) { return Err((StatusCode::TOO_MANY_REQUESTS, "too many active collectors".into())); }
            let s = self.sess(&mut inner, &batch.session, &batch.player, &batch.version).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            s.last_recv = now;
            if let Some(c) = &batch.clock { s.guard.note_clock(c); }
            s.guard.max_skew_ms = self.guard_cfg.max_skew_ms;
            let mut records = batch.records;
            records.sort_by_key(|r| (r.mono_ns, r.seq));
            for mut r in records {
                if r.session != batch.session { foreign += 1; continue; }
                if mode == guard::Mode::RejectNoSync { no_sync += 1; continue; }
                if let guard::Mode::Synced(off) = mode { r.ts_server_ms.get_or_insert(r.wall_ms + off.round() as i64); }
                let ts = r.ts_server_ms.unwrap_or(r.wall_ms);
                // Future clock tolerance is independent from the hard live-age deadline.
                if now - ts > wtc_common::LIVE_MAX_AGE_MS { too_old += 1; continue; }
                if ts - now > self.guard_cfg.max_skew_ms { future += 1; continue; }
                let order = (if r.mono_ns > 0 { r.mono_ns } else { ts.max(0) as u64 * 1_000_000 }, r.seq);
                if let Some(old) = s.watermarks.get(&r.ep) {
                    if *old >= order { if *old == order { dup += 1; } else { reordered += 1; } continue; }
                }
                s.watermarks.insert(r.ep.clone(), order);
                r.recv_ms = Some(now);
                s.guard.note_delay(now - ts);
                live_update(s, &r, now, parsed_records.get(&r.seq));
                accepted.push(r); acc += 1;
            }
            s.records += acc; s.dups += dup;
            s.guard.accepted += acc; s.guard.too_old += too_old; s.guard.from_future += future; s.guard.no_sync += no_sync;
        }
        // No session/sequence/sample/event files are created by live ingest.
        let mut overflow = 0;
        for r in accepted { if !room.offer(&batch.session, &batch.player, &batch.device_id, &batch.nickname, r) { overflow += 1; } }
        Ok(serde_json::json!({"accepted": acc, "dup": dup, "foreign": foreign, "out_of_order": reordered,
            "dropped": {"too_old": too_old, "from_future": future, "no_sync": no_sync, "overflow": overflow}, "server_ms": now}))
    }
}

impl App {
    /// Session dirs matching a selector: exact id, id prefix/substring, "latest", or all of a date.
    fn select_dirs(&self, session: Option<&str>, date: Option<&str>) -> Vec<(String, PathBuf)> {
        let inner = self.inner.lock().unwrap();
        let mut v: Vec<(String, PathBuf)> = inner.known_dirs.iter().map(|(k, d)| (k.clone(), d.clone())).collect();
        v.sort();
        match (session.filter(|s| !s.is_empty()), date.filter(|s| !s.is_empty())) {
            (Some("latest"), _) => {
                let best = inner.sessions.values().max_by_key(|s| s.last_recv).map(|s| s.id.clone()).or_else(|| v.last().map(|x| x.0.clone()));
                v.retain(|x| Some(&x.0) == best.as_ref());
            }
            (Some(sel), _) => {
                if v.iter().any(|x| x.0 == sel) {
                    v.retain(|x| x.0 == sel);
                } else {
                    v.retain(|x| x.0.contains(sel));
                }
            }
            (None, Some(d)) => v.retain(|x| x.1.parent().and_then(|p| p.file_name()).map(|n| n.to_string_lossy() == d).unwrap_or(false)),
            (None, None) => {}
        }
        v
    }
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn live_update(s: &mut Sess, r: &Record, now: i64, parsed: Option<&serde_json::Value>) {
    let e = s.eps.entry(r.ep.clone()).or_default();
    let err = r.err.is_some();
    e.total += 1;
    if r.changed && !err {
        e.changed += 1;
    }
    if err {
        e.errors += 1;
    }
    // backlog (old) records: count them, but keep the live view (rates, latest map/player) on current data
    let stale = r.wall_ms < e.last_wall - 10_000;
    e.last_recv = now;
    if stale {
        return;
    }
    e.last_wall = e.last_wall.max(r.wall_ms);
    e.last_status = r.status;
    e.last_bytes = r.bytes;
    e.recent.push_back((r.wall_ms, r.changed, r.lat_us, err));
    while let Some(f) = e.recent.front() {
        if f.0 < e.last_wall - 10_000 {
            e.recent.pop_front();
        } else {
            break;
        }
    }
    if err {
        return;
    }
    match r.ep.as_str() {
        "#note" => {
            if let Some(b) = &r.body {
                s.notes.push((r.wall_ms, b.clone()));
            }
        }
        "#label" => {
            if let Some(v) = parsed {
                let t = format!(
                    "[标签] {}{}{}",
                    v["text"].as_str().unwrap_or(""),
                    v["note"].as_str().map(|n| format!(" 「{n}」")).unwrap_or_default(),
                    if v["late"].as_bool() == Some(true) { " (late)" } else { "" }
                );
                s.notes.push((r.ts_server_ms.unwrap_or(r.wall_ms), t));
            }
        }
        "#state" | "#heartbeat" => {
            if let Some(v) = parsed {
                s.gate = Some((now, v["paused"].as_bool().unwrap_or(false), v["reason"].as_str().unwrap_or("").to_string()));
            }
        }
        "/map.img" => {
            if r.img_hash.is_some() {
                s.latest_img = r.img_hash.clone();
            }
        }
        "/map_info.json" | "/map_obj.json" | "/mission.json" | "/indicators" => {
            let Some(v) = parsed else { return };
            match r.ep.as_str() {
                "/map_info.json" => {
                    s.map_valid = v["valid"].as_bool();
                    s.map_gen = val_str(&v["map_generation"]);
                    s.hud_type = val_str(&v["hud_type"]);
                    s.map_range = Some(format!("{} .. {}", v["map_min"], v["map_max"]));
                }
                "/map_obj.json" => {
                    if let Some(a) = v.as_array() {
                        s.objs.clear();
                        s.player_pos = None;
                        for o in a {
                            let x = o["x"].as_f64().or(o["sx"].as_f64()).unwrap_or(-1.0);
                            let y = o["y"].as_f64().or(o["sy"].as_f64()).unwrap_or(-1.0);
                            let icon = o["icon"].as_str().unwrap_or("").to_string();
                            let ty = o["type"].as_str().unwrap_or("").to_string();
                            // own Player only: after death the Player icon follows a spectated teammate (#174DFF)
                            if icon == "Player" && wtc_common::mapobj::classify(o).side == wtc_common::mapobj::Side::Own {
                                s.player_pos = Some((x, y, ty.clone()));
                            }
                            s.objs.push((x, y, o["color"].as_str().unwrap_or("#888").to_string(), icon, ty));
                        }
                    }
                }
                "/mission.json" => s.mission = val_str(&v["status"]),
                "/indicators" => {
                    s.army = val_str(&v["army"]);
                    s.vehicle = val_str(&v["type"]);
                }
                _ => {}
            }
        }
        _ => {}
    }
}

type St = State<Arc<App>>;

fn err(code: StatusCode, msg: impl Into<String>) -> Response {
    (code, Json(serde_json::json!({"error": msg.into()}))).into_response()
}

async fn ingest(State(app): St, h: HeaderMap, body: Bytes) -> Response {
    if !app.authed(&h) {
        return err(StatusCode::UNAUTHORIZED, "bad token");
    }
    let gz = h.get(header::CONTENT_ENCODING).and_then(|v| v.to_str().ok()).map(|v| v.contains("gzip")).unwrap_or(false);
    match tokio::task::spawn_blocking(move || {
        let raw = if gz {
            let mut out = Vec::new();
            flate2::read::MultiGzDecoder::new(&body[..]).take(16 * 1024 * 1024 + 1).read_to_end(&mut out).map_err(|e| (StatusCode::BAD_REQUEST,e.to_string()))?;
            if out.len() > 16 * 1024 * 1024 { return Err((StatusCode::PAYLOAD_TOO_LARGE,"expanded batch too large".into())); } out
        } else { body.to_vec() };
        let batch: Batch = serde_json::from_slice(&raw).map_err(|e| (StatusCode::BAD_REQUEST,format!("bad json: {e}")))?;
        app.ingest(batch)
    }).await {
        Ok(Ok(v)) => Json(v).into_response(), Ok(Err((c,m))) => err(c,m), Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR,e.to_string())
    }
}

#[derive(Deserialize)]
struct ImgQ {
    session: String,
    hash: String,
    #[allow(dead_code)]
    ctype: Option<String>,
}

async fn image(State(app): St, h: HeaderMap, Query(q): Query<ImgQ>, body: Bytes) -> Response {
    if !app.authed(&h) {
        return err(StatusCode::UNAUTHORIZED, "bad token");
    }
    if !safe_id(&q.session) || !q.hash.chars().all(|c| c.is_ascii_hexdigit()) || q.hash.len() != 64 {
        return err(StatusCode::BAD_REQUEST, "bad session/hash");
    }
    use sha2::Digest;
    let got = hex::encode(sha2::Sha256::digest(&body));
    if got != q.hash {
        return err(StatusCode::BAD_REQUEST, "hash mismatch");
    }
    let app2 = app.clone();
    let r = tokio::task::spawn_blocking(move || -> Result<serde_json::Value, (StatusCode, String)> {
        let ext = wtc_common::sniff_image(&body);
        if ext == "bin" { return Err((StatusCode::BAD_REQUEST, "unsupported map image".into())); }
        let dir = app2.root.join("_maps");
        std::fs::create_dir_all(&dir).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        let p = dir.join(format!("{}.{ext}", q.hash));
        let new = !p.exists();
        if new {
            let incoming = body.len() as u64;
            { let inner = app2.inner.lock().unwrap(); app2.guard(&inner, incoming)?; }
            let tmp = dir.join(format!("{}-{:x}.tmp", q.hash, rand::random::<u64>()));
            std::fs::write(&tmp, &body).and_then(|_| std::fs::rename(&tmp, &p)).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            app2.inner.lock().unwrap().data_bytes += incoming;
        }
        app2.img_index.lock().unwrap().1.insert(q.hash.clone(), p);
        app2.hub.image_ready(q.hash);
        Ok(serde_json::json!({"stored": new, "bytes": body.len()}))
    }).await;
    match r { Ok(Ok(v)) => Json(v).into_response(), Ok(Err((c, m))) => err(c, m), Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()) }
}

#[derive(Deserialize)]
struct LabelsQ {
    session: Option<String>,
    date: Option<String>,
    /// 1 = include _undo/_tag meta events
    meta: Option<u8>,
    /// 1 = include labels tagged "test"
    test: Option<u8>,
    /// 1 = include undone labels (flagged undone=true)
    undone: Option<u8>,
    /// json (default) | csv
    format: Option<String>,
}

/// GET /api/labels?session=<id|substring|latest>&date=YYYY-MM-DD&test=1&undone=1&meta=1&format=csv (token required)
async fn labels_get(State(app): St, h: HeaderMap, Query(q): Query<LabelsQ>) -> Response {
    if !app.authed(&h) {
        return err(StatusCode::UNAUTHORIZED, "bad token");
    }
    let mut out = Vec::new();
    for (_id, dir) in app.select_dirs(q.session.as_deref(), q.date.as_deref()) {
        let Ok(t) = std::fs::read_to_string(dir.join("labels.jsonl")) else { continue };
        let all: Vec<serde_json::Value> = t.lines().filter_map(|l| serde_json::from_str(l).ok()).collect();
        out.extend(labels::resolve(all, q.meta == Some(1), q.test == Some(1), q.undone == Some(1)));
    }
    if q.format.as_deref() == Some("csv") {
        let mut c = String::from("t_server_local,t_server,session,uid,label_id,text,note,source,clock_synced,t_server_source,late,undone,tags,in_battle,map_gen,self_x,self_y,enemy_objects,map_obj_seq\n");
        let q = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
        for v in &out {
            let sn = &v["snapshot"];
            let _ = writeln!(
                c,
                "{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                fmt_local(v["t_server"].as_i64().unwrap_or(0)),
                v["t_server"],
                q(v["session"].as_str().unwrap_or("")),
                q(v["uid"].as_str().unwrap_or("")),
                q(v["label_id"].as_str().unwrap_or("")),
                q(v["text"].as_str().unwrap_or("")),
                q(v["note"].as_str().unwrap_or("")),
                v["source"].as_str().unwrap_or(""),
                v["clock_synced"],
                v["t_server_source"].as_str().unwrap_or(""),
                v["late"],
                v["undone"],
                q(&v["tags"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>().join(";")).unwrap_or_default()),
                sn["in_battle"],
                q(sn["map_gen"].as_str().unwrap_or("")),
                sn["self_xy"][0],
                sn["self_xy"][1],
                sn["enemy_objects"],
                sn["map_obj_seq"],
            );
        }
        return ([(header::CONTENT_TYPE, "text/csv; charset=utf-8")], c).into_response();
    }
    Json(serde_json::json!({"count": out.len(), "labels": out})).into_response()
}

#[derive(Deserialize)]
struct MarkersQ {
    session: Option<String>,
    date: Option<String>,
    /// only events with t >= since (server ms)
    since: Option<i64>,
    /// comma list of ev names (squad,signal_start,signal_end,signal_only,ping,location,radio,map_marker)
    ev: Option<String>,
    limit: Option<usize>,
}

/// GET /api/markers?session=<id|substring|latest>&since=<ms>&ev=ping,location (token required): derived squad / ping events.
async fn markers_get(State(app): St, h: HeaderMap, Query(q): Query<MarkersQ>) -> Response {
    if !app.authed(&h) {
        return err(StatusCode::UNAUTHORIZED, "bad token");
    }
    let evs: Option<Vec<String>> = q.ev.as_ref().map(|e| e.split(',').map(|x| x.trim().to_string()).filter(|x| !x.is_empty()).collect());
    let mut out = Vec::new();
    for (id, dir) in app.select_dirs(q.session.as_deref(), q.date.as_deref()) {
        let Ok(t) = std::fs::read_to_string(dir.join("markers.jsonl")) else { continue };
        for l in t.lines() {
            let Ok(mut v) = serde_json::from_str::<serde_json::Value>(l) else { continue };
            if q.since.map(|s| v["t"].as_i64().unwrap_or(0) < s).unwrap_or(false) {
                continue;
            }
            if let Some(e) = &evs {
                if !e.iter().any(|x| v["ev"].as_str() == Some(x.as_str())) {
                    continue;
                }
            }
            v["session"] = serde_json::Value::String(id.clone());
            out.push(v);
        }
    }
    let lim = q.limit.unwrap_or(5000).min(50_000);
    if out.len() > lim {
        out.drain(..out.len() - lim);
    }
    Json(serde_json::json!({"count": out.len(), "events": out})).into_response()
}

async fn img_get(State(app): St, AxPath((sid, file)): AxPath<(String, String)>) -> Response {
    if !safe_id(&sid) || !file.chars().all(|c| c.is_ascii_alphanumeric() || c == '.') {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let dir = { app.inner.lock().unwrap().known_dirs.get(&sid).cloned() };
    let Some(dir) = dir else { return StatusCode::NOT_FOUND.into_response() };
    let imgs = dir.join("images");
    let path = if file.contains('.') {
        Some(imgs.join(&file))
    } else {
        std::fs::read_dir(&imgs).ok().and_then(|rd| rd.flatten().map(|e| e.path()).find(|p| p.file_stem().and_then(|s| s.to_str()) == Some(file.as_str())))
    };
    match path.and_then(|p| std::fs::read(&p).ok()) {
        Some(b) => {
            let ct = match wtc_common::sniff_image(&b) {
                "png" => "image/png",
                "jpg" => "image/jpeg",
                "webp" => "image/webp",
                _ => "application/octet-stream",
            };
            ([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "max-age=86400")], b).into_response()
        }
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn ep_rows(s: &Sess, now: i64) -> Vec<serde_json::Value> {
    s.eps
        .iter()
        .map(|(k, e)| {
            let n = e.recent.len();
            let span = if n >= 2 { (e.recent.back().unwrap().0 - e.recent.front().unwrap().0) as f64 / 1000.0 } else { 0.0 };
            let hz = if span > 0.0 { (n - 1) as f64 / span } else { 0.0 };
            let okn = e.recent.iter().filter(|r| !r.3).count();
            let ch = e.recent.iter().filter(|r| r.1 && !r.3).count();
            let mut lats: Vec<u64> = e.recent.iter().filter(|r| !r.3).map(|r| r.2).collect();
            lats.sort();
            let p = |q: f64| if lats.is_empty() { 0.0 } else { lats[((lats.len() - 1) as f64 * q).round() as usize] as f64 / 1000.0 };
            serde_json::json!({
                "ep": k, "hz": hz, "new_pct": if okn > 0 { 100.0 * ch as f64 / okn as f64 } else { 0.0 },
                "err_pct": if n > 0 { 100.0 * (n - okn) as f64 / n as f64 } else { 0.0 },
                "lat_p50_ms": p(0.5), "lat_p95_ms": p(0.95), "total": e.total, "errors": e.errors,
                "last_age_s": (now - e.last_recv) as f64 / 1000.0, "last_status": e.last_status, "last_bytes": e.last_bytes,
            })
        })
        .collect()
}

/// Server clock for NTP-style sync (no auth: reveals nothing but the time and the skew threshold).
async fn api_time(State(app): St) -> Response {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0);
    (
        [(header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({"server_ms": t, "max_skew_ms": app.guard_cfg.max_skew_ms})),
    )
        .into_response()
}

fn guard_json(g: &guard::GuardStats) -> serde_json::Value {
    serde_json::json!({
        "accepted": g.accepted, "too_old": g.too_old, "from_future": g.from_future, "no_sync": g.no_sync,
        "batches_rejected_no_sync": g.batches_rejected_no_sync, "legacy_accepted": g.legacy_accepted,
        "delay_p50_ms": g.delay_pct(0.5), "delay_p95_ms": g.delay_pct(0.95), "delay_max_ms": if g.delay_n > 0 { Some(g.delay_max_ms) } else { None },
        "offset_ms": g.last_clock.as_ref().map(|c| c.offset_ms), "rtt_ms": g.last_clock.as_ref().map(|c| c.rtt_ms),
        "collector_drops": g.last_clock.as_ref().map(|c| serde_json::json!({"too_old": c.local_drop_too_old, "overflow": c.local_drop_overflow, "no_sync": c.local_drop_no_sync})),
    })
}

async fn status_json(State(app): St) -> Response {
    let inner = app.inner.lock().unwrap();
    let now = now_ms();
    let mut v: Vec<&Sess> = inner.sessions.values().collect();
    v.sort_by_key(|s| -s.last_recv);
    let sessions: Vec<_> = v
        .iter()
        .map(|s| {
            serde_json::json!({
                "session": s.id, "player": s.player, "version": s.version, "dir": s.dir, "records": s.records, "dups": s.dups,
                "raw_bytes": s.raw_bytes, "last_recv_age_s": (now - s.last_recv) as f64 / 1000.0,
                "map_valid": s.map_valid, "map_generation": s.map_gen, "hud_type": s.hud_type, "map_range": s.map_range,
                "player_pos": s.player_pos, "objects": s.objs.len(), "latest_img": s.latest_img, "mission": s.mission,
                "army": s.army, "vehicle": s.vehicle, "notes": s.notes.len(), "labels": s.labels,
                "upload_state": s.gate.as_ref().map(|g| serde_json::json!({"paused": g.1, "reason": g.2, "age_s": (now - g.0) as f64 / 1000.0})), "naval_objects_filtered": s.naval_objs, "data_lag_s": s.eps.values().map(|e| e.last_wall).max().map(|w| (now - w) as f64 / 1000.0), "naval_ticks_dropped": s.naval_ticks, "clock_guard": guard_json(&s.guard), "endpoints": ep_rows(s, now),
                "allies": s.markers.live(now),
            })
        })
        .collect();
    Json(serde_json::json!({
        "ok": true, "uptime_s": (now - app.started_ms) / 1000, "data_bytes": inner.data_bytes,
        "disk_free_bytes": free_bytes(&app.root), "known_sessions": inner.known_dirs.len(), "live_sessions": sessions,
        "clock_guard_cfg": {"max_skew_ms": app.guard_cfg.max_skew_ms, "accept_legacy": format!("{:?}", app.guard_cfg.legacy)},
    }))
    .into_response()
}

fn self_skew(app: &App) -> i64 {
    app.guard_cfg.max_skew_ms
}

async fn index(State(app): St) -> Html<String> {
    let inner = app.inner.lock().unwrap();
    let now = now_ms();
    let mut h = String::new();
    let _ = write!(
        h,
        r#"<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="refresh" content="2"><title>wt-receiver</title>
<style>body{{font:13px system-ui,sans-serif;background:#111;color:#ddd;margin:12px}}table{{border-collapse:collapse;margin:6px 0}}td,th{{border:1px solid #333;padding:2px 6px;text-align:right}}th{{background:#222}}td.l{{text-align:left}}.s{{border:1px solid #444;padding:8px;margin:10px 0;border-radius:6px}}.ok{{color:#6f6}}.bad{{color:#f66}}.map{{position:relative;width:420px;height:420px;background:#222;float:right;margin-left:12px}}.map img,.map svg{{position:absolute;left:0;top:0;width:420px;height:420px}}h2{{margin:2px 0}}</style></head><body>
<h1 style="margin:0">wt-receiver <small>{} · uptime {}s · data {:.1} MB · disk free {:.1} GB · {} session dirs</small></h1>
<p>Note: <input id=nt size=50 placeholder="test label, e.g. 'rate 20Hz test drive'"> <button onclick="sn()">add note to latest session</button> <span id=nr></span>
<script>function sn(){{let t=localStorage.wtok||prompt('token (from receiver.env)');if(!t)return;localStorage.wtok=t;fetch('/api/note',{{method:'POST',headers:{{'Content-Type':'application/json','Authorization':'Bearer '+t}},body:JSON.stringify({{text:document.getElementById('nt').value}})}}).then(r=>r.text()).then(x=>document.getElementById('nr').textContent=x)}}</script></p>"#,
        fmt_local(now),
        (now - app.started_ms) / 1000,
        inner.data_bytes as f64 / 1e6,
        free_bytes(&app.root) as f64 / 1e9,
        inner.known_dirs.len()
    );
    let mut v: Vec<&Sess> = inner.sessions.values().collect();
    v.sort_by_key(|s| -s.last_recv);
    if v.is_empty() {
        h.push_str("<p>No live sessions since receiver start. Waiting for wt-collector…</p>");
    }
    for (i, s) in v.iter().enumerate() {
        let age = (now - s.last_recv) as f64 / 1000.0;
        let paused = s.gate.as_ref().filter(|g| g.1 && now - g.0 < 75_000).map(|g| g.2.clone());
        let live = age < 5.0 || paused.is_some();
        let _ = write!(
            h,
            r#"<div class=s><h2>{} <small>player <b>{}</b> · v{} · <span class={}>{}</span> · last batch {:.1}s ago · {} records ({} dup) · raw {:.1} MB</small></h2>"#,
            esc(&s.id),
            esc(&s.player),
            esc(&s.version),
            if live { "ok" } else { "bad" },
            match &paused { Some(r) => format!("PAUSED ({}) · collector alive", esc(r)), None if live => "LIVE".into(), None => "idle".into() },
            age,
            s.records,
            s.dups,
            s.raw_bytes as f64 / 1e6
        );
        {
            let g = &s.guard;
            let c = g.last_clock.as_ref();
            let drops = g.too_old + g.from_future + g.no_sync;
            let _ = write!(
                h,
                "<p>clock guard (±{} ms): accepted <b>{}</b> · dropped <b class={}>{}</b> (too_old {} · from_future {} · no_sync {} in {} batches) · legacy accepted {} · delay p50/p95/max {}/{}/{} ms · offset {} · RTT {} · collector-side drops {}</p>",
                self_skew(&app),
                g.accepted,
                if drops > 0 { "bad" } else { "ok" },
                drops,
                g.too_old,
                g.from_future,
                g.no_sync,
                g.batches_rejected_no_sync,
                g.legacy_accepted,
                g.delay_pct(0.5).map(|v| v.to_string()).unwrap_or("-".into()),
                g.delay_pct(0.95).map(|v| v.to_string()).unwrap_or("-".into()),
                if g.delay_n > 0 { g.delay_max_ms.to_string() } else { "-".into() },
                c.map(|c| format!("{:+.1} ms", c.offset_ms)).unwrap_or("- (legacy)".into()),
                c.map(|c| format!("{:.1} ms", c.rtt_ms)).unwrap_or("-".into()),
                c.map(|c| format!("too_old {} · overflow {} · no_sync {}", c.local_drop_too_old, c.local_drop_overflow, c.local_drop_no_sync)).unwrap_or("-".into()),
            );
        }
        if i < 4 {
            h.push_str("<div class=map>");
            if let Some(img) = &s.latest_img {
                let _ = write!(h, r#"<img src="/img/{}/{}">"#, esc(&s.id), esc(img));
            }
            h.push_str(r#"<svg viewBox="0 0 1 1" preserveAspectRatio="none">"#);
            for (x, y, c, icon, _t) in &s.objs {
                if *x < 0.0 {
                    continue;
                }
                let r = if icon == "Player" { 0.014 } else { 0.008 };
                let stroke = if icon == "Player" { r##"stroke="#fff" stroke-width="0.004""## } else { "" };
                let _ = write!(h, r#"<circle cx="{x}" cy="{y}" r="{r}" fill="{}" {stroke}/>"#, esc(c));
            }
            h.push_str("</svg></div>");
        }
        let _ = write!(
            h,
            "<p>map_info valid <b>{:?}</b> · map_generation <b>{}</b> · hud_type <b>{}</b> · range {} <br>player: <b>{}</b> · objects {} · army {} · vehicle {} · mission {} · image {}</p>",
            s.map_valid,
            esc(s.map_gen.as_deref().unwrap_or("-")),
            esc(s.hud_type.as_deref().unwrap_or("-")),
            esc(s.map_range.as_deref().unwrap_or("-")),
            match &s.player_pos {
                Some((x, y, t)) => format!("FOUND ({}) x={x:.4} y={y:.4}", esc(t)),
                None => "not found".into(),
            },
            s.objs.len(),
            esc(s.army.as_deref().unwrap_or("-")),
            esc(s.vehicle.as_deref().unwrap_or("-")),
            esc(s.mission.as_deref().unwrap_or("-")),
            esc(&s.latest_img.as_deref().map(|x| x[..12].to_string()).unwrap_or("-".into()))
        );
        h.push_str("<table><tr><th>endpoint</th><th>Hz (10s)</th><th>new %</th><th>err %</th><th>lat p50 ms</th><th>p95 ms</th><th>last age s</th><th>status</th><th>bytes</th><th>total</th></tr>");
        for r in ep_rows(s, now) {
            let _ = write!(
                h,
                "<tr><td class=l>{}</td><td>{:.1}</td><td>{:.0}</td><td>{:.0}</td><td>{:.2}</td><td>{:.2}</td><td>{:.1}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(r["ep"].as_str().unwrap_or("")),
                r["hz"].as_f64().unwrap_or(0.0),
                r["new_pct"].as_f64().unwrap_or(0.0),
                r["err_pct"].as_f64().unwrap_or(0.0),
                r["lat_p50_ms"].as_f64().unwrap_or(0.0),
                r["lat_p95_ms"].as_f64().unwrap_or(0.0),
                r["last_age_s"].as_f64().unwrap_or(0.0),
                r["last_status"],
                r["last_bytes"],
                r["total"]
            );
        }
        h.push_str("</table>");
        if !s.notes.is_empty() {
            h.push_str("<p>notes: ");
            for (w, t) in s.notes.iter().rev().take(8) {
                let _ = write!(h, "<br>{} — {}", fmt_local(*w), esc(t));
            }
            h.push_str("</p>");
        }
        let _ = write!(h, "<p style='clear:both;color:#888'>dir: {}</p></div>", esc(&s.dir.display().to_string()));
    }
    h.push_str("</body></html>");
    Html(h)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("markers") {
        // offline: wt-receiver markers <session_dir> [--write]  -> derived events (stdout, or <dir>/markers.jsonl)
        let dir = PathBuf::from(std::env::args().nth(2).ok_or_else(|| anyhow::anyhow!("usage: wt-receiver markers <session_dir> [--write]"))?);
        let write = std::env::args().any(|a| a == "--write");
        let mut m = markers::Markers::default();
        let mut buf = Vec::new();
        for r in wtc_common::read_session(&dir)? {
            m.feed(&r);
            for v in m.out.drain(..) {
                buf.extend_from_slice(&serde_json::to_vec(&v)?);
                buf.push(b'\n');
            }
        }
        m.flush(i64::MAX / 2, true);
        for v in m.out.drain(..) {
            buf.extend_from_slice(&serde_json::to_vec(&v)?);
            buf.push(b'\n');
        }
        if write {
            std::fs::write(dir.join("markers.jsonl"), &buf)?;
            eprintln!("wrote {} ({} bytes) counts {:?}", dir.join("markers.jsonl").display(), buf.len(), m.counts);
        } else {
            std::io::stdout().write_all(&buf)?;
            eprintln!("counts {:?} squad_names {:?}", m.counts, m.squad_names);
        }
        return Ok(());
    }
    let root = PathBuf::from(env("WT_DATA", "/workspace/wt-collect/data"));
    std::fs::create_dir_all(&root)?;
    let token = env("WT_TOKEN", "");
    if token.len() < 16 {
        anyhow::bail!("WT_TOKEN missing/too short (source receiver.env)");
    }
    let bind = env("WT_BIND", "0.0.0.0:18111");
    let max_gb: f64 = env("WT_MAX_GB", "30").parse().unwrap_or(30.0);
    let min_free_gb: f64 = env("WT_MIN_FREE_GB", "5").parse().unwrap_or(5.0);
    let mut inner = Inner::default();
    // index existing session dirs: root/<date>/<session>
    if let Ok(rd) = std::fs::read_dir(&root) {
        for d in rd.flatten().filter(|e| e.path().is_dir()) {
            if let Ok(rd2) = std::fs::read_dir(d.path()) {
                for s in rd2.flatten().filter(|e| e.path().is_dir()) {
                    inner.known_dirs.insert(s.file_name().to_string_lossy().to_string(), s.path());
                }
            }
        }
    }
    inner.data_bytes = dir_size(&root);
    let naval_cfg = naval::NavalCfg::from_env();
    let icon_stats: naval::IconStats = std::fs::read(root.join("unknown_icons.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    if std::env::args().nth(1).as_deref() == Some("filter-existing") {
        return filter_existing(&root, &naval_cfg, icon_stats);
    }
    let guard_cfg = guard::GuardCfg::from_env();
    eprintln!("[{}] clock guard: max skew {} ms, legacy collectors {:?}", fmt_local(now_ms()), guard_cfg.max_skew_ms, guard_cfg.legacy);
    let hub = runtime::Runtime::new(&root);
    if hub.view_key.len() < 8 {
        eprintln!("[{}] WARNING: WT_VIEW_TOKEN missing/short: live map (/api/live, /api/map) disabled", fmt_local(now_ms()));
    }
    eprintln!("[{}] live: rooms dir {}, default room '{}'", fmt_local(now_ms()), root.display(), hub.default_room);
    let app = Arc::new(App {
        hub,
        img_index: Mutex::new((0, HashMap::new())),
        guard_cfg,
        root,
        token,
        max_bytes: (max_gb * 1073741824.0) as u64,
        min_free: (min_free_gb * 1073741824.0) as u64,
        started_ms: now_ms(),
        inner: Mutex::new(inner),
    });
    let r = Router::new()
        .route("/", get(index))
        .route("/status", get(index))
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/status", get(status_json))
        .route("/api/time", get(api_time))
        .route("/api/ingest", post(ingest))
        .route("/api/image", post(image))
        .route("/api/note", post(|| async { StatusCode::GONE }))
        .route("/api/label", post(|| async { StatusCode::GONE }))
        .route("/api/labels", get(labels_get))
        .route("/api/markers", get(markers_get))
        .route("/img/{session}/{file}", get(img_get))
        .route("/api/live", get(live_ws))
        .route("/api/live/check", get(live_check))
        .route("/api/map/{file}", get(map_get))
        .route("/api/replay", post(|| async { StatusCode::GONE }))
        .route("/api/replay/stop", post(|| async { StatusCode::GONE }))
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024))
        .with_state(app.clone());
    // Seed image readiness from both historical sessions and the shared static-resource cache.
    let mut image_dirs: Vec<_> = app.inner.lock().unwrap().known_dirs.values().map(|d| d.join("images")).collect();
    image_dirs.push(app.root.join("_maps"));
    for dir in image_dirs {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                let p = entry.path();
                if let Some(hash) = p.file_stem().and_then(|s| s.to_str()).filter(|s| s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())) {
                    app.hub.image_ready(hash.into()); app.img_index.lock().unwrap().1.insert(hash.into(), p);
                }
            }
        }
    }
    let l = tokio::net::TcpListener::bind(&bind).await?;
    let (used, known) = {
        let i = app.inner.lock().unwrap();
        (i.data_bytes, i.known_dirs.len())
    };
    eprintln!(
        "[{}] wt-receiver listening on {bind}, data {} ({} MB used, {} sessions known), cap {max_gb} GB, min free {min_free_gb} GB",
        chrono::Local::now().format("%F %T"),
        app.root.display(),
        used / 1_000_000,
        known
    );
    axum::serve(l, r)
        .with_graceful_shutdown(async {
            let mut t = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
            tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = t.recv() => {} }
        })
        .await?;
    app.hub.flush().await;
    Ok(())
}

fn write_icon_stats(root: &Path, st: &mut naval::IconStats) {
    if !st.dirty {
        return;
    }
    st.updated = chrono::Local::now().format("%F %T").to_string();
    let tmp = root.join("unknown_icons.json.tmp");
    if let Ok(b) = serde_json::to_vec_pretty(&*st) {
        if std::fs::write(&tmp, b).is_ok() && std::fs::rename(&tmp, root.join("unknown_icons.json")).is_ok() {
            st.dirty = false;
        }
    }
}

/// One-shot: apply the naval filter to all sessions already on disk (receiver must be stopped).
fn filter_existing(root: &Path, cfg: &naval::NavalCfg, mut stats: naval::IconStats) -> anyhow::Result<()> {
    let mut dirs = Vec::new();
    for d in std::fs::read_dir(root)?.flatten().filter(|e| e.path().is_dir()) {
        for s in std::fs::read_dir(d.path())?.flatten().filter(|e| e.path().is_dir()) {
            dirs.push(s.path());
        }
    }
    dirs.sort();
    for dir in dirs {
        let files = wtc_common::sample_files(&dir);
        // (file index, parsed record or raw unparsable line)
        let mut items: Vec<(usize, Result<Record, String>)> = Vec::new();
        for (fi, f) in files.iter().enumerate() {
            wtc_common::read_lines(f, |l| items.push((fi, serde_json::from_str::<Record>(l).map_err(|_| l.to_string()))))?;
        }
        let mut order: Vec<usize> = (0..items.len()).filter(|&i| items[i].1.is_ok()).collect();
        order.sort_by_key(|&i| items[i].1.as_ref().unwrap().seq);
        let mut st = naval::NavalState::default();
        let (mut objs, mut ticks) = (0u64, 0u64);
        let mut touched = vec![false; files.len()];
        for i in order {
            let fi = items[i].0;
            if let Ok(r) = items[i].1.as_mut() {
                let before = (r.body.clone(), r.changed);
                naval::apply(cfg, &mut st, &mut stats, r);
                if (r.body.clone(), r.changed) != before || r.dropped.is_some() {
                    touched[fi] = true;
                }
                objs += r.filtered_naval.unwrap_or(0) as u64;
                ticks += r.dropped.is_some() as u64;
            }
        }
        for (fi, f) in files.iter().enumerate() {
            if !touched[fi] {
                continue;
            }
            let mut lines = Vec::new();
            for (_, it) in items.iter().filter(|x| x.0 == fi) {
                match it {
                    Ok(r) => lines.extend_from_slice(&serde_json::to_vec(r)?),
                    Err(raw) => lines.extend_from_slice(raw.as_bytes()),
                }
                lines.push(b'\n');
            }
            let out = PathBuf::from(format!("{}.gz", f.display().to_string().trim_end_matches(".gz")));
            let tmp = PathBuf::from(format!("{}.tmp", out.display()));
            let mut enc = flate2::write::GzEncoder::new(std::fs::File::create(&tmp)?, flate2::Compression::new(6));
            enc.write_all(&lines)?;
            enc.finish()?.sync_all()?;
            std::fs::rename(&tmp, &out)?;
            if out != *f {
                std::fs::remove_file(f)?;
            }
        }
        println!(
            "{}: {} records, naval objects removed {objs}, naval ticks dropped {ticks}, files rewritten {}",
            dir.display(),
            items.len(),
            touched.iter().filter(|t| **t).count()
        );
    }
    stats.dirty = true;
    write_icon_stats(root, &mut stats);
    println!("unknown icons: {:?}\nunknown types: {:?}", stats.unknown_icons, stats.unknown_types);
    Ok(())
}

// ------------------------------------------------------------------ live map endpoints

#[derive(Deserialize)]
struct LiveQ {
    room: Option<String>,
    key: Option<String>,
    nick: Option<String>,
}

fn room_name(r: Option<&str>, def: &str) -> Option<String> {
    let r = r.filter(|s| !s.is_empty()).unwrap_or(def);
    if r.len() <= 32 && r.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        Some(r.to_string())
    } else {
        None
    }
}

impl App {
    fn view_ok(&self, key: Option<&str>, h: &HeaderMap) -> bool {
        if self.authed(h) {
            return true;
        }
        let hub = &self.hub;
        match key {
            Some(k) => hub.view_key.len() >= 8 && constant_eq(k.as_bytes(), hub.view_key.as_bytes()),
            None => false,
        }
    }
}

/// GET /api/live/check?key=  -> 200 / 401 (lets the settings sheet validate the room password)
async fn live_check(State(app): St, h: HeaderMap, Query(q): Query<LiveQ>) -> Response {
    if !app.view_ok(q.key.as_deref(), &h) {
        return err(StatusCode::UNAUTHORIZED, "bad room password");
    }
    let def = app.hub.default_room.clone();
    Json(serde_json::json!({"ok": true, "default_room": def})).into_response()
}

/// GET /api/live?room=<name>&key=<room password>  (WebSocket)
async fn live_ws(State(app): St, h: HeaderMap, Query(q): Query<LiveQ>, ws: axum::extract::ws::WebSocketUpgrade) -> Response {
    if !app.view_ok(q.key.as_deref(), &h) {
        return err(StatusCode::UNAUTHORIZED, "bad room password");
    }
    let def = app.hub.default_room.clone();
    let Some(room) = room_name(q.room.as_deref(), &def) else { return err(StatusCode::BAD_REQUEST, "bad room") };
    let nick: String = q.nick.unwrap_or_default().chars().take(24).collect();
    ws.max_message_size(1 << 20).on_upgrade(move |sock| live_session(app, room, nick, sock))
}

async fn live_session(app: Arc<App>, room: String, nick: String, sock: axum::extract::ws::WebSocket) {
    use axum::extract::ws::Message;
    use futures_util::{SinkExt, StreamExt};
    let client = format!("c{:032x}", rand::random::<u128>());
    let handle = app.hub.room(&room);
    let mut rx = handle.tx.subscribe();
    let (mut tx_ws, mut rx_ws) = sock.split();
    let hello = |raw: String| { let mut v: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default(); v["you"] = serde_json::json!(client); v.to_string() };
    if tx_ws.send(Message::Text(hello(handle.hello()).into())).await.is_err() { return; }
    eprintln!("[{}] live: viewer {client} ({nick}) joined room {room}", fmt_local(now_ms()));
    loop {
        tokio::select! {
            m = rx.recv() => {
                let msg = match m { Ok(s) => s.to_string(), Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => hello(handle.hello()), Err(_) => break };
                if tx_ws.send(Message::Text(msg.into())).await.is_err() { break; }
            },
            m = rx_ws.next() => match m {
                Some(Ok(Message::Text(t))) => {
                    let Ok(v) = serde_json::from_str::<serde_json::Value>(t.as_str()) else { continue };
                    let response = match v["op"].as_str() {
                        Some("ping") => Some(serde_json::json!({"type": "pong", "c": v["c"], "server_ms": now_ms()}).to_string()),
                        Some("sync") => Some(hello(handle.hello())),
                        Some("new_round") if v["protocol"].as_u64() == Some(2) => { handle.new_round().await; Some(hello(handle.hello())) },
                        Some("put") | Some("del") => match handle.draw(v.clone(), client.clone()).await {
                            Ok(ack) => Some(ack),
                            Err(e) => Some(serde_json::json!({"type": "error", "error": if e == "upgrade_required" { "旧版网页无法提交标绘，请刷新页面升级" } else { &e }, "error_code": e, "read_only": e == "upgrade_required", "op_id": v["op_id"], "snapshot": serde_json::from_str::<serde_json::Value>(&hello(handle.hello())).ok()}).to_string()),
                        },
                        _ => None,
                    };
                    if let Some(s) = response { if tx_ws.send(Message::Text(s.into())).await.is_err() { break; } }
                }
                Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                _ => {},
            },
        }
    }
}

#[derive(Deserialize)]
struct KeyQ {
    key: Option<String>,
}

/// GET /api/map/<sha256>[.ext]?key=<room password>  -> map image bytes (immutable)
async fn map_get(State(app): St, h: HeaderMap, AxPath(file): AxPath<String>, Query(q): Query<KeyQ>) -> Response {
    if !app.view_ok(q.key.as_deref(), &h) {
        return err(StatusCode::UNAUTHORIZED, "bad room password");
    }
    let hash = file.split('.').next().unwrap_or("").to_ascii_lowercase();
    if hash.len() != 64 || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return StatusCode::BAD_REQUEST.into_response();
    }
    let app2 = app.clone();
    let r = tokio::task::spawn_blocking(move || -> Option<Vec<u8>> {
        let lookup = |idx: &HashMap<String, PathBuf>| idx.get(&hash).cloned();
        let mut p = lookup(&app2.img_index.lock().unwrap().1);
        if p.is_none() {
            let dirs: Vec<PathBuf> = app2.inner.lock().unwrap().known_dirs.values().cloned().collect();
            let mut ix = app2.img_index.lock().unwrap();
            if now_ms() - ix.0 > 1500 {
                ix.0 = now_ms();
                for d in dirs {
                    if let Ok(rd) = std::fs::read_dir(d.join("images")) {
                        for e in rd.flatten() {
                            if let Some(stem) = e.path().file_stem().and_then(|s| s.to_str()) {
                                ix.1.insert(stem.to_ascii_lowercase(), e.path());
                            }
                        }
                    }
                }
            }
            p = lookup(&ix.1);
        }
        std::fs::read(p?).ok()
    })
    .await;
    match r {
        Ok(Some(b)) => {
            let ct = match wtc_common::sniff_image(&b) {
                "png" => "image/png",
                "jpg" => "image/jpeg",
                "webp" => "image/webp",
                _ => "application/octet-stream",
            };
            ([(header::CONTENT_TYPE, ct), (header::CACHE_CONTROL, "private, max-age=31536000, immutable")], b).into_response()
        }
        Ok(None) => (StatusCode::NOT_FOUND, [(header::CACHE_CONTROL, "no-store")], "not uploaded yet").into_response(),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
