//! wt-collector: polls the local War Thunder web API (8111/9222, this PC only) and uploads every raw
//! sample to the wt-receiver (HK server). Nothing is stored locally. 0.2.0: GUI with labeling buttons
//! (default when started without arguments on Windows), hangar upload pause (gate.rs), labels (labels.rs).
mod diag;
mod game;
mod clock;
mod upload;
pub mod gate;
pub mod labels;
#[cfg(feature = "gui")]
mod gui;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch, Notify};
use wtc_common::{now_ms, Record};

// The collector never talks to anything but the local game API: loopback or this PC's own
// private LAN IP, port 8111 or 9222 (auto-detected; see game.rs).
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_SERVER: &str = "http://43.129.251.18";

/// GUI mode: console output is muted (no console window); lines go to the in-memory log shown in the window.
pub static QUIET: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static LOG: Mutex<std::collections::VecDeque<String>> = Mutex::new(std::collections::VecDeque::new());

pub fn say_line(s: String) {
    if !QUIET.load(Ordering::Relaxed) {
        println!("{s}");
    }
    let mut l = LOG.lock().unwrap();
    l.push_back(format!("{} {}", chrono::Local::now().format("%H:%M:%S"), s.trim()));
    while l.len() > 200 {
        l.pop_front();
    }
}
#[macro_export]
macro_rules! say {
    ($($t:tt)*) => { $crate::say_line(format!($($t)*)) };
}

#[derive(Parser, Debug)]
#[command(name = "wt-collector", version, about = "War Thunder 8111 raw data collector (test pipeline)")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
    /// Receiver URL (default from wt-collector.toml, else http://43.129.251.18)
    #[arg(long, global = true)]
    server: Option<String>,
    /// Shared upload token
    #[arg(long, global = true)]
    token: Option<String>,
    /// Tester name / label
    #[arg(long, global = true)]
    player: Option<String>,
    /// Config file (default: wt-collector.toml next to the exe)
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// Game web-API port: 8111, 9222, ... or "auto" (default: probe 8111 then 9222)
    #[arg(long, global = true)]
    game_port: Option<String>,
    /// Game web-API host: 127.0.0.1 (default) or this PC's own LAN IP; "auto" also tries LAN IPs
    #[arg(long, global = true)]
    game_host: Option<String>,
    /// Rate preset for the fast group (map_obj, indicators, state): 2 / 5 / 10 / 20 Hz
    #[arg(long)]
    rate: Option<f64>,
    #[arg(long)]
    map_obj_hz: Option<f64>,
    #[arg(long)]
    indicators_hz: Option<f64>,
    #[arg(long)]
    state_hz: Option<f64>,
    #[arg(long)]
    map_info_hz: Option<f64>,
    #[arg(long)]
    mission_hz: Option<f64>,
    #[arg(long)]
    hudmsg_hz: Option<f64>,
    #[arg(long)]
    gamechat_hz: Option<f64>,
    /// Seconds between /map.img checks
    #[arg(long)]
    map_img_check_s: Option<f64>,
    /// Note/label recorded at session start (repeatable). You can also type a line + Enter while running.
    #[arg(long)]
    note: Vec<String>,
    /// Automatically cycle the fast group through 2/5/10/20 Hz, N seconds each (rate test; a note is recorded at each switch)
    #[arg(long)]
    rate_cycle_s: Option<f64>,
    /// Stop automatically after N seconds (testing)
    #[arg(long)]
    duration_s: Option<f64>,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Collect (default when no subcommand is given)
    Run,
    /// Diagnose 8111 connectivity (direct vs proxy, 127.0.0.1 vs localhost, proxy settings) and the upload server
    Diag,
    /// Send a note/label to the receiver (attached to your latest active session)
    Note {
        text: String,
        #[arg(long)]
        session: Option<String>,
    },
    /// Open the window with labeling buttons (default when the exe is started without arguments on Windows)
    Gui,
    /// Send one label event right now (testing; source "cli"), e.g. `wt-collector label kill --note test`
    Label {
        /// label id from the config (e.g. i_scouted, kill, note)
        id: String,
        #[arg(long)]
        note: Option<String>,
        /// session id (default: a one-off "cli" session)
        #[arg(long)]
        session: Option<String>,
        /// also mark it with these tags (e.g. --tag test)
        #[arg(long)]
        tag: Vec<String>,
    },
    /// Print the label buttons configured in wt-collector.toml
    Labels,
}

#[derive(Deserialize, Default, Debug)]
struct FileCfg {
    server: Option<String>,
    token: Option<String>,
    player: Option<String>,
    rate: Option<f64>,
    rates: Option<RatesCfg>,
    game_port: Option<toml::Value>,
    game_host: Option<String>,
    labels: Option<Vec<labels::LabelDef>>,
    gui: Option<GuiCfg>,
}

