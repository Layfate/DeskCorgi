//! The dog's drawing as data: `art/corgi.toml`.
//!
//! Every hand-drawn shape (head outline, face markings, muzzle, ears, nose, torso, chest fur…)
//! is a *part*: SVG path data in the part's own space (see `art/README.md`). The renderer
//! still owns everything that moves or is generated (legs, eyes, mouth, tail, keyboard), and
//! the skin still owns the colours. Editing the art file never needs a recompile: the
//! `art` example renders and checks it straight from disk, and the app reloads it live when
//! `DESKCORGI_ART` points at it.
//!
//! **2.5D:** a part may carry a `turn` keyframe: the same path drawn with the head turned to
//! the viewer's right by [`TURN_YAW`]. The renderer blends front → turn by how far the head
//! is turned, and mirrors the blend for turns to the left. Both paths must have the same
//! number of segments per subpath (all commands are converted to cubics first, so a line in
//! one may be a curve in the other).

use std::sync::OnceLock;

use serde::Deserialize;
use tiny_skia::{Path, PathBuilder};

use crate::geom::{pt, Pt};

pub const FORMAT_VERSION: i64 = 1;
/// Head yaw (radians) that a `turn` keyframe is drawn at: the most the head ever turns.
pub const TURN_YAW: f32 = 0.42;
/// Default tessellation step in canvas units (before the global scale).
pub const STEP: f32 = 2.5;

/// One subpath as absolute cubic segments `(c1, c2, end)`.
#[derive(Clone, Debug, PartialEq)]
pub struct SubPath {
    pub start: Pt,
    pub segs: Vec<[Pt; 3]>,
    pub closed: bool,
}

/// A path ready to map, draw or tessellate.
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub subs: Vec<SubPath>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub id: String,
    pub front: Shape,
    pub turn: Option<Shape>,
    /// The front view is meant to be mirror-symmetric about x = 0 (checked by the art tool).
    pub symmetric: bool,
}

/// Line weights and shading strengths. Every field can be set in the art file's `[style]`.
#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    /// Outline width of the head, ears and tail.
    pub head_ink: f32,
    pub ear_ink: f32,
    pub tail_ink: f32,
    /// Extra outline at the bottom of the head (a drop-shadowed, hand-inked look). 0 = even line.
    pub head_ink_drop: f32,
    /// How much darker the head gets from crown to jaw (0 = flat colour).
    pub head_shade: f32,
    /// Strength of the soft shadow the head casts on the chest.
    pub chin_shadow: f32,
    /// Strength of the light rim along the top of the head.
    pub rim_light: f32,
    /// Opacity of the crown highlight.
    pub crown_shine: f32,
    /// How far the ears' inner fur shades towards the ear colour at the base (0..1).
    pub ear_inner_fade: f32,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            head_ink: 2.5,
            ear_ink: 2.3,
            tail_ink: 2.2,
            head_ink_drop: 0.0,
            head_shade: 0.0,
            chin_shadow: 0.0,
            rim_light: 0.0,
            crown_shine: 0.18,
            ear_inner_fade: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Art {
    pub parts: Vec<Part>,
    pub style: Style,
}

/// Parts the renderer needs. An art file without one of these is rejected.
pub const REQUIRED: &[&str] = &[
    "head",
    "face.blaze",
    "face.urajiro",
    "muzzle",
    "blush",
    "brow",
    "ear.outer",
    "ear.inner",
    "ear.tuft",
    "ear.fur",
    "nose",
    "nose.shine",
    "torso",
    "chest.ruff",
    "chest.bib",
];

/// The built-in art, compiled into the app.
pub fn builtin() -> &'static Art {
    static ART: OnceLock<Art> = OnceLock::new();
    ART.get_or_init(|| parse_art(include_str!("../../../art/corgi.toml")).unwrap_or_else(|e| panic!("art/corgi.toml: {e}")))
}

impl Art {
    pub fn part(&self, id: &str) -> &Part {
        // Required parts are checked at parse time; this only fails for a typo in the renderer.
        self.parts.iter().find(|p| p.id == id).unwrap_or_else(|| panic!("art part \"{id}\" is missing"))
    }

    /// The part `id` at turn `t` (-1 = fully turned left, 0 = front, 1 = fully right).
    pub fn at(&self, id: &str, t: f32) -> Shape {
        self.part(id).at(t)
    }
}

