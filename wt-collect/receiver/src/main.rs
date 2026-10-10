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
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt::Write as _;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use wtc_common::{now_ms, Batch, Record};

mod guard;
mod labels;
mod markers;
mod naval;

struct App {
    root: PathBuf,
    token: String,
    max_bytes: u64,
    min_free: u64,
    started_ms: i64,
    inner: Mutex<Inner>,
    naval_cfg: naval::NavalCfg,
    icon_stats: Mutex<naval::IconStats>,
    guard_cfg: guard::GuardCfg,
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
    seqs: Option<HashSet<u64>>,
    file_idx: u32,
    file_size: u64,
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
    naval: naval::NavalState,
    naval_objs: u64,
    naval_ticks: u64,
    guard: guard::GuardStats,
    guard_written_ms: i64,
    guard_dirty: bool,
    /// latest #state / #heartbeat of a 0.2.0 collector: (recv_ms, paused, reason)
    gate: Option<(i64, bool, String)>,
    labels: u64,
    /// squad / map ping / radio derivation (markers.rs) -> markers.jsonl
    markers: markers::Markers,
}

/// Append records (already serialized lines) to the session's current samples file + seq.idx.
fn write_lines(s: &mut Sess, lines: &[u8], new_seqs: &[u8]) -> std::io::Result<u64> {
    if s.file_size > 128 * 1024 * 1024 {
        s.file_idx += 1;
        s.file_size = 0;
    }
    let path = s.dir.join(format!("samples-{:04}.jsonl.gz", s.file_idx));
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
    enc.write_all(lines)?;
    let member = enc.finish()?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    f.write_all(&member)?;
    f.sync_data().ok();
    let mut fi = std::fs::OpenOptions::new().create(true).append(true).open(s.dir.join("seq.idx"))?;
    fi.write_all(new_seqs)?;
    s.file_size += member.len() as u64;
    s.raw_bytes += lines.len() as u64;
    Ok(member.len() as u64 + new_seqs.len() as u64)
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
        if !inner.sessions.contains_key(id) {
            let dir = match inner.known_dirs.get(id) {
                Some(d) => d.clone(),
                None => {
                    let date = if id.len() >= 8 && id[..8].chars().all(|c| c.is_ascii_digit()) {
                        format!("{}-{}-{}", &id[0..4], &id[4..6], &id[6..8])
                    } else {
                        chrono::Local::now().format("%Y-%m-%d").to_string()
                    };
                    self.root.join(date).join(id)
                }
            };
            std::fs::create_dir_all(dir.join("images"))?;
            let meta_p = dir.join("session.json");
            let (mut pl, mut ver) = (player.to_string(), version.to_string());
            if let Ok(b) = std::fs::read(&meta_p) {
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&b) {
                    if pl.is_empty() {
                        pl = v["player"].as_str().unwrap_or("").into();
                    }
                    if ver.is_empty() {
                        ver = v["version"].as_str().unwrap_or("").into();
                    }
                }
            } else {
                let m = serde_json::json!({"session": id, "player": player, "version": version, "first_recv_ms": now_ms()});
                std::fs::write(&meta_p, serde_json::to_vec_pretty(&m).unwrap())?;
            }
            // continue numbering of sample files
            let files = wtc_common::sample_files(&dir);
            let file_idx = files.len() as u32;
            let file_size = files.last().and_then(|f| f.metadata().ok()).map(|m| m.len()).unwrap_or(u64::MAX);
            let mut notes = Vec::new();
            if let Ok(b) = std::fs::read_to_string(dir.join("notes.jsonl")) {
                for l in b.lines() {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(l) {
                        notes.push((v["wall_ms"].as_i64().unwrap_or(0), v["text"].as_str().unwrap_or("").to_string()));
                    }
                }
            }
            let s = Sess {
                id: id.to_string(),
                dir: dir.clone(),
                player: pl,
                version: ver,
                file_idx: file_idx.max(1),
                file_size: if file_idx == 0 { 0 } else { file_size },
                notes,
                guard: std::fs::read(dir.join("guard.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default(),
                ..Default::default()
            };
            inner.known_dirs.insert(id.to_string(), dir);
            inner.sessions.insert(id.to_string(), s);
        }
        let s = inner.sessions.get_mut(id).unwrap();
        if s.seqs.is_none() {
            let mut set = HashSet::new();
            if let Ok(b) = std::fs::read(s.dir.join("seq.idx")) {
                for c in b.chunks_exact(8) {
                    set.insert(u64::from_le_bytes(c.try_into().unwrap()));
                }
            }
            s.seqs = Some(set);
        }
        Ok(s)
    }

    fn ingest(&self, batch: Batch) -> Result<serde_json::Value, (StatusCode, String)> {
        if !safe_id(&batch.session) {
            return Err((StatusCode::BAD_REQUEST, "bad session id".into()));
        }
        let mut inner = self.inner.lock().unwrap();
        self.guard(&inner, 0)?;
        let ie = |e: std::io::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
        let s = self.sess(&mut inner, &batch.session, &batch.player, &batch.version).map_err(ie)?;
        let now = now_ms();
        s.last_recv = now;
        let mut lines = Vec::<u8>::new();
        let mut new_seqs = Vec::<u8>::new();
        let (mut acc, mut dup, mut foreign) = (0u64, 0u64, 0u64);
        let mode = guard::batch_mode(&self.guard_cfg, &batch);
        if s.guard.max_skew_ms == 0 {
            s.guard.max_skew_ms = self.guard_cfg.max_skew_ms;
        }
        if let Some(c) = &batch.clock {
            s.guard.note_clock(c);
        }
        let (mut too_old, mut future, mut no_sync, mut legacy) = (0u64, 0u64, 0u64, 0u64);
        let mut records = batch.records;
        if mode == guard::Mode::RejectNoSync {
            no_sync = records.len() as u64;
            s.guard.no_sync += no_sync;
            s.guard.batches_rejected_no_sync += 1;
            records.clear();
        }
        records.sort_by_key(|r| r.seq);
        let mut ist = self.icon_stats.lock().unwrap();
        for mut r in records {
            if r.session != batch.session {
                foreign += 1;
                continue;
            }
            r.recv_ms = Some(now);
            if let guard::Mode::Synced(off) = mode {
                let ts = *r.ts_server_ms.get_or_insert(r.wall_ms + off.round() as i64);
                match guard::verdict(&self.guard_cfg, ts, now) {
                    guard::Verdict::Accept => {}
                    guard::Verdict::TooOld => {
                        too_old += 1;
                        continue;
                    }
                    guard::Verdict::FromFuture => {
                        future += 1;
                        continue;
                    }
                }
            }
            if s.seqs.as_ref().unwrap().contains(&r.seq) {
                dup += 1;
                continue;
            }
            if let Some(ts) = r.ts_server_ms {
                s.guard.note_delay(now - ts);
            }
            if mode == guard::Mode::Legacy {
                legacy += 1;
            }
            if !s.seqs.as_mut().unwrap().insert(r.seq) {
                dup += 1;
                continue;
            }
            acc += 1;
            // naval filter BEFORE anything is written
            naval::apply(&self.naval_cfg, &mut s.naval, &mut ist, &mut r);
            s.naval_objs += r.filtered_naval.unwrap_or(0) as u64;
            if r.dropped.is_some() {
                s.naval_ticks += 1;
            }
            new_seqs.extend_from_slice(&r.seq.to_le_bytes());
            if let Ok(j) = serde_json::to_vec(&r) {
                lines.extend_from_slice(&j);
                lines.push(b'\n');
            }
            live_update(s, &r, now);
            s.markers.feed(&r);
        }
        drop(ist);
        if !s.markers.out.is_empty() {
            let mut buf = Vec::new();
            for v in s.markers.out.drain(..) {
                if let Ok(j) = serde_json::to_vec(&v) {
                    buf.extend_from_slice(&j);
                    buf.push(b'\n');
                }
            }
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(s.dir.join("markers.jsonl")) {
                let _ = f.write_all(&buf);
            }
        }
        s.guard.accepted += acc;
        s.guard.too_old += too_old;
        s.guard.from_future += future;
        s.guard.legacy_accepted += legacy;
        s.guard_dirty = true;
        if too_old + future + no_sync > 0 || now - s.guard_written_ms > 2000 {
            s.guard.updated_ms = now;
            if let Ok(b) = serde_json::to_vec_pretty(&s.guard) {
                let tmp = s.dir.join("guard.json.tmp");
                if std::fs::write(&tmp, b).is_ok() {
                    let _ = std::fs::rename(&tmp, s.dir.join("guard.json"));
                }
            }
            s.guard_written_ms = now;
            s.guard_dirty = false;
        }
        s.dups += dup;
        s.records += acc;
        let mut written = 0u64;
        if acc > 0 {
            written = write_lines(s, &lines, &new_seqs).map_err(ie)?;
        }
        inner.data_bytes += written;
        Ok(serde_json::json!({"accepted": acc, "dup": dup, "foreign": foreign,
            "dropped": {"too_old": too_old, "from_future": future, "no_sync": no_sync},
            "legacy": mode == guard::Mode::Legacy, "server_ms": now}))
    }
}