/// `[gui]` section of wt-collector.toml
#[derive(Deserialize, Debug, Clone)]
#[serde(default)]
pub struct GuiCfg {
    /// window stays above other windows (toggle in the window)
    pub always_on_top: bool,
    /// close button: "ask" (default) | "minimize" | "exit"
    pub on_close: String,
    /// font file with Chinese glyphs (default: Microsoft YaHei / SimHei / SimSun from C:\Windows\Fonts)
    pub font: Option<String>,
    /// UI scale (1.0 = 100 %)
    pub scale: f32,
    /// buttons per row
    pub columns: usize,
}
impl Default for GuiCfg {
    fn default() -> Self {
        GuiCfg { always_on_top: true, on_close: "ask".into(), font: None, scale: 1.0, columns: 2 }
    }
}
#[derive(Deserialize, Default, Debug)]
struct RatesCfg {
    map_obj: Option<f64>,
    indicators: Option<f64>,
    state: Option<f64>,
    map_info: Option<f64>,
    mission: Option<f64>,
    hudmsg: Option<f64>,
    gamechat: Option<f64>,
    map_img_check_s: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct Rates {
    pub map_obj: f64,
    pub indicators: f64,
    pub state: f64,
    pub map_info: f64,
    pub mission: f64,
    pub hudmsg: f64,
    pub gamechat: f64,
    pub map_img_check_s: f64,
}

#[derive(Debug, Clone)]
pub struct Cfg {
    pub server: String,
    pub token: String,
    pub player: String,
    pub rates: Rates,
    pub notes: Vec<String>,
    pub config_path: Option<PathBuf>,
    pub duration_s: Option<f64>,
    pub rate_cycle_s: Option<f64>,
    /// None = auto (127.0.0.1 + LAN IPs)
    pub game_host: Option<String>,
    /// None = auto (8111, 9222)
    pub game_port: Option<u16>,
    pub labels: Vec<labels::LabelDef>,
    pub gui: GuiCfg,
}

fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

fn load_cfg(cli: &Cli) -> Result<Cfg> {
    let candidates: Vec<PathBuf> = match &cli.config {
        Some(p) => vec![p.clone()],
        None => vec![exe_dir().join("wt-collector.toml"), PathBuf::from("wt-collector.toml")],
    };
    let mut fc = FileCfg::default();
    let mut config_path = None;
    for c in candidates {
        if c.exists() {
            let s = std::fs::read_to_string(&c).with_context(|| format!("read {}", c.display()))?;
            fc = toml::from_str(s.trim_start_matches('\u{feff}')).with_context(|| format!("parse {}", c.display()))?;
            config_path = Some(c);
            break;
        }
    }
    let rc = fc.rates.unwrap_or_default();
    let preset = cli.rate.or(fc.rate);
    let fast = |cli_v: Option<f64>, file_v: Option<f64>| -> f64 {
        cli_v.or(preset).or(file_v).unwrap_or(10.0)
    };
    let clamp = |v: f64| v.clamp(0.05, 50.0);
    let rates = Rates {
        map_obj: clamp(fast(cli.map_obj_hz, rc.map_obj)),
        indicators: clamp(fast(cli.indicators_hz, rc.indicators)),
        state: clamp(fast(cli.state_hz, rc.state)),
        map_info: clamp(cli.map_info_hz.or(rc.map_info).unwrap_or(1.0)),
        mission: clamp(cli.mission_hz.or(rc.mission).unwrap_or(1.0)),
        hudmsg: clamp(cli.hudmsg_hz.or(rc.hudmsg).unwrap_or(2.0)),
        gamechat: clamp(cli.gamechat_hz.or(rc.gamechat).unwrap_or(2.0)),
        map_img_check_s: cli.map_img_check_s.or(rc.map_img_check_s).unwrap_or(5.0).clamp(0.5, 600.0),
    };
    let player = cli
        .player
        .clone()
        .or(fc.player)
        .unwrap_or_else(|| std::env::var("USERNAME").or_else(|_| std::env::var("USER")).unwrap_or_else(|_| "tester".into()));
    let label_defs = labels::resolve_labels(fc.labels.take()).map_err(|e| anyhow::anyhow!("wt-collector.toml: {e}"))?;
    let mut gui_cfg = fc.gui.take().unwrap_or_default();
    gui_cfg.scale = gui_cfg.scale.clamp(0.6, 3.0);
    gui_cfg.columns = gui_cfg.columns.clamp(1, 6);
    Ok(Cfg {
        labels: label_defs,
        gui: gui_cfg,
        server: cli.server.clone().or(fc.server).unwrap_or_else(|| DEFAULT_SERVER.into()).trim_end_matches('/').to_string(),
        token: cli.token.clone().or(fc.token).unwrap_or_default(),
        player,
        rates,
        notes: cli.note.clone(),
        config_path,
        duration_s: cli.duration_s,
        rate_cycle_s: cli.rate_cycle_s.map(|v| v.max(5.0)),
        game_port: {
            let raw = cli.game_port.clone().or_else(|| fc.game_port.as_ref().map(|v| match v {
                toml::Value::String(s) => s.clone(),
                other => other.to_string(),
            }));
            match raw.as_deref().map(str::trim) {
                None | Some("") | Some("auto") => None,
                Some(p) => Some(p.parse::<u16>().map_err(|_| anyhow::anyhow!("game_port must be a number or \"auto\", got '{p}'"))?),
            }
        },
        game_host: match cli.game_host.clone().or(fc.game_host.clone()).as_deref().map(str::trim) {
            None | Some("") | Some("auto") => None,
            Some(h) => Some(game::validate_host(h)?),
        },
    })
}

pub fn sanitize(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .take(32)
        .collect();
    if t.is_empty() { "tester".into() } else { t }
}

// ---------------------------------------------------------------- shared state

#[derive(Default)]
struct EpStat {
    win_n: u32,
    win_changed: u32,
    win_err: u32,
    hz: f64,
    new_pct: f64,
    err_pct: f64,
    total: u64,
}

#[derive(Default, Clone)]
struct GameState {
    map_valid: Option<bool>,
    map_gen: Option<String>,
    hud_type: Option<String>,
    player: Option<String>, // Some(type) when Player found in map_obj
    map_obj_seen: bool,
    objects: usize,
    army: Option<String>,
    vehicle: Option<String>,
    enemies: usize,
    player_xy: Option<[f64; 2]>,
    player_dxdy: Option<[f64; 2]>,
    map_obj_seq: Option<u64>,
    map_obj_wall: Option<i64>,
    map_img_hash: Option<String>,
}

/// red-ish map_obj color = enemy (#fa0C00 / #f00C00 in real data)
fn is_enemy_color(o: &serde_json::Value) -> bool {
    if let Some(c) = o["color"].as_str().and_then(labels::parse_color) {
        return c[0] >= 180 && c[1] < 80 && c[2] < 80;
    }
    false
}

#[derive(Default)]
pub struct UpStat {
    pub ok_batches: u64,
    pub fail: u64,
    pub last_err: String,
    pub queued: i64,
    pub last_ok: Option<Instant>,
    pub records_sent: u64,
    pub inflight: usize,
    pub pending: usize,
    pub buf_age_ms: u64,
    pub rate: f64,
    pub offline: bool,
    /// dropped locally before sending
    pub drop_too_old: u64,
    pub drop_overflow: u64,
    pub drop_no_sync: u64,
    /// dropped by the server's clock guard (from ingest responses)
    pub srv_too_old: u64,
    pub srv_from_future: u64,
    pub srv_no_sync: u64,
    /// records in batches the server rejected (HTTP 400/409/413)
    pub rejected: u64,
}

pub struct Shared {
    pub session: String,
    pub player: String,
    start: Instant,
    seq: AtomicU64,
    epoch: AtomicU32,
    last_ok_mono_ms: AtomicI64,
    stats: Mutex<HashMap<&'static str, EpStat>>,
    game: Mutex<GameState>,
    pub up: Mutex<UpStat>,
    map_changed: Notify,
    total_records: AtomicU64,
    /// current poll rate per Kind (f64 bits), adjustable at runtime (`/rate N`, --rate-cycle)
    hz: [AtomicU64; 7],
    /// consecutive failed 8111 requests (any endpoint) and the last error kind
    consec_fail: AtomicU64,
    last_err_kind: Mutex<String>,
    /// current game API base URL, e.g. http://127.0.0.1:9222
    base: Mutex<String>,
    /// clock offset estimate vs the receiver
    pub clock: Mutex<clock::ClockState>,
    /// label log + send queue
    pub labels: labels::SharedBook,
    pub label_notify: Notify,
    /// upload gate (hangar pause) as seen by the writer: Some((reason, since))
    pub paused: Mutex<Option<(gate::Reason, Instant)>>,
    pub suppressed: AtomicU64,
}

impl Shared {
    fn base(&self) -> String {
        self.base.lock().unwrap().clone()
    }
    fn note_result(&self, err: Option<&str>) {
        match err {
            None => self.consec_fail.store(0, Ordering::Relaxed),
            Some(e) => {
                self.consec_fail.fetch_add(1, Ordering::Relaxed);
                *self.last_err_kind.lock().unwrap() = err_kind(e).to_string();
            }
        }
    }
    fn get_hz(&self, k: Kind) -> f64 {
        f64::from_bits(self.hz[k as usize].load(Ordering::Relaxed))
    }
    fn set_hz(&self, k: Kind, v: f64) {
        self.hz[k as usize].store(v.clamp(0.05, 50.0).to_bits(), Ordering::Relaxed)
    }
    fn set_fast(&self, v: f64) {
        for k in [Kind::MapObj, Kind::Indicators, Kind::State] {
            self.set_hz(k, v);
        }
    }
    fn mono_ns(&self) -> u64 {
        self.start.elapsed().as_nanos() as u64
    }
    fn mono_ms(&self) -> i64 {
        self.start.elapsed().as_millis() as i64
    }
    fn next_seq(&self) -> u64 {
        self.seq.fetch_add(1, Ordering::SeqCst)
    }
    fn reachable(&self) -> bool {
        let l = self.last_ok_mono_ms.load(Ordering::Relaxed);
        l >= 0 && self.mono_ms() - l < 3000
    }
    fn down_for(&self) -> Option<Duration> {
        let l = self.last_ok_mono_ms.load(Ordering::Relaxed);
        if l < 0 { None } else { Some(Duration::from_millis((self.mono_ms() - l).max(0) as u64)) }
    }
    pub fn gate_obs(&self) -> gate::Obs {
        let g = self.game.lock().unwrap();
        gate::Obs {
            map_valid: g.map_valid,
            map_obj_len: if g.map_obj_seen { Some(g.objects) } else { None },
            placeholder_img: g.map_img_hash.as_deref() == Some(gate::PLACEHOLDER_IMG_SHA256),
            reachable: self.reachable(),
            down_for: self.down_for(),
        }
    }
    pub fn clock_view(&self) -> labels::ClockView {
        let c = self.clock.lock().unwrap();
        labels::ClockView { synced: c.valid(), offset_ms: c.offset_ms, rtt_ms: c.rtt_ms }
    }
    pub fn snapshot(&self) -> labels::Snapshot {
        let g = self.game.lock().unwrap();
        labels::Snapshot {
            in_battle: g.map_valid == Some(true) || g.player.is_some(),
            map_gen: g.map_gen.clone(),
            map_img_hash: g.map_img_hash.clone(),
            hud_type: g.hud_type.clone(),
            self_xy: g.player_xy,
            self_dxdy: g.player_dxdy,
            self_type: g.player.clone(),
            army: g.army.clone(),
            vehicle: g.vehicle.clone(),
            objects: g.objects,
            enemy_objects: g.enemies,
            map_obj_seq: g.map_obj_seq,
            map_obj_wall_ms: g.map_obj_wall,
        }
    }
    /// Create a label event now and queue it for sending. Returns the event.
    pub fn add_label(&self, id: &str, text: &str, note: Option<String>, source: &str) -> labels::LabelEvent {
        let t_local = now_ms();
        let ev = labels::LabelEvent::new(
            &self.session, self.next_seq(), self.mono_ns(), self.epoch.load(Ordering::Relaxed),
            id, text, note, source, t_local, &self.clock_view(), self.snapshot(),
        );
        self.labels.lock().unwrap().push(ev.clone());
        self.label_notify.notify_one();
        say!("[标签] {} {}{}", ev.label_id, ev.text, ev.note.as_deref().map(|n| format!(" ({n})")).unwrap_or_default());
        ev
    }
    /// Undo the latest label (only within 10 s). Sends an "_undo" event referencing it.
    pub fn undo_last_label(&self) -> Option<String> {
        let target = self.labels.lock().unwrap().undoable().map(|(e, _)| (e.uid.clone(), e.text.clone()))?;
        let mut ev = labels::LabelEvent::new(
            &self.session, self.next_seq(), self.mono_ns(), self.epoch.load(Ordering::Relaxed),
            labels::UNDO_ID, &format!("撤销: {}", target.1), None, "button", now_ms(), &self.clock_view(), self.snapshot(),
        );
        ev.action = Some("undo".into());
        ev.ref_uid = Some(target.0.clone());
        let mut b = self.labels.lock().unwrap();
        b.mark_undone(&target.0);
        b.push(ev);
        drop(b);
        self.label_notify.notify_one();
        say!("[标签] 已撤销 {}", target.1);
        Some(target.1)
    }
    pub fn synthetic(&self, ep: &str, body: String) -> Record {
        Record {
            session: self.session.clone(),
            seq: self.next_seq(),
            ep: ep.into(),
            url: String::new(),
            wall_ms: now_ms(),
            mono_ns: self.mono_ns(),
            lat_us: 0,
            status: 0,
            err: None,
            ctype: None,
            bytes: body.len() as u64,
            body: Some(body),
            body_b64: None,
            img_hash: None,
            changed: true,
            cfg_hz: 0.0,
            epoch: self.epoch.load(Ordering::Relaxed),
            ..Default::default()
        }
    }
}

// ---------------------------------------------------------------- fetching

struct Resp {
    status: u16,
    ctype: Option<String>,
    bytes: Vec<u8>,
    lat_us: u64,
    err: Option<String>,
}

/// Per-request total timeout (connect + headers + body) for normal 8111 endpoints / for /map.img.
/// Must be > CONNECT_TIMEOUT so a connect failure is always classified as connect_*.
pub const REQ_TIMEOUT: Duration = Duration::from_millis(3000);
pub const IMG_TIMEOUT: Duration = Duration::from_secs(5);
/// Connect timeout is longer than the request budget on purpose: on Windows a connect to a
/// closed localhost port is only reported as "refused" after ~2 s of SYN retries; with a short
/// connect timeout "game not running" would look like a timeout.
pub const CONNECT_TIMEOUT: Duration = Duration::from_millis(2500);

/// Error kind for records/status: connect_refused | connect_timeout | read_timeout | proxy | connect_error | body_error | other.
/// Record `err` = "<kind>: <detail>".
pub fn classify(e: &reqwest::Error) -> String {
    use std::error::Error as _;
    let mut chain = String::new();
    let mut io_kind = None;
    let mut cur: Option<&(dyn std::error::Error + 'static)> = e.source();
    while let Some(c) = cur {
        if !chain.is_empty() {
            chain.push_str(" / ");
        }
        chain.push_str(&c.to_string());
        if let Some(io) = c.downcast_ref::<std::io::Error>() {
            io_kind = Some(io.kind());
        }
        cur = c.source();
    }
    let low = chain.to_ascii_lowercase();
    let kind = if low.contains("proxy") || low.contains("tunnel") {
        "proxy"
    } else if io_kind == Some(std::io::ErrorKind::ConnectionRefused) || low.contains("refused") || low.contains("10061") {
        "connect_refused"
    } else if e.is_connect() && (e.is_timeout() || io_kind == Some(std::io::ErrorKind::TimedOut) || low.contains("timed out") || low.contains("10060")) {
        "connect_timeout"
    } else if e.is_connect() {
        "connect_error"
    } else if e.is_timeout() {
        "read_timeout"
    } else if e.is_body() || e.is_decode() {
        "body_error"
    } else {
        "other"
    };
    let mut d = chain;
    d.truncate(160);
    if d.is_empty() { kind.to_string() } else { format!("{kind}: {d}") }
}

pub fn err_kind(err: &str) -> &str {
    err.split(':').next().unwrap_or(err)
}

pub fn local_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .no_proxy() // never use any proxy (env / system / Clash) for 127.0.0.1:8111
        .tcp_nodelay(true)
        .connect_timeout(CONNECT_TIMEOUT)
        .pool_max_idle_per_host(16)
        .build()?)
}

