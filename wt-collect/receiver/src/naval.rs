//! Naval (ship/boat) filter applied in the ingestion path before anything is written to disk.
use serde_json::value::RawValue;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};
use wtc_common::Record;

pub const DEFAULT_NAVAL_ICONS: &str = "ship,boat,torpedoboat,gunboat,destroyer,frigate,lightcruiser,heavycruiser,cruiser,battlecruiser,battleship,submarine,aircraftcarrier,carrier,barge,navalferrybarge,minelayer,minesweeper,corvette,patrolboat";
pub const DEFAULT_NAVAL_TYPE_SUBSTR: &str = "ship,boat,naval";
/// Icons known to be non-naval (spec §4.5 + observed); anything else is logged as unknown.
pub const DEFAULT_KNOWN_ICONS: &str = "player,fighter,bomber,assault,lighttank,mediumtank,heavytank,tankdestroyer,tracked,wheeled,spaa,sam,airdefence,structure,waypoint,capture_zone,bombing_point,defending_point,respawn_base_tank,respawn_base_fighter,respawn_base_bomber,none,a,b,c,d,e,helicopter,ucav,attackhelicopter";
pub const DEFAULT_KNOWN_TYPES: &str = "ground_model,aircraft,airfield,capture_zone,defending_point,bombing_point,respawn_base_tank,respawn_base_fighter,respawn_base_bomber,point_of_interest";

fn list(s: &str) -> Vec<String> {
    s.split(',').map(|x| x.trim().to_ascii_lowercase()).filter(|x| !x.is_empty()).collect()
}

#[derive(Clone, Debug)]
pub struct NavalCfg {
    pub icons: HashSet<String>,
    pub type_substr: Vec<String>,
    pub known_icons: HashSet<String>,
    pub known_types: HashSet<String>,
}

impl NavalCfg {
    pub fn new(icons: &str, type_substr: &str, known_icons: &str, known_types: &str) -> Self {
        NavalCfg {
            icons: list(icons).into_iter().collect(),
            type_substr: list(type_substr),
            known_icons: list(known_icons).into_iter().collect(),
            known_types: list(known_types).into_iter().collect(),
        }
    }
    #[allow(dead_code)]
    pub fn default_cfg() -> Self {
        Self::new(DEFAULT_NAVAL_ICONS, DEFAULT_NAVAL_TYPE_SUBSTR, DEFAULT_KNOWN_ICONS, DEFAULT_KNOWN_TYPES)
    }
    /// From env: WT_NAVAL_ICONS / WT_NAVAL_TYPE_SUBSTR replace the defaults; WT_NAVAL_ICONS_EXTRA and
    /// WT_KNOWN_ICONS_EXTRA extend them.
    pub fn from_env() -> Self {
        let g = |k: &str, d: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty()).unwrap_or_else(|| d.to_string());
        let mut c = Self::new(
            &g("WT_NAVAL_ICONS", DEFAULT_NAVAL_ICONS),
            &g("WT_NAVAL_TYPE_SUBSTR", DEFAULT_NAVAL_TYPE_SUBSTR),
            DEFAULT_KNOWN_ICONS,
            DEFAULT_KNOWN_TYPES,
        );
        c.icons.extend(list(&g("WT_NAVAL_ICONS_EXTRA", "")));
        c.known_icons.extend(list(&g("WT_KNOWN_ICONS_EXTRA", "")));
        c
    }
    pub fn is_naval_obj(&self, o: &Value) -> bool {
        // map markers / pings and friendly signal states are never naval
        if o["type"].as_str() == Some("point_of_interest") {
            return false;
        }
        let icon = o["icon"].as_str().unwrap_or("").to_ascii_lowercase();
        if self.icons.contains(&icon) {
            return true; // exact match only: "TankDestroyer" != "destroyer"
        }
        let ty = o["type"].as_str().unwrap_or("").to_ascii_lowercase();
        self.type_substr.iter().any(|s| ty.contains(s.as_str()))
    }
    /// Own vehicle naval according to /indicators.
    pub fn indicators_naval(&self, v: &Value) -> bool {
        let army = v["army"].as_str().unwrap_or("").to_ascii_lowercase();
        if ["ship", "boat", "naval"].contains(&army.as_str()) {
            return true;
        }
        let ty = v["type"].as_str().unwrap_or("").to_ascii_lowercase();
        ty.starts_with("shipmodels") || ty.starts_with("boatmodels") || ty.contains("ship") || ty.contains("boat")
    }
}

