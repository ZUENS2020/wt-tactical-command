#!/workspace/.txvenv/bin/python
"""Retryable DNS + TLS finisher for wt.zuens2020.top -> 43.129.251.18.

Steps (idempotent): domain registration status -> DNSPod zone present -> A records (wt, @) TTL 600 ->
public resolution check (DoH Google/Cloudflare/AliDNS) -> enable HTTPS site in Caddy on the server -> verify
https://wt.zuens2020.top/healthz.
Usage: wt-dns-setup.py [--wait SECONDS] [--interval 60]
Exit: 0 = HTTPS live, 2 = still pending (state printed), 1 = error."""
import argparse, json, os, subprocess, sys, time, urllib.request
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from txapi import call
from tencentcloud.common.exception.tencent_cloud_sdk_exception import TencentCloudSDKException

DOMAIN, SUB, IP, TTL = "zuens2020.top", "wt", "43.129.251.18", 600
FQDN = f"{SUB}.{DOMAIN}"
SSH = ["ssh", "-i", os.path.expanduser("~/.ssh/wt_hk_ed25519"), "-o", "BatchMode=yes", "-o", "ConnectTimeout=15", f"ubuntu@{IP}"]
HERE = os.path.dirname(os.path.abspath(__file__))

def log(*a):
    print(time.strftime("[%F %T]"), *a, flush=True)

def domain_status():
    try:
        r = call("domain", "DescribeDomainNameList", {"Limit": 100})
        for d in r.get("DomainSet", []):
            if d.get("DomainName") == DOMAIN:
                info = {k: d.get(k) for k in ("DomainName", "BuyStatus", "RegistrarType", "ExpirationDate", "CreationDate", "AutoRenew")}
                try:
                    b = call("domain", "DescribeDomainBaseInfo", {"Domain": DOMAIN})["DomainInfo"]
                    info.update({k: b.get(k) for k in ("RealNameAuditStatus", "DomainStatus", "NameServer", "BuyStatus", "RealNameAuditUnpassReason")})
                except TencentCloudSDKException as e:
                    info["baseinfo_err"] = e.get_code()
                return info
        return None
    except TencentCloudSDKException as e:
        return {"err": e.get_code(), "msg": e.get_message()}

def ensure_autorenew(st):
    """User-approved: enable auto-renew once the domain is listed (AutoRenew 1 = on)."""
    if not st or "err" in st or st.get("DomainName") != DOMAIN:
        return
    if st.get("AutoRenew") == 1:
        log("auto-renew already on")
        return
    try:
        lst = call("domain", "DescribeDomainNameList", {"Limit": 100})["DomainSet"]
        did = next(d["DomainId"] for d in lst if d["DomainName"] == DOMAIN)
        call("domain", "SetDomainAutoRenew", {"DomainId": did, "AutoRenew": 1})
        log("auto-renew enabled")
    except Exception as e:
        log("SetDomainAutoRenew failed:", e)

def ensure_zone():
    try:
        d = call("dnspod", "DescribeDomain", {"Domain": DOMAIN})["DomainInfo"]
        return d
    except TencentCloudSDKException as e:
        if "NotExist" not in e.get_code() and "DomainNotExist" not in e.get_code() and e.get_code() != "ResourceNotFound.NoDataOfDomain":
            log("DescribeDomain:", e.get_code(), e.get_message())
    try:
        call("dnspod", "CreateDomain", {"Domain": DOMAIN})
        log("DNSPod zone created")
        return call("dnspod", "DescribeDomain", {"Domain": DOMAIN})["DomainInfo"]
    except TencentCloudSDKException as e:
        log("CreateDomain:", e.get_code(), e.get_message())
        return None

