//! Input events coming from the platform layer, and small counters built on them.
//! Nothing here stores which keys were typed: only "this cell is down" and timestamps.

use crate::keymap::KeyId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InputEvent {
    KeyDown(KeyId),
    KeyUp(KeyId),
    /// Relative mouse motion in device counts (works in games where the cursor is locked).
    MouseMove { dx: f32, dy: f32 },
    MouseButton { button: MouseButton, down: bool },
    Wheel(f32),
    /// Forget every held key and button (focus loss, lock screen, resume).
    ReleaseAll,
}

/// Counts events inside a sliding time window.
#[derive(Clone, Debug)]
pub struct RateCounter {
    stamps: [f64; 64],
    head: usize,
    len: usize,
}

impl Default for RateCounter {
    fn default() -> Self {
        RateCounter { stamps: [0.0; 64], head: 0, len: 0 }
    }
}

impl RateCounter {
    pub fn push(&mut self, t: f64) {
        self.stamps[self.head] = t;
        self.head = (self.head + 1) % self.stamps.len();
        self.len = (self.len + 1).min(self.stamps.len());
    }

    /// Number of events at or after `since`.
    pub fn count_since(&self, since: f64) -> usize {
        let n = self.stamps.len();
        (0..self.len).filter(|i| self.stamps[(self.head + n - 1 - i) % n] >= since).count()
    }

    pub fn clear(&mut self) {
        self.len = 0;
    }
}

/// Typing speed in words per minute (5 key presses = 1 word), measured over the last few seconds.
pub fn wpm(counter: &RateCounter, now: f64) -> f32 {
    const WINDOW: f64 = 6.0;
    let presses = counter.count_since(now - WINDOW) as f32;
    presses / 5.0 * (60.0 / WINDOW as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_counter_window() {
        let mut c = RateCounter::default();
        for i in 0..10 {
            c.push(i as f64);
        }
        assert_eq!(c.count_since(5.0), 5);
        assert_eq!(c.count_since(-1.0), 10);
        assert_eq!(c.count_since(100.0), 0);
    }

    #[test]
    fn rate_counter_wraps() {
        let mut c = RateCounter::default();
        for i in 0..200 {
            c.push(i as f64);
        }
        assert_eq!(c.count_since(190.0), 10);
        assert_eq!(c.count_since(0.0), 64);
    }

    #[test]
    fn wpm_of_steady_typing() {
        let mut c = RateCounter::default();
        // 5 presses per second for 6 seconds = 60 words per minute.
        for i in 0..30 {
            c.push(i as f64 * 0.2);
        }
        let w = wpm(&c, 6.0);
        assert!((w - 60.0).abs() < 2.1, "{w}");
    }
}
