//! `wt-collector diag`: connectivity diagnostics for 127.0.0.1:8111 and the upload server.
use crate::{classify, Cfg, CONNECT_TIMEOUT, REQ_TIMEOUT, VERSION};
use anyhow::Result;
use std::time::{Duration, Instant};

fn mask(v: &str) -> String {
    // hide credentials in proxy URLs: scheme://user:pass@host -> scheme://***@host
    match (v.find("://"), v.rfind('@')) {
        (Some(i), Some(j)) if j > i => format!("{}***{}", &v[..i + 3], &v[j..]),
        _ => v.to_string(),
    }
}

#[cfg(windows)]
fn run_cmd(cmd: &str, args: &[&str]) -> String {
    use std::os::windows::process::CommandExt;
    match std::process::Command::new(cmd).args(args).creation_flags(0x0800_0000).output() {
        Ok(o) => {
            let mut s = String::from_utf8_lossy(&o.stdout).to_string();
            s.push_str(&String::from_utf8_lossy(&o.stderr));
            s
        }
        Err(e) => format!("(cannot run {cmd}: {e})"),
    }
}

async fn tcp_probe(addr: &str) -> String {
    let t0 = Instant::now();
    let r = tokio::time::timeout(Duration::from_secs(4), tokio::net::TcpStream::connect(addr)).await;
    let ms = t0.elapsed().as_millis();
    match r {
        Ok(Ok(_)) => format!("OPEN      ({ms} ms)"),
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => format!("REFUSED   ({ms} ms) -> nothing listening on this address"),
        Ok(Err(e)) => format!("ERROR     ({ms} ms) {e}"),
        Err(_) => format!("TIMEOUT   (>{ms} ms) -> SYN not answered (firewall / security software?)"),
    }
}

async fn http_probe(client: &reqwest::Client, url: &str) -> (bool, String) {
    let t0 = Instant::now();
    match client.get(url).timeout(REQ_TIMEOUT).send().await {
        Ok(r) => {
            let st = r.status().as_u16();
            let body = r.text().await.unwrap_or_default();
            let mut b: String = body.chars().take(90).collect();
            b = b.replace(['\n', '\r'], " ");
            (st == 200, format!("HTTP {st} ({} ms, {} bytes) {b}", t0.elapsed().as_millis(), body.len()))
        }
        Err(e) => (false, format!("FAIL ({} ms) {}", t0.elapsed().as_millis(), classify(&e))),
    }
}

