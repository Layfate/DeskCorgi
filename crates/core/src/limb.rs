//! Limb skeleton and skinning.
//!
//! A leg is a two-bone chain (joint → elbow → wrist) solved with inverse kinematics. Its
//! drawn shape is generated from that skeleton every frame: a smooth curve through the three
//! joints, offset on each side by an anatomical width profile (deltoid bulge on the outer side,
//! armpit on the inner side, tapering to the wrist). A curved *root* continues from the joint
//! deep into the body, so the limb never has a top end that could show. Widths never depend
//! on how far the limb is stretched, so thickness stays constant in every pose. Where an edge
//! leaves the body outline, a [`Fillet`] rounds the corner so the two silhouettes merge.

use crate::geom::{lerp, pt, smoothstep, Pt};

/// Places the elbow of a two-bone limb. `prefer` is the direction the elbow should point
/// towards (outward/downward for a dog's front leg). When the target is out of reach the
/// limb straightens; it never folds tighter than the bone lengths allow.
pub fn solve_elbow(joint: Pt, wrist: Pt, upper: f32, lower: f32, prefer: Pt) -> Pt {
    let d = wrist - joint;
    let dist = d.len();
    if dist < 1e-3 {
        return joint + prefer.normalized() * upper;
    }
    let dir = d * (1.0 / dist);
    // Law of cosines for the angle at the joint.
    let reach = upper + lower;
    let dist_c = dist.clamp((upper - lower).abs() + 1e-3, reach - 1e-3);
    let cos_a = ((upper * upper + dist_c * dist_c - lower * lower) / (2.0 * upper * dist_c)).clamp(-1.0, 1.0);
    let a = cos_a.acos();
    let perp = dir.perp();
    let side = if perp.x * prefer.x + perp.y * prefer.y >= 0.0 { 1.0 } else { -1.0 };
    // Rotating `dir` by `a` towards `perp*side`.
    let along = dir * (upper * a.cos());
    let across = perp * (side * upper * a.sin());
    let mut elbow = joint + along + across;
    if dist > reach {
        // Stretched beyond the bones: keep the elbow on the line (cartoon stretch).
        elbow = joint + dir * (dist * upper / reach);
    }
    elbow
}

/// Width profile of a limb, as half-widths at u = 0 (joint), 0.25, 0.5 (elbow), 0.75, 1 (wrist).
#[derive(Clone, Copy, Debug)]
pub struct Profile {
    /// Side the elbow bends towards (deltoid / back of the arm).
    pub outer: [f32; 5],
    /// The other side (armpit / inside of the arm).
    pub inner: [f32; 5],
    /// Half-width of the root deep inside the body.
    pub root: f32,
}

impl Profile {
    fn sample(arr: &[f32; 5], u: f32) -> f32 {
        let x = u.clamp(0.0, 1.0) * 4.0;
        let i = (x.floor() as usize).min(3);
        let t = x - i as f32;
        // Smooth (cosine) interpolation between keys avoids visible kinks in the outline.
        let t = 0.5 - 0.5 * (std::f32::consts::PI * t).cos();
        lerp(arr[i], arr[i + 1], t)
    }
}

/// The generated shape of one limb.
#[derive(Clone, Debug)]
pub struct LimbShape {
    pub center: Vec<Pt>,
    /// Arc parameter: negative inside the root, 0 at the joint, 1 at the wrist.
    pub u: Vec<f32>,
    /// Edge on the outer (elbow) side and on the inner side, point for point with `center`.
    pub outer: Vec<Pt>,
    pub inner: Vec<Pt>,
    pub joint: Pt,
    pub elbow: Pt,
    pub wrist: Pt,
}

const ROOT_SAMPLES: usize = 10;
const LIMB_SAMPLES: usize = 32;

fn cubic(p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: f32) -> Pt {
    let u = 1.0 - t;
    p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
}

fn quad(p0: Pt, p1: Pt, p2: Pt, t: f32) -> Pt {
    let u = 1.0 - t;
    p0 * (u * u) + p1 * (2.0 * u * t) + p2 * (t * t)
}