async fn fetch(client: &reqwest::Client, base: &str, path: &str, timeout: Duration) -> Resp {
    let t0 = Instant::now();
    // total budget (connect capped separately by CONNECT_TIMEOUT); body read shares the same deadline
    match client.get(format!("{base}{path}")).timeout(timeout).send().await {
        Ok(r) => {
            let status = r.status().as_u16();
            let ctype = r
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string());
            match r.bytes().await {
                Ok(b) => Resp { status, ctype, bytes: b.to_vec(), lat_us: t0.elapsed().as_micros() as u64, err: None },
                Err(e) => {
                    let k = classify(&e);
                    let k = if e.is_timeout() && !k.starts_with("read_timeout") { format!("read_timeout: {k}") } else { k };
                    Resp { status, ctype, bytes: vec![], lat_us: t0.elapsed().as_micros() as u64, err: Some(k) }
                }
            }
        }
        Err(e) => Resp { status: 0, ctype: None, bytes: vec![], lat_us: t0.elapsed().as_micros() as u64, err: Some(classify(&e)) },
    }
}

fn hash64(b: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    b.hash(&mut h);
    h.finish()
}

fn set_body(rec: &mut Record, bytes: Vec<u8>) {
    rec.bytes = bytes.len() as u64;
    match String::from_utf8(bytes) {
        Ok(s) => rec.body = Some(s),
        Err(e) => {
            use base64::Engine;
            rec.body_b64 = Some(base64::engine::general_purpose::STANDARD.encode(e.into_bytes()));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    MapObj,
    Indicators,
    State,
    MapInfo,
    Mission,
    Hud,
    Chat,
}
impl Kind {
    fn key(self) -> &'static str {
        match self {
            Kind::MapObj => "/map_obj.json",
            Kind::Indicators => "/indicators",
            Kind::State => "/state",
            Kind::MapInfo => "/map_info.json",
            Kind::Mission => "/mission.json",
            Kind::Hud => "/hudmsg",
            Kind::Chat => "/gamechat",
        }
    }
}

fn val_str(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Null => None,
        serde_json::Value::String(s) => Some(s.clone()),
        other => Some(other.to_string()),
    }
}