def ensure_a(sub):
    recs = []
    try:
        recs = call("dnspod", "DescribeRecordList", {"Domain": DOMAIN, "Subdomain": sub, "RecordType": "A"}).get("RecordList", [])
    except TencentCloudSDKException as e:
        if e.get_code() != "ResourceNotFound.NoDataOfRecord":
            raise
    if recs:
        r = recs[0]
        if r["Value"] == IP and int(r["TTL"]) == TTL:
            return f"A {sub} ok (id {r['RecordId']})"
        call("dnspod", "ModifyRecord", {"Domain": DOMAIN, "RecordId": r["RecordId"], "SubDomain": sub, "RecordType": "A", "RecordLine": "默认", "Value": IP, "TTL": TTL})
        return f"A {sub} modified {r['Value']} -> {IP}"
    rid = call("dnspod", "CreateRecord", {"Domain": DOMAIN, "SubDomain": sub, "RecordType": "A", "RecordLine": "默认", "Value": IP, "TTL": TTL})["RecordId"]
    return f"A {sub} created (id {rid})"

def doh(name):
    out = {}
    for n, url in [("google", f"https://dns.google/resolve?name={name}&type=A"), ("cloudflare", f"https://cloudflare-dns.com/dns-query?name={name}&type=A"), ("alidns", f"https://dns.alidns.com/resolve?name={name}&type=A")]:
        try:
            rq = urllib.request.Request(url, headers={"accept": "application/dns-json"})
            j = json.load(urllib.request.urlopen(rq, timeout=8))
            out[n] = [a["data"] for a in j.get("Answer", []) if a.get("type") == 1] or f"status {j.get('Status')}"
        except Exception as e:
            out[n] = f"err {type(e).__name__}"
    return out

def enable_https():
    subprocess.run(["scp", "-q", "-i", SSH[2], os.path.join(HERE, "server/Caddyfile.https"), f"ubuntu@{IP}:/tmp/Caddyfile.https"], check=True)
    cmd = "sudo install -m 644 /tmp/Caddyfile.https /etc/caddy/Caddyfile && sudo systemctl reload caddy && echo reloaded"
    return subprocess.run(SSH + [cmd], capture_output=True, text=True, timeout=60).stdout.strip()

def https_ok():
    try:
        return urllib.request.urlopen(f"https://{FQDN}/healthz", timeout=10).read().decode().strip() == "ok"
    except Exception as e:
        return False

def finish():
    """HTTPS is live: bake the https URL into the shipped zip (exe unchanged)."""
    r = subprocess.run([os.path.join(HERE, "package.sh"), f"https://{FQDN}"], capture_output=True, text=True)
    log("repackaged zip:", (r.stdout or r.stderr).strip())
    return 0

def once():
    st = domain_status()
    log("domain:", json.dumps(st, ensure_ascii=False))
    ensure_autorenew(st)
    zone = ensure_zone()
    if zone:
        log("dnspod zone:", json.dumps({k: zone.get(k) for k in ("Status", "DnsStatus", "Grade", "EffectiveDNS", "ActualNsList")}, ensure_ascii=False))
        for sub in (SUB, "@"):
            try:
                log(ensure_a(sub))
            except TencentCloudSDKException as e:
                log(f"A {sub}:", e.get_code(), e.get_message())
    res = doh(FQDN)
    log("public resolution:", res)
    if not all(isinstance(v, list) and IP in v for k, v in res.items() if k in ("google", "cloudflare")):
        return 2
    if https_ok():
        log(f"HTTPS live: https://{FQDN}/")
        return finish()
    log("caddy:", enable_https())
    for _ in range(18):
        time.sleep(10)
        if https_ok():
            log(f"HTTPS live: https://{FQDN}/")
            return finish()
    log("certificate not ready yet (check: sudo journalctl -u caddy | grep -i acme)")
    return 2

ap = argparse.ArgumentParser()
ap.add_argument("--wait", type=int, default=0)
ap.add_argument("--interval", type=int, default=60)
a = ap.parse_args()
t_end = time.time() + a.wait
while True:
    try:
        rc = once()
    except Exception as e:
        log("error:", type(e).__name__, e)
        rc = 1
    if rc == 0 or time.time() >= t_end:
        sys.exit(rc)
    time.sleep(a.interval)
