//! Damped springs. Every moving part of the dog is a spring: the state machine only
//! sets targets and the springs make the motion smooth (same idea as KuroCat's CatPhysics).

use crate::geom::{pt, Pt};

/// Longest frame we simulate; anything longer (a hitch, sleep/resume) is clamped.
pub const MAX_DT: f32 = 0.1;
/// Internal integration step. Semi-implicit Euler is stable for these stiffnesses at this step.
const SUB_STEP: f32 = 1.0 / 240.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring {
    pub value: f32,
    pub velocity: f32,
    pub target: f32,
    pub stiffness: f32,
    pub damping: f32,
}

impl Spring {
    pub const fn new(value: f32, stiffness: f32, damping: f32) -> Spring {
        Spring { value, velocity: 0.0, target: value, stiffness, damping }
    }

    /// Smooth, slightly lazy motion (head turns, ears).
    pub const fn soft(value: f32) -> Spring {
        Spring::new(value, 140.0, 17.0)
    }

    /// Quick motion with a tiny overshoot (paws, presses).
    pub const fn snappy(value: f32) -> Spring {
        Spring::new(value, 800.0, 42.0)
    }

    /// Visible bounce (hops, ear flicks).
    pub const fn bouncy(value: f32) -> Spring {
        Spring::new(value, 260.0, 11.0)
    }

    pub fn step(&mut self, dt: f32) {
        let dt = if dt.is_finite() { dt.clamp(0.0, MAX_DT) } else { 0.0 };
        if dt <= 0.0 {
            return;
        }
        let n = (dt / SUB_STEP).ceil().max(1.0) as u32;
        let h = dt / n as f32;
        for _ in 0..n {
            let accel = (self.target - self.value) * self.stiffness - self.velocity * self.damping;
            self.velocity += accel * h;
            self.value += self.velocity * h;
        }
        if !self.value.is_finite() || !self.velocity.is_finite() {
            self.value = self.target;
            self.velocity = 0.0;
        }
    }

    pub fn kick(&mut self, impulse: f32) {
        self.velocity += impulse;
    }

    pub fn snap(&mut self, value: f32) {
        self.value = value;
        self.target = value;
        self.velocity = 0.0;
    }

    pub fn is_settled(&self) -> bool {
        (self.target - self.value).abs() < 1e-3 && self.velocity.abs() < 1e-2
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spring2 {
    pub x: Spring,
    pub y: Spring,
}

impl Spring2 {
    pub const fn new(p: Pt, stiffness: f32, damping: f32) -> Spring2 {
        Spring2 { x: Spring::new(p.x, stiffness, damping), y: Spring::new(p.y, stiffness, damping) }
    }

    pub fn value(&self) -> Pt {
        pt(self.x.value, self.y.value)
    }

    pub fn target(&self) -> Pt {
        pt(self.x.target, self.y.target)
    }

    pub fn set_target(&mut self, p: Pt) {
        self.x.target = p.x;
        self.y.target = p.y;
    }

    pub fn step(&mut self, dt: f32) {
        self.x.step(dt);
        self.y.step(dt);
    }

    pub fn snap(&mut self, p: Pt) {
        self.x.snap(p.x);
        self.y.snap(p.y);
    }

    pub fn is_settled(&self) -> bool {
        self.x.is_settled() && self.y.is_settled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converges_to_target() {
        for mut s in [Spring::soft(0.0), Spring::snappy(0.0), Spring::bouncy(0.0)] {
            s.target = 10.0;
            for _ in 0..600 {
                s.step(1.0 / 60.0);
            }
            assert!((s.value - 10.0).abs() < 0.01, "{s:?}");
        }
    }

    #[test]
    fn stable_at_low_and_irregular_frame_rates() {
        let mut s = Spring::snappy(0.0);
        s.target = 1.0;
        let frames = [0.1, 0.0, 0.5, 1.0 / 30.0, 0.1, 3.0, f32::NAN, 0.07];
        for _ in 0..50 {
            for dt in frames {
                s.step(dt);
                assert!(s.value.is_finite() && s.value.abs() < 5.0, "diverged: {s:?}");
            }
        }
        assert!((s.value - 1.0).abs() < 0.01);
    }

    #[test]
    fn huge_dt_is_clamped() {
        let mut a = Spring::soft(0.0);
        let mut b = Spring::soft(0.0);
        a.target = 5.0;
        b.target = 5.0;
        a.step(60.0);
        b.step(MAX_DT);
        assert_eq!(a, b);
    }
}
