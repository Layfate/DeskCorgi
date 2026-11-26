//! The mood state machine. It only decides *what* the dog is doing; `pet.rs` turns the
//! mood into spring targets. Pure logic with an explicit clock, so it is easy to test.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Idle,
    Typing,
    Excited,
    Chase,
    Zoomies,
    Pat,
    Scruff,
    Sleep,
}

/// What happened since the last frame.
#[derive(Clone, Copy, Debug, Default)]
pub struct Signals {
    pub key_pressed: bool,
    pub mouse_active: bool,
    pub wpm: f32,
    /// Cursor speed in canvas units per second.
    pub cursor_speed: f32,
    pub click_burst: bool,
    /// Distance the cursor travelled over the dog's head this frame (petting).
    pub stroke: f32,
    pub dragging: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct MoodConfig {
    pub chase: bool,
    pub zoomies: bool,
    pub sleep: bool,
    pub sleep_after: f64,
    /// Yawns now and then while idle, and just before a nap.
    pub yawn: bool,
    /// Tilts its head, puzzled, at a few quick Backspaces.
    pub huh: bool,
}

impl Default for MoodConfig {
    fn default() -> Self {
        MoodConfig { chase: true, zoomies: true, sleep: true, sleep_after: crate::config::SLEEP_AFTER_SECS, yawn: true, huh: true }
    }
}

pub const TYPING_HOLD: f64 = 1.5;
pub const EXCITED_WPM: f32 = 65.0;
pub const CHASE_SPEED: f32 = 1900.0;
const CHASE_BUILDUP: f32 = 0.3;
const CHASE_HOLD: f64 = 1.2;
const ZOOMIES_TIME: f64 = 1.3;
const ZOOMIES_COOLDOWN: f64 = 4.0;
const PAT_THRESHOLD: f32 = 55.0;
const PAT_HOLD: f64 = 0.9;

#[derive(Clone, Debug)]
pub struct MoodMachine {
    pub mood: Mood,
    /// When the current mood started.
    pub since: f64,
    last_key: f64,
    last_input: f64,
    fast_mouse: f32,
    chase_until: f64,
    zoom_until: f64,
    zoom_ready_at: f64,
    pat_energy: f32,
    pat_until: f64,
}

impl MoodMachine {
    pub fn new(now: f64) -> MoodMachine {
        MoodMachine {
            mood: Mood::Idle,
            since: now,
            last_key: f64::NEG_INFINITY,
            last_input: now,
            fast_mouse: 0.0,
            chase_until: f64::NEG_INFINITY,
            zoom_until: f64::NEG_INFINITY,
            zoom_ready_at: f64::NEG_INFINITY,
            pat_energy: 0.0,
            pat_until: f64::NEG_INFINITY,
        }
    }

    pub fn seconds_since_input(&self, now: f64) -> f64 {
        now - self.last_input
    }