impl Part {
    pub fn at(&self, t: f32) -> Shape {
        let t = if t.is_finite() { t.clamp(-1.0, 1.0) } else { 0.0 };
        let Some(turn) = &self.turn else { return self.front.clone() };
        if t.abs() < 1e-4 {
            return self.front.clone();
        }
        let s = self.front.lerp(turn, t.abs());
        if t < 0.0 {
            s.map_ctrl(|q| pt(-q.x, q.y))
        } else {
            s
        }
    }
}

impl Shape {
    fn lerp(&self, other: &Shape, t: f32) -> Shape {
        let subs = self
            .subs
            .iter()
            .zip(&other.subs)
            .map(|(a, b)| SubPath {
                start: a.start.lerp(b.start, t),
                segs: a.segs.iter().zip(&b.segs).map(|(x, y)| [0, 1, 2].map(|k| x[k].lerp(y[k], t))).collect(),
                closed: a.closed,
            })
            .collect();
        Shape { subs }
    }

    /// Moves every end and control point (exact for affine maps and small deformations).
    pub fn map_ctrl(mut self, f: impl Fn(Pt) -> Pt) -> Shape {
        for s in &mut self.subs {
            s.start = f(s.start);
            for seg in &mut s.segs {
                *seg = seg.map(&f);
            }
        }
        self
    }

    /// The exact cubic path (use after an affine [`Shape::map_ctrl`]).
    pub fn to_path(&self) -> Option<Path> {
        let mut pb = PathBuilder::new();
        for s in &self.subs {
            if !s.start.is_finite() {
                return None;
            }
            pb.move_to(s.start.x, s.start.y);
            for [c1, c2, e] in &s.segs {
                pb.cubic_to(c1.x, c1.y, c2.x, c2.y, e.x, e.y);
            }
            if s.closed {
                pb.close();
            }
        }
        pb.finish()
    }

    /// Each subpath as points about `step` apart (closed ones without a repeated end point).
    pub fn polys(&self, step: f32) -> Vec<Vec<Pt>> {
        self.subs
            .iter()
            .map(|s| {
                let mut out = vec![s.start];
                let mut p0 = s.start;
                for &[c1, c2, p3] in &s.segs {
                    let len = (c1 - p0).len() + (c2 - c1).len() + (p3 - c2).len();
                    let n = ((len / step.max(0.1)).ceil() as usize).clamp(1, 64);
                    for i in 1..=n {
                        out.push(cubic(p0, c1, c2, p3, i as f32 / n as f32));
                    }
                    p0 = p3;
                }
                if s.closed && out.len() > 1 && (out[0] - out[out.len() - 1]).len() < 1e-3 {
                    out.pop();
                }
                out
            })
            .collect()
    }

    /// The first subpath as points ([`STEP`] apart).
    pub fn outline(&self) -> Vec<Pt> {
        self.polys(STEP).into_iter().next().unwrap_or_default()
    }

    /// Bounding box of the end and control points.
    pub fn bounds(&self) -> (Pt, Pt) {
        let mut lo = pt(f32::MAX, f32::MAX);
        let mut hi = pt(f32::MIN, f32::MIN);
        for s in &self.subs {
            for q in std::iter::once(&s.start).chain(s.segs.iter().flatten()) {
                lo = pt(lo.x.min(q.x), lo.y.min(q.y));
                hi = pt(hi.x.max(q.x), hi.y.max(q.y));
            }
        }
        (lo, hi)
    }
}

pub fn cubic(p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: f32) -> Pt {
    let u = 1.0 - t;
    p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
}

// ---------------------------------------------------------------------------------------------
// SVG path data
// ---------------------------------------------------------------------------------------------

struct Tokens<'a> {
    s: &'a [u8],
    i: usize,
}