fn max_id(arr: &serde_json::Value) -> Option<u64> {
    arr.as_array()?
        .iter()
        .filter_map(|m| {
            let id = &m["id"];
            id.as_u64().or_else(|| id.as_f64().map(|f| f as u64)).or_else(|| id.as_str().and_then(|s| s.parse().ok()))
        })
        .max()
}

async fn poll_loop(
    sh: Arc<Shared>,
    client: reqwest::Client,
    kind: Kind,
    tx: mpsc::UnboundedSender<Record>,
    mut stop: watch::Receiver<bool>,
) {
    let mut prev_hash: Option<u64> = None;
    let (mut c_evt, mut c_dmg, mut c_chat) = (0u64, 0u64, 0u64);
    let mut my_epoch = sh.epoch.load(Ordering::SeqCst);
    let mut fails = 0u32;
    let mut down_since: Option<Instant> = None;
    let mut next = Instant::now();
    loop {
        tokio::select! {
            _ = tokio::time::sleep_until(next.into()) => {}
            _ = stop.changed() => {}
        }
        if *stop.borrow() {
            break;
        }
        let started = Instant::now();
        let hz = sh.get_hz(kind);
        let period = Duration::from_secs_f64(1.0 / hz);
        let e = sh.epoch.load(Ordering::SeqCst);
        if e != my_epoch {
            my_epoch = e;
            c_evt = 0;
            c_dmg = 0;
            c_chat = 0;
        }
        let url = match kind {
            Kind::Hud => format!("/hudmsg?lastEvt={c_evt}&lastDmg={c_dmg}"),
            Kind::Chat => format!("/gamechat?lastId={c_chat}"),
            k => k.key().to_string(),
        };
        let wall = now_ms();
        let mono = sh.mono_ns();
        let r = fetch(&client, &sh.base(), &url, REQ_TIMEOUT).await;
        let mut rec = Record {
            session: sh.session.clone(),
            seq: 0,
            ep: kind.key().into(),
            url: url.clone(),
            wall_ms: wall,
            mono_ns: mono,
            lat_us: r.lat_us,
            status: r.status,
            err: r.err.clone(),
            ctype: r.ctype.clone(),
            bytes: 0,
            body: None,
            body_b64: None,
            img_hash: None,
            changed: false,
            cfg_hz: hz,
            epoch: my_epoch,
            ..Default::default()
        };
        let h = if r.err.is_some() { hash64(format!("err:{}", r.err.as_deref().map(err_kind).unwrap_or("")).as_bytes()) } else { hash64(&r.bytes) ^ (r.status as u64) };
        rec.changed = prev_hash != Some(h);
        prev_hash = Some(h);
        rec.seq = sh.next_seq();
        let ok = r.err.is_none();
        sh.note_result(r.err.as_deref());
        if ok {
            // game (re)appeared after being down >5s => probably restarted: IDs restart at 0
            if let Some(ds) = down_since.take() {
                if ds.elapsed() > Duration::from_secs(5) && kind == Kind::MapInfo {
                    sh.epoch.fetch_add(1, Ordering::SeqCst);
                }
            }
            sh.last_ok_mono_ms.store(sh.mono_ms(), Ordering::Relaxed);
            postprocess(&sh, kind, &r.bytes, (rec.seq, wall), &mut c_evt, &mut c_dmg, &mut c_chat);
        } else if down_since.is_none() {
            down_since = Some(Instant::now());
        }
        set_body(&mut rec, r.bytes);
        {
            let mut st = sh.stats.lock().unwrap();
            let s = st.entry(kind.key()).or_default();
            s.win_n += 1;
            s.total += 1;
            if rec.changed && ok {
                s.win_changed += 1;
            }
            if !ok {
                s.win_err += 1;
            }
        }
        sh.total_records.fetch_add(1, Ordering::Relaxed);
        if tx.send(rec).is_err() {
            break;
        }
        if ok {
            fails = 0;
            next += period;
            let now = Instant::now();
            if next < now {
                next = now; // slow response: postpone, never overlap / burst
            }
        } else {
            fails += 1;
            let back = period.mul_f64(2f64.powi(fails.min(6) as i32)).min(Duration::from_secs(3)).max(period);
            next = started + back;
        }
    }
}

fn postprocess(sh: &Shared, kind: Kind, bytes: &[u8], (seq, wall): (u64, i64), c_evt: &mut u64, c_dmg: &mut u64, c_chat: &mut u64) {
    let v: serde_json::Value = match serde_json::from_slice(bytes) {
        Ok(v) => v,
        Err(_) => return,
    };
    match kind {
        Kind::MapInfo => {
            let has_range = !v["map_min"].is_null() && !v["map_max"].is_null();
            let valid = v["valid"].as_bool().or(Some(has_range));
            let gen = val_str(&v["map_generation"]);
            let hud = val_str(&v["hud_type"]);
            let mut g = sh.game.lock().unwrap();
            let was_valid = g.map_valid == Some(true);
            let now_valid = valid == Some(true);
            let mut map_change = false;
            if now_valid && !was_valid {
                // entered a match (or first valid sample): new epoch => reset chat/hud cursors
                sh.epoch.fetch_add(1, Ordering::SeqCst);
                map_change = true;
            }
            if now_valid && gen.is_some() && gen != g.map_gen {
                map_change = true;
            }
            g.map_valid = valid;
            if now_valid {
                g.map_gen = gen;
                g.hud_type = hud;
            }
            drop(g);
            if map_change {
                sh.map_changed.notify_one();
            }
        }
        Kind::MapObj => {
            if let Some(a) = v.as_array() {
                let p = a.iter().find(|o| o["icon"].as_str() == Some("Player"));
                let mut g = sh.game.lock().unwrap();
                g.map_obj_seen = true;
                g.objects = a.len();
                g.player = p.map(|o| o["type"].as_str().unwrap_or("?").to_string());
                g.player_xy = p.and_then(|o| Some([o["x"].as_f64()?, o["y"].as_f64()?]));
                g.player_dxdy = p.and_then(|o| Some([o["dx"].as_f64()?, o["dy"].as_f64()?]));
                g.enemies = a.iter().filter(|o| o["icon"].as_str() != Some("Player") && is_enemy_color(o)).count();
                g.map_obj_seq = Some(seq);
                g.map_obj_wall = Some(wall);
            }
        }
        Kind::Indicators => {
            let mut g = sh.game.lock().unwrap();
            g.army = v["army"].as_str().map(|s| s.to_string());
            g.vehicle = v["type"].as_str().map(|s| s.to_string());
        }
        Kind::Hud => {
            if let Some(m) = max_id(&v["events"]) {
                *c_evt = (*c_evt).max(m);
            }
            if let Some(m) = max_id(&v["damage"]) {
                *c_dmg = (*c_dmg).max(m);
            }
        }
        Kind::Chat => {
            if let Some(m) = max_id(&v) {
                *c_chat = (*c_chat).max(m);
            }
        }
        _ => {}
    }
}

// ---------------------------------------------------------------- map.img + loc

