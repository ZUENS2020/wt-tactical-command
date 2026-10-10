//! Data-labeling events (ground truth for analysis). 0.2.0.
//!
//! A label is created when the user clicks a button in the GUI (source = "button"), or via
//! `wt-collector label <id>` (source = "cli"). Labels are stamped with the same synced clock as telemetry
//! (t_server_synced = t_local + clock offset) and sent to the receiver's POST /api/label on their own
//! queue: they are NEVER dropped by the staleness filter, are retried until the receiver confirms, and
//! are sent even without a clock sync (flagged clock_synced=false) and while uploads are paused (hangar).
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// One button definition (config: `[[labels]]` in wt-collector.toml).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelDef {
    /// stable machine id used by the analysis (ascii, e.g. "i_scouted")
    pub id: String,
    /// button text shown in the window (Chinese)
    pub text: String,
    /// optional button color "#rrggbb"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// true = clicking opens the note input (free text) instead of sending immediately
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub note: bool,
}

pub const NOTE_ID: &str = "note";
pub const UNDO_ID: &str = "_undo";
pub const TAG_ID: &str = "_tag";
pub const UNDO_WINDOW: Duration = Duration::from_secs(10);

pub fn default_labels() -> Vec<LabelDef> {
    let d = |id: &str, text: &str, color: &str| LabelDef { id: id.into(), text: text.into(), color: Some(color.into()), note: false };
    vec![
        d("i_scouted", "我侦察到敌人", "#2e7d32"),
        d("team_scouted", "队友侦察", "#00838f"),
        d("i_hit", "我击中敌人", "#ef6c00"),
        d("got_hit", "我被击中", "#c62828"),
        d("i_spotted", "我被侦察", "#ad1457"),
        d("enemy_icon", "敌机/敌人出现在地图上", "#6a1b9a"),
        d("kill", "击毁", "#37474f"),
        d("died", "阵亡", "#212121"),
        LabelDef { id: NOTE_ID.into(), text: "自定义备注".into(), color: Some("#455a64".into()), note: true },
    ]
}

/// Validate a configured label list; empty/missing -> defaults. Errors are human readable (Chinese).
pub fn resolve_labels(cfg: Option<Vec<LabelDef>>) -> Result<Vec<LabelDef>, String> {
    let list = match cfg {
        None => return Ok(default_labels()),
        Some(v) if v.is_empty() => return Ok(default_labels()),
        Some(v) => v,
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for (i, mut l) in list.into_iter().enumerate() {
        l.id = l.id.trim().to_string();
        l.text = l.text.trim().to_string();
        if l.id.is_empty() || l.id.len() > 40 || !l.id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(format!("labels 第 {} 项: id \"{}\" 无效（只能用英文字母/数字/_/-，最长 40）", i + 1, l.id));
        }
        if l.id.starts_with('_') {
            return Err(format!("labels 第 {} 项: id 不能以 _ 开头（保留给撤销/标记）", i + 1));
        }
        if l.text.is_empty() {
            l.text = l.id.clone();
        }
        if let Some(c) = &l.color {
            if parse_color(c).is_none() {
                return Err(format!("labels 第 {} 项: color \"{c}\" 无效（格式 #rrggbb）", i + 1));
            }
        }
        if !seen.insert(l.id.clone()) {
            return Err(format!("labels 第 {} 项: id \"{}\" 重复", i + 1, l.id));
        }
        out.push(l);
    }
    if out.len() > 24 {
        return Err("labels 最多 24 项".into());
    }
    Ok(out)
}

