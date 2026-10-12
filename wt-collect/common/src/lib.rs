//! Shared record format for wt-collector / wt-receiver / wt-analyze.
pub mod mapobj;
pub mod radio;

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

/// Maximum age of a live observation. Static resources and drawings have their own lifetime.
pub const LIVE_MAX_AGE_MS: i64 = 2000;

/// One raw sample of one 8111 request (or a synthetic record: ep starting with '#').
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Record {
    pub session: String,
    pub seq: u64,
    /// Endpoint key, e.g. "/map_obj.json", "/hudmsg", "/map.img"; "#note", "#meta" for synthetic records.
    pub ep: String,
    /// Exact path+query requested (e.g. "/hudmsg?lastEvt=12&lastDmg=40").
    #[serde(default)]
    pub url: String,
    /// Wall clock at request start, unix ms.
    pub wall_ms: i64,
    /// Monotonic ns since collector process start, at request start.
    #[serde(default)]
    pub mono_ns: u64,
    /// Request latency (start -> full body read), microseconds.
    #[serde(default)]
    pub lat_us: u64,
    /// HTTP status, 0 = transport error (see err).
    #[serde(default)]
    pub status: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub err: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ctype: Option<String>,
    /// Body length in bytes.
    #[serde(default)]
    pub bytes: u64,
    /// Raw body exactly as received (when valid UTF-8).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Raw body base64 (only when not valid UTF-8 and not an image).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_b64: Option<String>,
    /// sha256 hex of image bytes (map.img); bytes stored separately as images/<hash>.<ext>.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub img_hash: Option<String>,
    /// Body differs from previous body of the same endpoint.
    #[serde(default)]
    pub changed: bool,
    /// Configured poll rate for this endpoint at the time of the sample.
    #[serde(default)]
    pub cfg_hz: f64,
    /// Collector's match epoch (incremented when a new match is detected; chat/hud cursors reset).
    #[serde(default)]
    pub epoch: u32,
    /// Opaque collector-local map context captured at request start.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_context: Option<String>,
    /// Set by the receiver: number of naval objects removed from this map_obj body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filtered_naval: Option<u32>,
    /// Set by the receiver: record content dropped (e.g. "naval" = own vehicle is a ship); timing kept.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<String>,
    /// Collector: wall_ms corrected to server time (wall_ms + clock offset), unix ms. (0.1.5+)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts_server_ms: Option<i64>,
    /// Receiver: server time when the record was received, unix ms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recv_ms: Option<i64>,
}

/// Upload batch (JSON, gzip on the wire).
#[derive(Debug, Serialize, Deserialize)]
pub struct Batch {
    pub session: String,
    #[serde(default)]
    pub device_id: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub player: String,
    #[serde(default)]
    pub version: String,
    /// Collector clock-sync state when the batch was built (0.1.5+). None = legacy collector.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock: Option<ClockInfo>,
    pub records: Vec<Record>,
}

/// NTP-style clock estimate of the collector: server_time = local_time + offset_ms.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClockInfo {
    pub synced: bool,
    pub offset_ms: f64,
    /// round-trip time of the sample the offset was taken from (min-RTT of the burst)
    pub rtt_ms: f64,
    /// age of the last successful sync
    #[serde(default)]
    pub sync_age_ms: u64,
    #[serde(default)]
    pub syncs: u64,
    /// counters of records the collector dropped before sending
    #[serde(default)]
    pub local_drop_too_old: u64,
    #[serde(default)]
    pub local_drop_overflow: u64,
    #[serde(default)]
    pub local_drop_no_sync: u64,
}

/// "0.1.5" -> (0,1,5); unparsable -> (0,0,0)
pub fn parse_version(v: &str) -> (u32, u32, u32) {
    let mut it = v.trim().trim_start_matches('v').split(|c: char| c == '.' || c == '-').map(|x| x.parse::<u32>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

pub const DEFAULT_PORT: u16 = 18111;

/// List sample files (samples-*.jsonl / samples-*.jsonl.gz) in a session dir, sorted.
pub fn sample_files(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| {
                    let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
                    n.starts_with("samples-") && (n.ends_with(".jsonl") || n.ends_with(".jsonl.gz"))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Read all lines of a .jsonl or .jsonl.gz (multi-member gzip) file, tolerating a truncated tail.
pub fn read_lines(path: &Path, mut f: impl FnMut(&str)) -> anyhow::Result<()> {
    let file = std::fs::File::open(path)?;
    let rdr: Box<dyn Read> = if path.extension().map(|e| e == "gz").unwrap_or(false) {
        Box::new(flate2::read::MultiGzDecoder::new(BufReader::new(file)))
    } else {
        Box::new(file)
    };
    let mut br = BufReader::with_capacity(1 << 20, rdr);
    let mut line = String::new();
    loop {
        line.clear();
        match br.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let t = line.trim_end();
                if !t.is_empty() {
                    f(t)
                }
            }
            Err(e) => {
                eprintln!("warn: {}: stopped reading at error: {e}", path.display());
                break;
            }
        }
    }
    Ok(())
}

/// Read all records of a session dir (sorted by seq).
pub fn read_session(dir: &Path) -> anyhow::Result<Vec<Record>> {
    let mut out = Vec::new();
    let mut bad = 0usize;
    for p in sample_files(dir) {
        read_lines(&p, |l| match serde_json::from_str::<Record>(l) {
            Ok(r) => out.push(r),
            Err(_) => bad += 1,
        })?;
    }
    if bad > 0 {
        eprintln!("warn: {bad} unparsable lines skipped");
    }
    out.sort_by_key(|r| r.seq);
    out.dedup_by_key(|r| r.seq);
    Ok(out)
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn ext_for_ctype(ct: &str) -> &'static str {
    let c = ct.to_ascii_lowercase();
    if c.contains("png") {
        "png"
    } else if c.contains("jpeg") || c.contains("jpg") {
        "jpg"
    } else if c.contains("webp") {
        "webp"
    } else {
        "bin"
    }
}

/// Sniff actual image encoding from magic bytes (extension does not imply encoding per spec §4.6).
pub fn sniff_image(b: &[u8]) -> &'static str {
    if b.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if b.starts_with(&[0xFF, 0xD8]) {
        "jpg"
    } else if b.len() > 12 && &b[0..4] == b"RIFF" && &b[8..12] == b"WEBP" {
        "webp"
    } else {
        "bin"
    }
}
