//! Label events from wt-collector 0.2.0 (POST /api/label, GET /api/labels).
//! Labels are ground truth: never dropped by the clock guard. Each label is stored
//!  1) as a Record (ep "#label", body = label JSON) in the session's samples-*.jsonl.gz (same storage,
//!     read by wtc_common::read_session), de-duplicated by (session, seq) like every record, and
//!  2) as one line in <session>/labels.jsonl (easy grep / jq / export).
//! The receiver adds: recv_ms, t_server (best server-time estimate), t_server_source
//! ("synced" | "offset_at_send" | "recv_estimate"), delay_ms (recv - t_server) and late (delay > max skew).
//! Meta events: label_id "_undo" (action "undo", ref_uid) and "_tag" (action "tag", ref_uid, tags[]).
use serde::Deserialize;
use std::collections::HashMap;
use wtc_common::Record;

#[derive(Deserialize, Debug)]
pub struct LabelBatchIn {
    pub session: String,
    #[serde(default)]
    pub player: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub clock_synced_now: bool,
    #[serde(default)]
    pub clock_offset_now_ms: f64,
    #[serde(default)]
    pub sent_local_ms: i64,
    pub labels: Vec<serde_json::Value>,
}

/// Validate + enrich one label; returns the storage record. Err = reason (label rejected as malformed).
pub fn to_record(b: &LabelBatchIn, mut v: serde_json::Value, now: i64, max_skew_ms: i64) -> Result<Record, String> {
    let o = v.as_object_mut().ok_or("label is not an object")?;
    if o.get("type").and_then(|t| t.as_str()) != Some("label") {
        return Err("type != label".into());
    }
    let seq = o.get("seq").and_then(|s| s.as_u64()).ok_or("missing seq")?;
    let label_id = o.get("label_id").and_then(|s| s.as_str()).ok_or("missing label_id")?.to_string();
    if label_id.is_empty() || label_id.len() > 64 {
        return Err("bad label_id".into());
    }
    let t_local = o.get("t_local").and_then(|s| s.as_i64()).ok_or("missing t_local")?;
    if let Some(s) = o.get("session").and_then(|s| s.as_str()) {
        if s != b.session {
            return Err("session mismatch".into());
        }
    }
    let synced = o.get("t_server_synced").and_then(|t| t.as_i64());
    let (t_server, src) = match synced {
        Some(t) => (t, "synced"),
        None if b.clock_synced_now => ((t_local as f64 + b.clock_offset_now_ms).round() as i64, "offset_at_send"),
        // no clock at all: assume the batch took ~0 ms in flight; age measured on the collector's own clock
        None => (now - (b.sent_local_ms - t_local).max(0), "recv_estimate"),
    };
    let delay = now - t_server;
    o.insert("recv_ms".into(), now.into());
    o.insert("t_server".into(), t_server.into());
    o.insert("t_server_source".into(), src.into());
    o.insert("delay_ms".into(), delay.into());
    o.insert("late".into(), (delay.abs() > max_skew_ms).into());
    o.insert("player".into(), b.player.clone().into());
    let body = serde_json::to_string(&v).map_err(|e| e.to_string())?;
    Ok(Record {
        session: b.session.clone(),
        seq,
        ep: "#label".into(),
        wall_ms: t_local,
        mono_ns: v["mono_ns"].as_u64().unwrap_or(0),
        bytes: body.len() as u64,
        body: Some(body),
        changed: true,
        epoch: v["epoch"].as_u64().unwrap_or(0) as u32,
        ts_server_ms: Some(t_server),
        recv_ms: Some(now),
        ..Default::default()
    })
}