#[derive(Default, Debug)]
pub struct NavalState {
    ind_naval: bool,
    obj_naval: bool,
    last_mapobj_hash: Option<u64>,
    prev_filtered: bool,
    /// indicators seq -> own vehicle naval; lets /state records arriving out of order (backlog
    /// uploads interleaved with live data) be judged by the indicators sample closest before them.
    ind_by_seq: BTreeMap<u64, bool>,
    max_ind_seq: u64,
}
impl NavalState {
    fn naval_at(&self, seq: u64) -> bool {
        self.ind_by_seq.range(..seq).next_back().map(|(_, v)| *v).unwrap_or(self.ind_naval) || self.obj_naval
    }
}

#[derive(Default, Debug, serde::Serialize, serde::Deserialize)]
pub struct IconStats {
    #[serde(default)]
    pub unknown_icons: BTreeMap<String, u64>,
    #[serde(default)]
    pub unknown_types: BTreeMap<String, u64>,
    #[serde(default)]
    pub naval_dropped_icons: BTreeMap<String, u64>,
    #[serde(default)]
    pub naval_objects_dropped: u64,
    #[serde(default)]
    pub naval_ticks_dropped: u64,
    #[serde(default)]
    pub updated: String,
    #[serde(skip)]
    pub dirty: bool,
}

