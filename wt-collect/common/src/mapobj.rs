//! map_obj.json classification: side/role and kind. Port of wt-analysis/allies/classify.py (keep in sync).
//!
//! Confirmed on real RB ground data (2026-10-10, sessions 1709/df9b; ALLY_MARKERS_20261010.md):
//! `#faC81E` Player = self; `#174DFF` = teammate; `#67D756` (ground) / `#39D921` (air) = squadmate;
//! `#043FFF` blink 2 = the unit's owner just sent a radio message / map ping (1.9–5.9 s; hides squad/self colour);
//! `#1839A7` (ground) / `#134AFF` (air) = teammate that just spawned; Player icon in `#174DFF` = spectator camera
//! on a teammate after own death (NOT self). Enemy reds unchanged. No separate ping object was observed;
//! `point_of_interest` and unknown types are classified `MapMarker` (never a unit, never filtered).
use serde::Serialize;
use serde_json::Value;

pub const SELF_RGB: [u8; 3] = [250, 200, 30];
pub const SIGNAL_RGB: [u8; 3] = [4, 63, 255];
pub const SQUAD_RGB: [[u8; 3]; 2] = [[103, 215, 86], [57, 217, 33]];
pub const FRESH_SPAWN_RGB: [[u8; 3]; 2] = [[24, 57, 167], [19, 74, 255]];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Unit,
    CaptureZone,
    RespawnBase,
    Airfield,
    BombingPoint,
    DefendingPoint,
    /// point_of_interest or any unknown type: exact map marker candidate (ping), never a unit
    MapMarker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    #[serde(rename = "self")]
    Own,
    Squad,
    Ally,
    Enemy,
    Neutral,
    Unknown,
}

