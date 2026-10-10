"""Tencent Cloud helpers for wt-tactical-hk (credentials read from ~/.secrets, never printed)."""
import json, os, sys
from tencentcloud.common import credential
from tencentcloud.common.profile.client_profile import ClientProfile
from tencentcloud.common.profile.http_profile import HttpProfile
from tencentcloud.common.exception.tencent_cloud_sdk_exception import TencentCloudSDKException

def cred():
    r = lambda n: open(os.path.expanduser(f"~/.secrets/{n}")).read().strip()
    return credential.Credential(r("tx_root_secret_id"), r("tx_root_secret_key"))

def client(mod, region=""):
    if mod == "lighthouse":
        from tencentcloud.lighthouse.v20200324 import lighthouse_client as m
        return m.LighthouseClient(cred(), region or "ap-hongkong")
    if mod == "domain":
        from tencentcloud.domain.v20180808 import domain_client as m
        return m.DomainClient(cred(), region)
    if mod == "dnspod":
        from tencentcloud.dnspod.v20210323 import dnspod_client as m
        return m.DnspodClient(cred(), region)

def call(mod, action, params, region=""):
    c = client(mod, region)
    resp = c.call_json(action, params)
    return resp.get("Response", resp)