fn h64(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

pub struct MapObjResult {
    pub body: Option<String>, // Some = rewritten
    pub dropped: u32,
    pub player_naval: Option<bool>,
}

/// Filter a map_obj body. Kept objects are copied byte-for-byte (RawValue), only the array is rebuilt.
pub fn filter_map_obj(cfg: &NavalCfg, body: &str, stats: &mut IconStats) -> Option<MapObjResult> {
    let items: Vec<&RawValue> = serde_json::from_str(body).ok()?;
    let mut kept: Vec<&str> = Vec::with_capacity(items.len());
    let mut dropped = 0u32;
    let mut player_naval = None;
    for it in &items {
        let v: Value = serde_json::from_str(it.get()).unwrap_or(Value::Null);
        let icon = v["icon"].as_str().unwrap_or("");
        let ty = v["type"].as_str().unwrap_or("");
        let naval = cfg.is_naval_obj(&v);
        if icon == "Player" || icon.eq_ignore_ascii_case("player") {
            player_naval = Some(cfg.type_substr.iter().any(|s| ty.to_ascii_lowercase().contains(s.as_str())));
        }
        if naval {
            dropped += 1;
            *stats.naval_dropped_icons.entry(format!("{icon}|{ty}")).or_default() += 1;
            stats.dirty = true;
            continue;
        }
        let il = icon.to_ascii_lowercase();
        if !icon.is_empty() && !cfg.known_icons.contains(&il) {
            *stats.unknown_icons.entry(icon.to_string()).or_default() += 1;
            stats.dirty = true;
        }
        if !ty.is_empty() && !cfg.known_types.contains(&ty.to_ascii_lowercase()) {
            *stats.unknown_types.entry(ty.to_string()).or_default() += 1;
            stats.dirty = true;
        }
        kept.push(it.get());
    }
    stats.naval_objects_dropped += dropped as u64;
    let body = if dropped > 0 { Some(format!("[{}]", kept.join(", "))) } else { None };
    Some(MapObjResult { body, dropped, player_naval })
}

fn placeholder(r: &mut Record) {
    r.body = None;
    r.body_b64 = None;
    r.bytes = 0;
    r.ctype = None;
    r.img_hash = None;
    r.changed = false;
    r.dropped = Some("naval".into());
}

/// Apply the naval filter to one record (records of a session must be fed in seq order).
pub fn apply(cfg: &NavalCfg, st: &mut NavalState, stats: &mut IconStats, r: &mut Record) {
    if r.err.is_some() || r.dropped.is_some() {
        return;
    }
    match r.ep.as_str() {
        "/indicators" => {
            let mut own = None;
            if let Some(v) = r.body.as_deref().and_then(|b| serde_json::from_str::<Value>(b).ok()) {
                let n = cfg.indicators_naval(&v);
                own = Some(n);
                st.ind_by_seq.insert(r.seq, n);
                if st.ind_by_seq.len() > 200_000 {
                    let cut = *st.ind_by_seq.keys().nth(100_000).unwrap();
                    st.ind_by_seq = st.ind_by_seq.split_off(&cut);
                }
                if r.seq >= st.max_ind_seq {
                    st.max_ind_seq = r.seq;
                    st.ind_naval = n;
                }
            }
            if own.unwrap_or_else(|| st.naval_at(r.seq)) || st.obj_naval {
                placeholder(r);
                stats.naval_ticks_dropped += 1;
                stats.dirty = true;
            }
        }
        "/state" => {
            if st.naval_at(r.seq) {
                placeholder(r);
                stats.naval_ticks_dropped += 1;
                stats.dirty = true;
            }
        }
        "/map_obj.json" => {
            let Some(b) = r.body.as_deref() else { return };
            if let Some(res) = filter_map_obj(cfg, b, stats) {
                if let Some(p) = res.player_naval {
                    st.obj_naval = p;
                }
                let filtered = res.dropped > 0;
                if let Some(nb) = res.body {
                    r.bytes = nb.len() as u64;
                    r.body = Some(nb);
                    r.filtered_naval = Some(res.dropped);
                }
                let h = h64(r.body.as_deref().unwrap_or(""));
                if filtered || st.prev_filtered {
                    if let Some(prev) = st.last_mapobj_hash {
                        r.changed = h != prev;
                    }
                }
                st.last_mapobj_hash = Some(h);
                st.prev_filtered = filtered;
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn obj(icon: &str, ty: &str) -> String {
        format!(r##"{{"type": "{ty}", "color": "#fa0C00", "blink": 0, "icon": "{icon}", "icon_bg": "none", "x": 0.5, "y": 0.25}}"##)
    }
    fn arr(objs: &[String]) -> String {
        format!("[{}]", objs.join(", "))
    }
    fn rec(ep: &str, body: &str) -> Record {
        Record { session: "s".into(), ep: ep.into(), body: Some(body.into()), bytes: body.len() as u64, changed: true, status: 200, ..Default::default() }
    }

    #[test]
    fn tankdestroyer_kept_destroyer_dropped() {
        let c = NavalCfg::default_cfg();
        let mut st = IconStats::default();
        let b = arr(&[obj("TankDestroyer", "ground_model"), obj("Destroyer", "ground_model"), obj("MediumTank", "ground_model")]);
        let r = filter_map_obj(&c, &b, &mut st).unwrap();
        assert_eq!(r.dropped, 1);
        let nb = r.body.unwrap();
        assert!(nb.contains("TankDestroyer"));
        assert!(!nb.contains("\"Destroyer\""));
        assert!(nb.contains("MediumTank"));
        assert!(!st.unknown_icons.contains_key("TankDestroyer"));
    }

    #[test]
    fn ship_boat_and_case_insensitive() {
        let c = NavalCfg::default_cfg();
        let mut st = IconStats::default();
        for icon in ["Ship", "SHIP", "boat", "TorpedoBoat", "lightCruiser", "AircraftCarrier", "Submarine", "patrolboat"] {
            let r = filter_map_obj(&c, &arr(&[obj(icon, "ground_model")]), &mut st).unwrap();
            assert_eq!(r.dropped, 1, "{icon} should be dropped");
            assert_eq!(r.body.unwrap(), "[]");
        }
        // naval by type substring, unknown icon
        for ty in ["ship", "Naval_Unit", "torpedo_BOAT", "shipModel"] {
            let r = filter_map_obj(&c, &arr(&[obj("Unknown", ty)]), &mut st).unwrap();
            assert_eq!(r.dropped, 1, "type {ty} should be dropped");
        }
        // substrings of icons are NOT naval
        for icon in ["TankDestroyer", "Shipyard", "Boatswain"] {
            let r = filter_map_obj(&c, &arr(&[obj(icon, "ground_model")]), &mut st).unwrap();
            assert_eq!(r.dropped, 0, "{icon} must be kept");
        }
    }

    #[test]
    fn kept_objects_byte_identical_and_unchanged_body_untouched() {
        let c = NavalCfg::default_cfg();
        let mut st = IconStats::default();
        let raw = r##"[{"type":"aircraft","x":0.421074,"y":0.56343,"dx":0.939689,"icon":"Fighter","name":"Игрок 玩家"} , {"type":"ship","icon":"Ship","x":0.1,"y":0.2}]"##;
        let r = filter_map_obj(&c, raw, &mut st).unwrap();
        assert_eq!(r.body.unwrap(), r##"[{"type":"aircraft","x":0.421074,"y":0.56343,"dx":0.939689,"icon":"Fighter","name":"Игрок 玩家"}]"##);
        let raw2 = arr(&[obj("MediumTank", "ground_model")]);
        let r2 = filter_map_obj(&c, &raw2, &mut st).unwrap();
        assert!(r2.body.is_none(), "no naval => body kept as-is");
    }

    #[test]
    fn unknown_icons_counted() {
        let c = NavalCfg::default_cfg();
        let mut st = IconStats::default();
        filter_map_obj(&c, &arr(&[obj("WeirdIcon", "ground_model"), obj("WeirdIcon", "new_type")]), &mut st).unwrap();
        assert_eq!(st.unknown_icons["WeirdIcon"], 2);
        assert_eq!(st.unknown_types["new_type"], 1);
    }

    #[test]
    fn own_naval_vehicle_drops_indicators_and_state() {
        let c = NavalCfg::default_cfg();
        let (mut ns, mut stats) = (NavalState::default(), IconStats::default());
        let mut i1 = rec("/indicators", r#"{"valid": true, "army": "tank", "type": "tankModels/ussr_t_80u"}"#);
        apply(&c, &mut ns, &mut stats, &mut i1);
        assert!(i1.dropped.is_none() && i1.body.is_some());
        let mut i2 = rec("/indicators", r#"{"valid": true, "army": "ship", "type": "shipModels/uk_destroyer"}"#);
        apply(&c, &mut ns, &mut stats, &mut i2);
        assert_eq!(i2.dropped.as_deref(), Some("naval"));
        assert!(i2.body.is_none() && i2.bytes == 0);
        let mut s1 = rec("/state", r#"{"valid": true, "H, m": 3}"#);
        apply(&c, &mut ns, &mut stats, &mut s1);
        assert_eq!(s1.dropped.as_deref(), Some("naval"));
        let mut mi = rec("/map_info.json", r#"{"valid": true}"#);
        apply(&c, &mut ns, &mut stats, &mut mi);
        assert!(mi.dropped.is_none() && mi.body.is_some(), "map_info kept");
        // type-only detection
        let mut i3 = rec("/indicators", r#"{"valid": true, "army": "", "type": "boatModels/pr_123"}"#);
        apply(&c, &mut ns, &mut stats, &mut i3);
        assert_eq!(i3.dropped.as_deref(), Some("naval"));
        // back to tank
        let mut i4 = rec("/indicators", r#"{"valid": true, "army": "tank", "type": "tankModels/germ_jagdpanther"}"#);
        apply(&c, &mut ns, &mut stats, &mut i4);
        assert!(i4.dropped.is_none());
        let mut s2 = rec("/state", r#"{"valid": false}"#);
        apply(&c, &mut ns, &mut stats, &mut s2);
        assert!(s2.dropped.is_none());
        assert_eq!(stats.naval_ticks_dropped, 3);
    }

    #[test]
    fn out_of_order_state_judged_by_nearest_indicators() {
        let c = NavalCfg::default_cfg();
        let (mut ns, mut stats) = (NavalState::default(), IconStats::default());
        let mut live = rec("/indicators", r#"{"valid": true, "army": "tank", "type": "tankModels/x"}"#);
        live.seq = 1000;
        apply(&c, &mut ns, &mut stats, &mut live);
        // backlog from earlier (naval) arrives later
        let mut i = rec("/indicators", r#"{"valid": true, "army": "ship", "type": "shipModels/y"}"#);
        i.seq = 10;
        apply(&c, &mut ns, &mut stats, &mut i);
        let mut s = rec("/state", r#"{"valid": true}"#);
        s.seq = 11;
        apply(&c, &mut ns, &mut stats, &mut s);
        assert_eq!(s.dropped.as_deref(), Some("naval"));
        let mut s2 = rec("/state", r#"{"valid": true}"#);
        s2.seq = 1001;
        apply(&c, &mut ns, &mut stats, &mut s2);
        assert!(s2.dropped.is_none(), "live tank state kept");
    }

    #[test]
    fn changed_recomputed_after_filtering() {
        let c = NavalCfg::default_cfg();
        let (mut ns, mut stats) = (NavalState::default(), IconStats::default());
        // two bodies differing only in the ship's position => identical after filtering
        let a = arr(&[obj("MediumTank", "ground_model"), r#"{"type": "ship", "icon": "Ship", "x": 0.1, "y": 0.1}"#.into()]);
        let b = arr(&[obj("MediumTank", "ground_model"), r#"{"type": "ship", "icon": "Ship", "x": 0.2, "y": 0.1}"#.into()]);
        let mut r1 = rec("/map_obj.json", &a);
        apply(&c, &mut ns, &mut stats, &mut r1);
        let mut r2 = rec("/map_obj.json", &b);
        apply(&c, &mut ns, &mut stats, &mut r2);
        assert_eq!(r1.filtered_naval, Some(1));
        assert_eq!(r2.filtered_naval, Some(1));
        assert!(!r2.changed, "only naval content changed => not a new sample");
        assert_eq!(r2.bytes, r2.body.as_ref().unwrap().len() as u64);
        // player in a boat (map_obj) => indicators dropped even if army field missing
        let mut m = rec("/map_obj.json", &arr(&[obj("Player", "ship")]));
        apply(&c, &mut ns, &mut stats, &mut m);
        let mut ind = rec("/indicators", r#"{"valid": true}"#);
        apply(&c, &mut ns, &mut stats, &mut ind);
        assert_eq!(ind.dropped.as_deref(), Some("naval"));
    }
}