async fn map_img_loop(
    sh: Arc<Shared>,
    client: reqwest::Client,
    every: f64,
    tx: mpsc::UnboundedSender<Record>,
    up_tx: mpsc::UnboundedSender<upload::UpItem>,
    mut stop: watch::Receiver<bool>,
) {
    let mut seen: HashSet<String> = HashSet::new();
    let mut prev_hash: Option<String> = None;
    let mut fetched_loc_for: Option<String> = None;
    loop {
        let mut notified = false;
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs_f64(every)) => {}
            _ = sh.map_changed.notified() => { notified = true; }
            _ = stop.changed() => {}
        }
        if *stop.borrow() {
            break;
        }
        let epoch = sh.epoch.load(Ordering::SeqCst);
        let gen = sh.game.lock().unwrap().map_gen.clone();
        let mut urls = vec!["/map.img".to_string()];
        if notified {
            if let Some(g) = &gen {
                urls.push(format!("/map.img?gen={g}"));
            }
        }
        let mut map_changed = notified;
        for url in urls {
            let wall = now_ms();
            let mono = sh.mono_ns();
            let r = fetch(&client, &sh.base(), &url, IMG_TIMEOUT).await;
            let ok = r.err.is_none() && r.status == 200 && !r.bytes.is_empty();
            let hash = if ok {
                use sha2::Digest;
                Some(hex::encode(sha2::Sha256::digest(&r.bytes)))
            } else {
                None
            };
            let is_main = url == "/map.img";
            if is_main && hash.is_some() {
                sh.game.lock().unwrap().map_img_hash = hash.clone();
            }
            let changed = is_main && hash.is_some() && hash != prev_hash;
            if is_main && hash.is_some() {
                if changed && prev_hash.is_some() {
                    map_changed = true;
                }
                prev_hash = hash.clone();
            }
            sh.note_result(r.err.as_deref());
            if r.err.is_none() {
                sh.last_ok_mono_ms.store(sh.mono_ms(), Ordering::Relaxed);
            }
            let rec = Record {
                session: sh.session.clone(),
                seq: sh.next_seq(),
                ep: if is_main { "/map.img".into() } else { "/map.img?gen".into() },
                url: url.clone(),
                wall_ms: wall,
                mono_ns: mono,
                lat_us: r.lat_us,
                status: r.status,
                err: r.err.clone(),
                ctype: r.ctype.clone(),
                bytes: r.bytes.len() as u64,
                body: None,
                body_b64: None,
                img_hash: hash.clone(),
                changed,
                cfg_hz: 1.0 / every,
                epoch,
                ..Default::default()
            };
            {
                let mut st = sh.stats.lock().unwrap();
                let s = st.entry("/map.img").or_default();
                if is_main {
                    s.win_n += 1;
                    s.total += 1;
                    if changed {
                        s.win_changed += 1;
                    }
                    if r.err.is_some() {
                        s.win_err += 1;
                    }
                }
            }
            sh.total_records.fetch_add(1, Ordering::Relaxed);
            let _ = tx.send(rec);
            if let Some(h) = hash {
                // the hangar placeholder is known (gate::PLACEHOLDER_IMG_SHA256): never upload its bytes
                if h != gate::PLACEHOLDER_IMG_SHA256 && seen.insert(h.clone()) {
                    let ext = wtc_common::sniff_image(&r.bytes);
                    let ctype = r.ctype.clone().unwrap_or_else(|| format!("image/{ext}"));
                    let _ = up_tx.send(upload::UpItem::Image { session: sh.session.clone(), hash: h, ctype, bytes: r.bytes });
                }
            }
        }
        // localized objective strings: once per map change
        let map_key = format!("{epoch}/{:?}/{:?}", gen, prev_hash);
        if (map_changed || fetched_loc_for.is_none()) && prev_hash.is_some() && fetched_loc_for.as_deref() != Some(&map_key) {
            fetched_loc_for = Some(map_key);
            for (ep, url) in [
                ("/loc/map/primary_objectives", "/loc/map/primary_objectives?fmt=js"),
                ("/loc/map/secondary_objectives", "/loc/map/secondary_objectives?fmt=js"),
            ] {
                let wall = now_ms();
                let mono = sh.mono_ns();
                let r = fetch(&client, &sh.base(), url, REQ_TIMEOUT).await;
                let mut rec = Record {
                    session: sh.session.clone(),
                    seq: sh.next_seq(),
                    ep: ep.into(),
                    url: url.into(),
                    wall_ms: wall,
                    mono_ns: mono,
                    lat_us: r.lat_us,
                    status: r.status,
                    err: r.err.clone(),
                    ctype: r.ctype.clone(),
                    bytes: 0,
                    body: None,
                    body_b64: None,
                    img_hash: None,
                    changed: true,
                    cfg_hz: 0.0,
                    epoch,
                    ..Default::default()
                };
                set_body(&mut rec, r.bytes);
                sh.total_records.fetch_add(1, Ordering::Relaxed);
                let _ = tx.send(rec);
            }
        }
    }
}

// ---------------------------------------------------------------- batching (memory only)

async fn writer_loop(
    mut rx: mpsc::UnboundedReceiver<Record>,
    up_tx: mpsc::UnboundedSender<upload::UpItem>,
    sh: Arc<Shared>,
    mut stop: watch::Receiver<bool>,
) {
    let mut batch: Vec<Record> = Vec::new();
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut gw = GateWriter::default();
    let flush = |batch: &mut Vec<Record>| {
        if !batch.is_empty() {
            sh.up.lock().unwrap().queued += 1;
            let _ = up_tx.send(upload::UpItem::Batch(std::mem::take(batch)));
        }
    };
    loop {
        tokio::select! {
            r = rx.recv() => match r {
                Some(r) => {
                    gw.step(&sh, &mut batch, Instant::now());
                    gw.offer(&sh, r, &mut batch);
                    if batch.len() >= 4000 {
                        flush(&mut batch);
                    }
                }
                None => break,
            },
            _ = stop.changed() => {
                if *stop.borrow() {
                    // pollers are already joined; drain what is left and finish
                    while let Ok(r) = rx.try_recv() {
                        gw.offer(&sh, r, &mut batch);
                    }
                    break;
                }
            }
            _ = tick.tick() => {
                gw.step(&sh, &mut batch, Instant::now());
                flush(&mut batch);
            }
        }
    }
    flush(&mut batch);
}

/// Applies the upload gate (gate.rs) to the record stream.
#[derive(Default)]
struct GateWriter {
    gate: gate::Gate,
    /// latest record per endpoint while paused (sent on resume when fresh)
    last: HashMap<String, Record>,
    suppressed: HashMap<String, u64>,
    suppressed_total: u64,
    vehicle: Option<String>,
}

impl GateWriter {
    fn state_body(&self, sh: &Shared, extra: serde_json::Value) -> String {
        let g = sh.game.lock().unwrap().clone();
        let mut v = serde_json::json!({
            "paused": self.gate.is_paused(),
            "reason": self.gate.paused.map(|p| p.0.key()),
            "map_valid": g.map_valid, "map_obj_objects": g.objects, "map_img_hash": g.map_img_hash,
            "placeholder_img": g.map_img_hash.as_deref() == Some(gate::PLACEHOLDER_IMG_SHA256),
            "vehicle": g.vehicle, "army": g.army,
            "game_base": sh.base(), "game_reachable": sh.reachable(),
            "consec_fail": sh.consec_fail.load(Ordering::Relaxed),
            "last_err_kind": sh.last_err_kind.lock().unwrap().clone(),
            "suppressed_total": self.suppressed_total, "suppressed": self.suppressed,
        });
        if let (Some(o), serde_json::Value::Object(e)) = (v.as_object_mut(), extra) {
            o.extend(e);
        }
        v.to_string()
    }

