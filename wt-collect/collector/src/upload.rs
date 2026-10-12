//! Latest-only bounded endpoint slots and an independent immutable-image upload lane. Failed observations are discarded.
use crate::{clock, Cfg, Shared, VERSION};
use anyhow::{bail, Context, Result};
use std::collections::{HashMap, VecDeque};
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

pub fn batch_req(session: &str, player: &str, device_id: &str, nickname: &str, clock: Option<ClockInfo>, records: Vec<Record>) -> Req {
    let b = Batch { session: session.to_string(), player: player.to_string(), device_id: device_id.into(), nickname: nickname.into(), version: VERSION.to_string(), clock, records };
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
    Retry(String), // transient failure; observations are never retried
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
const MAX_IMAGES: usize = 6;
const LOCAL_MARGIN_MS: f64 = 100.0;

pub fn up_client() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(2)).pool_max_idle_per_host(MAX_INFLIGHT)
        .tcp_nodelay(true).build().expect("http client")
}

/// One slot per endpoint. Replacing a slot never reintroduces an older sample.
#[derive(Default)]
struct Latest { slots: HashMap<String, Record> }
impl Latest {
    fn offer(&mut self, r: Record) -> bool {
        if self.slots.get(&r.ep).map(|o| (o.mono_ns, o.seq) >= (r.mono_ns, r.seq)).unwrap_or(false) { return false; }
        self.slots.insert(r.ep.clone(), r); true
    }
    fn take(&mut self, now: f64, budget: f64) -> (Vec<Record>, u64) {
        let all: Vec<_> = self.slots.drain().map(|(_, r)| r).collect();
        let (good, bad): (Vec<_>, Vec<_>) = all.into_iter().partition(|r| now - r.wall_ms as f64 <= budget);
        (good, bad.len() as u64)
    }
}
struct JobOut {
    nrec: usize,
    image: Option<Req>,
    res: Result<Option<serde_json::Value>, SendErr>,
}