    pub fn update(&mut self, now: f64, dt: f32, s: &Signals, cfg: &MoodConfig) -> Mood {
        if s.key_pressed {
            self.last_key = now;
        }
        if s.key_pressed || s.mouse_active || s.click_burst || s.stroke > 0.0 || s.dragging {
            self.last_input = now;
        }

        if cfg.chase && s.cursor_speed > CHASE_SPEED {
            self.fast_mouse += dt;
        } else {
            self.fast_mouse = (self.fast_mouse - dt * 2.0).max(0.0);
        }
        if self.fast_mouse >= CHASE_BUILDUP {
            self.chase_until = now + CHASE_HOLD;
        }
        if !cfg.chase {
            self.chase_until = f64::NEG_INFINITY;
        }

        if cfg.zoomies && s.click_burst && now >= self.zoom_ready_at {
            self.zoom_until = now + ZOOMIES_TIME;
            self.zoom_ready_at = now + ZOOMIES_COOLDOWN;
        }

        self.pat_energy = (self.pat_energy + s.stroke).min(PAT_THRESHOLD * 1.5);
        self.pat_energy = (self.pat_energy - dt * 60.0).max(0.0);
        if self.pat_energy >= PAT_THRESHOLD {
            self.pat_until = now + PAT_HOLD;
        }

        let typing = now - self.last_key < TYPING_HOLD;
        let next = if s.dragging {
            Mood::Scruff
        } else if now < self.zoom_until {
            Mood::Zoomies
        } else if now < self.pat_until {
            Mood::Pat
        } else if now < self.chase_until {
            Mood::Chase
        } else if typing && s.wpm >= EXCITED_WPM {
            Mood::Excited
        } else if typing {
            Mood::Typing
        } else if cfg.sleep && now - self.last_input > cfg.sleep_after {
            Mood::Sleep
        } else {
            Mood::Idle
        };
        if next != self.mood {
            self.mood = next;
            self.since = now;
        }
        self.mood
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DT: f32 = 1.0 / 30.0;

    fn run(m: &mut MoodMachine, t: &mut f64, secs: f64, s: Signals, cfg: &MoodConfig) -> Mood {
        let mut mood = m.mood;
        let steps = (secs / DT as f64).round() as usize;
        for _ in 0..steps.max(1) {
            *t += DT as f64;
            mood = m.update(*t, DT, &s, cfg);
        }
        mood
    }

    #[test]
    fn typing_then_back_to_idle() {
        let cfg = MoodConfig::default();
        let mut t = 0.0;
        let mut m = MoodMachine::new(t);
        let key = Signals { key_pressed: true, wpm: 30.0, ..Default::default() };
        assert_eq!(run(&mut m, &mut t, 0.1, key, &cfg), Mood::Typing);
        let fast = Signals { key_pressed: true, wpm: 90.0, ..Default::default() };
        assert_eq!(run(&mut m, &mut t, 0.1, fast, &cfg), Mood::Excited);
        assert_eq!(run(&mut m, &mut t, 1.0, Signals::default(), &cfg), Mood::Typing);
        assert_eq!(run(&mut m, &mut t, 1.0, Signals::default(), &cfg), Mood::Idle);
    }

    #[test]
    fn falls_asleep_and_wakes_up() {
        let cfg = MoodConfig { sleep_after: 10.0, ..Default::default() };
        let mut t = 0.0;
        let mut m = MoodMachine::new(t);
        assert_eq!(run(&mut m, &mut t, 9.0, Signals::default(), &cfg), Mood::Idle);
        assert_eq!(run(&mut m, &mut t, 2.0, Signals::default(), &cfg), Mood::Sleep);
        let wiggle = Signals { mouse_active: true, ..Default::default() };
        assert_eq!(run(&mut m, &mut t, 0.04, wiggle, &cfg), Mood::Idle);
        let no_sleep = MoodConfig { sleep: false, ..cfg };
        assert_eq!(run(&mut m, &mut t, 30.0, Signals::default(), &no_sleep), Mood::Idle);
    }

    #[test]
    fn chase_needs_sustained_fast_movement() {
        let cfg = MoodConfig::default();
        let mut t = 0.0;
        let mut m = MoodMachine::new(t);
        let fast = Signals { mouse_active: true, cursor_speed: 3000.0, ..Default::default() };
        let slow = Signals { mouse_active: true, cursor_speed: 400.0, ..Default::default() };
        // A short flick is not enough.
        assert_eq!(run(&mut m, &mut t, 0.1, fast, &cfg), Mood::Idle);
        assert_eq!(run(&mut m, &mut t, 0.3, slow, &cfg), Mood::Idle);
        assert_eq!(run(&mut m, &mut t, 0.4, fast, &cfg), Mood::Chase);
        assert_eq!(run(&mut m, &mut t, 0.5, slow, &cfg), Mood::Chase);
        assert_eq!(run(&mut m, &mut t, 1.5, slow, &cfg), Mood::Idle);
        let off = MoodConfig { chase: false, ..cfg };
        assert_eq!(run(&mut m, &mut t, 1.0, fast, &off), Mood::Idle);
    }

    #[test]
    fn zoomies_have_a_cooldown() {
        let cfg = MoodConfig::default();
        let mut t = 0.0;
        let mut m = MoodMachine::new(t);
        let burst = Signals { click_burst: true, mouse_active: true, ..Default::default() };
        assert_eq!(run(&mut m, &mut t, 0.04, burst, &cfg), Mood::Zoomies);
        assert_eq!(run(&mut m, &mut t, 1.5, Signals::default(), &cfg), Mood::Idle);
        // Still cooling down: another burst right away does nothing.
        assert_eq!(run(&mut m, &mut t, 0.04, burst, &cfg), Mood::Idle);
        run(&mut m, &mut t, 3.0, Signals::default(), &cfg);
        assert_eq!(run(&mut m, &mut t, 0.04, burst, &cfg), Mood::Zoomies);
    }

    #[test]
    fn petting_and_scruff_priority() {
        let cfg = MoodConfig::default();
        let mut t = 0.0;
        let mut m = MoodMachine::new(t);
        let stroke = Signals { stroke: 12.0, mouse_active: true, ..Default::default() };
        assert_eq!(run(&mut m, &mut t, 0.5, stroke, &cfg), Mood::Pat);
        let drag = Signals { dragging: true, ..Default::default() };
        assert_eq!(run(&mut m, &mut t, 0.04, drag, &cfg), Mood::Scruff);
        assert_eq!(run(&mut m, &mut t, 2.0, Signals::default(), &cfg), Mood::Idle);
        // A tiny accidental touch does not count as petting.
        let touch = Signals { stroke: 2.0, ..Default::default() };
        assert_eq!(run(&mut m, &mut t, 1.0, touch, &cfg), Mood::Idle);
    }
}
