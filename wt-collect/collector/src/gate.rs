//! Upload gate (0.2.0): in the hangar / loading screen the game API returns the same placeholder data
//! over and over (map_info `{"valid":false}`, map_obj `[]`, the 128x128 placeholder map.img). Uploading it
//! at 10 Hz is useless, so the collector then pauses telemetry upload:
//!  - one `#state` record {"state":"paused","reason":"hangar"|"game_down",...} when entering the pause,
//!  - a tiny `#heartbeat` every HEARTBEAT (30 s) while paused (collector alive + suppressed counters),
//!  - `#state` {"event":"vehicle_changed"} when the selected vehicle changes in the hangar,
//!  - on resume (map_obj not empty, or map_info valid=true, or game reachable again) a `#state`
//!    {"state":"active",...} record, then the latest record of every endpoint (if < 2 s old) and full rate.
//! Synthetic records (#meta/#clock/#note/#game/...) always pass. Labels never go through here.
use std::time::{Duration, Instant};

/// sha256 of the hangar/loading placeholder map.img (PNG 128x128, 1519 B; WTP dHash "no_map").
pub const PLACEHOLDER_IMG_SHA256: &str = "cfb99c04947c94c19d4c523c1e0fb3a2d71d403263fefcad5f4c4fc8485d07d0";
pub const HEARTBEAT: Duration = Duration::from_secs(30);
/// the pause condition must hold this long before uploads stop (end-of-battle frames still go out)
pub const ENTER_DELAY: Duration = Duration::from_secs(2);
/// game API unreachable this long => pause too
pub const DOWN_DELAY: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Hangar,
    GameDown,
}
impl Reason {
    pub fn key(self) -> &'static str {
        match self {
            Reason::Hangar => "hangar",
            Reason::GameDown => "game_down",
        }
    }
}

/// What the gate sees (from Shared/GameState).
#[derive(Debug, Clone, Default)]
pub struct Obs {
    pub map_valid: Option<bool>,
    /// objects in the latest map_obj frame (None = no frame yet)
    pub map_obj_len: Option<usize>,
    pub placeholder_img: bool,
    pub reachable: bool,
    /// time since the last successful game API response (None = never)
    pub down_for: Option<Duration>,
}

