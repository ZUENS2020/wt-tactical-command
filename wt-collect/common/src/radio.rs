//! /gamechat radio / quick-chat parsing + grid labels. Port of wt-analysis/allies/{radio,grid}.py (keep in sync).
//!
//! Confirmed (zh-CN client, RB ground 2026-10-10): pings 注意战术地图 / 注意该坐标位置 / 注意当前标识区域 carry the
//! *target* cell ([c4]); location calls 请求掩护 / 请求支援 / 需要支援 / 掩护我 / 请求指示空袭目标 carry the *sender's*
//! cell (10/10 equal to the blinking sender icon's cell). Own messages come back without the grid suffix.
//! Grid: columns = numbers, rows = letters, counted from map_info.grid_zero (top-left, world y north-up) in
//! grid_steps; NOT from map_min.
use serde::Serialize;
use serde_json::Value;

pub const OWN_NAMES: &[&str] = &[];

const PING_KINDS: &[(&str, &[&str])] = &[
    ("ping_map", &["注意战术地图", "attention to the map"]),
    ("ping_grid", &["注意该坐标位置", "attention to the designated grid", "attention to designated grid"]),
    ("ping_marked", &["注意当前标识区域", "attention to the marked area", "attention to the designated area"]),
];
const SENDER_KINDS: &[(&str, &[&str])] = &[
    ("air_target_request", &["请求指示空袭目标", "request air strike", "requesting air support"]),
    ("need_cover", &["请求掩护", "掩护我", "cover me"]),
    ("need_support", &["请求支援", "需要支援", "need backup", "need help", "help me"]),
    ("follow_me", &["跟我来", "跟着我", "follow me", "guide on me", "move after me", "on me"]),
];
const OTHER_KINDS: &[(&str, &[&str])] = &[
    ("thanks", &["谢谢", "感谢", "thank"]),
    ("reloading", &["装填"]),
    ("air_raid", &["空袭警报", "air raid", "enemy aircraft"]),
    ("attack_point", &["进攻"]),
    ("defend_point", &["防守"]),
];

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RadioMsg {
    pub id: i64,
    pub sender: String,
    pub own: bool,
    pub enemy: bool,
    pub mode: String,
    pub text: String,
    pub kind: &'static str,
    /// "target" (ping), "sender" (location call), "unknown" (grid in unrecognised text), None
    pub subject: Option<&'static str>,
    pub grid: Option<String>,
    pub alt_m: Option<f64>,
    pub grid_color: Option<String>,
}

impl RadioMsg {
    pub fn is_ping(&self) -> bool {
        self.kind.starts_with("ping_")
    }
    pub fn category(&self) -> &'static str {
        if self.is_ping() {
            "ping"
        } else if self.subject == Some("sender") {
            "location"
        } else {
            "radio"
        }
    }
}

