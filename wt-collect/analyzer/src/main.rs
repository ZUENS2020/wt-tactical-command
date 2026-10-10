//! wt-analyze <session>: answers the 8111 spec §10 open questions from a raw session.
use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use wtc_common::read_session;

const DATA_ROOT: &str = "/workspace/wt-collect/data";

fn find_session(arg: &str) -> Result<PathBuf> {
    let p = PathBuf::from(arg);
    if p.is_dir() {
        return Ok(p);
    }
    let mut hits = Vec::new();
    if let Ok(rd) = std::fs::read_dir(DATA_ROOT) {
        for d in rd.flatten() {
            if let Ok(rd2) = std::fs::read_dir(d.path()) {
                for s in rd2.flatten() {
                    let n = s.file_name().to_string_lossy().to_string();
                    if n == arg || n.contains(arg) || (arg == "latest") {
                        hits.push(s.path());
                    }
                }
            }
        }
    }
    hits.sort_by_key(|p| p.file_name().map(|n| n.to_os_string()));
    if arg == "latest" {
        return hits.pop().context("no sessions");
    }
    match hits.len() {
        0 => bail!("session '{arg}' not found (give a dir or id under {DATA_ROOT}/<date>/)"),
        1 => Ok(hits.remove(0)),
        _ => bail!("ambiguous: {:?}", hits),
    }
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}
fn vs(v: &Value) -> Option<String> {
    match v {
        Value::Null => None,
        Value::String(s) => Some(s.clone()),
        o => Some(o.to_string()),
    }
}
fn pair(v: &Value) -> String {
    match v.as_array() {
        Some(a) if a.len() == 2 => format!("[{},{}]", fmtn(num(&a[0])), fmtn(num(&a[1]))),
        _ => if v.is_null() { "-".into() } else { v.to_string() },
    }
}
fn fmtn(x: Option<f64>) -> String {
    match x {
        Some(x) if x.fract() == 0.0 => format!("{x:.0}"),
        Some(x) => format!("{x:.2}"),
        None => "?".into(),
    }
}
fn pct(a: f64, b: f64) -> f64 {
    if b > 0.0 { 100.0 * a / b } else { 0.0 }
}
fn perc(v: &mut Vec<f64>, q: f64) -> f64 {
    if v.is_empty() {
        return f64::NAN;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let i = ((v.len() - 1) as f64 * q).round() as usize;
    v[i]
}
fn lt(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local.timestamp_millis_opt(ms).single().map(|t| t.format("%H:%M:%S%.3f").to_string()).unwrap_or_default()
}
fn csv_esc(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() }
}
fn write_csv(dir: &Path, name: &str, header: &[&str], rows: &[Vec<String>]) -> Result<()> {
    let mut s = header.join(",");
    s.push('\n');
    for r in rows {
        s.push_str(&r.iter().map(|c| csv_esc(c)).collect::<Vec<_>>().join(","));
        s.push('\n');
    }
    std::fs::write(dir.join(name), s)?;
    Ok(())
}
fn color_class(c: &str) -> &'static str {
    let h = c.trim_start_matches('#');
    if h.len() != 6 {
        return "unknown";
    }
    let p = |i: usize| u8::from_str_radix(&h[i..i + 2], 16).unwrap_or(0) as u32;
    let (r, g, b) = (p(0), p(2), p(4));
    if r >= 200 && g < 130 && b < 130 {
        "enemy(red)"
    } else if g > 150 && r < 150 && b < 150 {
        "squad(green)"
    } else if b > 150 && r < 100 {
        "friend(blue/cyan)"
    } else if r > 200 && g > 150 && b < 100 {
        "yellow/orange"
    } else if r > 200 && g > 200 && b > 200 {
        "white"
    } else {
        "other"
    }
}

#[derive(Clone, Default, Debug)]
struct Ctx {
    up: Option<bool>,
    map_valid: Option<bool>,
    gen: Option<String>,
    hud: Option<String>,
    range: Option<String>,
    ptype: Option<String>, // "none" when absent
    army: Option<String>,
    vehicle: Option<String>,
    img: Option<String>,
    mission: Option<String>,
    state_valid: Option<bool>,
    seg: usize,
}
impl Ctx {
    fn label(&self) -> String {
        if self.up == Some(false) {
            "8111_down".into()
        } else if self.map_valid != Some(true) {
            "no_match".into()
        } else {
            match self.ptype.as_deref() {
                Some("none") | None => "no_player".into(),
                Some(t) => format!("player:{t}"),
            }
        }
    }
}

#[derive(Default)]
struct Grp {
    n: u64,
    ok: u64,
    err: u64,
    changed: u64,
    dt_sum: f64,
    dt_n: u64,
    lat: Vec<f64>,
    bytes: u64,
    change_iv: Vec<f64>,
    status: BTreeMap<u16, u64>,
}

#[derive(Default)]
struct ObjStat {
    obs: u64,
    dxdy: u64,
    samples: u64,
    max_per_sample: u64,
    ctx: BTreeMap<String, u64>,
}