impl Side {
    pub fn as_str(&self) -> &'static str {
        match self {
            Side::Own => "self",
            Side::Squad => "squad",
            Side::Ally => "ally",
            Side::Enemy => "enemy",
            Side::Neutral => "neutral",
            Side::Unknown => "unknown",
        }
    }
    pub fn friendly(&self) -> bool {
        matches!(self, Side::Own | Side::Squad | Side::Ally)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ObjClass {
    pub kind: Kind,
    pub side: Side,
    /// #043FFF blink 2 on a unit: owner sent a radio message / ping
    pub signal: bool,
    pub fresh_spawn: bool,
    /// Player icon that is not self (spectator camera)
    pub spectated: bool,
    pub known_type: bool,
}

impl ObjClass {
    pub fn is_unit(&self) -> bool {
        self.kind == Kind::Unit
    }
}

pub fn kind_of_type(ty: &str) -> Option<Kind> {
    Some(match ty {
        "ground_model" | "aircraft" => Kind::Unit,
        "capture_zone" => Kind::CaptureZone,
        "airfield" => Kind::Airfield,
        "bombing_point" => Kind::BombingPoint,
        "defending_point" => Kind::DefendingPoint,
        t if t.starts_with("respawn_base") => Kind::RespawnBase,
        "point_of_interest" => Kind::MapMarker,
        _ => return None,
    })
}

pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.trim().trim_start_matches('#');
    if h.len() < 6 {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    Some([p(0)?, p(2)?, p(4)?])
}

pub fn rgb_of(o: &Value) -> [u8; 3] {
    if let Some(a) = o["color[]"].as_array() {
        if a.len() >= 3 {
            let c = |i: usize| a[i].as_u64().unwrap_or(0).min(255) as u8;
            return [c(0), c(1), c(2)];
        }
    }
    o["color"].as_str().and_then(parse_hex).unwrap_or([0, 0, 0])
}

pub fn side_of_rgb(c: [u8; 3]) -> Side {
    let (r, g, b) = (c[0] as i32, c[1] as i32, c[2] as i32);
    if c == SELF_RGB {
        Side::Own
    } else if SQUAD_RGB.contains(&c) || (g >= 180 && r < 150 && b < 150) {
        Side::Squad
    } else if r >= 200 && g < 130 && b < 130 {
        Side::Enemy
    } else if b >= 150 && b > r + 60 && b > g + 40 {
        Side::Ally
    } else if r >= 200 && g >= 200 && b >= 200 {
        Side::Neutral
    } else {
        Side::Unknown
    }
}

pub fn classify(o: &Value) -> ObjClass {
    let ty = o["type"].as_str().unwrap_or("");
    let icon = o["icon"].as_str().unwrap_or("");
    let blink = o["blink"].as_i64().unwrap_or(0);
    let rgb = rgb_of(o);
    let known = kind_of_type(ty);
    let kind = known.unwrap_or(Kind::MapMarker);
    let unit = kind == Kind::Unit;
    let signal = unit && rgb == SIGNAL_RGB && blink == 2;
    let fresh = unit && FRESH_SPAWN_RGB.contains(&rgb);
    let mk = |side, signal, fresh_spawn, spectated| ObjClass { kind, side, signal, fresh_spawn, spectated, known_type: known.is_some() };
    if unit && icon == "Player" {
        if rgb == SELF_RGB || signal {
            return mk(Side::Own, signal, false, false);
        }
        let s = match side_of_rgb(rgb) {
            Side::Own => Side::Ally,
            s => s,
        };
        return mk(s, false, false, true);
    }
    if signal || fresh {
        return mk(Side::Ally, signal, fresh, false);
    }
    let side = match (side_of_rgb(rgb), unit) {
        (Side::Own, _) => Side::Unknown,
        (Side::Squad, false) => Side::Ally,
        (s, _) => s,
    };
    mk(side, false, false, false)
}

/// Own Player object (None while spectating).
pub fn find_self(a: &[Value]) -> Option<&Value> {
    a.iter().find(|o| o["icon"].as_str() == Some("Player") && o["x"].is_number() && classify(o).side == Side::Own)
}

/// Enemy *unit* (not zones/bases/markers).
pub fn is_enemy_unit(o: &Value) -> bool {
    let c = classify(o);
    c.is_unit() && c.side == Side::Enemy && o["icon"].as_str() != Some("Player")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn o(ty: &str, icon: &str, color: &str, blink: i64) -> Value {
        json!({"type": ty, "icon": icon, "color": color, "blink": blink, "x": 0.5, "y": 0.5, "icon_bg": "none"})
    }
    #[test]
    fn real_colours() {
        assert_eq!(classify(&o("ground_model", "Player", "#faC81E", 0)).side, Side::Own);
        assert_eq!(classify(&o("ground_model", "HeavyTank", "#67D756", 0)).side, Side::Squad);
        assert_eq!(classify(&o("aircraft", "Fighter", "#39D921", 0)).side, Side::Squad);
        assert_eq!(classify(&o("ground_model", "HeavyTank", "#174DFF", 0)).side, Side::Ally);
        let s = classify(&o("ground_model", "HeavyTank", "#043FFF", 2));
        assert!(s.signal && s.side == Side::Ally && s.is_unit());
        let p = classify(&o("aircraft", "Player", "#043FFF", 2));
        assert!(p.signal && p.side == Side::Own);
        let f = classify(&o("ground_model", "MediumTank", "#1839A7", 0));
        assert!(f.fresh_spawn && f.side == Side::Ally);
        assert!(classify(&o("aircraft", "Fighter", "#134AFF", 0)).fresh_spawn);
        let sp = classify(&o("ground_model", "Player", "#174DFF", 0));
        assert!(sp.spectated && sp.side == Side::Ally);
        assert_eq!(classify(&o("ground_model", "MediumTank", "#fa0C00", 0)).side, Side::Enemy);
        let e2 = classify(&o("ground_model", "MediumTank", "#f00C00", 2));
        assert!(e2.side == Side::Enemy && !e2.signal);
    }
    #[test]
    fn markers_and_objectives_never_units() {
        for ty in ["point_of_interest", "new_ping_thing", "capture_zone", "respawn_base_tank", "bombing_point", "airfield"] {
            let c = classify(&o(ty, ty, "#043FFF", 2));
            assert!(!c.is_unit() && !c.signal, "{ty}");
        }
        assert_eq!(classify(&o("point_of_interest", "none", "#174DFF", 0)).kind, Kind::MapMarker);
        assert!(!classify(&o("new_ping_thing", "x", "#174DFF", 0)).known_type);
        assert!(!is_enemy_unit(&o("respawn_base_tank", "respawn_base_tank", "#fa0C00", 0)));
        assert!(is_enemy_unit(&o("ground_model", "LightTank", "#fa0C00", 0)));
    }
    #[test]
    fn spectated_player_is_not_self() {
        let a = vec![o("ground_model", "Player", "#174DFF", 0)];
        assert!(find_self(&a).is_none());
        let b = vec![o("ground_model", "Player", "#faC81E", 0)];
        assert!(find_self(&b).is_some());
    }
}