/// Resolve undo/tag meta events. Returns user labels (oldest first) with "undone" and "tags" fields.
/// include_meta: also return the meta events themselves. include_test: keep labels tagged "test".
pub fn resolve(all: Vec<serde_json::Value>, include_meta: bool, include_test: bool, include_undone: bool) -> Vec<serde_json::Value> {
    let mut undone: HashMap<String, bool> = HashMap::new();
    let mut tags: HashMap<String, Vec<String>> = HashMap::new();
    let mut seen = std::collections::HashSet::new();
    let mut evs = Vec::new();
    for v in all {
        let uid = v["uid"].as_str().unwrap_or("").to_string();
        if !uid.is_empty() && !seen.insert(uid.clone()) {
            continue; // retried duplicate
        }
        match v["action"].as_str() {
            Some("undo") => {
                if let Some(r) = v["ref_uid"].as_str() {
                    undone.insert(r.to_string(), true);
                }
            }
            Some("tag") => {
                if let (Some(r), Some(t)) = (v["ref_uid"].as_str(), v["tags"].as_array()) {
                    let e = tags.entry(r.to_string()).or_default();
                    for x in t.iter().filter_map(|x| x.as_str()) {
                        if !e.iter().any(|y| y == x) {
                            e.push(x.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
        evs.push(v);
    }
    let mut out = Vec::new();
    for mut v in evs {
        let uid = v["uid"].as_str().unwrap_or("").to_string();
        let meta = v["action"].is_string();
        if meta && !include_meta {
            continue;
        }
        let u = undone.get(&uid).copied().unwrap_or(false);
        let mut t: Vec<String> = v["tags"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
        for x in tags.get(&uid).cloned().unwrap_or_default() {
            if !t.contains(&x) {
                t.push(x);
            }
        }
        if !meta && !include_test && t.iter().any(|x| x == "test") {
            continue;
        }
        if !meta && !include_undone && u {
            continue;
        }
        v["undone"] = u.into();
        v["tags"] = serde_json::json!(t);
        out.push(v);
    }
    out.sort_by_key(|v| v["t_server"].as_i64().or(v["t_local"].as_i64()).unwrap_or(0));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    fn batch(synced_now: bool) -> LabelBatchIn {
        LabelBatchIn { session: "s1".into(), player: "p".into(), version: "0.2.0".into(), clock_synced_now: synced_now, clock_offset_now_ms: 100.0, sent_local_ms: 10_000, labels: vec![] }
    }
    fn lab(seq: u64, t: i64, synced: Option<i64>) -> serde_json::Value {
        serde_json::json!({"type":"label","uid":format!("s1#{seq}"),"label_id":"kill","text":"击毁","t_local":t,"t_server_synced":synced,"clock_synced":synced.is_some(),"source":"button","session":"s1","seq":seq,"mono_ns":5,"epoch":2,"snapshot":{"in_battle":true,"objects":0,"enemy_objects":0},"version":"0.2.0"})
    }

    #[test]
    fn stale_label_is_kept_and_flagged() {
        // synced label 60 s old (e.g. queued during an outage): accepted, late=true
        let r = to_record(&batch(true), lab(7, 1000, Some(1100)), 61_100, 3000).unwrap();
        assert_eq!(r.ep, "#label");
        assert_eq!(r.seq, 7);
        assert_eq!(r.ts_server_ms, Some(1100));
        let v: serde_json::Value = serde_json::from_str(r.body.as_ref().unwrap()).unwrap();
        assert_eq!(v["late"], true);
        assert_eq!(v["delay_ms"], 60_000);
        assert_eq!(v["t_server_source"], "synced");
        assert_eq!(v["text"], "击毁");
    }

    #[test]
    fn unsynced_label_estimates() {
        let r = to_record(&batch(true), lab(1, 9000, None), 20_000, 3000).unwrap();
        assert_eq!(r.ts_server_ms, Some(9100));
        let r = to_record(&batch(false), lab(1, 9000, None), 20_000, 3000).unwrap();
        // sent at local 10000 -> label was 1000 ms old when sent
        assert_eq!(r.ts_server_ms, Some(19_000));
        let v: serde_json::Value = serde_json::from_str(r.body.as_ref().unwrap()).unwrap();
        assert_eq!(v["t_server_source"], "recv_estimate");
        assert_eq!(v["late"], false);
    }

    #[test]
    fn malformed_rejected() {
        assert!(to_record(&batch(true), serde_json::json!({"type":"x"}), 0, 3000).is_err());
        let mut l = lab(1, 1, None);
        l["session"] = "other".into();
        assert!(to_record(&batch(true), l, 0, 3000).is_err());
    }

    #[test]
    fn resolve_undo_and_tags() {
        let mut a = lab(1, 100, Some(100));
        a["t_server"] = 100.into();
        let mut b = lab(2, 200, Some(200));
        b["t_server"] = 200.into();
        let undo = serde_json::json!({"type":"label","uid":"s1#3","label_id":"_undo","action":"undo","ref_uid":"s1#1","t_local":300,"t_server":300});
        let tag = serde_json::json!({"type":"label","uid":"s1#4","label_id":"_tag","action":"tag","ref_uid":"s1#2","tags":["test"],"t_local":400,"t_server":400});
        let all = vec![a.clone(), b.clone(), b.clone(), undo, tag];
        let r = resolve(all.clone(), false, false, false);
        assert!(r.is_empty(), "{r:?}");
        let r = resolve(all.clone(), false, true, true);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0]["undone"], true);
        assert_eq!(r[1]["tags"][0], "test");
        assert_eq!(resolve(all, true, true, true).len(), 4);
    }
}
