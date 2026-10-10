//! Upload to the receiver. 0.1.5: NOTHING is written to disk. Records live in a bounded in-memory buffer
//! (<= MAX_BUF_AGE / MAX_BUF_BYTES); records that would arrive at the server later than the server's
//! staleness threshold are dropped locally (counted), as are records while no clock sync exists.
use crate::{clock, Cfg, Shared, VERSION};
use anyhow::{bail, Context, Result};
use std::collections::VecDeque;
use std::io::Write;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use wtc_common::{Batch, ClockInfo, Record};

pub enum UpItem {
    Batch(Vec<Record>),
    Image { session: String, hash: String, ctype: String, bytes: Vec<u8> },
}

#[derive(Clone, Debug)]
pub struct ReqMeta {
    pub path: String, // path+query on the receiver
    pub ctype: String,
    pub gzip: bool,
}

pub struct Req {
    pub meta: ReqMeta,
    pub body: Vec<u8>,
}

fn gz(b: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::with_capacity(b.len() / 4), flate2::Compression::new(6));
    let _ = e.write_all(b);
    e.finish().unwrap_or_default()
}

pub fn batch_req(session: &str, player: &str, clock: Option<ClockInfo>, records: Vec<Record>) -> Req {
    let b = Batch { session: session.to_string(), player: player.to_string(), version: VERSION.to_string(), clock, records };
    let json = serde_json::to_vec(&b).unwrap_or_default();
    Req { meta: ReqMeta { path: "/api/ingest".into(), ctype: "application/json".into(), gzip: true }, body: gz(&json) }
}

pub fn image_req(session: &str, hash: &str, ctype: &str, bytes: Vec<u8>) -> Req {
    Req {
        meta: ReqMeta {
            path: format!("/api/image?session={}&hash={}&ctype={}", enc(session), enc(hash), enc(ctype)),
            ctype: "application/octet-stream".into(),
            gzip: false,
        },
        body: bytes,
    }
}

fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

pub enum SendErr {
    Retry(String), // network / 5xx / 507 / 401: keep in spool
    Reject(String), // 400: payload broken, move aside
}

pub async fn send(client: &reqwest::Client, cfg: &Cfg, req: &Req) -> Result<Option<serde_json::Value>, SendErr> {
    let mut rb = client
        .post(format!("{}{}", cfg.server, req.meta.path))
        .header("Authorization", format!("Bearer {}", cfg.token))
        .header("Content-Type", req.meta.ctype.clone())
        .body(req.body.clone());
    if req.meta.gzip {
        rb = rb.header("Content-Encoding", "gzip");
    }
    match rb.send().await {
        Ok(r) => {
            let st = r.status().as_u16();
            if (200..300).contains(&st) {
                Ok(r.bytes().await.ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()))
            } else if st == 400 || st == 413 || st == 409 {
                Err(SendErr::Reject(format!("HTTP {st}")))
            } else {
                Err(SendErr::Retry(format!("HTTP {st}")))
            }
        }
        Err(e) => Err(SendErr::Retry(if e.is_timeout() { "timeout".into() } else if e.is_connect() { "connect failed".into() } else { "request error".into() })),
    }
}

pub const MAX_INFLIGHT: usize = 4;
const MAX_REQ_RAW: usize = 2 << 20;
const MAX_REQ_RECS: usize = 20_000;
/// in-memory buffer bound (records older than the server threshold are dropped long before this)
const MAX_BUF_AGE_MS: i64 = 10_000;
const MAX_BUF_BYTES: usize = 8 << 20;
const MAX_IMAGES: usize = 6;
/// safety margin on top of the expected one-way delay when dropping stale records locally
const LOCAL_MARGIN_MS: f64 = 200.0;

fn rec_raw(r: &Record) -> usize {
    r.bytes as usize + r.body_b64.as_ref().map(|b| b.len()).unwrap_or(0) + 260
}

pub fn up_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(8))
        .pool_max_idle_per_host(MAX_INFLIGHT)
        .tcp_nodelay(true)
        .build()
        .expect("http client")
}

