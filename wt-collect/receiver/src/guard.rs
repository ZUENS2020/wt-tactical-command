//! Clock-sync / staleness guard: every record must carry a server-corrected timestamp close to the
//! server receive time. Records outside +-WT_MAX_SKEW_MS are dropped (too_old / from_future); batches from
//! collectors without a valid clock sync are rejected (no_sync). Legacy collectors (no clock info) are
//! accepted only per WT_ACCEPT_LEGACY (auto = only versions < 0.1.5).
use serde::Serialize;
use wtc_common::{parse_version, Batch, ClockInfo};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Legacy {
    Auto,
    On,
    Off,
}

#[derive(Clone, Debug)]
pub struct GuardCfg {
    pub max_skew_ms: i64,
    pub legacy: Legacy,
}

impl GuardCfg {
    pub fn from_env() -> Self {
        let max_skew_ms = std::env::var("WT_MAX_SKEW_MS").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(wtc_common::LIVE_MAX_AGE_MS);
        let legacy = match std::env::var("WT_ACCEPT_LEGACY").unwrap_or_default().trim().to_ascii_lowercase().as_str() {
            "on" | "1" | "true" | "yes" => Legacy::On,
            "off" | "0" | "false" | "no" => Legacy::Off,
            _ => Legacy::Auto,
        };
        GuardCfg { max_skew_ms, legacy }
    }
}

#[derive(Debug, PartialEq)]
pub enum Mode {
    Synced(f64),
    Legacy,
    RejectNoSync,
}

pub fn batch_mode(cfg: &GuardCfg, b: &Batch) -> Mode {
    match &b.clock {
        Some(c) if c.synced && c.offset_ms.is_finite() => Mode::Synced(c.offset_ms),
        Some(_) => Mode::RejectNoSync,
        None => {
            let legacy_ok = match cfg.legacy {
                Legacy::On => true,
                Legacy::Off => false,
                Legacy::Auto => parse_version(&b.version) < (0, 1, 5),
            };
            if legacy_ok {
                Mode::Legacy
            } else {
                Mode::RejectNoSync
            }
        }
    }
}

#[cfg(test)]
#[derive(Debug, PartialEq)]
pub enum Verdict {
    Accept,
    TooOld,
    FromFuture,
}

/// late = recv - ts (positive = record is in the past)
#[cfg(test)]
pub fn verdict(cfg: &GuardCfg, ts_server_ms: i64, recv_ms: i64) -> Verdict {
    let late = recv_ms - ts_server_ms;
    if late > cfg.max_skew_ms {
        Verdict::TooOld
    } else if late < -cfg.max_skew_ms {
        Verdict::FromFuture
    } else {
        Verdict::Accept
    }
}

#[derive(Default, Debug, Serialize, serde::Deserialize, Clone)]
pub struct GuardStats {
    pub accepted: u64,
    pub too_old: u64,
    pub from_future: u64,
    pub no_sync: u64,
    pub batches_rejected_no_sync: u64,
    pub legacy_accepted: u64,
    pub max_skew_ms: i64,
    /// receive delay of accepted records (recv - ts_server), ms
    pub delay_max_ms: i64,
    pub delay_min_ms: i64,
    pub delay_sum_ms: i64,
    pub delay_n: u64,
    /// histogram of accepted delays, 100 ms buckets from -3000..+3000 (index 0 = < -2900)
    pub delay_hist: Vec<u64>,
    pub last_clock: Option<ClockInfo>,
    pub offset_min_ms: Option<f64>,
    pub offset_max_ms: Option<f64>,
    pub rtt_min_ms: Option<f64>,
    pub rtt_max_ms: Option<f64>,
    pub updated_ms: i64,
}

impl GuardStats {
    pub fn note_clock(&mut self, c: &ClockInfo) {
        if c.synced {
            self.offset_min_ms = Some(self.offset_min_ms.map_or(c.offset_ms, |v| v.min(c.offset_ms)));
            self.offset_max_ms = Some(self.offset_max_ms.map_or(c.offset_ms, |v| v.max(c.offset_ms)));
            self.rtt_min_ms = Some(self.rtt_min_ms.map_or(c.rtt_ms, |v| v.min(c.rtt_ms)));
            self.rtt_max_ms = Some(self.rtt_max_ms.map_or(c.rtt_ms, |v| v.max(c.rtt_ms)));
        }
        self.last_clock = Some(c.clone());
    }
    pub fn note_delay(&mut self, d: i64) {
        if self.delay_n == 0 {
            self.delay_min_ms = d;
            self.delay_max_ms = d;
        }
        self.delay_min_ms = self.delay_min_ms.min(d);
        self.delay_max_ms = self.delay_max_ms.max(d);
        self.delay_sum_ms += d;
        self.delay_n += 1;
        if self.delay_hist.len() != 61 {
            self.delay_hist = vec![0; 61];
        }
        let i = ((d + 3050).div_euclid(100)).clamp(0, 60) as usize;
        self.delay_hist[i] += 1;
    }
    pub fn delay_pct(&self, q: f64) -> Option<i64> {
        if self.delay_n == 0 {
            return None;
        }
        let target = (self.delay_n as f64 * q).ceil() as u64;
        let mut acc = 0;
        for (i, n) in self.delay_hist.iter().enumerate() {
            acc += n;
            if acc >= target {
                return Some(i as i64 * 100 - 3000);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn b(version: &str, clock: Option<ClockInfo>) -> Batch {
        Batch { device_id: String::new(), nickname: String::new(), session: "s".into(), player: "p".into(), version: version.into(), clock, records: vec![] }
    }
    #[test]
    fn modes() {
        let auto = GuardCfg { max_skew_ms: 3000, legacy: Legacy::Auto };
        assert_eq!(batch_mode(&auto, &b("0.1.4", None)), Mode::Legacy);
        assert_eq!(batch_mode(&auto, &b("0.1.5", None)), Mode::RejectNoSync);
        assert_eq!(batch_mode(&auto, &b("0.2.0", None)), Mode::RejectNoSync);
        let c = ClockInfo { synced: true, offset_ms: -45000.5, rtt_ms: 200.0, ..Default::default() };
        assert_eq!(batch_mode(&auto, &b("0.1.5", Some(c))), Mode::Synced(-45000.5));
        let nc = ClockInfo { synced: false, ..Default::default() };
        assert_eq!(batch_mode(&auto, &b("0.1.5", Some(nc))), Mode::RejectNoSync);
        let off = GuardCfg { legacy: Legacy::Off, ..auto.clone() };
        assert_eq!(batch_mode(&off, &b("0.1.4", None)), Mode::RejectNoSync);
        let on = GuardCfg { legacy: Legacy::On, ..auto };
        assert_eq!(batch_mode(&on, &b("0.1.9", None)), Mode::Legacy);
    }
    #[test]
    fn verdicts() {
        let g = GuardCfg { max_skew_ms: 3000, legacy: Legacy::Auto };
        assert_eq!(verdict(&g, 10_000, 12_999), Verdict::Accept);
        assert_eq!(verdict(&g, 10_000, 13_001), Verdict::TooOld);
        assert_eq!(verdict(&g, 10_000, 6_999), Verdict::FromFuture);
        assert_eq!(verdict(&g, 10_000, 7_000), Verdict::Accept);
    }
    #[test]
    fn hist() {
        let mut s = GuardStats::default();
        for d in [50, 120, 180, 250, 2900] {
            s.note_delay(d);
        }
        assert_eq!(s.delay_pct(0.5), Some(200));
        assert_eq!(s.delay_pct(1.0), Some(2900));
        assert_eq!(s.delay_max_ms, 2900);
    }
}
