"""Enemy-contact parsing for War Thunder /map_obj.json (8111/9222 API).

Pure, deterministic, dependency-free (stdlib only) so it ports 1:1 to Rust:
  - model.py     plain data types (map to Rust structs/enums)
  - classify.py  colour / icon / naval rules
  - geo.py       normalized <-> world, distance, compass bearing, sectors
  - tracker.py   gated nearest-neighbour tracker (no stable IDs from the game)
  - reveal.py    reveal_kind inference rules (scouted / hit_revealed / ...)
  - summary.py   attack-direction summary relative to the player
  - stream.py    analysis-only helpers that turn collector records into frames
Time is always passed in explicitly (ms); nothing reads a clock.
"""
from .model import MapInfo, MapObj, Frame, Contact, ThreatSummary, HudEvent
from .tracker import EnemyTracker, TrackerConfig
from .summary import threat_summary

__all__ = ["MapInfo", "MapObj", "Frame", "Contact", "ThreatSummary", "HudEvent",
           "EnemyTracker", "TrackerConfig", "threat_summary"]
