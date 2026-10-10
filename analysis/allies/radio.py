"""/gamechat radio / quick-chat parsing. Keep in sync with common/src/radio.rs.

Real zh-CN client (2026-10-10, mode "友军" = team):
  ping (target = grid, NOT the sender):   注意战术地图！ [c4] · 注意该坐标位置！ [d4] · 注意当前标识区域！ [b3]
  sender location (grid = sender's cell): 请求掩护！/请求支援！/需要支援！/掩护我！ [e6] · 请求指示空袭目标 [a14, 高度 1000 米]
  no location: 谢谢你！/感谢！/装填中！ 00:32/空袭警报！/进攻 C 点！/防守 C 点！ ...
The sender's OWN messages come back without the grid suffix (confirmed: own "注意战术地图！" had no [..]).
Grid suffix is wrapped in <color=#AARRGGBB> ... </color> (seen FF00FF50, FF969650, FF50BE46, FF96966E; meaning unknown).
Text contains U+200B between CJK characters. English strings from wt-modern-8111 docs (Air SB) included.
"""
from dataclasses import dataclass
import re
from typing import Optional, Tuple

ZW = "\u200b"
OWN_NAMES = ("传奇耐摔王无声铃鹿",)

_TAG = re.compile(r"<[^>]*>")
_COLOR = re.compile(r"<color=#([0-9A-Fa-f]{6,8})>")
_GRID = re.compile(r"\[\s*([A-Za-z]{1,2})(\d{1,2})(?:\s*,\s*[^\]\d+-]*([+-]?\d+(?:\.\d+)?)\s*(?:m|米)?)?\s*\]")

# kind -> substrings (normalized: no zero-width, lower case)
PING_KINDS = (
    ("ping_map", ("注意战术地图", "attention to the map")),
    ("ping_grid", ("注意该坐标位置", "attention to the designated grid", "attention to designated grid")),
    ("ping_marked", ("注意当前标识区域", "attention to the marked area", "attention to the designated area")),
)
SENDER_KINDS = (
    ("air_target_request", ("请求指示空袭目标", "request air strike", "requesting air support")),
    ("need_cover", ("请求掩护", "掩护我", "cover me")),
    ("need_support", ("请求支援", "需要支援", "need backup", "need help", "help me")),
    ("follow_me", ("跟我来", "跟着我", "follow me", "guide on me", "move after me", "on me")),
)
OTHER_KINDS = (
    ("thanks", ("谢谢", "感谢", "thank")),
    ("reloading", ("装填",)),
    ("air_raid", ("空袭警报", "air raid", "enemy aircraft")),
    ("attack_point", ("进攻",)),
    ("defend_point", ("防守",)),
)


@dataclass(frozen=True)
class RadioMsg:
    id: int
    t_ms: int
    sender: str
    own: bool
    enemy: bool
    mode: str
    text: str                 # markup and U+200B removed
    kind: str                 # ping_* | need_cover | need_support | follow_me | air_target_request | thanks | ... | chat
    subject: Optional[str]    # "target" (ping), "sender" (location call), None
    grid: Optional[str]       # "E6" (upper case) or None
    alt_m: Optional[float] = None
    grid_color: Optional[str] = None
    backlog: bool = False     # history returned by the first poll (lastId=0) -> not live

    @property
    def is_ping(self) -> bool:
        return self.kind.startswith("ping_")


def clean(msg: str) -> str:
    return _TAG.sub("", msg.replace(ZW, "")).strip()


def classify_text(text: str) -> Tuple[str, Optional[str]]:
    n = _GRID.sub("", text).lower()
    for kind, pats in PING_KINDS:
        if any(p in n for p in pats):
            return kind, "target"
    for kind, pats in SENDER_KINDS:
        if any(p in n for p in pats):
            return kind, "sender"
    for kind, pats in OTHER_KINDS:
        if any(p in n for p in pats):
            return kind, None
    return "chat", None


def parse_chat(m: dict, t_ms: int, own_names=OWN_NAMES, backlog: bool = False) -> RadioMsg:
    raw = str(m.get("msg", ""))
    text = clean(raw)
    kind, subject = classify_text(text)
    g = _GRID.search(text)
    grid = (g.group(1) + g.group(2)).upper() if g else None
    alt = float(g.group(3)) if g and g.group(3) else None
    c = _COLOR.search(raw.replace(ZW, ""))
    sender = str(m.get("sender", "")).replace(ZW, "")
    own = any(n and n in sender for n in own_names)
    if grid and subject is None and kind == "chat":
        subject = "unknown"
    return RadioMsg(int(m.get("id", -1)), t_ms, sender, own, bool(m.get("enemy", False)),
                    str(m.get("mode", "")).replace(ZW, ""), text, kind, subject, grid, alt,
                    c.group(1).upper() if c else None, backlog)


class ChatCursor:
    """Dedup /gamechat polls by id and flag history: the first successful poll of a collector epoch
    asked with lastId=0 returns old messages (seen in df9b: 13 messages from before the restart)."""

    def __init__(self, own_names=OWN_NAMES):
        self.seen = set()
        self.polled_epochs = set()
        self.own_names = own_names

    def feed(self, url: str, epoch: int, t_ms: int, body):
        first = epoch not in self.polled_epochs
        self.polled_epochs.add(epoch)
        backlog = first and "lastId=0" in (url or "")
        out = []
        for m in body if isinstance(body, list) else []:
            if not isinstance(m, dict) or m.get("id") in self.seen:
                continue
            self.seen.add(m.get("id"))
            out.append(parse_chat(m, t_ms, self.own_names, backlog))
        return out