impl App {
    /// POST /api/label: store label events. Never dropped for clock skew (flagged `late` instead).
    fn ingest_labels(&self, b: labels::LabelBatchIn) -> Result<serde_json::Value, (StatusCode, String)> {
        if !safe_id(&b.session) {
            return Err((StatusCode::BAD_REQUEST, "bad session id".into()));
        }
        if b.labels.len() > 1000 {
            return Err((StatusCode::PAYLOAD_TOO_LARGE, "too many labels".into()));
        }
        let mut inner = self.inner.lock().unwrap();
        self.guard(&inner, 0)?;
        let ie = |e: std::io::Error| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
        let now = now_ms();
        let s = self.sess(&mut inner, &b.session, &b.player, &b.version).map_err(ie)?;
        s.last_recv = now;
        let (mut lines, mut flat, mut new_seqs) = (Vec::<u8>::new(), Vec::<u8>::new(), Vec::<u8>::new());
        let (mut acc, mut dup) = (0u64, 0u64);
        let mut bad = Vec::new();
        let mut uids = Vec::new();
        let items = b.labels.clone();
        for v in items {
            let uid = v["uid"].as_str().unwrap_or("").to_string();
            let r = match labels::to_record(&b, v, now, self.guard_cfg.max_skew_ms) {
                Ok(r) => r,
                Err(e) => {
                    bad.push(serde_json::json!({"uid": uid, "error": e}));
                    continue;
                }
            };
            uids.push(uid);
            if !s.seqs.as_mut().unwrap().insert(r.seq) {
                dup += 1;
                continue;
            }
            acc += 1;
            new_seqs.extend_from_slice(&r.seq.to_le_bytes());
            if let Ok(j) = serde_json::to_vec(&r) {
                lines.extend_from_slice(&j);
                lines.push(b'\n');
            }
            flat.extend_from_slice(r.body.as_deref().unwrap_or("{}").as_bytes());
            flat.push(b'\n');
            live_update(s, &r, now);
        }
        let mut written = 0;
        if acc > 0 {
            written = write_lines(s, &lines, &new_seqs).map_err(ie)?;
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(s.dir.join("labels.jsonl")).map_err(ie)?;
            f.write_all(&flat).map_err(ie)?;
            f.sync_data().ok();
            written += flat.len() as u64;
            s.records += acc;
            s.labels += acc;
        }
        s.dups += dup;
        let sid = s.id.clone();
        inner.data_bytes += written;
        if acc > 0 {
            eprintln!("[{}] labels: session {sid} +{acc} (dup {dup}, bad {})", fmt_local(now), bad.len());
        }
        // malformed labels are reported but the batch is confirmed: retrying them would never succeed
        Ok(serde_json::json!({"accepted": acc, "dup": dup, "bad": bad, "uids": uids, "session": sid, "server_ms": now}))
    }

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

fn live_update(s: &mut Sess, r: &Record, now: i64) {
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
            if let Some(v) = r.body.as_deref().and_then(|b| serde_json::from_str::<serde_json::Value>(b).ok()) {
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
            if let Some(v) = r.body.as_deref().and_then(|b| serde_json::from_str::<serde_json::Value>(b).ok()) {
                s.gate = Some((now, v["paused"].as_bool().unwrap_or(false), v["reason"].as_str().unwrap_or("").to_string()));
            }
        }
        "/map.img" => {
            if r.img_hash.is_some() {
                s.latest_img = r.img_hash.clone();
            }
        }
        "/map_info.json" | "/map_obj.json" | "/mission.json" | "/indicators" => {
            let Some(b) = &r.body else { return };
            let Ok(v) = serde_json::from_str::<serde_json::Value>(b) else { return };
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
    let raw = if gz {
        let mut out = Vec::new();
        let d = flate2::read::MultiGzDecoder::new(&body[..]);
        if d.take(1 << 30).read_to_end(&mut out).is_err() {
            return err(StatusCode::BAD_REQUEST, "bad gzip");
        }
        out
    } else {
        body.to_vec()
    };
    let batch: Batch = match serde_json::from_slice(&raw) {
        Ok(b) => b,
        Err(e) => return err(StatusCode::BAD_REQUEST, format!("bad json: {e}")),
    };
    let app2 = app.clone();
    match tokio::task::spawn_blocking(move || app2.ingest(batch)).await {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err((c, m))) => err(c, m),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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
        let mut inner = app2.inner.lock().unwrap();
        app2.guard(&inner, body.len() as u64)?;
        let s = app2.sess(&mut inner, &q.session, "", "").map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        s.last_recv = now_ms();
        let ext = wtc_common::sniff_image(&body);
        let p = s.dir.join("images").join(format!("{}.{ext}", q.hash));
        let mut new = false;
        if !p.exists() {
            std::fs::write(&p, &body).map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            new = true;
        }
        if new {
            inner.data_bytes += body.len() as u64;
        }
        Ok(serde_json::json!({"stored": new, "bytes": body.len()}))
    })
    .await;
    match r {
        Ok(Ok(v)) => Json(v).into_response(),
        Ok(Err((c, m))) => err(c, m),
        Err(e) => err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[derive(Deserialize)]
struct NoteReq {
    text: String,
    session: Option<String>,
    player: Option<String>,
}

async fn note(State(app): St, h: HeaderMap, Json(n): Json<NoteReq>) -> Response {
    if !app.authed(&h) {
        return err(StatusCode::UNAUTHORIZED, "bad token");
    }
    let mut inner = app.inner.lock().unwrap();
    let sid = match n.session.clone().filter(|s| !s.is_empty()) {
        Some(s) => Some(s),
        None => {
            let pl = n.player.clone().filter(|p| !p.is_empty());
            inner
                .sessions
                .values()
                .filter(|s| pl.as_ref().map(|p| &s.player == p).unwrap_or(true))
                .max_by_key(|s| s.last_recv)
                .or_else(|| inner.sessions.values().max_by_key(|s| s.last_recv))
                .map(|s| s.id.clone())
        }
    };
    let Some(sid) = sid else { return err(StatusCode::NOT_FOUND, "no session yet") };
    if !safe_id(&sid) {
        return err(StatusCode::BAD_REQUEST, "bad session");
    }
    let s = match app.sess(&mut inner, &sid, "", "") {
        Ok(s) => s,
        Err(e) => return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    };
    let w = now_ms();
    let line = serde_json::json!({"wall_ms": w, "text": n.text, "source": "api", "player": n.player});
    let res = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(s.dir.join("notes.jsonl"))
        .and_then(|mut f| f.write_all(format!("{line}\n").as_bytes()));
    if let Err(e) = res {
        return err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    s.notes.push((w, n.text.clone()));
    Json(serde_json::json!({"session": sid, "wall_ms": w})).into_response()
}

async fn label_post(State(app): St, h: HeaderMap, body: Bytes) -> Response {
    if !app.authed(&h) {
        return err(StatusCode::UNAUTHORIZED, "bad token");
    }
    let b: labels::LabelBatchIn = match serde_json::from_slice(&body) {
        Ok(b) => b,
        Err(e) => return err(StatusCode::BAD_REQUEST, format!("bad json: {e}")),
    };
    match app.ingest_labels(b) {
        Ok(v) => Json(v).into_response(),
        Err((c, m)) => err(c, m),
    }
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
    let app = Arc::new(App {
        guard_cfg,
        naval_cfg,
        icon_stats: Mutex::new(icon_stats),
        root,
        token,
        max_bytes: (max_gb * 1073741824.0) as u64,
        min_free: (min_free_gb * 1073741824.0) as u64,
        started_ms: now_ms(),
        inner: Mutex::new(inner),
    });
    let r = Router::new()
        .route("/", get(index))
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/status", get(status_json))
        .route("/api/time", get(api_time))
        .route("/api/ingest", post(ingest))
        .route("/api/image", post(image))
        .route("/api/note", post(note))
        .route("/api/label", post(label_post))
        .route("/api/labels", get(labels_get))
        .route("/api/markers", get(markers_get))
        .route("/img/{session}/{file}", get(img_get))
        .layer(DefaultBodyLimit::max(256 * 1024 * 1024))
        .with_state(app.clone());
    {
        // persist unknown-icon counters
        let app = app.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                write_icon_stats(&app.root, &mut app.icon_stats.lock().unwrap());
                let mut inner = app.inner.lock().unwrap();
                for s in inner.sessions.values_mut().filter(|s| s.guard_dirty) {
                    s.guard.updated_ms = now_ms();
                    if let Ok(b) = serde_json::to_vec_pretty(&s.guard) {
                        let tmp = s.dir.join("guard.json.tmp");
                        if std::fs::write(&tmp, b).is_ok() && std::fs::rename(&tmp, s.dir.join("guard.json")).is_ok() {
                            s.guard_dirty = false;
                        }
                    }
                }
            }
        });
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
    write_icon_stats(&app.root, &mut app.icon_stats.lock().unwrap());
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