    /// evaluate the gate; emit state/heartbeat records into batch
    fn step(&mut self, sh: &Shared, batch: &mut Vec<Record>, now: Instant) {
        let obs = sh.gate_obs();
        if let Some(t) = self.gate.update(&obs, now) {
            match t {
                gate::Transition::Pause(r) => {
                    self.suppressed.clear();
                    self.suppressed_total = 0;
                    self.vehicle = sh.game.lock().unwrap().vehicle.clone();
                    let body = self.state_body(sh, serde_json::json!({"state": "paused", "event": format!("entered_{}", r.key())}));
                    batch.push(sh.synthetic("#state", body));
                    say!("[上传] 已暂停: {}（只发送状态心跳，每 {} 秒一次；标签照常发送）", if r == gate::Reason::Hangar { "机库/占位地图" } else { "游戏接口不可用" }, gate::HEARTBEAT.as_secs());
                }
                gate::Transition::Reason(r) => {
                    let body = self.state_body(sh, serde_json::json!({"state": "paused", "event": format!("reason_{}", r.key())}));
                    batch.push(sh.synthetic("#state", body));
                }
                gate::Transition::Resume { reason, paused } => {
                    let body = self.state_body(sh, serde_json::json!({"state": "active", "event": format!("left_{}", reason.key()), "paused_ms": paused.as_millis() as u64}));
                    // gate already resumed -> "paused": false in body
                    batch.push(sh.synthetic("#state", body));
                    let fresh = now_ms() - 2000;
                    let mut keep: Vec<Record> = self.last.drain().map(|(_, r)| r).filter(|r| r.wall_ms >= fresh).collect();
                    keep.sort_by_key(|r| r.seq);
                    batch.extend(keep);
                    say!("[上传] 已恢复（暂停 {:.0} 秒，跳过 {} 条重复数据）", paused.as_secs_f64(), self.suppressed_total);
                }
            }
        }
        *sh.paused.lock().unwrap() = self.gate.paused;
        if self.gate.is_paused() {
            if self.gate.heartbeat_due(now) {
                let body = self.state_body(sh, serde_json::json!({"state": "paused", "event": "heartbeat"}));
                batch.push(sh.synthetic("#heartbeat", body));
            }
            let v = sh.game.lock().unwrap().vehicle.clone();
            if v.is_some() && v != self.vehicle {
                self.vehicle = v;
                let body = self.state_body(sh, serde_json::json!({"state": "paused", "event": "vehicle_changed"}));
                batch.push(sh.synthetic("#state", body));
            }
        }
    }

    fn offer(&mut self, sh: &Shared, r: Record, batch: &mut Vec<Record>) {
        if !self.gate.is_paused() || r.ep.starts_with('#') {
            batch.push(r);
            return;
        }
        *self.suppressed.entry(r.ep.clone()).or_default() += 1;
        self.suppressed_total += 1;
        sh.suppressed.fetch_add(1, Ordering::Relaxed);
        self.last.insert(r.ep.clone(), r);
    }
}

// ---------------------------------------------------------------- status line

fn status_line(sh: &Shared) -> String {
    let mut st = sh.stats.lock().unwrap();
    let order: [(&str, &str); 8] = [
        ("/map_obj.json", "obj"),
        ("/indicators", "ind"),
        ("/state", "st"),
        ("/map_info.json", "inf"),
        ("/mission.json", "mis"),
        ("/hudmsg", "hud"),
        ("/gamechat", "chat"),
        ("/map.img", "img"),
    ];
    let mut parts = Vec::new();
    for (k, short) in order {
        if let Some(s) = st.get_mut(k) {
            parts.push(format!("{short} {:.1}Hz/{:.0}%", s.hz, s.new_pct));
        }
    }
    drop(st);
    let g = sh.game.lock().unwrap().clone();
    let up = sh.up.lock().unwrap();
    let ck = sh.clock.lock().unwrap().clone();
    let clk = if ck.valid() {
        format!("clk {:+.0}ms rtt {:.0}ms", ck.offset_ms, ck.rtt_ms)
    } else if ck.last_err.is_empty() {
        "clk SYNCING".to_string()
    } else {
        format!("clk NO SYNC ({})", ck.last_err)
    };
    let drops = up.drop_too_old + up.drop_overflow + up.drop_no_sync;
    let srv = up.srv_too_old + up.srv_from_future + up.srv_no_sync + up.rejected;
    let dropstr = if drops + srv == 0 {
        String::new()
    } else {
        format!(" drop old {} ovf {} nosync {} srv {}", up.drop_too_old, up.drop_overflow, up.drop_no_sync, srv)
    };
    let upstr = if up.offline || !up.last_err.is_empty() {
        format!("upload DOWN ({}) buf {:.1}s | {clk}{dropstr}", up.last_err, up.buf_age_ms as f64 / 1000.0)
    } else {
        format!("upload OK {:.0} rec/s infl {} buf {:.1}s | {clk}{dropstr}", up.rate, up.inflight, up.buf_age_ms as f64 / 1000.0)
    };
    drop(up);
    let upstr = match *sh.paused.lock().unwrap() {
        Some((r, t)) => format!("PAUSED({} {:.0}s, skipped {}) {upstr}", r.key(), t.elapsed().as_secs_f64(), sh.suppressed.load(Ordering::Relaxed)),
        None => upstr,
    };
    let upstr = {
        let b = sh.labels.lock().unwrap();
        if b.entries.is_empty() { upstr } else { format!("{upstr} | labels {} pend {}", b.count_effective(), b.pending()) }
    };
    let player = if !g.map_obj_seen {
        "-".to_string()
    } else {
        match &g.player {
            Some(t) => format!("YES({t})"),
            None => "no".into(),
        }
    };
    format!(
        "[{}] game={} {} | {} | {} | gen {} hud {} | player {} objs {} {} | rec {}",
        chrono::Local::now().format("%H:%M:%S"),
        game::short(&sh.base()),
        if sh.reachable() { "UP".to_string() } else { match sh.last_err_kind.lock().unwrap().as_str() { "" => "DOWN".to_string(), k => format!("DOWN({k})") } },
        parts.join(" "),
        upstr,
        g.map_gen.as_deref().unwrap_or("-"),
        g.hud_type.as_deref().unwrap_or("-"),
        player,
        g.objects,
        g.army.as_deref().unwrap_or(""),
        sh.total_records.load(Ordering::Relaxed)
    )
}

async fn status_loop(sh: Arc<Shared>, mut stop: watch::Receiver<bool>) {
    let mut last = Instant::now();
    let mut last_hint: Option<Instant> = None;
    loop {
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            _ = stop.changed() => {}
        }
        if *stop.borrow() {
            break;
        }
        let el = last.elapsed().as_secs_f64();
        last = Instant::now();
        {
            let mut st = sh.stats.lock().unwrap();
            for s in st.values_mut() {
                s.hz = s.win_n as f64 / el;
                let okn = s.win_n.saturating_sub(s.win_err);
                s.new_pct = if okn > 0 { 100.0 * s.win_changed as f64 / okn as f64 } else { 0.0 };
                s.err_pct = if s.win_n > 0 { 100.0 * s.win_err as f64 / s.win_n as f64 } else { 0.0 };
                s.win_n = 0;
                s.win_changed = 0;
                s.win_err = 0;
            }
        }
        if !QUIET.load(Ordering::Relaxed) {
            println!("{}", status_line(&sh));
        }
        let fails = sh.consec_fail.load(Ordering::Relaxed);
        if fails >= 20 && !sh.reachable() && last_hint.map(|t: Instant| t.elapsed() > Duration::from_secs(30)).unwrap_or(true) {
            last_hint = Some(Instant::now());
            let k = sh.last_err_kind.lock().unwrap().clone();
            say!("  !! {}", hint_for(&k, fails));
        }
    }
}

pub fn hint_for(kind: &str, fails: u64) -> String {
    let why = match kind {
        "connect_refused" => "接口端口未监听：游戏未启动/尚未开放接口（等待即可；会自动在 8111 与 9222 之间重新探测）",
        "connect_timeout" => "连接游戏接口超时：游戏未启动？或被代理/防火墙/安全软件拦截（会自动重试 8111/9222）",
        "read_timeout" => "已连上 8111 但响应超时：游戏卡顿/加载中？",
        "proxy" => "请求被代理拦截（本程序本应直连，请运行 diag 并反馈）",
        _ => "8111 无响应：游戏未启动？或被代理/防火墙拦截",
    };
    format!("8111 无响应（连续 {fails} 次失败，类型 {kind}）：{why}。运行 `wt-collector.exe diag` 可诊断。")
}