/// Builds the limb outline. `root` is a point deep inside the body the limb grows out of.
pub fn skin_limb(root: Pt, joint: Pt, elbow: Pt, wrist: Pt, profile: &Profile) -> LimbShape {
    // The arm curve passes exactly through the elbow at its midpoint. Very tight folds are
    // relaxed so the inner edge can never loop over itself.
    let mid = joint.lerp(wrist, 0.5);
    let span = (wrist - joint).len().max(1.0);
    let mut bulge = elbow - mid;
    // A parabola with this bulge has a tightest radius of span²/(8·bulge); keep it wider than
    // the inner half-width so the inner edge never folds back on itself.
    let widest_inner = profile.inner.iter().cloned().fold(0.0f32, f32::max).max(1.0);
    let max_bulge = (0.42 * span).min(span * span / (8.0 * 1.25 * widest_inner));
    if bulge.len() > max_bulge {
        bulge = bulge.normalized() * max_bulge;
    }
    let ctrl = mid + bulge * 2.0;

    let mut center = Vec::with_capacity(ROOT_SAMPLES + LIMB_SAMPLES + 1);
    // Root: a cubic from deep inside the body that arrives at the joint along the arm's tangent.
    let tangent = (ctrl - joint).normalized();
    let root_len = (joint - root).len();
    let r1 = root + (joint - root) * 0.35;
    let r2 = joint - tangent * (root_len * 0.3);
    for i in 0..ROOT_SAMPLES {
        center.push(cubic(root, r1, r2, joint, i as f32 / ROOT_SAMPLES as f32));
    }
    for i in 0..=LIMB_SAMPLES {
        center.push(quad(joint, ctrl, wrist, i as f32 / LIMB_SAMPLES as f32));
    }

    // Arc-length parameter: 0 at the joint, 1 at the wrist.
    let mut acc = vec![0.0f32; center.len()];
    for i in 1..center.len() {
        acc[i] = acc[i - 1] + (center[i] - center[i - 1]).len();
    }
    let at_joint = acc[ROOT_SAMPLES];
    let arm_len = (acc[center.len() - 1] - at_joint).max(1e-3);
    let u: Vec<f32> = acc.iter().map(|&a| (a - at_joint) / arm_len).collect();
    let u_root = u[0].min(-1e-3);

    // Which side of the curve the elbow is on: that side is "outer".
    let n_mid = {
        let a = center[ROOT_SAMPLES + LIMB_SAMPLES / 2 - 1];
        let b = center[ROOT_SAMPLES + LIMB_SAMPLES / 2 + 1];
        (b - a).normalized().perp()
    };
    let outer_sign = if (bulge.x * n_mid.x + bulge.y * n_mid.y) >= 0.0 { 1.0 } else { -1.0 };

    let n = center.len();
    let mut outer = Vec::with_capacity(n);
    let mut inner = Vec::with_capacity(n);
    for i in 0..n {
        let a = center[i.saturating_sub(1)];
        let b = center[(i + 1).min(n - 1)];
        let normal = (b - a).normalized().perp() * outer_sign;
        let (ho, hi) = if u[i] >= 0.0 {
            (Profile::sample(&profile.outer, u[i]), Profile::sample(&profile.inner, u[i]))
        } else {
            // Inside the root, widen smoothly towards the root half-width.
            let k = smoothstep(0.0, 1.0, u[i] / u_root);
            (lerp(profile.outer[0], profile.root, k), lerp(profile.inner[0], profile.root, k))
        };
        outer.push(center[i] + normal * ho);
        inner.push(center[i] - normal * hi);
    }
    LimbShape { center, u, outer, inner, joint, elbow: mid + bulge, wrist }
}

impl LimbShape {
    /// Closed polygon of the part of the limb with `u` in `[from, to]`.
    pub fn polygon(&self, from: f32, to: f32) -> Vec<Pt> {
        let idx: Vec<usize> = (0..self.center.len()).filter(|&i| self.u[i] >= from && self.u[i] <= to).collect();
        let mut pts: Vec<Pt> = idx.iter().map(|&i| self.outer[i]).collect();
        pts.extend(idx.iter().rev().map(|&i| self.inner[i]));
        pts
    }

    /// Index range of samples with `u >= from`.
    pub fn first_index_from(&self, from: f32) -> usize {
        self.u.iter().position(|&u| u >= from).unwrap_or(self.u.len() - 1)
    }

    /// Point on the centre curve at arc parameter `u` (0..1).
    pub fn point_at(&self, u: f32) -> Pt {
        Self::sample_at(&self.u, &self.center, u)
    }

    /// Point on the outer (`true`) or inner edge at arc parameter `u`.
    pub fn edge_at(&self, u: f32, outer: bool) -> Pt {
        Self::sample_at(&self.u, if outer { &self.outer } else { &self.inner }, u)
    }

    fn sample_at(us: &[f32], pts: &[Pt], u: f32) -> Pt {
        let i = us.iter().position(|&x| x >= u).unwrap_or(us.len() - 1).max(1);
        let (u0, u1) = (us[i - 1], us[i]);
        let t = if u1 > u0 { ((u - u0) / (u1 - u0)).clamp(0.0, 1.0) } else { 0.0 };
        pts[i - 1].lerp(pts[i], t)
    }
}