/// Remove U+200B and <...> markup.
pub fn clean(msg: &str) -> String {
    let mut out = String::with_capacity(msg.len());
    let mut in_tag = false;
    for ch in msg.chars() {
        match ch {
            '\u{200b}' => {}
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// First "[E6]" / "[a14, 高度 1000 米]" / "[C5, alt. 600 m]" -> (label upper-case, altitude, byte range of the bracket)
pub fn find_grid(text: &str) -> Option<(String, Option<f64>, (usize, usize))> {
    let mut from = 0;
    while let Some(i) = text[from..].find('[').map(|i| i + from) {
        let Some(j) = text[i..].find(']').map(|j| j + i) else { return None };
        let inner = text[i + 1..j].trim();
        let mut head = inner.split(',');
        let lab = head.next().unwrap_or("").trim();
        let letters: String = lab.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        let digits = &lab[letters.len()..];
        if (1..=2).contains(&letters.len()) && (1..=2).contains(&digits.len()) && digits.chars().all(|c| c.is_ascii_digit()) {
            let alt = head.next().and_then(|rest| {
                let num: String = rest.chars().skip_while(|c| !(c.is_ascii_digit() || *c == '-' || *c == '+')).take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-' || *c == '+').collect();
                num.parse::<f64>().ok()
            });
            return Some((format!("{}{}", letters.to_ascii_uppercase(), digits), alt, (i, j + 1)));
        }
        from = j + 1;
    }
    None
}

pub fn classify_text(text: &str) -> (&'static str, Option<&'static str>) {
    let n = match find_grid(text) {
        Some((_, _, (a, b))) => format!("{}{}", &text[..a], &text[b..]),
        None => text.to_string(),
    }
    .to_lowercase();
    for (k, pats) in PING_KINDS {
        if pats.iter().any(|p| n.contains(p)) {
            return (k, Some("target"));
        }
    }
    for (k, pats) in SENDER_KINDS {
        if pats.iter().any(|p| n.contains(p)) {
            return (k, Some("sender"));
        }
    }
    for (k, pats) in OTHER_KINDS {
        if pats.iter().any(|p| n.contains(p)) {
            return (k, None);
        }
    }
    ("chat", None)
}

pub fn parse_chat(m: &Value, own_names: &[&str]) -> RadioMsg {
    let raw = m["msg"].as_str().unwrap_or("");
    let text = clean(raw);
    let (kind, mut subject) = classify_text(&text);
    let g = find_grid(&text);
    let raw_nz: String = raw.chars().filter(|c| *c != '\u{200b}').collect();
    let grid_color = raw_nz.find("<color=#").map(|i| raw_nz[i + 8..].chars().take_while(|c| c.is_ascii_hexdigit()).collect::<String>().to_ascii_uppercase());
    let sender: String = m["sender"].as_str().unwrap_or("").chars().filter(|c| *c != '\u{200b}').collect();
    let own = own_names.iter().any(|n| !n.is_empty() && sender.contains(n));
    if g.is_some() && subject.is_none() && kind == "chat" {
        subject = Some("unknown");
    }
    RadioMsg {
        id: m["id"].as_i64().unwrap_or(-1),
        own,
        enemy: m["enemy"].as_bool().unwrap_or(false),
        mode: m["mode"].as_str().unwrap_or("").chars().filter(|c| *c != '\u{200b}').collect(),
        text,
        kind,
        subject,
        grid: g.as_ref().map(|g| g.0.clone()),
        alt_m: g.and_then(|g| g.1),
        grid_color: grid_color.filter(|s| !s.is_empty()),
        sender,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct GridInfo {
    pub map_min: [f64; 2],
    pub map_max: [f64; 2],
    pub steps: [f64; 2],
    pub zero: [f64; 2],
}

impl GridInfo {
    pub fn from_map_info(j: &Value) -> Option<GridInfo> {
        if j["valid"].as_bool() != Some(true) {
            return None;
        }
        let p = |k: &str| -> Option<[f64; 2]> { Some([j[k][0].as_f64()?, j[k][1].as_f64()?]) };
        let (mn, mx) = (p("map_min")?, p("map_max")?);
        let st = p("grid_steps")?;
        let st = [st[0].abs(), st[1].abs()];
        if mx[0] <= mn[0] || mx[1] <= mn[1] || st[0] <= 0.0 || st[1] <= 0.0 {
            return None;
        }
        let zero = p("grid_zero").unwrap_or([mn[0], mx[1]]);
        Some(GridInfo { map_min: mn, map_max: mx, steps: st, zero })
    }
    fn span(&self) -> [f64; 2] {
        [self.map_max[0] - self.map_min[0], self.map_max[1] - self.map_min[1]]
    }
    /// "E6" -> normalized [x0, y0, x1, y1] (may extend past 0..1)
    pub fn cell_bounds(&self, label: &str) -> Option<[f64; 4]> {
        let l = label.trim();
        let letters: String = l.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        let digits = &l[letters.len()..];
        if !(1..=2).contains(&letters.len()) || digits.is_empty() || digits.len() > 2 {
            return None;
        }
        let col: i64 = digits.parse().ok()?;
        if col < 1 {
            return None;
        }
        let row = letters.to_ascii_uppercase().bytes().fold(0i64, |n, b| n * 26 + (b - b'A' + 1) as i64) - 1;
        let sp = self.span();
        let wx0 = self.zero[0] + (col - 1) as f64 * self.steps[0];
        let wy_top = self.zero[1] - row as f64 * self.steps[1];
        let x0 = (wx0 - self.map_min[0]) / sp[0];
        let y0 = (self.map_max[1] - wy_top) / sp[1];
        Some([x0, y0, x0 + self.steps[0] / sp[0], y0 + self.steps[1] / sp[1]])
    }
    pub fn cell_of(&self, x: f64, y: f64) -> Option<String> {
        let sp = self.span();
        let wx = self.map_min[0] + x * sp[0];
        let wy = self.map_max[1] - y * sp[1];
        let col = ((wx - self.zero[0]) / self.steps[0]).floor() as i64 + 1;
        let row = ((self.zero[1] - wy) / self.steps[1]).floor() as i64;
        if row < 0 || col < 1 {
            return None;
        }
        let (mut s, mut i) = (String::new(), row + 1);
        while i > 0 {
            let rem = (i - 1) % 26;
            s.insert(0, (b'A' + rem as u8) as char);
            i = (i - 1) / 26;
        }
        Some(format!("{s}{col}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn real_chat_lines() {
        let m = parse_chat(&json!({"id": 109, "msg": "注\u{200b}意\u{200b}战\u{200b}术\u{200b}地\u{200b}图！<color=#FF00FF50> [c4]</color>", "sender": "RAB_Witesprite", "enemy": false, "mode": "友\u{200b}军"}), OWN_NAMES);
        assert_eq!((m.kind, m.subject, m.grid.as_deref(), m.grid_color.as_deref(), m.mode.as_str(), m.category()), ("ping_map", Some("target"), Some("C4"), Some("FF00FF50"), "友军", "ping"));
        let m = parse_chat(&json!({"id": 1, "msg": "请\u{200b}求\u{200b}掩\u{200b}护！<color=#FF00FF50> [e6]</color>", "sender": "c"}), OWN_NAMES);
        assert_eq!((m.kind, m.category(), m.grid.as_deref()), ("need_cover", "location", Some("E6")));
        let m = parse_chat(&json!({"id": 1, "msg": "请\u{200b}求\u{200b}指\u{200b}示\u{200b}空\u{200b}袭\u{200b}目\u{200b}标<color=#FF96966E> [a14, 高\u{200b}度 1000 米]</color>", "sender": "c"}), OWN_NAMES);
        assert_eq!((m.kind, m.grid.as_deref(), m.alt_m), ("air_target_request", Some("A14"), Some(1000.0)));
        let m = parse_chat(&json!({"id": 127, "msg": "注\u{200b}意\u{200b}战\u{200b}术\u{200b}地\u{200b}图！", "sender": "tester"}), &["tester"]);
        assert!(m.own && m.is_ping() && m.grid.is_none());
        assert_eq!(parse_chat(&json!({"id": 2, "msg": "注\u{200b}意\u{200b}当\u{200b}前\u{200b}标\u{200b}识\u{200b}区\u{200b}域！<color=#FF50BE46> [b3]</color>"}), OWN_NAMES).kind, "ping_marked");
        assert_eq!(parse_chat(&json!({"id": 2, "msg": "Guide on me [C5, alt. 600 m]"}), OWN_NAMES).alt_m, Some(600.0));
        assert_eq!(parse_chat(&json!({"id": 2, "msg": "装\u{200b}填\u{200b}中！ 00:32"}), OWN_NAMES).kind, "reloading");
        assert_eq!(parse_chat(&json!({"id": 2, "msg": "size does matter"}), OWN_NAMES).kind, "chat");
    }
    #[test]
    fn grid_from_zero() {
        let gi = GridInfo::from_map_info(&json!({"valid": true, "grid_steps": [225.0, 225.0], "grid_zero": [-884.35, -1107.0], "map_max": [2048.0, 0.0], "map_min": [-2048.0, -4096.0]})).unwrap();
        let b = gi.cell_bounds("E6").unwrap();
        assert_eq!(gi.cell_of((b[0] + b[2]) / 2.0, (b[1] + b[3]) / 2.0).as_deref(), Some("E6"));
        assert!((b[2] - b[0] - 225.0 / 4096.0).abs() < 1e-12);
        assert!(gi.cell_bounds("?").is_none());
    }
}