// ---------------------------------------------------------------- main

/// Link between the GUI thread and the collector running on the tokio runtime.
pub struct UiLink {
    pub shared_tx: std::sync::mpsc::Sender<Arc<Shared>>,
    pub stop: watch::Receiver<bool>,
}

fn main() {
    let interactive_default = std::env::args().len() <= 1;
    let cli = Cli::parse();
    // GUI: explicit `gui`, or no arguments at all on Windows (double-click)
    let want_gui = matches!(cli.cmd, Some(Cmd::Gui)) || (interactive_default && cfg!(windows) && cfg!(feature = "gui"));
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().expect("tokio runtime");
    #[cfg(feature = "gui")]
    if want_gui {
        let code = match load_cfg(&cli) {
            Ok(cfg) => gui::run_gui(cfg, &rt),
            Err(e) => {
                gui::fatal(&format!("{e:#}"));
                1
            }
        };
        rt.shutdown_timeout(Duration::from_secs(1));
        std::process::exit(code);
    }
    #[cfg(not(feature = "gui"))]
    if want_gui && matches!(cli.cmd, Some(Cmd::Gui)) {
        eprintln!("this build has no GUI (built without the `gui` feature)");
        std::process::exit(2);
    }
    let res = rt.block_on(real_main(cli));
    let code = match res {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("\nERROR: {e:#}");
            1
        }
    };
    if interactive_default && code != 0 {
        eprintln!("按回车键退出 / press Enter to exit");
        let mut s = String::new();
        let _ = std::io::stdin().read_line(&mut s);
    }
    rt.shutdown_timeout(Duration::from_secs(1));
    std::process::exit(code);
}

async fn real_main(cli: Cli) -> Result<()> {
    let cfg = load_cfg(&cli)?;
    match &cli.cmd {
        Some(Cmd::Diag) => return diag::run(&cfg).await,
        Some(Cmd::Note { text, session }) => return upload::send_note(&cfg, text, session.as_deref()).await,
        Some(Cmd::Label { id, note, session, tag }) => return send_label_once(&cfg, id, note.clone(), session.clone(), tag.clone()).await,
        Some(Cmd::Labels) => {
            say!("标签按钮（来自 {}）:", cfg.config_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "默认".into()));
            for (i, l) in cfg.labels.iter().enumerate() {
                say!("  {:>2}. {:<14} {}{}", i + 1, l.id, l.text, if l.note { "  [输入备注]" } else { "" });
            }
            return Ok(());
        }
        _ => {}
    }
    run(cfg, None).await
}

/// `wt-collector label <id>`: one label event in a one-off session (or --session), clock-synced first.
async fn send_label_once(cfg: &Cfg, id: &str, note: Option<String>, session: Option<String>, tags: Vec<String>) -> Result<()> {
    let def = cfg.labels.iter().find(|l| l.id == id).cloned().unwrap_or(labels::LabelDef { id: id.into(), text: id.into(), color: None, note: false });
    let session = session.unwrap_or_else(|| format!("{}-{}-cli{:03x}", chrono::Local::now().format("%Y%m%d-%H%M%S"), sanitize(&cfg.player), rand::random::<u16>() & 0xfff));
    let sh = Arc::new(new_shared(session.clone(), cfg.player.clone(), String::new()));
    // seq space distinct from a running collector's (which counts from 0)
    sh.seq.store(1u64 << 40 | (now_ms() as u64 & 0xff_ffff) << 8, Ordering::SeqCst);
    match clock::initial(cfg, &sh).await {
        Ok(b) => say!("clock offset {:+.1} ms rtt {:.1} ms", b.offset_ms, b.rtt_ms),
        Err(e) => say!("clock NOT synced ({e}); label is sent flagged clock_synced=false"),
    }
    let ev = sh.add_label(&def.id, &def.text, note, "cli");
    if !tags.is_empty() {
        let mut t = labels::LabelEvent::new(&session, sh.next_seq(), sh.mono_ns(), 0, labels::TAG_ID, "标记", None, "cli", now_ms(), &sh.clock_view(), Default::default());
        t.action = Some("tag".into());
        t.ref_uid = Some(ev.uid.clone());
        t.tags = tags;
        sh.labels.lock().unwrap().push(t);
    }
    let lb = labels::LabelBatch {
        session: session.clone(),
        player: cfg.player.clone(),
        version: VERSION.into(),
        clock_synced_now: sh.clock_view().synced,
        clock_offset_now_ms: sh.clock_view().offset_ms,
        sent_local_ms: now_ms(),
        labels: sh.labels.lock().unwrap().queue.iter().cloned().collect(),
    };
    let r = upload::send_labels(&upload::up_client(), cfg, &lb).await.map_err(|e| anyhow::anyhow!("send label: {e}"))?;
    say!("label {} sent: {}", ev.uid, r);
    Ok(())
}

fn new_shared(session: String, player: String, base: String) -> Shared {
    Shared {
        session,
        player,
        start: Instant::now(),
        seq: AtomicU64::new(0),
        epoch: AtomicU32::new(0),
        last_ok_mono_ms: AtomicI64::new(-1),
        stats: Mutex::new(HashMap::new()),
        game: Mutex::new(GameState::default()),
        up: Mutex::new(UpStat::default()),
        map_changed: Notify::new(),
        total_records: AtomicU64::new(0),
        hz: Default::default(),
        consec_fail: AtomicU64::new(0),
        last_err_kind: Mutex::new(String::new()),
        base: Mutex::new(base),
        clock: Mutex::new(clock::ClockState::default()),
        labels: Mutex::new(labels::LabelBook::default()),
        label_notify: Notify::new(),
        paused: Mutex::new(None),
        suppressed: AtomicU64::new(0),
    }
}