struct JobOut {
    nrec: usize,
    res: Result<Option<serde_json::Value>, SendErr>,
    /// records handed back after a retryable failure
    back: Option<Vec<Record>>,
    image: Option<(Req, u32)>,
}

pub async fn uploader_loop(cfg: Cfg, sh: Arc<Shared>, mut rx: mpsc::UnboundedReceiver<UpItem>) {
    let cfg = Arc::new(cfg);
    let client = up_client();
    let mut pending: VecDeque<Record> = VecDeque::new();
    let mut pending_raw = 0usize;
    let mut images: VecDeque<(Req, u32)> = VecDeque::new();
    let mut set = tokio::task::JoinSet::<JobOut>::new();
    let mut offline_until: Option<Instant> = None;
    let mut backoff = Duration::from_millis(500);
    let mut closed = false;
    let mut close_deadline: Option<Instant> = None;
    let mut tick = tokio::time::interval(Duration::from_millis(100));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let (mut rate_t, mut rate_n, mut rate) = (Instant::now(), 0u64, 0f64);

    loop {
        let now = Instant::now();
        let online = offline_until.map(|t| now >= t).unwrap_or(true);
        let ck = sh.clock.lock().unwrap().clone();
        let valid = ck.valid();

        // ---- 1. drop what can no longer be accepted (oldest first)
        {
            let local_now = clock::local_ms_f64();
            let budget = ck.max_skew_ms.max(500) as f64 - ck.rtt_ms / 2.0 - LOCAL_MARGIN_MS;
            let mut u = sh.up.lock().unwrap();
            while let Some(r) = pending.front() {
                let age = local_now - r.wall_ms as f64; // local clock only: offset cancels out
                let drop_reason = if valid && age > budget {
                    Some(0)
                } else if !valid && age > ck.max_skew_ms.max(3000) as f64 {
                    Some(2)
                } else if age > MAX_BUF_AGE_MS as f64 || pending_raw > MAX_BUF_BYTES {
                    Some(1)
                } else {
                    None
                };
                let Some(k) = drop_reason else { break };
                let r = pending.pop_front().unwrap();
                pending_raw = pending_raw.saturating_sub(rec_raw(&r));
                match k {
                    0 => u.drop_too_old += 1,
                    1 => u.drop_overflow += 1,
                    _ => u.drop_no_sync += 1,
                }
            }
        }

        // ---- 2. dispatch: records (only with a valid clock sync), then images
        while online && valid && set.len() < MAX_INFLIGHT && !pending.is_empty() {
            let mut recs = Vec::new();
            let mut raw = 0;
            while let Some(r) = pending.front() {
                if !recs.is_empty() && (raw + rec_raw(r) > MAX_REQ_RAW || recs.len() >= MAX_REQ_RECS) {
                    break;
                }
                let mut r = pending.pop_front().unwrap();
                let rr = rec_raw(&r);
                raw += rr;
                pending_raw = pending_raw.saturating_sub(rr);
                r.ts_server_ms = Some((r.wall_ms as f64 + ck.offset_ms).round() as i64);
                recs.push(r);
            }
            let info = {
                let u = sh.up.lock().unwrap();
                ClockInfo {
                    synced: true,
                    offset_ms: ck.offset_ms,
                    rtt_ms: ck.rtt_ms,
                    sync_age_ms: ck.last_sync.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0),
                    syncs: ck.syncs,
                    local_drop_too_old: u.drop_too_old,
                    local_drop_overflow: u.drop_overflow,
                    local_drop_no_sync: u.drop_no_sync,
                }
            };
            let (c, cf, sess, player) = (client.clone(), cfg.clone(), sh.session.clone(), sh.player.clone());
            set.spawn(async move {
                let n = recs.len();
                let req = batch_req(&sess, &player, Some(info), recs.clone());
                let res = send(&c, &cf, &req).await;
                let back = if matches!(res, Err(SendErr::Retry(_))) { Some(recs) } else { None };
                JobOut { nrec: n, res, back, image: None }
            });
        }
        while online && set.len() < MAX_INFLIGHT {
            let Some((img, tries)) = images.pop_front() else { break };
            let (c, cf) = (client.clone(), cfg.clone());
            set.spawn(async move {
                let res = send(&c, &cf, &img).await;
                let image = if matches!(res, Err(SendErr::Retry(_))) && tries < 5 { Some((img, tries + 1)) } else { None };
                JobOut { nrec: 0, res, back: None, image }
            });
        }

        // ---- 3. status
        {
            let mut u = sh.up.lock().unwrap();
            u.inflight = set.len();
            u.pending = pending.len();
            u.buf_age_ms = pending.front().map(|r| (clock::local_ms_f64() - r.wall_ms as f64).max(0.0) as u64).unwrap_or(0);
            u.rate = rate;
            u.offline = !online;
        }

        if closed {
            let timeout = close_deadline.map(|t| now >= t).unwrap_or(false);
            if (pending.is_empty() && images.is_empty() && set.is_empty()) || timeout {
                set.abort_all();
                break;
            }
        }

        tokio::select! {
            it = rx.recv(), if !closed => {
                match it {
                    None => { closed = true; close_deadline = Some(Instant::now() + Duration::from_secs(5)); }
                    Some(UpItem::Batch(recs)) => {
                        sh.up.lock().unwrap().queued -= 1;
                        for r in recs {
                            pending_raw += rec_raw(&r);
                            pending.push_back(r);
                        }
                    }
                    Some(UpItem::Image { session, hash, ctype, bytes }) => {
                        images.push_back((image_req(&session, &hash, &ctype, bytes), 0));
                        while images.len() > MAX_IMAGES { images.pop_front(); }
                    }
                }
            }
            Some(j) = set.join_next(), if !set.is_empty() => {
                let Ok(out) = j else { continue };
                match out.res {
                    Ok(v) => {
                        backoff = Duration::from_millis(500);
                        offline_until = None;
                        rate_n += out.nrec as u64;
                        let mut u = sh.up.lock().unwrap();
                        u.ok_batches += 1; u.records_sent += out.nrec as u64; u.last_ok = Some(Instant::now()); u.last_err.clear();
                        if let Some(d) = v.as_ref().and_then(|v| v.get("dropped")) {
                            u.srv_too_old += d["too_old"].as_u64().unwrap_or(0);
                            u.srv_from_future += d["from_future"].as_u64().unwrap_or(0);
                            u.srv_no_sync += d["no_sync"].as_u64().unwrap_or(0);
                        }
                    }
                    Err(SendErr::Reject(m)) => {
                        let mut u = sh.up.lock().unwrap();
                        u.fail += 1;
                        u.rejected += out.nrec as u64;
                        u.last_err = m;
                    }
                    Err(SendErr::Retry(m)) => {
                        if let Some(mut back) = out.back {
                            // put back in front (keep seq order); stale ones are dropped in step 1
                            back.sort_by_key(|r| std::cmp::Reverse(r.seq));
                            for r in back {
                                pending_raw += rec_raw(&r);
                                pending.push_front(r);
                            }
                            pending.make_contiguous().sort_by_key(|r| r.seq);
                        }
                        if let Some(img) = out.image { images.push_front(img); }
                        offline_until = Some(Instant::now() + backoff);
                        backoff = (backoff * 2).min(Duration::from_secs(2));
                        let mut u = sh.up.lock().unwrap();
                        u.fail += 1; u.last_err = m;
                    }
                }
            }
            _ = tick.tick() => {}
        }
        let el = rate_t.elapsed().as_secs_f64();
        if el >= 1.0 {
            let inst = rate_n as f64 / el;
            rate = if rate == 0.0 { inst } else { rate * 0.6 + inst * 0.4 };
            rate_n = 0;
            rate_t = Instant::now();
        }
    }
    let left = pending.len();
    if left > 0 {
        crate::say!("[upload] {left} record(s) could not be sent before exit (not stored anywhere, by design)");
    }
}