impl Obs {
    pub fn hangar(&self) -> bool {
        let empty = self.map_obj_len == Some(0);
        empty && (self.map_valid == Some(false) || (self.map_valid.is_none() && self.placeholder_img))
    }
    pub fn want_pause(&self) -> Option<Reason> {
        if self.hangar() && self.reachable {
            Some(Reason::Hangar)
        } else if !self.reachable && self.down_for.map(|d| d >= DOWN_DELAY).unwrap_or(true) {
            Some(Reason::GameDown)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Transition {
    Pause(Reason),
    Resume { reason: Reason, paused: Duration },
    /// paused, but the reason changed (hangar <-> game_down)
    Reason(Reason),
}

#[derive(Debug)]
pub struct Gate {
    pub paused: Option<(Reason, Instant)>,
    candidate: Option<(Reason, Instant)>,
    pub last_heartbeat: Option<Instant>,
}

impl Default for Gate {
    fn default() -> Self {
        Gate { paused: None, candidate: None, last_heartbeat: None }
    }
}

impl Gate {
    pub fn update(&mut self, o: &Obs, now: Instant) -> Option<Transition> {
        let want = o.want_pause();
        match (self.paused, want) {
            (None, None) => {
                self.candidate = None;
                None
            }
            (None, Some(r)) => {
                let since = match self.candidate {
                    Some((_, t)) => t,
                    None => {
                        self.candidate = Some((r, now));
                        now
                    }
                };
                self.candidate = Some((r, since));
                if now.duration_since(since) >= ENTER_DELAY {
                    self.paused = Some((r, now));
                    self.candidate = None;
                    self.last_heartbeat = Some(now);
                    Some(Transition::Pause(r))
                } else {
                    None
                }
            }
            (Some((r, t)), None) => {
                self.paused = None;
                self.candidate = None;
                Some(Transition::Resume { reason: r, paused: now.duration_since(t) })
            }
            (Some((r, t)), Some(r2)) if r2 != r => {
                self.paused = Some((r2, t));
                Some(Transition::Reason(r2))
            }
            _ => None,
        }
    }
    pub fn is_paused(&self) -> bool {
        self.paused.is_some()
    }
    pub fn heartbeat_due(&mut self, now: Instant) -> bool {
        if self.paused.is_none() {
            return false;
        }
        match self.last_heartbeat {
            Some(t) if now.duration_since(t) < HEARTBEAT => false,
            _ => {
                self.last_heartbeat = Some(now);
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hangar() -> Obs {
        Obs { map_valid: Some(false), map_obj_len: Some(0), placeholder_img: true, reachable: true, down_for: Some(Duration::ZERO) }
    }
    fn battle() -> Obs {
        Obs { map_valid: Some(true), map_obj_len: Some(25), placeholder_img: false, reachable: true, down_for: Some(Duration::ZERO) }
    }

    #[test]
    fn hangar_detection() {
        assert!(hangar().hangar());
        assert!(!battle().hangar());
        // Player already in map_obj but map_info still valid=false (spawn): not hangar
        assert!(!Obs { map_obj_len: Some(1), ..hangar() }.hangar());
        // map_info not seen yet, but placeholder image + empty map_obj
        assert!(Obs { map_valid: None, ..hangar() }.hangar());
        assert!(!Obs { map_valid: None, placeholder_img: false, ..hangar() }.hangar());
        // map_obj never seen: unknown -> not hangar
        assert!(!Obs { map_obj_len: None, ..hangar() }.hangar());
    }

    #[test]
    fn pause_after_delay_resume_immediately() {
        let t0 = Instant::now();
        let mut g = Gate::default();
        assert_eq!(g.update(&battle(), t0), None);
        assert_eq!(g.update(&hangar(), t0 + Duration::from_millis(100)), None);
        assert_eq!(g.update(&hangar(), t0 + Duration::from_millis(1500)), None);
        assert!(!g.is_paused());
        assert_eq!(g.update(&hangar(), t0 + Duration::from_millis(2200)), Some(Transition::Pause(Reason::Hangar)));
        assert!(g.is_paused());
        assert_eq!(g.update(&hangar(), t0 + Duration::from_secs(5)), None);
        // heartbeat every 30 s
        assert!(!g.heartbeat_due(t0 + Duration::from_secs(10)));
        assert!(g.heartbeat_due(t0 + Duration::from_secs(33)));
        assert!(!g.heartbeat_due(t0 + Duration::from_secs(34)));
        // battle map appears -> resume at once
        match g.update(&Obs { map_valid: Some(false), map_obj_len: Some(3), ..hangar() }, t0 + Duration::from_secs(40)) {
            Some(Transition::Resume { reason: Reason::Hangar, paused }) => assert!(paused >= Duration::from_secs(37)),
            o => panic!("{o:?}"),
        }
        assert!(!g.is_paused());
        assert!(!g.heartbeat_due(t0 + Duration::from_secs(100)));
    }

    #[test]
    fn short_hangar_blip_does_not_pause() {
        let t0 = Instant::now();
        let mut g = Gate::default();
        g.update(&hangar(), t0);
        g.update(&battle(), t0 + Duration::from_millis(1000));
        assert_eq!(g.update(&hangar(), t0 + Duration::from_millis(2500)), None);
        assert!(!g.is_paused());
    }

    #[test]
    fn game_down() {
        let t0 = Instant::now();
        let mut g = Gate::default();
        let down = |s| Obs { reachable: false, down_for: Some(Duration::from_secs(s)), ..battle() };
        assert_eq!(g.update(&down(3), t0), None);
        g.update(&down(20), t0 + Duration::from_secs(1));
        assert_eq!(g.update(&down(21), t0 + Duration::from_secs(4)), Some(Transition::Pause(Reason::GameDown)));
        // game comes back, in hangar -> reason changes, stays paused
        assert_eq!(g.update(&hangar(), t0 + Duration::from_secs(5)), Some(Transition::Reason(Reason::Hangar)));
        assert!(matches!(g.update(&battle(), t0 + Duration::from_secs(6)), Some(Transition::Resume { .. })));
        // never reachable since start counts as down
        assert_eq!(Obs { reachable: false, down_for: None, ..battle() }.want_pause(), Some(Reason::GameDown));
    }
}
