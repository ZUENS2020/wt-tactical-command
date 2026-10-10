//! Game API endpoint selection: the 8111 web API may live on 8111 or (current clients) 9222.
//! Only loopback or this PC's own private LAN IPv4 addresses are ever used.
use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

pub const DEFAULT_PORTS: [u16; 2] = [8111, 9222];

fn is_private(ip: &Ipv4Addr) -> bool {
    let o = ip.octets();
    o[0] == 10 || (o[0] == 172 && (16..=31).contains(&o[1])) || (o[0] == 192 && o[1] == 168)
}

/// This PC's private (RFC1918) IPv4 addresses.
pub fn local_private_ips() -> Vec<Ipv4Addr> {
    let mut v: Vec<Ipv4Addr> = Vec::new();
    // primary interface via a connected UDP socket (no packet is sent)
    for probe in ["192.168.1.1:9", "10.0.0.1:9", "8.8.8.8:9"] {
        if let Ok(s) = std::net::UdpSocket::bind("0.0.0.0:0") {
            if s.connect(probe).is_ok() {
                if let Ok(IpAddr::V4(ip)) = s.local_addr().map(|a| a.ip()) {
                    if is_private(&ip) && !v.contains(&ip) {
                        v.push(ip);
                    }
                }
            }
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        if let Ok(o) = std::process::Command::new("ipconfig").creation_flags(0x0800_0000).output() {
            let s = String::from_utf8_lossy(&o.stdout);
            for l in s.lines() {
                if l.contains("IPv4") {
                    if let Some(ip) = l.rsplit(':').next().and_then(|x| x.trim().trim_end_matches("(Preferred)").trim().parse::<Ipv4Addr>().ok()) {
                        if is_private(&ip) && !v.contains(&ip) {
                            v.push(ip);
                        }
                    }
                }
            }
        }
    }
    v
}

/// Allowed game hosts: loopback, or one of this PC's own private LAN addresses.
pub fn validate_host(h: &str) -> anyhow::Result<String> {
    let t = h.trim().trim_start_matches('[').trim_end_matches(']');
    if t.eq_ignore_ascii_case("localhost") {
        return Ok("127.0.0.1".into());
    }
    let ip: IpAddr = t.parse().map_err(|_| anyhow::anyhow!("game_host '{h}' must be an IP address (loopback or this PC's LAN IP)"))?;
    match ip {
        IpAddr::V4(v4) if v4.is_loopback() => Ok(v4.to_string()),
        IpAddr::V6(v6) if v6.is_loopback() => Ok("[::1]".into()),
        IpAddr::V4(v4) if local_private_ips().contains(&v4) => Ok(v4.to_string()),
        _ => anyhow::bail!("game_host '{h}' refused: only 127.0.0.1 or this PC's own private LAN IPv4 ({:?}) are allowed", local_private_ips()),
    }
}

/// Candidate base URLs in priority order.
pub fn candidates(host: Option<&str>, port: Option<u16>) -> Vec<String> {
    let hosts: Vec<String> = match host {
        Some(h) => vec![h.to_string()],
        None => {
            let mut v = vec!["127.0.0.1".to_string()];
            v.extend(local_private_ips().iter().map(|i| i.to_string()));
            v
        }
    };
    let ports: Vec<u16> = match port {
        Some(p) => vec![p],
        None => DEFAULT_PORTS.to_vec(),
    };
    let mut out = Vec::new();
    for h in &hosts {
        for p in &ports {
            out.push(format!("http://{h}:{p}"));
        }
    }
    out
}

/// Probe one base: GET /map_info.json must return a JSON object with a `valid` field.
pub async fn probe_one(client: &reqwest::Client, base: &str) -> Result<String, String> {
    let t0 = std::time::Instant::now();
    match client.get(format!("{base}/map_info.json")).timeout(Duration::from_millis(3000)).send().await {
        Ok(r) => {
            let st = r.status().as_u16();
            let b = r.bytes().await.unwrap_or_default();
            match serde_json::from_slice::<serde_json::Value>(&b) {
                Ok(v) if v.get("valid").is_some() => Ok(format!("OK HTTP {st} {} ms valid={}", t0.elapsed().as_millis(), v["valid"])),
                _ => Err(format!("HTTP {st} but not 8111 map_info JSON ({} bytes)", b.len())),
            }
        }
        Err(e) => Err(crate::classify(&e)),
    }
}

/// Probe all candidates in parallel; return first working (priority order) + per-candidate results.
pub async fn probe_all(client: &reqwest::Client, cands: &[String]) -> (Option<String>, Vec<(String, Result<String, String>)>) {
    let mut set = tokio::task::JoinSet::new();
    for (i, c) in cands.iter().enumerate() {
        let (cl, c) = (client.clone(), c.clone());
        set.spawn(async move { (i, c.clone(), probe_one(&cl, &c).await) });
    }
    let mut res: Vec<(usize, String, Result<String, String>)> = Vec::new();
    while let Some(Ok(r)) = set.join_next().await {
        res.push(r);
    }
    res.sort_by_key(|r| r.0);
    let best = res.iter().find(|r| r.2.is_ok()).map(|r| r.1.clone());
    (best, res.into_iter().map(|r| (r.1, r.2)).collect())
}

pub fn short(base: &str) -> &str {
    base.trim_start_matches("http://")
}
