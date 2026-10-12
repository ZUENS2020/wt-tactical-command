//! NTP-style clock sync against the receiver's GET /api/time.
//! offset = server_time - (t0 + t1)/2 of the min-RTT sample of a burst; server_time ≈ local_time + offset.
use crate::{Cfg, Shared};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};
use wtc_common::Record;

pub const RESYNC: Duration = Duration::from_secs(30);
/// sync considered lost when no successful burst for this long
pub const SYNC_STALE: Duration = Duration::from_secs(300);

#[derive(Default, Clone, Debug)]
pub struct ClockState {
    pub synced: bool,
    pub offset_ms: f64,
    pub rtt_ms: f64,
    pub last_sync: Option<Instant>,
    pub syncs: u64,
    pub fails: u64,
    /// server's staleness threshold (from /api/time)
    pub max_skew_ms: i64,
    pub last_err: String,
}

impl ClockState {
    pub fn valid(&self) -> bool {
        self.synced && self.last_sync.map(|t| t.elapsed() < SYNC_STALE).unwrap_or(false)
    }
}

pub fn local_ms_f64() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Sample {
    pub t0_ms: f64,
    pub server_ms: f64,
    pub rtt_ms: f64,
    pub offset_ms: f64,
}

pub fn time_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(5))
        .pool_max_idle_per_host(1)
        .tcp_nodelay(true)
        .build()
        .expect("http client")
}

pub async fn sample(client: &reqwest::Client, server: &str) -> Result<(Sample, i64), String> {
    let t0 = local_ms_f64();
    let m0 = Instant::now();
    let r = client.get(format!("{server}/api/time")).send().await.map_err(|e| if e.is_timeout() { "timeout".to_string() } else if e.is_connect() { "connect failed".into() } else { "request error".into() })?;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status().as_u16()));
    }
    let b = r.bytes().await.map_err(|_| "read error".to_string())?;
    let rtt = m0.elapsed().as_secs_f64() * 1000.0;
    let v: serde_json::Value = serde_json::from_slice(&b).map_err(|_| "bad /api/time (receiver too old?)".to_string())?;
    let server_ms = v["server_ms"].as_f64().ok_or("bad /api/time (receiver too old?)")?;
    let skew = v["max_skew_ms"].as_i64().unwrap_or(3000);
    // local midpoint measured with the monotonic clock so a local clock step mid-request cannot distort it
    let offset = server_ms - (t0 + rtt / 2.0);
    Ok((Sample { t0_ms: t0, server_ms, rtt_ms: rtt, offset_ms: offset }, skew))
}

/// Burst of n samples; returns the min-RTT sample.
pub async fn burst(client: &reqwest::Client, server: &str, n: usize) -> Result<(Sample, i64, Vec<Sample>), String> {
    let mut all = Vec::new();
    let mut skew = 3000;
    let mut last_err = String::new();
    for i in 0..n {
        match sample(client, server).await {
            Ok((s, k)) => {
                skew = k;
                all.push(s)
            }
            Err(e) => last_err = e,
        }
        if i + 1 < n {
            tokio::time::sleep(Duration::from_millis(60)).await;
        }
    }
    // first sample usually pays TCP/TLS setup: min-RTT selection discards it naturally
    let best = all.iter().min_by(|a, b| a.rtt_ms.partial_cmp(&b.rtt_ms).unwrap()).cloned().ok_or(last_err)?;
    Ok((best, skew, all))
}

fn apply(sh: &Shared, best: &Sample, skew: i64) -> (f64, bool) {
    let mut c = sh.clock.lock().unwrap();
    let prev = if c.synced { Some(c.offset_ms) } else { None };
    let first = !c.synced;
    c.synced = true;
    c.offset_ms = best.offset_ms;
    c.rtt_ms = best.rtt_ms;
    c.last_sync = Some(Instant::now());
    c.syncs += 1;
    c.max_skew_ms = skew;
    c.last_err.clear();
    (best.offset_ms - prev.unwrap_or(best.offset_ms), first)
}

/// Initial sync (blocking, short) used before polling starts.
pub async fn initial(cfg: &Cfg, sh: &Shared) -> Result<Sample, String> {
    let client = time_client();
    let (best, skew, _) = burst(&client, &cfg.server, 8).await?;
    apply(sh, &best, skew);
    Ok(best)
}

/// Periodic re-sync; emits a "#clock" record per burst and the session "#meta" on the first success.
pub async fn sync_loop(cfg: Cfg, sh: Arc<Shared>, tx: mpsc::Sender<Record>, mut meta: Option<String>, mut stop: watch::Receiver<bool>) {
    let client = time_client();
    let mut first_round = true;
    loop {
        let synced_before = sh.clock.lock().unwrap().synced;
        // until synced: retry every 2 s; then every RESYNC
        let wait = if first_round && synced_before { Duration::ZERO } else if synced_before { RESYNC } else { Duration::from_secs(2) };
        first_round = false;
        if !wait.is_zero() {
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = stop.changed() => {}
            }
        }
        if *stop.borrow() {
            break;
        }
        let n = if synced_before { 4 } else { 8 };
        match burst(&client, &cfg.server, n).await {
            Ok((best, skew, all)) => {
                let (step, _first) = apply(&sh, &best, skew);
                let body = serde_json::json!({
                    "offset_ms": best.offset_ms, "rtt_ms": best.rtt_ms, "step_ms": step, "max_skew_ms": skew,
                    "samples": all,
                });
                if let Some(m) = meta.take() {
                    let _ = tx.try_send(sh.synthetic("#meta", m));
                }
                let _ = tx.try_send(sh.synthetic("#clock", body.to_string()));
            }
            Err(e) => {
                let mut c = sh.clock.lock().unwrap();
                c.fails += 1;
                c.last_err = e;
                if c.synced && !c.valid() {
                    c.synced = false;
                }
            }
        }
    }
}