pub fn parse_color(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// Snapshot of what the collector knows at the moment of the click (cheap: from shared state).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub in_battle: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_gen: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_img_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hud_type: Option<String>,
    /// own position from map_obj (normalized 0..1), heading vector
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_xy: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_dxdy: Option<[f64; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub army: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vehicle: Option<String>,
    /// number of objects in the latest map_obj frame and how many are enemy red
    pub objects: usize,
    pub enemy_objects: usize,
    /// seq and local wall time of the latest map_obj frame
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_obj_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map_obj_wall_ms: Option<i64>,
}

/// The label record (body of the "#label" Record stored by the receiver).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LabelEvent {
    #[serde(rename = "type")]
    pub kind: String, // always "label"
    /// unique id of this event (session + seq)
    pub uid: String,
    pub label_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// local wall clock at click, unix ms
    pub t_local: i64,
    /// t_local corrected to server time (t_local + offset) when the clock was synced at click; else None
    #[serde(default)]
    pub t_server_synced: Option<i64>,
    pub clock_synced: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock_offset_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clock_rtt_ms: Option<f64>,
    /// "button" | "cli"
    pub source: String,
    pub session: String,
    pub seq: u64,
    /// collector monotonic ns since start (same base as Record.mono_ns)
    pub mono_ns: u64,
    pub epoch: u32,
    /// "<session>:e<epoch>:g<map_gen>" when in battle
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub battle_id: Option<String>,
    /// "undo" / "tag" for meta events (label_id _undo / _tag), referencing ref_uid
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_uid: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub snapshot: Snapshot,
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct ClockView {
    pub synced: bool,
    pub offset_ms: f64,
    pub rtt_ms: f64,
}

impl LabelEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        session: &str,
        seq: u64,
        mono_ns: u64,
        epoch: u32,
        def_id: &str,
        text: &str,
        note: Option<String>,
        source: &str,
        t_local: i64,
        clock: &ClockView,
        snapshot: Snapshot,
    ) -> Self {
        let battle_id = if snapshot.in_battle { Some(format!("{session}:e{epoch}:g{}", snapshot.map_gen.as_deref().unwrap_or("?"))) } else { None };
        LabelEvent {
            kind: "label".into(),
            uid: format!("{session}#{seq}"),
            label_id: def_id.into(),
            text: text.into(),
            note: note.map(|n| n.trim().to_string()).filter(|n| !n.is_empty()),
            t_local,
            t_server_synced: if clock.synced { Some((t_local as f64 + clock.offset_ms).round() as i64) } else { None },
            clock_synced: clock.synced,
            clock_offset_ms: if clock.synced { Some(clock.offset_ms) } else { None },
            clock_rtt_ms: if clock.synced { Some(clock.rtt_ms) } else { None },
            source: source.into(),
            session: session.into(),
            seq,
            mono_ns,
            epoch,
            battle_id,
            action: None,
            ref_uid: None,
            tags: vec![],
            snapshot,
            version: crate::VERSION.into(),
        }
    }

    /// Wrap as a storage Record (ep "#label"; body = this JSON). ts_server_ms is the synced time if known.
    pub fn to_record(&self) -> wtc_common::Record {
        let body = serde_json::to_string(self).unwrap_or_default();
        wtc_common::Record {
            session: self.session.clone(),
            seq: self.seq,
            ep: "#label".into(),
            url: String::new(),
            wall_ms: self.t_local,
            mono_ns: self.mono_ns,
            bytes: body.len() as u64,
            body: Some(body),
            changed: true,
            epoch: self.epoch,
            ts_server_ms: self.t_server_synced,
            ..Default::default()
        }
    }
}

/// Wire format of POST /api/label.
#[derive(Debug, Serialize, Deserialize)]
pub struct LabelBatch {
    pub session: String,
    pub player: String,
    pub version: String,
    /// collector clock at send time (lets the receiver estimate server time for unsynced labels)
    pub clock_synced_now: bool,
    #[serde(default)]
    pub clock_offset_now_ms: f64,
    pub sent_local_ms: i64,
    pub labels: Vec<LabelEvent>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum LabelState {
    Pending,
    Sent,
    Undone,
}

pub struct LabelEntry {
    pub ev: LabelEvent,
    pub state: LabelState,
    pub created: Instant,
    pub tries: u32,
    pub last_err: String,
}

/// Session-wide label log + send queue (shared between GUI, sender task, status).
#[derive(Default)]
pub struct LabelBook {
    pub entries: Vec<LabelEntry>,
    pub queue: VecDeque<LabelEvent>,
    pub sent: u64,
    pub fail: u64,
    pub last_err: String,
    pub last_ok: Option<Instant>,
}

impl LabelBook {
    pub fn push(&mut self, ev: LabelEvent) {
        self.queue.push_back(ev.clone());
        self.entries.push(LabelEntry { ev, state: LabelState::Pending, created: Instant::now(), tries: 0, last_err: String::new() });
        if self.entries.len() > 500 {
            self.entries.remove(0);
        }
    }
    /// user labels (not meta) created this session, minus undone
    pub fn count_effective(&self) -> usize {
        self.entries.iter().filter(|e| e.ev.action.is_none() && e.state != LabelState::Undone).count()
    }
    pub fn pending(&self) -> usize {
        self.queue.len()
    }
    /// latest user label still inside the undo window
    pub fn undoable(&self) -> Option<(&LabelEvent, Duration)> {
        let e = self.entries.iter().rev().find(|e| e.ev.action.is_none())?;
        if e.state == LabelState::Undone {
            return None;
        }
        let age = e.created.elapsed();
        if age <= UNDO_WINDOW {
            Some((&e.ev, UNDO_WINDOW - age))
        } else {
            None
        }
    }
    pub fn mark_sent(&mut self, uids: &[String]) {
        self.sent += uids.len() as u64;
        for e in self.entries.iter_mut() {
            if e.state == LabelState::Pending && uids.contains(&e.ev.uid) {
                e.state = LabelState::Sent;
            }
        }
    }
    pub fn mark_undone(&mut self, uid: &str) {
        for e in self.entries.iter_mut() {
            if e.ev.uid == uid {
                e.state = LabelState::Undone;
            }
        }
        // not sent yet: just remove it from the queue (the undo event still documents it)
        self.queue.retain(|q| q.uid != uid);
    }
}

pub type SharedBook = Mutex<LabelBook>;

#[cfg(test)]
mod tests {
    use super::*;

    fn ck(s: bool) -> ClockView {
        ClockView { synced: s, offset_ms: 213.6, rtt_ms: 180.0 }
    }