pub async fn uploader_loop(cfg: Cfg, sh: Arc<Shared>, mut rx: mpsc::Receiver<UpItem>) {
    let cfg = Arc::new(cfg);
    let client = up_client();
    let mut pending = Latest::default();
    let mut images: VecDeque<Req> = VecDeque::new();
    let mut jobs = tokio::task::JoinSet::<JobOut>::new();
    let (mut record_jobs, mut image_jobs) = (0usize, 0usize);
    let mut offline_until = Instant::now();
    let mut closed = false;
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let (mut rate_t, mut rate_n) = (Instant::now(), 0u64);
    loop {
        let ck = sh.clock.lock().unwrap().clone();
        let budget = wtc_common::LIVE_MAX_AGE_MS as f64 - ck.rtt_ms / 2.0 - LOCAL_MARGIN_MS;
        let now = clock::local_ms_f64();
        let before = pending.slots.len();
        pending.slots.retain(|_, r| now - r.wall_ms as f64 <= budget);
        sh.up.lock().unwrap().drop_too_old += (before - pending.slots.len()) as u64;
        if !closed && ck.valid() && Instant::now() >= offline_until && record_jobs < MAX_INFLIGHT - 1 && !pending.slots.is_empty() {
            let (mut recs, dropped) = pending.take(now, budget);
            sh.up.lock().unwrap().drop_too_old += dropped;
            recs.sort_by_key(|r| r.seq);
            for r in &mut recs { r.ts_server_ms = Some((r.wall_ms as f64 + ck.offset_ms).round() as i64); }
            if !recs.is_empty() {
                let info = { let u = sh.up.lock().unwrap(); ClockInfo { synced: true, offset_ms: ck.offset_ms, rtt_ms: ck.rtt_ms, sync_age_ms: ck.last_sync.map(|t| t.elapsed().as_millis() as u64).unwrap_or(0), syncs: ck.syncs,
                    local_drop_too_old: u.drop_too_old, local_drop_overflow: u.drop_overflow, local_drop_no_sync: u.drop_no_sync, ..Default::default() } };
                let nrec = recs.len();
                let req = batch_req(&sh.session, &cfg.player, &cfg.device_id, cfg.gui.nickname.as_deref().unwrap_or(""), Some(info), recs);
                let (client, cfg) = (client.clone(), cfg.clone());
                jobs.spawn(async move { JobOut { nrec, image: None, res: send(&client, &cfg, &req).await } });
                record_jobs += 1;
            }
        }
        // A reserved resource lane cannot be starved by observations.
        if image_jobs == 0 && Instant::now() >= offline_until {
            if let Some(req) = images.pop_front() {
                let (client, cfg) = (client.clone(), cfg.clone());
                jobs.spawn(async move { let res = send(&client, &cfg, &req).await; JobOut { nrec: 0, image: Some(req), res } });
                image_jobs += 1;
            }
        }
        {
            let mut u = sh.up.lock().unwrap();
            u.inflight = jobs.len(); u.pending = pending.slots.len();
            u.buf_age_ms = pending.slots.values().map(|r| (now - r.wall_ms as f64).max(0.0) as u64).max().unwrap_or(0);
            u.offline = Instant::now() < offline_until;
        }
        if closed { jobs.abort_all(); break; }
        tokio::select! {
            it = rx.recv() => match it {
                None => closed = true,
                Some(UpItem::Batch(recs)) => {
                    sh.up.lock().unwrap().queued -= 1;
                    for r in recs {
                        if !ck.valid() { sh.up.lock().unwrap().drop_no_sync += 1; continue; }
                        if !pending.offer(r) { sh.up.lock().unwrap().drop_overflow += 1; }
                    }
                }
                Some(UpItem::Image { session, hash, ctype, bytes }) => {
                    images.retain(|r| !r.meta.path.contains(&hash));
                    images.push_back(image_req(&session, &hash, &ctype, bytes));
                    while images.len() > MAX_IMAGES { images.pop_front(); }
                }
            },
            Some(j) = jobs.join_next(), if !jobs.is_empty() => {
                let Ok(out) = j else { continue };
                if out.image.is_some() { image_jobs -= 1; } else { record_jobs -= 1; }
                match out.res {
                    Ok(v) => {
                        rate_n += out.nrec as u64;
                        let mut u = sh.up.lock().unwrap();
                        u.ok_batches += 1; u.records_sent += out.nrec as u64; u.last_ok = Some(Instant::now()); u.last_err.clear();
                        if let Some(d) = v.as_ref().and_then(|v| v.get("dropped")) {
                            u.srv_too_old += d["too_old"].as_u64().unwrap_or(0);
                            u.srv_from_future += d["from_future"].as_u64().unwrap_or(0);
                            u.srv_no_sync += d["no_sync"].as_u64().unwrap_or(0);
                        }
                    }
                    Err(e) => {
                        let (m, retry) = match e { SendErr::Retry(m) => (m, true), SendErr::Reject(m) => (m, false) };
                        // Only immutable image resources may retry. Failed observations are discarded.
                        if retry { if let Some(req) = out.image { if images.len() < MAX_IMAGES { images.push_back(req); } } }
                        offline_until = Instant::now() + Duration::from_millis(250);
                        let mut u = sh.up.lock().unwrap(); u.fail += 1; u.rejected += out.nrec as u64; u.last_err = m;
                    }
                }
            }
            _ = tick.tick() => {}
        }
        if rate_t.elapsed().as_secs_f64() >= 1.0 {
            sh.up.lock().unwrap().rate = rate_n as f64 / rate_t.elapsed().as_secs_f64();
            rate_n = 0; rate_t = Instant::now();
        }
    }
}

#[cfg(test)]
mod latest_tests {
    use super::*;
    #[test]
    fn replaces_by_sample_order_and_expires_every_slot() {
        let mut l = Latest::default();
        let r = |seq, mono_ns, wall_ms, ep: &str| Record { seq, mono_ns, wall_ms, ep: ep.into(), ..Default::default() };
        assert!(l.offer(r(5, 100, 1000, "a")));
        assert!(!l.offer(r(6, 90, 900, "a")));
        assert!(l.offer(r(7, 110, 1100, "a")));
        l.offer(r(8, 120, 3000, "b"));
        let (good, dropped) = l.take(3200.0, 2000.0);
        assert_eq!(dropped, 1); assert_eq!(good[0].ep, "b"); assert!(l.slots.is_empty());
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
