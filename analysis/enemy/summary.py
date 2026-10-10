from typing import List, Optional
from .model import Contact, ThreatSummary
from .geo import SECTORS, sector_index


def threat_summary(contacts: List[Contact], t_ms: int, include_air: bool = True,
                   stale_horizon_ms: int = 30000) -> ThreatSummary:
    """8-sector attack-direction histogram + nearest threat, relative to the player."""
    sec = [0] * 8
    vis = [0] * 8
    nearest: Optional[Contact] = None
    nv = ns = 0
    for c in contacts:
        if c.bearing_deg is None or c.distance_m is None:
            continue
        if not include_air and c.type == "aircraft":
            continue
        if not c.visible and c.age_ms > stale_horizon_ms:
            continue
        i = sector_index(c.bearing_deg)
        sec[i] += 1
        if c.visible:
            vis[i] += 1; nv += 1
            if nearest is None or c.distance_m < nearest.distance_m:
                nearest = c
        else:
            ns += 1
    dom = None
    if any(sec):
        m = max(sec)
        dom = SECTORS[sec.index(m)]
    return ThreatSummary(
        t_ms=t_ms, n_visible=nv, n_stale=ns, sectors=tuple(sec), sectors_visible=tuple(vis),
        nearest_id=nearest.contact_id if nearest else None,
        nearest_distance_m=nearest.distance_m if nearest else None,
        nearest_bearing_deg=nearest.bearing_deg if nearest else None,
        nearest_sector=SECTORS[sector_index(nearest.bearing_deg)] if nearest else None,
        dominant_sector=dom)