    #[test]
    fn defaults_are_valid_and_chinese() {
        let d = resolve_labels(None).unwrap();
        assert_eq!(d.len(), 9);
        assert_eq!(d[0].text, "我侦察到敌人");
        assert_eq!(d[7].id, "died");
        assert!(d[8].note);
        assert_eq!(resolve_labels(Some(vec![])).unwrap(), d);
    }

    #[test]
    fn label_config_parsing() {
        #[derive(Deserialize)]
        struct F {
            labels: Option<Vec<LabelDef>>,
        }
        let t = r##"
[[labels]]
id = "flank"
text = "敌人绕后"
color = "#ff0000"

[[labels]]
id = "memo"
text = "写备注"
note = true
"##;
        let f: F = toml::from_str(t).unwrap();
        let l = resolve_labels(f.labels).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].id, "flank");
        assert_eq!(l[0].color.as_deref(), Some("#ff0000"));
        assert!(!l[0].note);
        assert!(l[1].note);
        // missing section -> defaults
        let f: F = toml::from_str("server = 'x'").unwrap_or(F { labels: None });
        assert_eq!(resolve_labels(f.labels).unwrap().len(), 9);
        // errors
        let bad = |s: &str| resolve_labels(toml::from_str::<F>(s).unwrap().labels).unwrap_err();
        assert!(bad("[[labels]]\nid='a b'\ntext='x'").contains("无效"));
        assert!(bad("[[labels]]\nid='a'\ntext='x'\n[[labels]]\nid='a'\ntext='y'").contains("重复"));
        assert!(bad("[[labels]]\nid='_x'\ntext='x'").contains("_"));
        assert!(bad("[[labels]]\nid='a'\ntext='x'\ncolor='red'").contains("color"));
        // text defaults to id
        assert_eq!(resolve_labels(toml::from_str::<F>("[[labels]]\nid='a'\ntext=''").unwrap().labels).unwrap()[0].text, "a");
        assert_eq!(parse_color("#2e7d32"), Some([0x2e, 0x7d, 0x32]));
    }

    #[test]
    fn label_serialization() {
        let snap = Snapshot { in_battle: true, map_gen: Some("27".into()), self_xy: Some([0.5, 0.25]), objects: 12, enemy_objects: 3, map_obj_seq: Some(900), ..Default::default() };
        let ev = LabelEvent::new("20261010-200000-shengyuan-abcd", 1234, 99, 3, "i_hit", "我击中敌人", None, "button", 1_760_000_000_000, &ck(true), snap);
        let j: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(j["type"], "label");
        assert_eq!(j["label_id"], "i_hit");
        assert_eq!(j["text"], "我击中敌人");
        assert_eq!(j["source"], "button");
        assert_eq!(j["t_local"], 1_760_000_000_000i64);
        assert_eq!(j["t_server_synced"], 1_760_000_000_214i64);
        assert_eq!(j["clock_synced"], true);
        assert_eq!(j["uid"], "20261010-200000-shengyuan-abcd#1234");
        assert_eq!(j["battle_id"], "20261010-200000-shengyuan-abcd:e3:g27");
        assert_eq!(j["snapshot"]["self_xy"][0], 0.5);
        assert!(j.get("note").is_none());
        let back: LabelEvent = serde_json::from_value(j).unwrap();
        assert_eq!(back, ev);
        let r = ev.to_record();
        assert_eq!(r.ep, "#label");
        assert_eq!(r.seq, 1234);
        assert_eq!(r.ts_server_ms, Some(1_760_000_000_214));
        let inner: LabelEvent = serde_json::from_str(r.body.as_ref().unwrap()).unwrap();
        assert_eq!(inner, ev);
    }

    #[test]
    fn unsynced_label_is_flagged_not_dropped() {
        let ev = LabelEvent::new("s", 1, 0, 0, "note", "自定义备注", Some("  看到3辆  ".into()), "button", 5000, &ck(false), Snapshot::default());
        let j: serde_json::Value = serde_json::to_value(&ev).unwrap();
        assert_eq!(j["clock_synced"], false);
        assert!(j["t_server_synced"].is_null());
        assert_eq!(j["note"], "看到3辆");
        assert!(j.get("battle_id").is_none());
        assert_eq!(ev.to_record().ts_server_ms, None);
    }

    #[test]
    fn undo_window() {
        let mut b = LabelBook::default();
        let ev = LabelEvent::new("s", 1, 0, 0, "kill", "击毁", None, "button", 1, &ck(true), Snapshot::default());
        b.push(ev.clone());
        assert_eq!(b.count_effective(), 1);
        assert_eq!(b.undoable().unwrap().0.uid, "s#1");
        b.mark_undone("s#1");
        assert_eq!(b.count_effective(), 0);
        assert!(b.undoable().is_none());
        assert_eq!(b.pending(), 0);
        b.entries[0].state = LabelState::Pending;
        b.entries[0].created = Instant::now() - Duration::from_secs(11);
        assert!(b.undoable().is_none());
    }
}