pub async fn send_note(cfg: &Cfg, text: &str, session: Option<&str>) -> Result<()> {
    let client = up_client();
    let body = serde_json::json!({ "text": text, "player": cfg.player, "session": session });
    let r = client
        .post(format!("{}/api/note", cfg.server))
        .header("Authorization", format!("Bearer {}", cfg.token))
        .json_body(&body)
        .send()
        .await
        .context("send note")?;
    let st = r.status();
    let t = r.text().await.unwrap_or_default();
    if !st.is_success() {
        bail!("receiver: HTTP {st}: {t}");
    }
    println!("note stored: {t}");
    Ok(())
}

trait JsonBody {
    fn json_body(self, v: &serde_json::Value) -> Self;
}
impl JsonBody for reqwest::RequestBuilder {
    fn json_body(self, v: &serde_json::Value) -> Self {
        self.header("Content-Type", "application/json").body(serde_json::to_vec(v).unwrap_or_default())
    }
}

// ---------------------------------------------------------------- labels (0.2.0)

/// POST /api/label. Ok(server json) / Err((retryable, message)).
pub async fn send_labels(client: &reqwest::Client, cfg: &Cfg, b: &crate::labels::LabelBatch) -> Result<serde_json::Value, String> {
    let r = client
        .post(format!("{}/api/label", cfg.server))
        .header("Authorization", format!("Bearer {}", cfg.token))
        .json_body(&serde_json::to_value(b).unwrap_or_default())
        .send()
        .await
        .map_err(|e| if e.is_timeout() { "timeout".to_string() } else if e.is_connect() { "connect failed".into() } else { "request error".into() })?;
    let st = r.status().as_u16();
    let t = r.text().await.unwrap_or_default();
    if (200..300).contains(&st) {
        Ok(serde_json::from_str(&t).unwrap_or(serde_json::Value::Null))
    } else if st == 404 || st == 405 {
        Err("服务器不支持标签（接收端需升级）HTTP 404".into())
    } else {
        Err(format!("HTTP {st}"))
    }
}