/// Even-odd point-in-polygon test.
pub fn point_in_polygon(p: Pt, poly: &[Pt]) -> bool {
    let mut inside = false;
    let n = poly.len();
    let mut j = n.wrapping_sub(1);
    for i in 0..n {
        let (a, b) = (poly[i], poly[j]);
        if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
            inside = !inside;
        }
        j = i;
    }
    inside
}

/// Intersection parameter `t` along `a→b` with segment `c→d`, if they cross.
fn seg_hit(a: Pt, b: Pt, c: Pt, d: Pt) -> Option<f32> {
    let r = b - a;
    let s = d - c;
    let den = r.x * s.y - r.y * s.x;
    if den.abs() < 1e-9 {
        return None;
    }
    let q = c - a;
    let t = (q.x * s.y - q.y * s.x) / den;
    let u = (q.x * r.y - q.y * r.x) / den;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

/// Follows `pts` starting at `from` for `dist` units; returns the path (without `from`), its
/// last point interpolated to land exactly at `dist` if the points run that far.
fn walk(from: Pt, pts: impl Iterator<Item = Pt>, dist: f32) -> Vec<Pt> {
    let mut out = Vec::new();
    let mut prev = from;
    let mut left = dist;
    for q in pts {
        let l = (q - prev).len();
        if l >= left {
            out.push(prev.lerp(q, left / l.max(1e-6)));
            return out;
        }
        out.push(q);
        left -= l;
        prev = q;
    }
    out
}

/// The smooth web where a limb grows out of the body. At the point an edge of the limb leaves
/// the body outline, the union of the two shapes has a sharp inward corner; the fillet fills
/// that corner up to a quadratic curve, so the silhouette flows from the body into the limb
/// the way a real shoulder or hip does.
#[derive(Clone, Debug)]
pub struct Fillet {
    /// Closed region between the body outline, the limb edge and the new curve.
    pub region: Vec<Pt>,
    /// The new piece of silhouette (open curve), to be inked.
    pub curve: Vec<Pt>,
    /// Edge it belongs to (true = outer) and where that edge leaves the body.
    pub outer: bool,
    pub exit: Pt,
    pub exit_u: f32,
}

/// Fillets for the first place each edge of `shape` leaves the closed `body` outline (walking
/// from the root towards the wrist), with arms of length `radius` along the edge and along
/// the body. Only exits above `max_y` count (below it the body is hidden).
pub fn fillets(shape: &LimbShape, body: &[Pt], radius: f32, max_y: f32) -> Vec<Fillet> {
    let mut out = Vec::new();
    let nb = body.len();
    if nb < 3 {
        return out;
    }
    for outer in [true, false] {
        let edge = if outer { &shape.outer } else { &shape.inner };
        let n = edge.len();
        let mut inside = point_in_polygon(edge[0], body);
        for i in 0..n - 1 {
            let next_inside = point_in_polygon(edge[i + 1], body);
            if inside && !next_inside {
                let (a, b) = (edge[i], edge[i + 1]);
                // Closest crossing along a→b.
                let mut best: Option<(f32, usize)> = None;
                for j in 0..nb {
                    if let Some(t) = seg_hit(a, b, body[j], body[(j + 1) % nb]) {
                        if best.map_or(true, |(bt, _)| t < bt) {
                            best = Some((t, j));
                        }
                    }
                }
                let Some((t, j)) = best else { break };
                let x = a.lerp(b, t);
                if x.y > max_y {
                    break;
                }
                let exit_u = lerp(shape.u[i], shape.u[i + 1], t);
                // Along the limb edge, outwards.
                let along_edge = walk(x, edge[i + 1..].iter().copied(), radius);
                // Along the body, in the direction that leaves the limb's interior.
                let inward = shape.center[i].lerp(shape.center[i + 1], t) - x;
                let dir = body[(j + 1) % nb] - body[j];
                let forward = dir.x * inward.x + dir.y * inward.y < 0.0;
                let along_body = if forward {
                    walk(x, (1..=nb).map(|k| body[(j + k) % nb]), radius)
                } else {
                    walk(x, (0..nb).map(|k| body[(j + nb - k) % nb]), radius)
                };
                let (Some(&ea), Some(&bb)) = (along_edge.last(), along_body.last()) else { break };
                const STEPS: usize = 12;
                let curve: Vec<Pt> = (0..=STEPS).map(|k| quad(ea, x, bb, k as f32 / STEPS as f32)).collect();
                let mut region = vec![x];
                region.extend_from_slice(&along_edge);
                region.extend_from_slice(&curve[1..STEPS]);
                region.extend(along_body.iter().rev());
                out.push(Fillet { region, curve, outer, exit: x, exit_u });
                break;
            }
            inside = next_inside;
        }
    }
    out
}

/// Default bend preference for a front leg on side `s` (-1 viewer-left, +1 viewer-right):
/// elbows point outward and a little down.
pub fn front_bend(s: f32) -> Pt {
    pt(s, 0.35)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> Profile {
        Profile { outer: [16.0, 12.5, 11.5, 11.0, 10.0], inner: [11.0, 10.5, 10.5, 10.0, 9.5], root: 15.0 }
    }

    #[test]
    fn ik_keeps_bone_lengths_when_reachable() {
        let j = pt(0.0, 0.0);
        for target in [pt(10.0, 60.0), pt(-40.0, 30.0), pt(50.0, -20.0), pt(5.0, 20.0)] {
            let e = solve_elbow(j, target, 40.0, 40.0, pt(1.0, 0.3));
            assert!(((e - j).len() - 40.0).abs() < 0.05, "upper {target:?}");
            assert!(((target - e).len() - 40.0).abs() < 0.05, "lower {target:?}");
        }
    }

    #[test]
    fn ik_bends_towards_preference_and_straightens_when_stretched() {
        let j = pt(0.0, 0.0);
        let e = solve_elbow(j, pt(0.0, 60.0), 40.0, 40.0, pt(1.0, 0.0));
        assert!(e.x > 5.0, "{e:?}");
        let e = solve_elbow(j, pt(0.0, 60.0), 40.0, 40.0, pt(-1.0, 0.0));
        assert!(e.x < -5.0, "{e:?}");
        let e = solve_elbow(j, pt(0.0, 120.0), 40.0, 40.0, pt(1.0, 0.0));
        assert!(e.x.abs() < 1e-3 && (e.y - 60.0).abs() < 1e-3, "{e:?}");
    }

    #[test]
    fn skinned_limb_has_constant_thickness_and_ends_at_the_wrist() {
        let p = profile();
        for (wrist, elbow_pref) in [(pt(10.0, 70.0), pt(1.0, 0.3)), (pt(-90.0, 60.0), pt(-1.0, 0.3)), (pt(40.0, -40.0), pt(1.0, 0.3))] {
            let joint = pt(0.0, 0.0);
            let elbow = solve_elbow(joint, wrist, 40.0, 40.0, elbow_pref);
            let shape = skin_limb(pt(-10.0, -30.0), joint, elbow, wrist, &p);
            let last = shape.center.len() - 1;
            assert!((shape.center[last] - wrist).len() < 1e-3);
            // Width at the wrist is outer[4] + inner[4] whatever the pose.
            let w = (shape.outer[last] - shape.inner[last]).len();
            assert!((w - 19.5).abs() < 0.3, "{w}");
            // No point jumps: the outline is continuous.
            for i in 1..shape.center.len() {
                assert!((shape.outer[i] - shape.outer[i - 1]).len() < 12.0, "gap at {i}");
                assert!((shape.inner[i] - shape.inner[i - 1]).len() < 12.0, "gap at {i}");
            }
            assert!(shape.u[0] < 0.0 && (shape.u[last] - 1.0).abs() < 1e-4);
        }
    }

    fn segments_cross(a: Pt, b: Pt, c: Pt, d: Pt) -> bool {
        let orient = |p: Pt, q: Pt, r: Pt| (q.x - p.x) * (r.y - p.y) - (q.y - p.y) * (r.x - p.x);
        let (o1, o2, o3, o4) = (orient(a, b, c), orient(a, b, d), orient(c, d, a), orient(c, d, b));
        o1 * o2 < 0.0 && o3 * o4 < 0.0
    }

    fn self_intersects(pts: &[Pt]) -> bool {
        for i in 0..pts.len().saturating_sub(1) {
            for j in i + 2..pts.len() - 1 {
                if segments_cross(pts[i], pts[i + 1], pts[j], pts[j + 1]) {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn edges_never_loop_even_when_folded() {
        let p = profile();
        let joint = pt(0.0, 0.0);
        for k in 0..36 {
            for dist in [24.0f32, 40.0, 60.0, 79.0, 120.0] {
                let a = k as f32 / 36.0 * std::f32::consts::TAU;
                let wrist = pt(a.cos(), a.sin()) * dist;
                let elbow = solve_elbow(joint, wrist, 40.0, 40.0, pt(1.0, 0.3));
                let s = skin_limb(pt(0.0, -30.0), joint, elbow, wrist, &p);
                let from = s.first_index_from(0.0);
                assert!(!self_intersects(&s.inner[from..]), "inner edge loops: angle {k}, dist {dist}");
                assert!(!self_intersects(&s.outer[from..]), "outer edge loops: angle {k}, dist {dist}");
            }
        }
    }
}
