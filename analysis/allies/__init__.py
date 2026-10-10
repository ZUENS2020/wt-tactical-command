"""allies/ — squadmates, own/teammate map pings (标点) and radio calls from 8111 data.

Reference implementation (Python) for the Rust port in wt-collect/common/src/{mapobj,radio}.rs and
wt-collect/receiver/src/markers.rs. Evidence: ALLY_MARKERS_20261010.md (real RB ground data, HK receiver).
"""
from .classify import classify, ObjClass, SQUAD_COLORS, SIGNAL_RGB, FRESH_SPAWN_COLORS, find_self
from .radio import parse_chat, RadioMsg, ChatCursor
from .grid import cell_bounds, cell_of, GridInfo
from .tracker import FriendlyTracker, SignalEvent, FriendlyTrack
from .pings import PingFuser, MapPing