pub async fn run(cfg: Cfg, ui: Option<UiLink>) -> Result<()> {
    let session = format!(
        "{}-{}-{:04x}",
        chrono::Local::now().format("%Y%m%d-%H%M%S"),
        sanitize(&cfg.player),
        rand::random::<u16>()
    );

    say!("wt-collector {VERSION}  session {session}");
    let cands = game::candidates(cfg.game_host.as_deref(), cfg.game_port);
    let probe_client = local_client()?;
    let (found, results) = game::probe_all(&probe_client, &cands).await;
    let initial_base = found.clone().unwrap_or_else(|| cands[0].clone());
    say!(
        "  game   : {} ({})  candidates: {}",
        game::short(&initial_base),
        if found.is_some() { "detected" } else { "not detected yet, will keep probing" },
        cands.iter().map(|c| game::short(c)).collect::<Vec<_>>().join(", ")
    );
    let probe_json: Vec<serde_json::Value> = results
        .iter()
        .map(|(c, r)| serde_json::json!({"base": c, "ok": r.is_ok(), "result": match r { Ok(s) | Err(s) => s }}))
        .collect();
    say!("  upload : {}  (nothing is stored on this PC; data older than the server threshold is dropped)", cfg.server);
    say!("  config : {}", cfg.config_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(none, defaults)".into()));
    let r = &cfg.rates;
    say!(
        "  rates  : map_obj {} indicators {} state {} map_info {} mission {} hudmsg {} gamechat {} Hz; map.img every {} s",
        r.map_obj, r.indicators, r.state, r.map_info, r.mission, r.hudmsg, r.gamechat, r.map_img_check_s
    );
    if cfg.token.is_empty() {
        say!("  WARNING: no token configured; uploads will be rejected");
    }
    if ui.is_none() {
        say!("  在窗口中输入文字并回车 = 添加备注(note)；输入 /rate 20 = 切换采样率；Ctrl+C = 停止\n");
    }

    let sh = Arc::new(new_shared(session.clone(), cfg.player.clone(), initial_base.clone()));
    if let Some(u) = &ui {
        let _ = u.shared_tx.send(sh.clone());
    }
    match clock::initial(&cfg, &sh).await {
        Ok(b) => say!("  clock  : offset {:+.1} ms (server - local), RTT {:.1} ms", b.offset_ms, b.rtt_ms),
        Err(e) => say!("  clock  : NOT synced yet ({e}); retrying every 2 s, nothing is uploaded until synced"),
    }

    let meta = serde_json::json!({
        "session": session, "player": cfg.player, "version": VERSION,
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "rates": cfg.rates, "server": cfg.server,
        "game_base": initial_base, "game_detected": found.is_some(), "game_probe": probe_json,
        "game_host_cfg": cfg.game_host, "game_port_cfg": cfg.game_port,
        "start_wall_ms": now_ms(),
        "host": std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default(),
    });

    let (tx, rx) = mpsc::unbounded_channel::<Record>();
    let (up_tx, up_rx) = mpsc::unbounded_channel::<upload::UpItem>();
    let (stop_tx, stop_rx) = watch::channel(false);

    for n in &cfg.notes {
        let _ = tx.send(sh.synthetic("#note", n.clone()));
    }

    let local_client = local_client()?;

    let uploader = tokio::spawn(upload::uploader_loop(cfg.clone(), sh.clone(), up_rx));
    let labeler = tokio::spawn(upload::label_loop(cfg.clone(), sh.clone(), stop_rx.clone()));
    let (wstop_tx, wstop_rx) = watch::channel(false);
    let writer = tokio::spawn(writer_loop(rx, up_tx.clone(), sh.clone(), wstop_rx));
    tokio::spawn(clock::sync_loop(cfg.clone(), sh.clone(), tx.clone(), Some(meta.to_string()), stop_rx.clone()));

    let mut tasks = Vec::new();
    for (k, hz) in [
        (Kind::MapObj, r.map_obj),
        (Kind::Indicators, r.indicators),
        (Kind::State, r.state),
        (Kind::MapInfo, r.map_info),
        (Kind::Mission, r.mission),
        (Kind::Hud, r.hudmsg),
        (Kind::Chat, r.gamechat),
    ] {
        sh.set_hz(k, hz);
        tasks.push(tokio::spawn(poll_loop(sh.clone(), local_client.clone(), k, tx.clone(), stop_rx.clone())));
    }
    tasks.push(tokio::spawn(map_img_loop(
        sh.clone(),
        local_client.clone(),
        r.map_img_check_s,
        tx.clone(),
        up_tx.clone(),
        stop_rx.clone(),
    )));
    let status = tokio::spawn(status_loop(sh.clone(), stop_rx.clone()));
    {
        // re-probe when the current base keeps failing (port can change between game launches)
        let (sh, tx, mut stop, cands) = (sh.clone(), tx.clone(), stop_rx.clone(), cands.clone());
        let mut detected = found.is_some();
        tokio::spawn(async move {
            let mut last_probe = Instant::now();
            loop {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                    _ = stop.changed() => break,
                }
                let failing = sh.consec_fail.load(Ordering::Relaxed) >= 15 && !sh.reachable();
                if !(failing || !detected) || last_probe.elapsed() < Duration::from_secs(5) {
                    continue;
                }
                last_probe = Instant::now();
                let (best, res) = game::probe_all(&probe_client, &cands).await;
                if let Some(b) = best {
                    detected = true;
                    let old = sh.base();
                    if b != old {
                        *sh.base.lock().unwrap() = b.clone();
                        sh.consec_fail.store(0, Ordering::Relaxed);
                        sh.epoch.fetch_add(1, Ordering::SeqCst);
                        say!("  [game] API now at {} (was {})", game::short(&b), game::short(&old));
                    }
                    let pj: Vec<serde_json::Value> = res.iter().map(|(c, r)| serde_json::json!({"base": c, "ok": r.is_ok(), "result": match r { Ok(s) | Err(s) => s }})).collect();
                    let _ = tx.send(sh.synthetic("#game", serde_json::json!({"base": b, "previous": old, "probe": pj}).to_string()));
                }
            }
        });
    }
    if let Some(secs) = cfg.rate_cycle_s {
        let sh = sh.clone();
        let tx = tx.clone();
        let mut stop = stop_rx.clone();
        tokio::spawn(async move {
            let presets = [2.0, 5.0, 10.0, 20.0];
            let mut i = 0usize;
            loop {
                let v = presets[i % presets.len()];
                sh.set_fast(v);
                let _ = tx.send(sh.synthetic("#note", format!("rate preset {v} Hz (map_obj/indicators/state) [auto cycle]")));
                say!("  [rate-cycle] fast group now {v} Hz");
                i += 1;
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs_f64(secs)) => {}
                    _ = stop.changed() => break,
                }
            }
        });
    }

    // notes typed into the console (detached OS thread; blocking stdin) -- CLI mode only
    if ui.is_none() {
        let tx = tx.clone();
        let sh = sh.clone();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            let mut line = String::new();
            loop {
                line.clear();
                match stdin.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {
                        let t = line.trim();
                        if let Some(v) = t.strip_prefix("/rate ").and_then(|v| v.trim().parse::<f64>().ok()) {
                            sh.set_fast(v);
                            let _ = tx.send(sh.synthetic("#note", format!("rate preset {v} Hz (map_obj/indicators/state)")));
                            say!("  [rate] fast group now {v} Hz (note recorded)");
                        } else if !t.is_empty() {
                            if tx.send(sh.synthetic("#note", t.to_string())).is_err() {
                                break;
                            }
                            say!("  [note recorded] {t}");
                        }
                    }
                }
            }
        });
    }
    drop(tx);
    drop(up_tx);

    wait_shutdown(cfg.duration_s, ui.map(|u| u.stop)).await;
    say!("\nstopping... sending what is still in memory (max ~5 s)");
    let _ = stop_tx.send(true);
    for t in tasks {
        let _ = t.await;
    }
    let _ = status.await;
    let _ = wstop_tx.send(true);
    let _ = tokio::time::timeout(Duration::from_secs(10), writer).await;
    let _ = tokio::time::timeout(Duration::from_secs(7), uploader).await;
    let _ = tokio::time::timeout(Duration::from_secs(7), labeler).await;
    say!("{}", status_line(&sh));
    say!("done.");
    Ok(())
}

async fn wait_shutdown(duration_s: Option<f64>, ui_stop: Option<watch::Receiver<bool>>) {
    let dur = async {
        let d = async {
            match duration_s {
                Some(s) => tokio::time::sleep(Duration::from_secs_f64(s)).await,
                None => std::future::pending::<()>().await,
            }
        };
        let u = async {
            match ui_stop {
                Some(mut r) => {
                    while !*r.borrow() {
                        if r.changed().await.is_err() {
                            break;
                        }
                    }
                }
                None => std::future::pending::<()>().await,
            }
        };
        tokio::select! { _ = d => {}, _ = u => {} }
    };
    #[cfg(windows)]
    {
        let mut close = tokio::signal::windows::ctrl_close().ok();
        let close_f = async {
            match close.as_mut() {
                Some(c) => {
                    c.recv().await;
                }
                None => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = close_f => {}
            _ = dur => {}
        }
    }
    #[cfg(not(windows))]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).ok();
        let term_f = async {
            match term.as_mut() {
                Some(t) => {
                    t.recv().await;
                }
                None => std::future::pending::<()>().await,
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = term_f => {}
            _ = dur => {}
        }
    }
}