/// Dedicated label sender: independent of the clock guard, the hangar pause and the telemetry buffer.
/// Labels stay queued (and are retried with backoff up to 10 s) until the receiver confirms them.
pub async fn label_loop(cfg: Cfg, sh: Arc<Shared>, mut stop: tokio::sync::watch::Receiver<bool>) {
    let client = up_client();
    let mut backoff = Duration::from_millis(500);
    let mut stopping: Option<Instant> = None;
    loop {
        let batch: Vec<crate::labels::LabelEvent> = sh.labels.lock().unwrap().queue.iter().take(50).cloned().collect();
        if batch.is_empty() {
            if stopping.is_some() {
                break;
            }
            tokio::select! {
                _ = sh.label_notify.notified() => {}
                _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                _ = stop.changed() => { if *stop.borrow() { stopping.get_or_insert(Instant::now()); } }
            }
            continue;
        }
        let ck = sh.clock_view();
        let lb = crate::labels::LabelBatch {
            session: sh.session.clone(),
            player: sh.player.clone(),
            version: VERSION.into(),
            clock_synced_now: ck.synced,
            clock_offset_now_ms: ck.offset_ms,
            sent_local_ms: wtc_common::now_ms(),
            labels: batch,
        };
        match send_labels(&client, &cfg, &lb).await {
            Ok(_) => {
                backoff = Duration::from_millis(500);
                let uids: Vec<String> = lb.labels.iter().map(|l| l.uid.clone()).collect();
                let mut b = sh.labels.lock().unwrap();
                b.queue.retain(|q| !uids.contains(&q.uid));
                b.mark_sent(&uids);
                b.last_ok = Some(Instant::now());
                b.last_err.clear();
            }
            Err(e) => {
                {
                    let mut b = sh.labels.lock().unwrap();
                    b.fail += 1;
                    b.last_err = e;
                }
                if stopping.map(|t| t.elapsed() > Duration::from_secs(5)).unwrap_or(false) {
                    break;
                }
                tokio::select! {
                    _ = tokio::time::sleep(backoff) => {}
                    _ = stop.changed() => { if *stop.borrow() { stopping.get_or_insert(Instant::now()); } }
                }
                backoff = (backoff * 2).min(Duration::from_secs(10));
            }
        }
    }
    let left = sh.labels.lock().unwrap().queue.len();
    if left > 0 {
        crate::say!("[标签] {left} 个标签未能发送（退出前服务器不可达）");
    }
}