impl Tokens<'_> {
    fn skip_sep(&mut self) {
        while self.i < self.s.len() && (self.s[self.i].is_ascii_whitespace() || self.s[self.i] == b',') {
            self.i += 1;
        }
    }

    fn peek_command(&mut self) -> Option<u8> {
        self.skip_sep();
        self.s.get(self.i).copied().filter(|c| c.is_ascii_alphabetic() && *c != b'e' && *c != b'E')
    }

    fn at_number(&mut self) -> bool {
        self.skip_sep();
        self.s.get(self.i).is_some_and(|c| c.is_ascii_digit() || matches!(c, b'-' | b'+' | b'.'))
    }

    fn number(&mut self) -> Result<f32, String> {
        self.skip_sep();
        let start = self.i;
        let s = self.s;
        let mut i = self.i;
        if i < s.len() && matches!(s[i], b'-' | b'+') {
            i += 1;
        }
        let mut dot = false;
        while i < s.len() && (s[i].is_ascii_digit() || (s[i] == b'.' && !dot)) {
            dot |= s[i] == b'.';
            i += 1;
        }
        if i < s.len() && matches!(s[i], b'e' | b'E') {
            i += 1;
            if i < s.len() && matches!(s[i], b'-' | b'+') {
                i += 1;
            }
            while i < s.len() && s[i].is_ascii_digit() {
                i += 1;
            }
        }
        let text = std::str::from_utf8(&s[start..i]).unwrap_or("");
        let v: f32 = text.parse().map_err(|_| format!("expected a number at character {start}"))?;
        if !v.is_finite() || v.abs() > 1000.0 {
            return Err(format!("number out of range at character {start}"));
        }
        self.i = i;
        Ok(v)
    }

    fn point(&mut self) -> Result<Pt, String> {
        Ok(pt(self.number()?, self.number()?))
    }
}

fn line_seg(a: Pt, b: Pt) -> [Pt; 3] {
    [a.lerp(b, 1.0 / 3.0), a.lerp(b, 2.0 / 3.0), b]
}

/// Parses SVG path data (`M L H V C S Q T A Z`, absolute and relative) into cubics.
pub fn parse_path(d: &str) -> Result<Shape, String> {
    let mut t = Tokens { s: d.as_bytes(), i: 0 };
    let mut subs: Vec<SubPath> = Vec::new();
    let mut cur = Pt::ZERO;
    let mut start = Pt::ZERO;
    // Reflection points for S and T.
    let mut last_c2: Option<Pt> = None;
    let mut last_q: Option<Pt> = None;
    let mut cmd: Option<u8> = None;
    loop {
        let c = match t.peek_command() {
            Some(c) => {
                t.i += 1;
                c
            }
            None if t.i >= t.s.len() => break,
            None => match cmd {
                // Repeated parameters reuse the previous command (M becomes L).
                Some(b'M') if t.at_number() => b'L',
                Some(b'm') if t.at_number() => b'l',
                Some(c) if t.at_number() => c,
                _ => return Err(format!("unexpected character at {}", t.i)),
            },
        };
        cmd = Some(c);
        let rel = c.is_ascii_lowercase();
        let base = if rel { cur } else { Pt::ZERO };
        let need_sub = |subs: &Vec<SubPath>| subs.last().map_or(true, |s| s.closed);
        let (mut c2_next, mut q_next) = (None, None);
        match c.to_ascii_uppercase() {
            b'M' => {
                cur = base + t.point()?;
                start = cur;
                subs.push(SubPath { start: cur, segs: Vec::new(), closed: false });
            }
            b'Z' => {
                if let Some(s) = subs.last_mut() {
                    if (cur - start).len() > 1e-4 {
                        s.segs.push(line_seg(cur, start));
                    }
                    s.closed = true;
                }
                cur = start;
            }
            other => {
                if need_sub(&subs) {
                    // Drawing after Z (or without M) starts a new subpath at the current point.
                    subs.push(SubPath { start: cur, segs: Vec::new(), closed: false });
                    start = cur;
                }
                let seg: Vec<[Pt; 3]> = match other {
                    b'L' => vec![line_seg(cur, base + t.point()?)],
                    b'H' => {
                        let x = t.number()? + if rel { cur.x } else { 0.0 };
                        vec![line_seg(cur, pt(x, cur.y))]
                    }
                    b'V' => {
                        let y = t.number()? + if rel { cur.y } else { 0.0 };
                        vec![line_seg(cur, pt(cur.x, y))]
                    }
                    b'C' => {
                        let (c1, c2, e) = (base + t.point()?, base + t.point()?, base + t.point()?);
                        c2_next = Some(c2);
                        vec![[c1, c2, e]]
                    }
                    b'S' => {
                        let c1 = last_c2.map_or(cur, |p| cur + (cur - p));
                        let (c2, e) = (base + t.point()?, base + t.point()?);
                        c2_next = Some(c2);
                        vec![[c1, c2, e]]
                    }
                    b'Q' => {
                        let (q, e) = (base + t.point()?, base + t.point()?);
                        q_next = Some(q);
                        vec![quad_seg(cur, q, e)]
                    }
                    b'T' => {
                        let q = last_q.map_or(cur, |p| cur + (cur - p));
                        let e = base + t.point()?;
                        q_next = Some(q);
                        vec![quad_seg(cur, q, e)]
                    }
                    b'A' => {
                        let (rx, ry, rot) = (t.number()?, t.number()?, t.number()?);
                        let (large, sweep) = (t.number()? != 0.0, t.number()? != 0.0);
                        let e = base + t.point()?;
                        arc_segs(cur, rx, ry, rot, large, sweep, e)
                    }
                    _ => return Err(format!("unsupported command '{}'", other as char)),
                };
                if let Some(last) = seg.last() {
                    cur = last[2];
                }
                if let Some(s) = subs.last_mut() {
                    s.segs.extend(seg);
                }
            }
        }
        last_c2 = c2_next;
        last_q = q_next;
    }
    subs.retain(|s| !s.segs.is_empty());
    if subs.is_empty() {
        return Err("the path draws nothing".into());
    }
    Ok(Shape { subs })
}