struct Seg {
    start_ms: i64,
    label: String,
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 || args[1] == "-h" || args[1] == "--help" {
        eprintln!("usage: wt-analyze <session-id|prefix|dir|latest> [--out DIR]\n  sessions live in {DATA_ROOT}/<date>/<session>/ (receiver) or wt-data/<session>/ (collector local copy)");
        std::process::exit(2);
    }
    let dir = find_session(&args[1])?;
    let out = match args.iter().position(|a| a == "--out") {
        Some(i) => PathBuf::from(args.get(i + 1).context("--out DIR")?),
        None => dir.join("report"),
    };
    std::fs::create_dir_all(&out)?;
    eprintln!("reading {}", dir.display());
    let mut recs = read_session(&dir)?;
    if recs.is_empty() {
        bail!("no records in {}", dir.display());
    }
    recs.sort_by_key(|r| (r.mono_ns, r.seq));
    let session = recs[0].session.clone();
    let meta: Value = std::fs::read(dir.join("session.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();

    // notes: #note records + receiver api notes
    let mut notes: Vec<(i64, String, &str)> = recs.iter().filter(|r| r.ep == "#note").map(|r| (r.wall_ms, r.body.clone().unwrap_or_default(), "collector")).collect();
    if let Ok(s) = std::fs::read_to_string(dir.join("notes.jsonl")) {
        for l in s.lines() {
            if let Ok(v) = serde_json::from_str::<Value>(l) {
                notes.push((v["wall_ms"].as_i64().unwrap_or(0), v["text"].as_str().unwrap_or("").to_string(), "api"));
            }
        }
    }
    notes.sort_by_key(|n| n.0);
    let t_first = recs.iter().map(|r| r.wall_ms).min().unwrap();
    let t_last = recs.iter().map(|r| r.wall_ms).max().unwrap();
    let dur_s = ((t_last - t_first) as f64 / 1000.0).max(0.001);
    let mut segs: Vec<Seg> = vec![Seg { start_ms: i64::MIN, label: "(before first note)".into() }];
    for n in &notes {
        segs.push(Seg { start_ms: n.0, label: n.1.clone() });
    }
    let seg_of = |w: i64| -> usize { segs.iter().rposition(|s| s.start_ms <= w).unwrap_or(0) };

    // images on disk
    let mut img_info: HashMap<String, (u64, String, String)> = HashMap::new(); // hash -> (bytes, enc, dims)
    if let Ok(rd) = std::fs::read_dir(dir.join("images")) {
        for e in rd.flatten() {
            let p = e.path();
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
            let b = std::fs::read(&p).unwrap_or_default();
            let dims = imagesize::blob_size(&b).map(|d| format!("{}x{}", d.width, d.height)).unwrap_or("?".into());
            img_info.insert(stem, (b.len() as u64, wtc_common::sniff_image(&b).to_string(), dims));
        }
    }

    let mut ctx = Ctx::default();
    let mut grps: BTreeMap<(String, String, String), Grp> = BTreeMap::new(); // (ep, cfg_hz, ctx)
    let mut last_ep: HashMap<String, (i64, u64)> = HashMap::new(); // ep -> (wall, mono)
    let mut last_change: HashMap<String, u64> = HashMap::new();
    let mut consec_err = 0u32;
    let mut transitions: Vec<Vec<String>> = Vec::new();
    let mut objs: BTreeMap<(String, String, String), ObjStat> = BTreeMap::new();
    let mut colors_seg: BTreeMap<(usize, String, String, String), u64> = BTreeMap::new(); // seg, color, type, icon
    let mut player_colors: BTreeMap<String, u64> = BTreeMap::new();
    let mut hud_types: BTreeMap<(String, String, String), u64> = BTreeMap::new(); // hud, ptype, army
    let mut gens: BTreeMap<String, Vec<String>> = BTreeMap::new(); // gen key -> row
    let mut gen_order: Vec<String> = Vec::new();
    let mut gen_imgs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut gen_span: HashMap<String, (i64, i64, u64)> = HashMap::new();
    let mut map_info_types: BTreeSet<String> = BTreeSet::new();
    let mut intervals: Vec<(String, i64, i64, u64)> = Vec::new(); // state, start, end, samples
    let mut mission_seq: Vec<Vec<String>> = Vec::new();
    let mut last_mission_sig = String::new();
    let mut ids: BTreeMap<(String, u32), (u64, u64, u64, u64, u64, String, u64)> = BTreeMap::new(); // (ep, epoch)->(responses, nonempty, msgs, min, max, first_url, regressions)
    let mut id_seen_max: HashMap<String, u64> = HashMap::new();
    let mut hud_events_nonempty = 0u64;
    let mut poi: BTreeMap<String, u64> = BTreeMap::new();
    let mut state_valid: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut indicators_seen: BTreeMap<(String, String), u64> = BTreeMap::new();
    let mut bytes_ep: BTreeMap<String, (u64, u64, u64)> = BTreeMap::new(); // body bytes, records, json bytes
    let mut prev_mapobj: Option<Vec<Value>> = None;
    let mut prev_player_pos: Option<(f64, f64)> = None;
    let mut mo_motion: BTreeMap<(String, String), (u64, u64, u64)> = BTreeMap::new(); // (cfg_hz, moving/static) -> (n, body changed, pos changed)
    let mut frame_fits: Vec<Vec<String>> = Vec::new();
    let mut pending_switch: Option<(i64, String, Vec<Value>)> = None; // time, desc, objects before
    let mut img_vs_gen: Vec<(String, String)> = Vec::new();
    let mut last_main_img: Option<String> = None;
    let mut empty_bodies: BTreeMap<String, u64> = BTreeMap::new();
    let mut seg_stats: BTreeMap<usize, (u64, u64, f64, BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new(); // map_obj n, changed, dt_sum, ptypes, hud

    let t0 = t_first;
    let rel = |w: i64| format!("{:.1}", (w - t0) as f64 / 1000.0);
    macro_rules! trans {
        ($w:expr, $field:expr, $from:expr, $to:expr) => {
            transitions.push(vec![lt($w), rel($w), $field.to_string(), $from, $to, ctx.label()]);
        };
    }

    let mut naval_objs: u64 = 0;
    let mut naval_obj_samples: u64 = 0;
    let mut naval_dropped: BTreeMap<String, u64> = BTreeMap::new();
    for r in &recs {
        if let Some(n) = r.filtered_naval {
            naval_objs += n as u64;
            naval_obj_samples += 1;
        }
        if let Some(d) = &r.dropped {
            *naval_dropped.entry(format!("{} ({d})", r.ep)).or_default() += 1;
        }
    }
    for r in &recs {
        let line_len = serde_json::to_string(r).map(|s| s.len() as u64).unwrap_or(0);
        let be = bytes_ep.entry(r.ep.clone()).or_default();
        be.0 += r.bytes;
        be.1 += 1;
        be.2 += line_len;
        if r.ep.starts_with('#') {
            continue;
        }
        ctx.seg = seg_of(r.wall_ms);
        let ok = r.err.is_none();
        // 8111 up/down (debounced: 3 consecutive transport errors)
        if ok {
            consec_err = 0;
            if ctx.up != Some(true) {
                trans!(r.wall_ms, "8111", format!("{:?}", ctx.up), "up".into());
                ctx.up = Some(true);
            }
        } else {
            consec_err += 1;
            if consec_err >= 3 && ctx.up != Some(false) {
                trans!(r.wall_ms, "8111", format!("{:?}", ctx.up), format!("down ({})", r.err.clone().unwrap_or_default()));
                ctx.up = Some(false);
            }
        }
        let body_v: Option<Value> = if ok { r.body.as_deref().and_then(|b| serde_json::from_str(b).ok()) } else { None };
        if ok && r.bytes == 0 && r.dropped.is_none() {
            *empty_bodies.entry(r.ep.clone()).or_default() += 1;
        }
        // context updates first (so a sample is labeled with the state it reports)
        match (r.ep.as_str(), &body_v) {
            ("/map_info.json", Some(v)) => {
                let valid = v["valid"].as_bool().or(Some(!v["map_min"].is_null()));
                if valid != ctx.map_valid {
                    trans!(r.wall_ms, "map_info.valid", format!("{:?}", ctx.map_valid), format!("{:?}", valid));
                    ctx.map_valid = valid;
                }
                for k in ["map_generation", "map_min", "grid_steps", "hud_type"] {
                    if !v[k].is_null() {
                        map_info_types.insert(format!("{k}:{}", if v[k].is_string() || v[k].get(0).map(|x| x.is_string()).unwrap_or(false) { "string" } else { "number" }));
                    }
                }
                if valid == Some(true) {
                    let gen = vs(&v["map_generation"]);
                    let hud = vs(&v["hud_type"]);
                    let range = format!("{}..{}", pair(&v["map_min"]), pair(&v["map_max"]));
                    if gen != ctx.gen {
                        trans!(r.wall_ms, "map_generation", ctx.gen.clone().unwrap_or("-".into()), gen.clone().unwrap_or("-".into()));
                    }
                    if hud != ctx.hud {
                        trans!(r.wall_ms, "hud_type", ctx.hud.clone().unwrap_or("-".into()), hud.clone().unwrap_or("-".into()));
                    }
                    if Some(&range) != ctx.range.as_ref() {
                        trans!(r.wall_ms, "map_range", ctx.range.clone().unwrap_or("-".into()), range.clone());
                        if ctx.range.is_some() {
                            if let Some(prev) = &prev_mapobj {
                                pending_switch = Some((r.wall_ms, format!("range {} -> {}", ctx.range.clone().unwrap_or_default(), range), prev.clone()));
                            }
                        }
                    }
                    ctx.gen = gen.clone();
                    ctx.hud = hud.clone();
                    ctx.range = Some(range.clone());
                    let gk = format!("e{}/gen {}", r.epoch, gen.clone().unwrap_or("-".into()));
                    if !gens.contains_key(&gk) {
                        gen_order.push(gk.clone());
                    }
                    gens.insert(
                        gk.clone(),
                        vec![
                            gk.clone(),
                            pair(&v["map_min"]),
                            pair(&v["map_max"]),
                            pair(&v["grid_steps"]),
                            pair(&v["grid_size"]),
                            pair(&v["grid_zero"]),
                            hud.unwrap_or("-".into()),
                        ],
                    );
                    let sp = gen_span.entry(gk).or_insert((r.wall_ms, r.wall_ms, 0));
                    sp.1 = r.wall_ms;
                    sp.2 += 1;
                }
            }
            ("/indicators", Some(v)) => {
                let army = vs(&v["army"]);
                let veh = vs(&v["type"]);
                if army != ctx.army {
                    trans!(r.wall_ms, "indicators.army", ctx.army.clone().unwrap_or("-".into()), army.clone().unwrap_or("-".into()));
                    ctx.army = army.clone();
                }
                if veh != ctx.vehicle {
                    trans!(r.wall_ms, "indicators.type", ctx.vehicle.clone().unwrap_or("-".into()), veh.clone().unwrap_or("-".into()));
                    ctx.vehicle = veh.clone();
                }
                *indicators_seen.entry((army.unwrap_or("-".into()), veh.unwrap_or("-".into()))).or_default() += 1;
            }
            ("/state", Some(v)) => {
                let sv = v["valid"].as_bool();
                if sv != ctx.state_valid {
                    trans!(r.wall_ms, "state.valid", format!("{:?}", ctx.state_valid), format!("{:?}", sv));
                    ctx.state_valid = sv;
                }
                *state_valid.entry((ctx.label(), format!("{:?}", sv))).or_default() += 1;
            }
            ("/mission.json", Some(v)) => {
                let st = vs(&v["status"]).unwrap_or("-".into());
                let objs_s = match v["objectives"].as_array() {
                    Some(a) => a
                        .iter()
                        .map(|o| format!("{}{}:{}", if o["primary"].as_bool() == Some(true) { "P " } else { "S " }, o["status"].as_str().unwrap_or("?"), o["text"].as_str().unwrap_or("")))
                        .collect::<Vec<_>>()
                        .join(" | "),
                    None => "null".into(),
                };
                let sig = format!("{st}#{objs_s}");
                if sig != last_mission_sig {
                    if Some(&st) != ctx.mission.as_ref() {
                        trans!(r.wall_ms, "mission.status", ctx.mission.clone().unwrap_or("-".into()), st.clone());
                    }
                    mission_seq.push(vec![lt(r.wall_ms), rel(r.wall_ms), st.clone(), objs_s, ctx.label()]);
                    last_mission_sig = sig;
                    ctx.mission = Some(st);
                }
            }
            ("/map.img", _) => {
                if let Some(h) = &r.img_hash {
                    if Some(h) != ctx.img.as_ref() {
                        trans!(r.wall_ms, "map.img", ctx.img.clone().map(|x| x[..12].to_string()).unwrap_or("-".into()), h[..12].to_string());
                        ctx.img = Some(h.clone());
                    }
                    last_main_img = Some(h.clone());
                    let gk = format!("e{}/gen {}", r.epoch, ctx.gen.clone().unwrap_or("-".into()));
                    if ctx.map_valid == Some(true) {
                        gen_imgs.entry(gk).or_default().insert(h.clone());
                    } else {
                        gen_imgs.entry("(no match)".into()).or_default().insert(h.clone());
                    }
                }
            }
            ("/map.img?gen", _) => {
                if let (Some(h), Some(m)) = (&r.img_hash, &last_main_img) {
                    img_vs_gen.push((r.url.clone(), if h == m { "identical".into() } else { format!("DIFFERENT {}", &h[..12]) }));
                }
            }
            ("/map_obj.json", Some(v)) => {
                if let Some(a) = v.as_array() {
                    let pl = a.iter().find(|o| o["icon"].as_str() == Some("Player"));
                    let pt = pl.map(|o| o["type"].as_str().unwrap_or("?").to_string()).unwrap_or("none".into());
                    if Some(&pt) != ctx.ptype.as_ref() {
                        trans!(r.wall_ms, "player", ctx.ptype.clone().unwrap_or("-".into()), pt.clone());
                        if ctx.ptype.is_some() && pt != "none" && ctx.ptype.as_deref() != Some("none") && pending_switch.is_none() {
                            if let Some(prev) = &prev_mapobj {
                                pending_switch = Some((r.wall_ms, format!("player {} -> {}", ctx.ptype.clone().unwrap_or_default(), pt), prev.clone()));
                            }
                        }
                        ctx.ptype = Some(pt.clone());
                    }
                    // intervals
                    let state = if pt == "none" { "absent".to_string() } else { format!("present:{pt}") };
                    match intervals.last_mut() {
                        Some(iv) if iv.0 == state => {
                            iv.2 = r.wall_ms;
                            iv.3 += 1
                        }
                        _ => intervals.push((state, r.wall_ms, r.wall_ms, 1)),
                    }
                    let label = ctx.label();
                    let mut per: HashMap<(String, String, String), u64> = HashMap::new();
                    for o in a {
                        let ty = o["type"].as_str().unwrap_or("?").to_string();
                        let icon = o["icon"].as_str().unwrap_or("?").to_string();
                        let color = o["color"].as_str().unwrap_or("-").to_ascii_lowercase();
                        if ty == "point_of_interest" || icon == "point_of_interest" {
                            *poi.entry(label.clone()).or_default() += 1;
                        }
                        if icon == "Player" {
                            *player_colors.entry(format!("{color} blink={}", o["blink"])).or_default() += 1;
                        }
                        let k = (ty.clone(), icon.clone(), color.clone());
                        *per.entry(k.clone()).or_default() += 1;
                        let s = objs.entry(k).or_default();
                        s.obs += 1;
                        if !o["dx"].is_null() && !o["dy"].is_null() {
                            s.dxdy += 1;
                        }
                        *s.ctx.entry(label.clone()).or_default() += 1;
                        *colors_seg.entry((ctx.seg, color, ty, icon)).or_default() += 1;
                    }
                    for (k, c) in per {
                        let s = objs.get_mut(&k).unwrap();
                        s.samples += 1;
                        s.max_per_sample = s.max_per_sample.max(c);
                    }
                    // motion split
                    let pos = pl.and_then(|o| Some((o["x"].as_f64()?, o["y"].as_f64()?)));
                    if let (Some(p), Some(q)) = (pos, prev_player_pos) {
                        let moved = (p.0 - q.0).abs() > 1e-7 || (p.1 - q.1).abs() > 1e-7;
                        let e = mo_motion.entry((format!("{}", r.cfg_hz), if moved { "moving".into() } else { "static".into() })).or_default();
                        e.0 += 1;
                        if r.changed {
                            e.1 += 1;
                        }
                        if moved {
                            e.2 += 1;
                        }
                    }
                    prev_player_pos = pos;
                    // frame switch fit
                    if let Some((tw, desc, before)) = pending_switch.take() {
                        frame_fits.push(fit_switch(tw, &desc, &before, a, &lt(tw), &rel(tw)));
                    }
                    prev_mapobj = Some(a.clone());
                    let ss = seg_stats.entry(ctx.seg).or_default();
                    ss.0 += 1;
                    if r.changed {
                        ss.1 += 1;
                    }
                    ss.3.insert(pt);
                    if let Some(h) = &ctx.hud {
                        ss.4.insert(h.clone());
                    }
                }
            }
            ("/gamechat", Some(v)) | ("/hudmsg", Some(v)) => {
                let arrs: Vec<(&str, &Value)> = if r.ep == "/gamechat" { vec![("chat", v)] } else { vec![("evt", &v["events"]), ("dmg", &v["damage"])] };
                if r.ep == "/hudmsg" && v["events"].as_array().map(|a| !a.is_empty()).unwrap_or(false) {
                    hud_events_nonempty += 1;
                }
                for (name, arr) in arrs {
                    let key = (format!("{}:{name}", r.ep), r.epoch);
                    let e = ids.entry(key.clone()).or_insert((0, 0, 0, u64::MAX, 0, r.url.clone(), 0));
                    e.0 += 1;
                    let idv: Vec<u64> = arr.as_array().map(|a| a.iter().filter_map(|m| num(&m["id"]).map(|f| f as u64)).collect()).unwrap_or_default();
                    if !idv.is_empty() {
                        e.1 += 1;
                        e.2 += idv.len() as u64;
                        let mn = *idv.iter().min().unwrap();
                        let mx = *idv.iter().max().unwrap();
                        e.3 = e.3.min(mn);
                        e.4 = e.4.max(mx);
                        let gm = id_seen_max.entry(key.0.clone()).or_default();
                        if mn <= *gm && *gm > 0 {
                            e.6 += 1; // ids at or below an id already seen (replay after cursor reset, or restart)
                        }
                        *gm = (*gm).max(mx);
                    }
                }
            }
            _ => {}
        }
        let hud = ctx.hud.clone().unwrap_or("-".into());
        if r.ep == "/map_info.json" && ok {
            *hud_types.entry((hud, ctx.ptype.clone().unwrap_or("-".into()), ctx.army.clone().unwrap_or("-".into()))).or_default() += 1;
        }
        // endpoint group stats
        let key = (r.ep.clone(), format!("{}", (r.cfg_hz * 100.0).round() / 100.0), ctx.label());
        let g = grps.entry(key).or_default();
        g.n += 1;
        *g.status.entry(r.status).or_default() += 1;
        if let Some((_, pm)) = last_ep.get(&r.ep) {
            let dt = (r.mono_ns.saturating_sub(*pm)) as f64 / 1e9;
            let period = if r.cfg_hz > 0.0 { 1.0 / r.cfg_hz } else { 10.0 };
            if dt < period * 5.0 + 1.0 {
                g.dt_sum += dt;
                g.dt_n += 1;
            }
            if r.ep == "/map_obj.json" {
                let ss = seg_stats.entry(ctx.seg).or_default();
                ss.2 += dt;
            }
        }
        last_ep.insert(r.ep.clone(), (r.wall_ms, r.mono_ns));
        if ok {
            g.ok += 1;
            g.lat.push(r.lat_us as f64 / 1000.0);
            g.bytes += r.bytes;
            if r.changed {
                g.changed += 1;
                if let Some(pc) = last_change.get(&r.ep) {
                    g.change_iv.push((r.mono_ns.saturating_sub(*pc)) as f64 / 1e6);
                }
                last_change.insert(r.ep.clone(), r.mono_ns);
            }
        } else {
            g.err += 1;
        }
    }

    // ------------------------------------------------------------ outputs
    let mut md = String::new();
    let _ = writeln!(md, "# wt-analyze report — session `{session}`\n");
    let _ = writeln!(
        md,
        "- player: **{}** · collector {} · host {} · records {} · span {} → {} ({:.1} min)\n- source dir: `{}`\n- generated {}\n",
        meta["player"].as_str().unwrap_or("?"),
        meta["version"].as_str().unwrap_or("?"),
        meta["host"].as_str().unwrap_or("?"),
        recs.len(),
        lt(t_first),
        lt(t_last),
        dur_s / 60.0,
        dir.display(),
        chrono::Local::now().format("%F %T")
    );
    if session.contains("mock") || meta["player"].as_str().map(|p| p.contains("mock")).unwrap_or(false) {
        let _ = writeln!(md, "> ⚠️ This session looks like a **synthetic mock** run (mock8111.py). Numbers describe the mock, not the game.\n");
    }
    let _ = writeln!(
        md,
        "**Naval filter (receiver):** {naval_objs} naval objects removed from {naval_obj_samples} map_obj samples; naval ticks dropped (own vehicle naval, content removed, timing kept): {}.\n",
        if naval_dropped.is_empty() { "0".to_string() } else { naval_dropped.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(", ") }
    );
    let _ = writeln!(md, "Times are local (Asia/Shanghai). `t+` = seconds since first record. CSVs in this folder hold the full tables.\n");
    // clock sync / staleness guard (0.1.5+)
    {
        let _ = std::fs::create_dir_all(dir.join("report"));
        let _ = writeln!(md, "## Clock sync & staleness guard\n");
        let g: Value = std::fs::read(dir.join("guard.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        if g.is_null() {
            let _ = writeln!(md, "_no guard.json (session older than receiver clock guard)_\n");
        } else {
            let lc = &g["last_clock"];
            let _ = writeln!(
                md,
                "Server guard (±{} ms): accepted **{}**, dropped too_old **{}**, from_future **{}**, no_sync **{}** (in {} rejected batches), legacy (pre-0.1.5, unsynced) accepted {}. Collector-side drops (last report): too_old {}, overflow {}, no_sync {}.\n",
                g["max_skew_ms"], g["accepted"], g["too_old"], g["from_future"], g["no_sync"], g["batches_rejected_no_sync"], g["legacy_accepted"],
                lc["local_drop_too_old"], lc["local_drop_overflow"], lc["local_drop_no_sync"]
            );
        }
        let mut rows = Vec::new();
        let mut offs = Vec::new();
        let mut rtts = Vec::new();
        for r in recs.iter().filter(|r| r.ep == "#clock") {
            let v: Value = r.body.as_deref().and_then(|b| serde_json::from_str(b).ok()).unwrap_or(Value::Null);
            let (o, t) = (v["offset_ms"].as_f64().unwrap_or(f64::NAN), v["rtt_ms"].as_f64().unwrap_or(f64::NAN));
            offs.push(o);
            rtts.push(t);
            rows.push(vec![lt(r.wall_ms), format!("{o:.1}"), format!("{t:.1}"), format!("{:.1}", v["step_ms"].as_f64().unwrap_or(0.0))]);
        }
        let _ = write_csv(&dir.join("report"), "clock.csv", &["local_time", "offset_ms", "rtt_ms", "step_ms"], &rows);
        if !offs.is_empty() {
            let mut o2 = offs.clone();
            let _ = writeln!(
                md,
                "Clock syncs: {} · offset (server − PC) first {:+.1} / last {:+.1} / min {:+.1} / max {:+.1} ms (drift over session {:+.1} ms) · RTT p50 {:.1} ms, min {:.1} ms → offset uncertainty ≤ ±{:.1} ms.\n",
                offs.len(), offs[0], offs[offs.len() - 1],
                o2.iter().cloned().fold(f64::INFINITY, f64::min), o2.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
                offs[offs.len() - 1] - offs[0], perc(&mut rtts.clone(), 0.5), rtts.iter().cloned().fold(f64::INFINITY, f64::min),
                rtts.iter().cloned().fold(f64::INFINITY, f64::min) / 2.0
            );
            o2.clear();
        }
        let mut delays: Vec<f64> = recs.iter().filter_map(|r| Some((r.recv_ms? - r.ts_server_ms?) as f64)).collect();
        if !delays.is_empty() {
            let n = delays.len();
            let _ = writeln!(
                md,
                "Delivery delay of stored records (server receive − corrected sample time), n={n}: p50 {:.0} ms, p95 {:.0} ms, p99 {:.0} ms, max {:.0} ms.\n",
                perc(&mut delays, 0.5), perc(&mut delays, 0.95), perc(&mut delays, 0.99), perc(&mut delays, 1.0)
            );
        }
    }

    // notes
    let _ = writeln!(md, "## Notes / test labels\n");
    if notes.is_empty() {
        let _ = writeln!(md, "_none_\n");
    } else {
        let _ = writeln!(md, "| time | t+ | source | note |\n|---|---|---|---|");
        for n in &notes {
            let _ = writeln!(md, "| {} | {} | {} | {} |", lt(n.0), rel(n.0), n.2, n.1.replace('|', "\\|"));
        }
        let _ = writeln!(md);
    }

    // endpoint table
    let mut ep_rows = Vec::new();
    let _ = writeln!(md, "## 1. Per-endpoint rate, new-sample ratio, latency (spec §10 Q6, DATA-02)\n\nGrouped by endpoint × configured Hz × context (context = state reported at that moment: `player:<type>`, `no_player`, `no_match` (map_info invalid), `8111_down`). `new%` = body differs from previous body of the same endpoint. `chg p50/p95` = interval between body changes (content refresh period).\n");
    let _ = writeln!(md, "| endpoint | cfg Hz | context | n | eff Hz | new% | err% | lat p50 | p95 | p99 ms | chg p50 | chg p95 ms | avg bytes |\n|---|---|---|---|---|---|---|---|---|---|---|---|---|");
    for ((ep, hz, c), g) in grps.iter_mut() {
        let eff = if g.dt_sum > 0.0 { g.dt_n as f64 / g.dt_sum } else { f64::NAN };
        let (p50, p95, p99) = (perc(&mut g.lat, 0.5), perc(&mut g.lat, 0.95), perc(&mut g.lat, 0.99));
        let (c50, c95) = (perc(&mut g.change_iv, 0.5), perc(&mut g.change_iv, 0.95));
        let newp = pct(g.changed as f64, g.ok as f64);
        let errp = pct(g.err as f64, g.n as f64);
        let avgb = if g.ok > 0 { g.bytes / g.ok } else { 0 };
        let _ = writeln!(md, "| {ep} | {hz} | {c} | {} | {eff:.2} | {newp:.1} | {errp:.1} | {p50:.2} | {p95:.2} | {p99:.2} | {c50:.0} | {c95:.0} | {avgb} |", g.n);
        ep_rows.push(vec![
            ep.clone(), hz.clone(), c.clone(), g.n.to_string(), format!("{eff:.3}"), format!("{newp:.2}"), format!("{errp:.2}"),
            format!("{p50:.3}"), format!("{p95:.3}"), format!("{p99:.3}"), format!("{c50:.1}"), format!("{c95:.1}"), avgb.to_string(),
            format!("{:?}", g.status),
        ]);
    }
    write_csv(&out, "endpoints.csv", &["endpoint", "cfg_hz", "context", "n", "eff_hz", "new_pct", "err_pct", "lat_p50_ms", "lat_p95_ms", "lat_p99_ms", "change_iv_p50_ms", "change_iv_p95_ms", "avg_bytes", "status_counts"], &ep_rows)?;

    // map_obj freshness
    let _ = writeln!(md, "\n## 2. map_obj freshness: moving vs static Player (Q6)\n\n| cfg Hz | player | samples | body new% | player-pos changed% |\n|---|---|---|---|---|");
    let mut mo_rows = Vec::new();
    for ((hz, m), (n, ch, pc)) in &mo_motion {
        let _ = writeln!(md, "| {hz} | {m} | {n} | {:.1} | {:.1} |", pct(*ch as f64, *n as f64), pct(*pc as f64, *n as f64));
        mo_rows.push(vec![hz.clone(), m.clone(), n.to_string(), format!("{:.2}", pct(*ch as f64, *n as f64))]);
    }
    write_csv(&out, "map_obj_motion.csv", &["cfg_hz", "player_motion", "samples", "body_new_pct"], &mo_rows)?;
    let _ = writeln!(md, "\n_If body new% at 20 Hz ≈ 50% while 10 Hz ≈ 100%, the game refreshes map_obj at ~10 Hz. Compare `chg p50` in §1._");

    // objects
    let _ = writeln!(md, "\n## 3. Objects by type / icon / color (Q1, Q7; dx/dy presence)\n\n`dxdy%` = share of observations carrying dx/dy. `class` = spec §4.5 rule (r≥200,g<130,b<130 → enemy). Full table: objects.csv.\n");
    let _ = writeln!(md, "| type | icon | color | class | obs | samples | max/sample | dxdy% | contexts |\n|---|---|---|---|---|---|---|---|---|");
    let mut obj_rows = Vec::new();
    let mut ov: Vec<_> = objs.iter().collect();
    ov.sort_by_key(|(_, s)| std::cmp::Reverse(s.obs));
    for ((ty, icon, color), s) in &ov {
        let ctxs = s.ctx.iter().map(|(k, v)| format!("{k}:{v}")).collect::<Vec<_>>().join(" ");
        let _ = writeln!(md, "| {ty} | {icon} | {color} | {} | {} | {} | {} | {:.0} | {ctxs} |", color_class(color), s.obs, s.samples, s.max_per_sample, pct(s.dxdy as f64, s.obs as f64));
        obj_rows.push(vec![ty.clone(), icon.clone(), color.clone(), color_class(color).into(), s.obs.to_string(), s.samples.to_string(), s.max_per_sample.to_string(), format!("{:.1}", pct(s.dxdy as f64, s.obs as f64)), ctxs]);
    }
    write_csv(&out, "objects.csv", &["type", "icon", "color", "class", "observations", "samples_present", "max_per_sample", "dxdy_pct", "contexts"], &obj_rows)?;
    let gm: Vec<_> = ov.iter().filter(|((t, i, _), _)| t == "ground_model" && i != "Player").collect();
    let gm_dx: u64 = gm.iter().map(|(_, s)| s.dxdy).sum();
    let gm_n: u64 = gm.iter().map(|(_, s)| s.obs).sum();
    let _ = writeln!(md, "\n**ground_model (non-Player) with dx/dy:** {gm_dx} / {gm_n} observations ({:.1}%).", pct(gm_dx as f64, gm_n as f64));
    let _ = writeln!(md, "\n**Player object colors:** {}", player_colors.iter().map(|(k, v)| format!("`{k}`×{v}")).collect::<Vec<_>>().join(", "));
    let _ = writeln!(md, "\n**point_of_interest objects (Q8):** {}", if poi.is_empty() { "none seen".to_string() } else { format!("{poi:?}") });
    let mut cs_rows = Vec::new();
    for ((seg, color, ty, icon), n) in &colors_seg {
        cs_rows.push(vec![seg.to_string(), segs[*seg].label.clone(), color.clone(), color_class(color).into(), ty.clone(), icon.clone(), n.to_string()]);
    }
    write_csv(&out, "colors_by_note_segment.csv", &["segment", "note", "color", "class", "type", "icon", "observations"], &cs_rows)?;
    let _ = writeln!(md, "\nColors per note segment (e.g. before/after toggling color-blind mode): colors_by_note_segment.csv. Distinct colors per segment:\n");
    let mut segcol: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
    for (seg, color, _, _) in colors_seg.keys() {
        segcol.entry(*seg).or_default().insert(color.clone());
    }
    for (seg, cols) in &segcol {
        let _ = writeln!(md, "- seg {seg} _{}_: {}", segs[*seg].label, cols.iter().cloned().collect::<Vec<_>>().join(" "));
    }

    // hud_type
    let _ = writeln!(md, "\n## 4. hud_type values (Q4)\n\n| hud_type | Player type | indicators.army | map_info samples |\n|---|---|---|---|");
    for ((h, p, a), n) in &hud_types {
        let _ = writeln!(md, "| {h} | {p} | {a} | {n} |");
    }
    let _ = writeln!(md, "\nmap_info value types seen (number vs numeric-string schema): {}", map_info_types.iter().cloned().collect::<Vec<_>>().join(", "));

    // map generations
    let _ = writeln!(md, "\n## 5. Map range & map.img per map_generation (Q2, Q3, DATA-03)\n\n`eN` = collector match epoch (new epoch when map_info becomes valid / game restarts).\n");
    let _ = writeln!(md, "| epoch/gen | first | last | map_min | map_max | grid_steps | grid_size | grid_zero | hud_type | images (hash12 bytes enc WxH) |\n|---|---|---|---|---|---|---|---|---|---|");
    let mut gen_rows = Vec::new();
    for gk in &gen_order {
        let row = &gens[gk];
        let sp = gen_span[gk];
        let imgs = gen_imgs
            .get(gk)
            .map(|s| {
                s.iter()
                    .map(|h| match img_info.get(h) {
                        Some((b, e, d)) => format!("{} {}B {} {}", &h[..12], b, e, d),
                        None => format!("{} (file missing)", &h[..12]),
                    })
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or("-".into());
        let _ = writeln!(md, "| {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |", row[0], lt(sp.0), lt(sp.1), row[1], row[2], row[3], row[4], row[5], row[6], imgs);
        let mut rr = row.clone();
        rr.insert(1, lt(sp.0));
        rr.insert(2, lt(sp.1));
        rr.push(imgs);
        gen_rows.push(rr);
    }
    if let Some(s) = gen_imgs.get("(no match)") {
        let _ = writeln!(md, "\nImages served while no match (map_info invalid): {}", s.iter().map(|h| format!("{} {:?}", &h[..12], img_info.get(h))).collect::<Vec<_>>().join("; "));
    }
    write_csv(&out, "map_generations.csv", &["epoch_gen", "first", "last", "map_min", "map_max", "grid_steps", "grid_size", "grid_zero", "hud_type", "images"], &gen_rows)?;
    let ident = img_vs_gen.iter().filter(|x| x.1 == "identical").count();
    let _ = writeln!(md, "\n`/map.img?gen=N` vs `/map.img` at map change: {} identical of {} checks{}", ident, img_vs_gen.len(), img_vs_gen.iter().filter(|x| x.1 != "identical").map(|x| format!("; {} {}", x.0, x.1)).collect::<String>());

    // transitions
    let _ = writeln!(md, "\n## 6. Transitions timeline (Q2, Q5, DATA-04)\n\nEvery change of 8111 reachability, map_info.valid, map_generation, map range, hud_type, map.img hash, Player type (`none` = absent), indicators.army/type, state.valid, mission.status. Full list: transitions.csv ({} rows). First 150:\n", transitions.len());
    let _ = writeln!(md, "| time | t+ | field | from | to | context after |\n|---|---|---|---|---|---|");
    for t in transitions.iter().take(150) {
        let _ = writeln!(md, "| {} |", t.iter().map(|c| c.replace('|', "\\|")).collect::<Vec<_>>().join(" | "));
    }
    write_csv(&out, "transitions.csv", &["time", "t_plus_s", "field", "from", "to", "context"], &transitions)?;

    let _ = writeln!(md, "\n### Coordinate-frame switches (e.g. ground player takes off)\n\nAt each map-range change or Player type change, non-Player objects matched by (type, icon, color, unique) between the last map_obj before and the first after; per-axis linear fit `new = a·old + b` (WTP method). `resid` = max abs residual.\n");
    let _ = writeln!(md, "| time | t+ | change | pairs | ax | bx | ay | by | resid |\n|---|---|---|---|---|---|---|---|---|");
    for f in &frame_fits {
        let _ = writeln!(md, "| {} |", f.join(" | "));
    }
    write_csv(&out, "frame_switches.csv", &["time", "t_plus_s", "change", "pairs", "ax", "bx", "ay", "by", "max_resid"], &frame_fits)?;

    // player intervals
    let _ = writeln!(md, "\n## 7. Player present / absent intervals (Q5)\n\n| state | start | end | duration s | samples |\n|---|---|---|---|---|");
    let mut iv_rows = Vec::new();
    for (s, a, b, n) in &intervals {
        let d = (b - a) as f64 / 1000.0;
        let _ = writeln!(md, "| {s} | {} | {} | {d:.1} | {n} |", lt(*a), lt(*b));
        iv_rows.push(vec![s.clone(), lt(*a), lt(*b), format!("{d:.2}"), n.to_string()]);
    }
    write_csv(&out, "player_intervals.csv", &["state", "start", "end", "duration_s", "samples"], &iv_rows)?;

    // ids
    let _ = writeln!(md, "\n## 8. Chat / HUD id behavior across matches (Q10, spec §4.8/4.9)\n\nPer collector epoch (cursor reset to 0 at each new epoch). `replays` = responses containing ids ≤ an id already seen earlier in the session (history returned again after reset ⇒ game keeps history; 0 ⇒ ids are only served once / history cleared).\n");
    let _ = writeln!(md, "| stream | epoch | first url | responses | non-empty | msgs | min id | max id | replays |\n|---|---|---|---|---|---|---|---|---|");
    let mut id_rows = Vec::new();
    for ((s, e), v) in &ids {
        let mn = if v.3 == u64::MAX { "-".into() } else { v.3.to_string() };
        let _ = writeln!(md, "| {s} | {e} | `{}` | {} | {} | {} | {mn} | {} | {} |", v.5, v.0, v.1, v.2, v.4, v.6);
        id_rows.push(vec![s.clone(), e.to_string(), v.5.clone(), v.0.to_string(), v.1.to_string(), v.2.to_string(), mn, v.4.to_string(), v.6.to_string()]);
    }
    write_csv(&out, "ids.csv", &["stream", "epoch", "first_url", "responses", "nonempty", "msgs", "min_id", "max_id", "replays"], &id_rows)?;
    let _ = writeln!(md, "\nhudmsg responses with non-empty `events`: **{hud_events_nonempty}**");

    // mission
    let _ = writeln!(md, "\n## 9. mission.json state sequence (Q9)\n\n| time | t+ | status | objectives | context |\n|---|---|---|---|---|");
    for m in &mission_seq {
        let _ = writeln!(md, "| {} |", m.iter().map(|c| c.replace('|', "/")).collect::<Vec<_>>().join(" | "));
    }
    write_csv(&out, "mission.csv", &["time", "t_plus_s", "status", "objectives", "context"], &mission_seq)?;
    let statuses: BTreeSet<&String> = mission_seq.iter().map(|m| &m[2]).collect();
    let _ = writeln!(md, "\nmission.status values seen: {statuses:?}");

    // misc
    let _ = writeln!(md, "\n## 10. Other observations\n");
    let _ = writeln!(md, "- `/state` valid by context: {}", state_valid.iter().map(|((c, v), n)| format!("{c} valid={v}: {n}")).collect::<Vec<_>>().join("; "));
    let _ = writeln!(md, "- `/indicators` army / type: {}", indicators_seen.iter().map(|((a, t), n)| format!("{a} {t} ×{n}")).collect::<Vec<_>>().join("; "));
    let _ = writeln!(md, "- empty bodies (HTTP ok, 0 bytes): {}", if empty_bodies.is_empty() { "none".to_string() } else { format!("{empty_bodies:?}") });

    // note segments
    let _ = writeln!(md, "\n## 11. Per note-segment summary\n\n| seg | note | map_obj samples | eff Hz | new% | Player types | hud_type |\n|---|---|---|---|---|---|---|");
    let mut seg_rows = Vec::new();
    for (i, (n, ch, dt, pts, huds)) in &seg_stats {
        let hz = if *dt > 0.0 { *n as f64 / dt } else { f64::NAN };
        let lab = segs[*i].label.replace('|', "/");
        let _ = writeln!(md, "| {i} | {lab} | {n} | {hz:.2} | {:.1} | {} | {} |", pct(*ch as f64, *n as f64), pts.iter().cloned().collect::<Vec<_>>().join(","), huds.iter().cloned().collect::<Vec<_>>().join(","));
        seg_rows.push(vec![i.to_string(), lab, n.to_string(), format!("{hz:.3}"), format!("{:.2}", pct(*ch as f64, *n as f64))]);
    }
    write_csv(&out, "segments.csv", &["segment", "note", "map_obj_samples", "eff_hz", "new_pct"], &seg_rows)?;

    // bandwidth
    let hours = dur_s / 3600.0;
    let stored: u64 = wtc_common::sample_files(&dir).iter().filter_map(|p| p.metadata().ok()).map(|m| m.len()).sum();
    let imgs: u64 = img_info.values().map(|v| v.0).sum();
    let _ = writeln!(md, "\n## 12. Bandwidth\n\nSession span {:.2} h. `body` = raw 8111 response bytes (what the collector reads from the game); `record json` ≈ uncompressed upload size.\n", hours);
    let _ = writeln!(md, "| endpoint | records | body bytes | body MB/h | record json MB/h |\n|---|---|---|---|---|");
    let mut bw_rows = Vec::new();
    let (mut tb, mut tj) = (0u64, 0u64);
    for (ep, (b, n, j)) in &bytes_ep {
        tb += b;
        tj += j;
        let _ = writeln!(md, "| {ep} | {n} | {b} | {:.2} | {:.2} |", *b as f64 / 1e6 / hours, *j as f64 / 1e6 / hours);
        bw_rows.push(vec![ep.clone(), n.to_string(), b.to_string(), format!("{:.3}", *b as f64 / 1e6 / hours), format!("{:.3}", *j as f64 / 1e6 / hours)]);
    }
    let _ = writeln!(md, "| **total** | {} | {tb} | {:.2} | {:.2} |", recs.len(), tb as f64 / 1e6 / hours, tj as f64 / 1e6 / hours);
    let _ = writeln!(md, "\nStored compressed sample files: {:.2} MB ({:.2} MB/h ≈ gzip upload volume); images {} bytes in {} file(s).", stored as f64 / 1e6, stored as f64 / 1e6 / hours, imgs, img_info.len());
    write_csv(&out, "bandwidth.csv", &["endpoint", "records", "body_bytes", "body_mb_per_h", "json_mb_per_h"], &bw_rows)?;

    let _ = writeln!(md, "\n## Open-question checklist (spec §10)\n");
    let _ = writeln!(md, "1. Visibility per mode → §3 objects by context + notes (label the mode in notes).\n2. Ground→air map_info change → §6 transitions + frame switches, §5.\n3. map.img sizes/encodings → §5.\n4. hud_type values → §4.\n5. Death/respawn/spectate order → §6, §7.\n6. map_obj new-sample rate / latency → §1, §2, §11.\n7. Colors / color-blind → §3 + colors_by_note_segment.csv.\n8. point_of_interest → §3.\n9. mission.status set → §9.\n10. HUD events / ids → §8.\n11. Remote-access binding → not measurable by this collector (it only uses 127.0.0.1); check `netstat -ano | findstr 8111` on the PC.");

    std::fs::write(out.join("report.md"), &md)?;
    println!("report: {}", out.join("report.md").display());
    println!("csv   : {}/*.csv", out.display());
    Ok(())
}

fn obj_key(o: &Value) -> Option<(String, (f64, f64))> {
    if o["icon"].as_str() == Some("Player") {
        return None;
    }
    let x = o["x"].as_f64().or(o["sx"].as_f64())?;
    let y = o["y"].as_f64().or(o["sy"].as_f64())?;
    Some((format!("{}|{}|{}", o["type"].as_str().unwrap_or(""), o["icon"].as_str().unwrap_or(""), o["color"].as_str().unwrap_or("").to_ascii_lowercase()), (x, y)))
}

fn linfit(p: &[(f64, f64)]) -> (f64, f64, f64) {
    let n = p.len() as f64;
    let mx = p.iter().map(|q| q.0).sum::<f64>() / n;
    let my = p.iter().map(|q| q.1).sum::<f64>() / n;
    let sxx: f64 = p.iter().map(|q| (q.0 - mx).powi(2)).sum();
    let sxy: f64 = p.iter().map(|q| (q.0 - mx) * (q.1 - my)).sum();
    let a = if sxx > 1e-12 { sxy / sxx } else { 1.0 };
    let b = my - a * mx;
    let res = p.iter().map(|q| (q.1 - (a * q.0 + b)).abs()).fold(0.0, f64::max);
    (a, b, res)
}

fn fit_switch(_tw: i64, desc: &str, before: &[Value], after: &[Value], t: &str, rel: &str) -> Vec<String> {
    let index = |v: &[Value]| {
        let mut m: HashMap<String, Vec<(f64, f64)>> = HashMap::new();
        for o in v {
            if let Some((k, p)) = obj_key(o) {
                m.entry(k).or_default().push(p);
            }
        }
        m
    };
    let (b, a) = (index(before), index(after));
    let mut px = Vec::new();
    let mut py = Vec::new();
    for (k, vb) in &b {
        if let Some(va) = a.get(k) {
            if vb.len() == 1 && va.len() == 1 {
                px.push((vb[0].0, va[0].0));
                py.push((vb[0].1, va[0].1));
            }
        }
    }
    if px.len() < 3 {
        return vec![t.into(), rel.into(), desc.into(), px.len().to_string(), "-".into(), "-".into(), "-".into(), "-".into(), "too few unique pairs".into()];
    }
    let (ax, bx, rx) = linfit(&px);
    let (ay, by, ry) = linfit(&py);
    vec![t.into(), rel.into(), desc.into(), px.len().to_string(), format!("{ax:.4}"), format!("{bx:.4}"), format!("{ay:.4}"), format!("{by:.4}"), format!("{:.4}", rx.max(ry))]
}