pub async fn run(cfg: &Cfg) -> Result<()> {
    println!("wt-collector {VERSION} diag  ({} {})\n", std::env::consts::OS, std::env::consts::ARCH);

    println!("[1] proxy environment variables");
    let mut any = false;
    let mut env_proxy = false;
    for k in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY", "http_proxy", "https_proxy", "all_proxy", "no_proxy"] {
        if let Ok(v) = std::env::var(k) {
            println!("    {k} = {}", mask(&v));
            any = true;
            if !k.eq_ignore_ascii_case("NO_PROXY") && !v.trim().is_empty() {
                env_proxy = true;
            }
        }
    }
    if !any {
        println!("    (none set)");
    }
    #[cfg(windows)]
    {
        println!("\n[2] Windows proxy settings (WinINET registry, WinHTTP)");
        let reg = run_cmd("reg", &["query", r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings"]);
        for l in reg.lines() {
            let t = l.trim();
            if ["ProxyEnable", "ProxyServer", "ProxyOverride", "AutoConfigURL", "AutoDetect"].iter().any(|k| t.starts_with(k)) {
                println!("    {}", mask(t));
            }
        }
        for l in run_cmd("netsh", &["winhttp", "show", "proxy"]).lines().filter(|l| !l.trim().is_empty()) {
            println!("    winhttp: {}", l.trim());
        }
        println!("\n[3] who listens on 8111 / 9222 (netstat -ano)");
        let ns = run_cmd("netstat", &["-ano", "-p", "TCP"]);
        let hits: Vec<&str> = ns.lines().filter(|l| l.contains(":8111 ") || l.contains(":9222 ")).collect();
        if hits.is_empty() {
            println!("    nothing on port 8111 / 9222 (game not running, or not yet in a state that opens the API)");
        }
        for l in hits {
            println!("    {}", l.trim());
        }
        let tl = run_cmd("tasklist", &["/FI", "IMAGENAME eq aces.exe", "/NH"]);
        println!("    aces.exe (War Thunder) process: {}", if tl.to_ascii_lowercase().contains("aces.exe") { "RUNNING" } else { "not found" });
    }
    #[cfg(not(windows))]
    println!("\n[2]/[3] Windows-only checks skipped on this OS");

    let lan = crate::game::local_private_ips();
    println!("\n    this PC's private LAN IPv4: {:?}", lan);
    println!("\n[4] raw TCP connect");
    let mut addrs = vec!["127.0.0.1:8111".to_string(), "127.0.0.1:9222".to_string(), "[::1]:8111".to_string(), "[::1]:9222".to_string()];
    for ip in &lan {
        addrs.push(format!("{ip}:8111"));
        addrs.push(format!("{ip}:9222"));
    }
    for a in &addrs {
        println!("    {a:<22} {}", tcp_probe(a).await);
    }

    println!("\n[5] HTTP GET /map_info.json (direct, no proxy) -- auto-detect candidates in priority order");
    let direct = crate::local_client()?;
    let mut cands = crate::game::candidates(None, None);
    cands.push("http://localhost:8111".into());
    cands.push("http://localhost:9222".into());
    let (best, res) = crate::game::probe_all(&direct, &cands).await;
    for (c, r) in &res {
        println!("    {:<28} {}", crate::game::short(c), match r { Ok(s) => s.clone(), Err(e) => format!("FAIL {e}") });
    }
    let configured = crate::game::candidates(cfg.game_host.as_deref(), cfg.game_port);
    let (cfg_best, _) = crate::game::probe_all(&direct, &configured).await;
    let sys = reqwest::Client::builder().connect_timeout(CONNECT_TIMEOUT).build()?; // honours env/system proxy
    let mut sys_fail = false;
    let mut proxy_ok = false;
    for c in ["http://127.0.0.1:8111", "http://127.0.0.1:9222"] {
        let (ok, d) = http_probe(&sys, &format!("{c}/map_info.json")).await;
        if best.as_deref() == Some(c) {
            sys_fail |= !ok;
        }
        proxy_ok |= ok;
        println!("    {:<28} {d}", format!("{} via system proxy", crate::game::short(c)));
    }
    if let Some(b) = &best {
        for ep in ["/indicators", "/hudmsg?lastEvt=0&lastDmg=0", "/gamechat?lastId=0"] {
            let (_, d) = http_probe(&direct, &format!("{b}{ep}")).await;
            println!("    {:<28} {d}", format!("{}{}", crate::game::short(b), ep.split('?').next().unwrap()));
        }
    }
    let direct_ok = cfg_best.is_some();

    println!("\n[6] upload server {}", cfg.server);
    let up = crate::upload::up_client();
    let (up_ok, d) = http_probe(&up, &format!("{}/healthz", cfg.server)).await;
    println!("    /healthz (no proxy): {d}");
    if up_ok {
        let c = crate::clock::time_client();
        match crate::clock::burst(&c, &cfg.server, 6).await {
            Ok((b, skew, all)) => println!(
                "    clock sync: offset {:+.1} ms (server - this PC), best RTT {:.1} ms of {} samples; server drops data older than {} ms",
                b.offset_ms, b.rtt_ms, all.len(), skew
            ),
            Err(e) => println!("    clock sync: FAILED ({e})"),
        }
    }
    println!("    token configured   : {}", if cfg.token.is_empty() { "NO" } else { "yes" });

    println!("\n== summary");
    if direct_ok {
        println!("    game API OK at {} (direct). The collector will use it (setting: host={}, port={}).",
            crate::game::short(cfg_best.as_deref().unwrap_or("")),
            cfg.game_host.as_deref().unwrap_or("auto"), cfg.game_port.map(|p| p.to_string()).unwrap_or("auto".into()));
        if sys_fail {
            println!("    note: going through the system/env proxy FAILS{} -- the collector always bypasses proxies, so this is harmless.",
                if env_proxy { " (proxy env vars are set)" } else { "" });
        }
    } else if best.is_some() {
        println!("    game API found at {} but NOT with your configured game_host/game_port; set them to auto.", crate::game::short(best.as_deref().unwrap()));
    } else if proxy_ok {
        println!("    8111 only reachable via proxy?! Unexpected; please send this output.");
    } else {
        println!("    game API NOT reachable on 8111/9222. If [4] says REFUSED / nothing in netstat: start War Thunder (enter hangar/battle).");
        println!("    If [4] says TIMEOUT: a firewall / security tool is dropping localhost connections.");
    }
    println!("    upload server: {}", if up_ok { "reachable" } else { "NOT reachable (internet ok? firewall/proxy blocking the server?)" });
    Ok(())
}
