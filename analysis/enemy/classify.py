"""Colour / icon rules. Keep in sync with the Rust server."""
from typing import Tuple

NAVAL_ICONS = frozenset({"BattleShip", "Boat", "Destroyer", "HeavyCruiser", "LightCruiser",
                         "Cruiser", "Ship", "Submarine", "TorpedoBoat", "Frigate"})

ICON_CLASS = {
    "LightTank": "tank_light", "MediumTank": "tank_medium", "HeavyTank": "tank_heavy",
    "TankDestroyer": "tank_destroyer", "SPAA": "spaa", "Wheeled": "wheeled",
    "Tracked": "tracked", "Airdefence": "air_defence", "SAM": "sam", "Structure": "structure",
    "Fighter": "air_fighter", "Bomber": "air_bomber", "Assault": "air_assault",
    "Helicopter": "air_helicopter", "Heli": "air_helicopter",
}

TRACKED_TYPES = frozenset({"ground_model", "aircraft"})


def is_enemy_rgb(rgb: Tuple[int, int, int]) -> bool:
    """Spec rule: r>=200 && g<130 && b<130. Covers #fa0C00, #f00C00, #fa3200, #ff0000."""
    r, g, b = rgb
    return r >= 200 and g < 130 and b < 130


def red_shade(color: str, rgb: Tuple[int, int, int]) -> str:
    """'fa' (250,12,0) vs 'f0' (240,12,0) vs other red. Raw shade is kept for analysis."""
    if rgb == (250, 12, 0):
        return "fa0C00"
    if rgb == (240, 12, 0):
        return "f00C00"
    return color.lstrip("#")


def is_naval(icon: str) -> bool:
    return icon in NAVAL_ICONS


def icon_class(type_: str, icon: str) -> str:
    c = ICON_CLASS.get(icon)
    if c:
        return c
    return ("air_" if type_ == "aircraft" else "ground_") + (icon.lower() or "unknown")


def is_trackable_enemy(o) -> bool:
    """Enemy unit that should become a contact. Excludes Player, objectives, airfields, naval."""
    if o.icon == "Player" or o.type not in TRACKED_TYPES:
        return False
    if o.x is None or o.y is None:
        return False
    if is_naval(o.icon):
        return False
    return is_enemy_rgb(o.rgb)


SELF_RGB = (250, 200, 30)     # #faC81E
SIGNAL_RGB = (4, 63, 255)     # #043FFF blink 2 (own radio message / ping)


def find_player(objs):
    """Own Player object. While dead/spectating the Player icon follows a teammate in #174DFF
    (confirmed 2026-10-10, ALLY_MARKERS_20261010.md) -> not self, return None (caller keeps last fix)."""
    for o in objs:
        if o.icon == "Player" and o.x is not None and (tuple(o.rgb) == SELF_RGB or (tuple(o.rgb) == SIGNAL_RGB and o.blink == 2)):
            return o
    return None


def is_own_spawn(o) -> bool:
    """Our team's respawn_base_* point (friendly blue #174DFF). Enemy bases are #fa0C00."""
    if not o.type.startswith("respawn_base") or o.x is None or o.y is None:
        return False
    r, g, b = o.rgb
    return b >= 200 and r < 130
