//! Small 2D math helpers. Canvas coordinates have y pointing down.

use core::ops::{Add, AddAssign, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

pub const fn pt(x: f32, y: f32) -> Pt {
    Pt { x, y }
}

impl Pt {
    pub const ZERO: Pt = pt(0.0, 0.0);

    pub fn len(self) -> f32 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Rotates around the origin. Positive angles turn +x towards +y (clockwise on screen).
    pub fn rotate(self, angle: f32) -> Pt {
        let (s, c) = angle.sin_cos();
        pt(self.x * c - self.y * s, self.x * s + self.y * c)
    }

    pub fn lerp(self, other: Pt, t: f32) -> Pt {
        pt(self.x + (other.x - self.x) * t, self.y + (other.y - self.y) * t)
    }

    pub fn normalized(self) -> Pt {
        let l = self.len();
        if l > 1e-6 {
            pt(self.x / l, self.y / l)
        } else {
            Pt::ZERO
        }
    }

    /// Perpendicular vector (rotated +90°).
    pub fn perp(self) -> Pt {
        pt(-self.y, self.x)
    }

    pub fn clamp_len(self, max: f32) -> Pt {
        let l = self.len();
        if l > max && l > 1e-6 {
            self * (max / l)
        } else {
            self
        }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

impl Add for Pt {
    type Output = Pt;
    fn add(self, o: Pt) -> Pt {
        pt(self.x + o.x, self.y + o.y)
    }
}

impl AddAssign for Pt {
    fn add_assign(&mut self, o: Pt) {
        self.x += o.x;
        self.y += o.y;
    }
}

impl Sub for Pt {
    type Output = Pt;
    fn sub(self, o: Pt) -> Pt {
        pt(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f32> for Pt {
    type Output = Pt;
    fn mul(self, k: f32) -> Pt {
        pt(self.x * k, self.y * k)
    }
}

impl Neg for Pt {
    type Output = Pt;
    fn neg(self) -> Pt {
        pt(-self.x, -self.y)
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub fn clamp01(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = clamp01((x - e0) / (e1 - e0));
    t * t * (3.0 - 2.0 * t)
}

/// 2D affine transform: `x' = a·x + c·y + e`, `y' = b·x + d·y + f`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub e: f32,
    pub f: f32,
}

impl Affine {
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    pub fn translate(t: Pt) -> Affine {
        Affine { e: t.x, f: t.y, ..Affine::IDENTITY }
    }

    pub fn scale(sx: f32, sy: f32) -> Affine {
        Affine { a: sx, d: sy, ..Affine::IDENTITY }
    }

    pub fn rotate(angle: f32) -> Affine {
        let (s, c) = angle.sin_cos();
        Affine { a: c, b: s, c: -s, d: c, e: 0.0, f: 0.0 }
    }

    /// Affine map that sends `origin` + x·`x_axis` + y·`y_axis`.
    pub fn from_axes(origin: Pt, x_axis: Pt, y_axis: Pt) -> Affine {
        Affine { a: x_axis.x, b: x_axis.y, c: y_axis.x, d: y_axis.y, e: origin.x, f: origin.y }
    }

    /// `self ∘ other`: apply `other` first, then `self`.
    pub fn then_after(self, other: Affine) -> Affine {
        Affine {
            a: self.a * other.a + self.c * other.b,
            b: self.b * other.a + self.d * other.b,
            c: self.a * other.c + self.c * other.d,
            d: self.b * other.c + self.d * other.d,
            e: self.a * other.e + self.c * other.f + self.e,
            f: self.b * other.e + self.d * other.f + self.f,
        }
    }

    pub fn map(&self, p: Pt) -> Pt {
        pt(self.a * p.x + self.c * p.y + self.e, self.b * p.x + self.d * p.y + self.f)
    }

    /// Maps a direction (ignores translation).
    pub fn map_vec(&self, v: Pt) -> Pt {
        pt(self.a * v.x + self.c * v.y, self.b * v.x + self.d * v.y)
    }

    pub fn to_skia(self) -> tiny_skia::Transform {
        tiny_skia::Transform::from_row(self.a, self.b, self.c, self.d, self.e, self.f)
    }
}

/// Tiny deterministic xorshift RNG (no dependency, reproducible in tests).
#[derive(Clone, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Rng {
        Rng(seed.max(1))
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// Uniform in [0, 1).
    pub fn f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affine_composition_matches_sequential_mapping() {
        let a = Affine::translate(pt(10.0, -4.0)).then_after(Affine::rotate(0.3)).then_after(Affine::scale(2.0, 0.5));
        let p = pt(3.0, 7.0);
        let manual = (pt(p.x * 2.0, p.y * 0.5)).rotate(0.3) + pt(10.0, -4.0);
        let got = a.map(p);
        assert!((got - manual).len() < 1e-4, "{got:?} vs {manual:?}");
    }

    #[test]
    fn from_axes_maps_basis() {
        let a = Affine::from_axes(pt(1.0, 2.0), pt(3.0, 0.0), pt(0.0, -2.0));
        assert_eq!(a.map(pt(0.0, 0.0)), pt(1.0, 2.0));
        assert_eq!(a.map(pt(1.0, 0.0)), pt(4.0, 2.0));
        assert_eq!(a.map(pt(0.0, 1.0)), pt(1.0, 0.0));
    }

    #[test]
    fn rng_stays_in_range() {
        let mut r = Rng::new(7);
        for _ in 0..10_000 {
            let v = r.f32();
            assert!((0.0..1.0).contains(&v));
        }
    }
}