fn quad_seg(a: Pt, q: Pt, b: Pt) -> [Pt; 3] {
    [a + (q - a) * (2.0 / 3.0), b + (q - b) * (2.0 / 3.0), b]
}

/// SVG elliptical arc → cubics (SVG 1.1 implementation notes, F.6.5).
fn arc_segs(p0: Pt, rx: f32, ry: f32, rot_deg: f32, large: bool, sweep: bool, p1: Pt) -> Vec<[Pt; 3]> {
    if (p1 - p0).len() < 1e-5 {
        return Vec::new();
    }
    let (mut rx, mut ry) = (rx.abs(), ry.abs());
    if rx < 1e-5 || ry < 1e-5 {
        return vec![line_seg(p0, p1)];
    }
    let (s, c) = rot_deg.to_radians().sin_cos();
    let d = (p0 - p1) * 0.5;
    let x1 = c * d.x + s * d.y;
    let y1 = -s * d.x + c * d.y;
    let lambda = (x1 * x1) / (rx * rx) + (y1 * y1) / (ry * ry);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let num = rx * rx * ry * ry - rx * rx * y1 * y1 - ry * ry * x1 * x1;
    let den = rx * rx * y1 * y1 + ry * ry * x1 * x1;
    let coef = (num / den).max(0.0).sqrt() * if large == sweep { -1.0 } else { 1.0 };
    let cxp = coef * rx * y1 / ry;
    let cyp = -coef * ry * x1 / rx;
    let mid = (p0 + p1) * 0.5;
    let center = pt(c * cxp - s * cyp + mid.x, s * cxp + c * cyp + mid.y);
    let angle = |u: Pt, v: Pt| (u.x * v.y - u.y * v.x).atan2(u.x * v.x + u.y * v.y);
    let u = pt((x1 - cxp) / rx, (y1 - cyp) / ry);
    let v = pt((-x1 - cxp) / rx, (-y1 - cyp) / ry);
    let theta = angle(pt(1.0, 0.0), u);
    let mut dtheta = angle(u, v);
    let tau = std::f32::consts::TAU;
    if !sweep && dtheta > 0.0 {
        dtheta -= tau;
    } else if sweep && dtheta < 0.0 {
        dtheta += tau;
    }
    let n = (dtheta.abs() / (tau / 4.0)).ceil().max(1.0) as usize;
    let step = dtheta / n as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let at = |a: f32| {
        let (sa, ca) = a.sin_cos();
        let e = center + pt(c * rx * ca - s * ry * sa, s * rx * ca + c * ry * sa);
        let de = pt(-c * rx * sa - s * ry * ca, -s * rx * sa + c * ry * ca);
        (e, de)
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let a0 = theta + step * i as f32;
        let a1 = a0 + step;
        let (e0, d0) = at(a0);
        let (e1, d1) = at(a1);
        let end = if i + 1 == n { p1 } else { e1 };
        out.push([e0 + d0 * k, e1 - d1 * k, end]);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// The art file
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArt {
    format_version: Option<i64>,
    #[serde(default)]
    style: RawStyle,
    #[serde(default)]
    part: Vec<RawPart>,
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct RawStyle {
    head_ink: Option<f32>,
    ear_ink: Option<f32>,
    tail_ink: Option<f32>,
    head_ink_drop: Option<f32>,
    head_shade: Option<f32>,
    chin_shadow: Option<f32>,
    rim_light: Option<f32>,
    crown_shine: Option<f32>,
    ear_inner_fade: Option<f32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPart {
    id: String,
    d: String,
    #[serde(default)]
    turn: Option<String>,
    #[serde(default)]
    symmetric: bool,
    /// Free text for whoever edits the file next.
    #[serde(default)]
    #[allow(dead_code)]
    note: Option<String>,
}

fn same_structure(a: &Shape, b: &Shape) -> Result<(), String> {
    if a.subs.len() != b.subs.len() {
        return Err(format!("turn has {} subpaths but d has {}", b.subs.len(), a.subs.len()));
    }
    for (i, (x, y)) in a.subs.iter().zip(&b.subs).enumerate() {
        if x.segs.len() != y.segs.len() {
            return Err(format!("subpath {i}: turn has {} segments but d has {} (draw both with the same steps)", y.segs.len(), x.segs.len()));
        }
        if x.closed != y.closed {
            return Err(format!("subpath {i}: closed in one path but not the other"));
        }
    }
    Ok(())
}

pub fn parse_art(text: &str) -> Result<Art, String> {
    let raw: RawArt = toml::from_str(text).map_err(|e| format!("not a valid art file: {e}"))?;
    match raw.format_version {
        Some(FORMAT_VERSION) => {}
        Some(v) => return Err(format!("format_version must be {FORMAT_VERSION}, got {v}")),
        None => return Err("format_version is missing".into()),
    }
    let d = Style::default();
    let r = raw.style;
    let num = |name: &str, v: Option<f32>, def: f32, lo: f32, hi: f32| -> Result<f32, String> {
        match v {
            None => Ok(def),
            Some(x) if x.is_finite() && (lo..=hi).contains(&x) => Ok(x),
            Some(x) => Err(format!("style.{name} = {x} is outside {lo}..{hi}")),
        }
    };
    let style = Style {
        head_ink: num("head_ink", r.head_ink, d.head_ink, 0.0, 8.0)?,
        ear_ink: num("ear_ink", r.ear_ink, d.ear_ink, 0.0, 8.0)?,
        tail_ink: num("tail_ink", r.tail_ink, d.tail_ink, 0.0, 8.0)?,
        head_ink_drop: num("head_ink_drop", r.head_ink_drop, d.head_ink_drop, 0.0, 4.0)?,
        head_shade: num("head_shade", r.head_shade, d.head_shade, 0.0, 1.0)?,
        chin_shadow: num("chin_shadow", r.chin_shadow, d.chin_shadow, 0.0, 1.0)?,
        rim_light: num("rim_light", r.rim_light, d.rim_light, 0.0, 1.0)?,
        crown_shine: num("crown_shine", r.crown_shine, d.crown_shine, 0.0, 1.0)?,
        ear_inner_fade: num("ear_inner_fade", r.ear_inner_fade, d.ear_inner_fade, 0.0, 1.0)?,
    };
    let mut parts: Vec<Part> = Vec::with_capacity(raw.part.len());
    for p in raw.part {
        if parts.iter().any(|q| q.id == p.id) {
            return Err(format!("part \"{}\" is defined twice", p.id));
        }
        let front = parse_path(&p.d).map_err(|e| format!("part \"{}\" d: {e}", p.id))?;
        let turn = match &p.turn {
            Some(t) => {
                let s = parse_path(t).map_err(|e| format!("part \"{}\" turn: {e}", p.id))?;
                same_structure(&front, &s).map_err(|e| format!("part \"{}\": {e}", p.id))?;
                Some(s)
            }
            None => None,
        };
        parts.push(Part { id: p.id, front, turn, symmetric: p.symmetric });
    }
    for id in REQUIRED {
        if !parts.iter().any(|p| p.id == *id) {
            return Err(format!("part \"{id}\" is missing"));
        }
    }
    Ok(Art { parts, style })
}

/// Ellipse path data, handy in tests and tools.
pub fn ellipse_d(c: Pt, rx: f32, ry: f32) -> String {
    format!("M {} {} A {rx} {ry} 0 1 0 {} {} A {rx} {ry} 0 1 0 {} {} Z", c.x - rx, c.y, c.x + rx, c.y, c.x - rx, c.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_art_parses() {
        let art = builtin();
        for id in REQUIRED {
            assert!(!art.at(id, 0.0).outline().is_empty(), "{id}");
        }
    }

    /// Turns to the left are the mirror image of turns to the right, which only joins up at
    /// the front view if the front view is symmetric.
    #[test]
    fn builtin_turning_parts_are_symmetric_at_the_front() {
        for part in builtin().parts.iter().filter(|p| p.turn.is_some() && !p.id.starts_with("ear.")) {
            assert!(part.symmetric, "{} has a turn keyframe, so it must be symmetric", part.id);
            let (lo, hi) = part.front.bounds();
            assert!((lo.x + hi.x).abs() < 0.5, "{}: front view is off-centre ({} .. {})", part.id, lo.x, hi.x);
        }
    }

    #[test]
    fn path_commands_agree() {
        let abs = parse_path("M 0 0 L 10 0 L 10 10 Z").unwrap();
        let rel = parse_path("m 0 0 l 10 0 v 10 z").unwrap();
        assert_eq!(abs, rel);
        assert!(abs.subs[0].closed);
        assert_eq!(abs.subs[0].segs.len(), 3);
        // Implicit lineto after moveto.
        assert_eq!(parse_path("M0,0 10,0 10,10z").unwrap(), abs);
    }

    #[test]
    fn arcs_make_round_ellipses() {
        let s = parse_path(&ellipse_d(pt(5.0, 5.0), 10.0, 4.0)).unwrap();
        for q in s.outline() {
            let e = ((q.x - 5.0) / 10.0).powi(2) + ((q.y - 5.0) / 4.0).powi(2);
            assert!((e - 1.0).abs() < 0.01, "{q:?} {e}");
        }
    }

    #[test]
    fn turn_blends_and_mirrors() {
        let text = format!(
            "format_version = 1\n{}\n[[part]]\nid = \"x\"\nd = \"M -10 0 L 10 0 L 0 10 Z\"\nturn = \"M -6 0 L 14 0 C 10 4 6 8 4 10 Z\"\n",
            REQUIRED.iter().map(|id| format!("[[part]]\nid = \"{id}\"\nd = \"M 0 0 L 1 0 L 0 1 Z\"\n")).collect::<String>()
        );
        let art = parse_art(&text).unwrap();
        assert_eq!(art.at("x", 0.0).subs[0].start, pt(-10.0, 0.0));
        assert_eq!(art.at("x", 1.0).subs[0].start, pt(-6.0, 0.0));
        assert_eq!(art.at("x", 0.5).subs[0].start, pt(-8.0, 0.0));
        assert_eq!(art.at("x", -1.0).subs[0].start, pt(6.0, 0.0));
    }

    #[test]
    fn bad_files_have_clear_errors() {
        let ok_parts: String = REQUIRED.iter().map(|id| format!("[[part]]\nid = \"{id}\"\nd = \"M 0 0 L 1 0 L 0 1 Z\"\n")).collect();
        let cases = [
            (ok_parts.clone(), "format_version is missing"),
            (format!("format_version = 1\n[style]\nhead_inc = 2\n{ok_parts}"), "head_inc"),
            (format!("format_version = 1\n[style]\nhead_ink = 99\n{ok_parts}"), "outside"),
            ("format_version = 1\n".to_string(), "is missing"),
            (format!("format_version = 1\n{ok_parts}[[part]]\nid = \"x\"\nd = \"M 0 0 L 1 0\"\nturn = \"M 0 0 L 1 0 L 2 2\"\n"), "segments"),
            (format!("format_version = 1\n{ok_parts}[[part]]\nid = \"x\"\nd = \"M 0 0 X 1 0\"\n"), "part \"x\" d"),
            (format!("format_version = 1\n{ok_parts}[[part]]\nid = \"head\"\nd = \"M 0 0 L 1 0\"\n"), "twice"),
        ];
        for (text, needle) in cases {
            let err = parse_art(&text).unwrap_err();
            assert!(err.contains(needle), "expected {needle:?} in {err:?}");
        }
    }
}
