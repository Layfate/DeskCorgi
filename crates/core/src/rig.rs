//! Body geometry shared by the behaviour code and the renderer: where the body, shoulders,
//! hips and head are for a given skin shape and pose, plus the 2.5D head projection.

use crate::geom::{pt, Affine, Pt};
use crate::pet::Pose;
use crate::scene::{paw_rest, side_sign, SEAT};
use crate::skin::Shape;

/// Head half-width / half-height in head space, and the depth of the dome the face sits on.
pub const HEAD_RX: f32 = 62.0;
pub const HEAD_RY: f32 = 52.0;
pub const HEAD_DEPTH: f32 = 30.0;
/// Height of the torso in body space (neck at y = -TORSO_H).
pub const TORSO_H: f32 = 134.0;

/// Head transform with a dome projection: points near the middle of the face move further
/// than points near the edge when the head turns, which gives the 2.5D look.
#[derive(Clone, Copy, Debug)]
pub struct HeadXf {
    pub center: Pt,
    pub scale: f32,
    pub tilt: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl HeadXf {
    pub fn dome_z(p: Pt) -> f32 {
        let u = p.x / HEAD_RX;
        let v = p.y / HEAD_RY;
        HEAD_DEPTH * (1.0 - u * u - v * v).max(0.0).sqrt()
    }

    fn finish(&self, p: Pt) -> Pt {
        self.center + (p * self.scale).rotate(self.tilt)
    }

    /// Projects a head-space point at depth `z` (positive = towards the viewer).
    pub fn map3(&self, p: Pt, z: f32) -> Pt {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        let x1 = p.x * cy + z * sy;
        let z1 = -p.x * sy + z * cy;
        let y1 = p.y * cp + z1 * sp;
        self.finish(pt(x1, y1))
    }

    /// A point on the face dome, optionally raised by `extra` (nose, muzzle).
    pub fn map_dome(&self, p: Pt, extra: f32) -> Pt {
        self.map3(p, HeadXf::dome_z(p) + extra)
    }

    /// Silhouette-style mapping: only a small shift with yaw/pitch.
    pub fn map_flat(&self, p: Pt, depth: f32) -> Pt {
        self.finish(pt(p.x + self.yaw.sin() * depth, p.y + self.pitch.sin() * depth))
    }

    /// Local frame for a small feature at head-space point `p` on the dome. The x axis is
    /// foreshortened by how much the dome is turned at that spot.
    pub fn feature_frame(&self, p: Pt, extra: f32, rotate: f32) -> Affine {
        let o = self.map_dome(p, extra);
        let d = 3.0;
        let ax = self.map_dome(p + pt(d, 0.0), extra) - self.map_dome(p - pt(d, 0.0), extra);
        let ay = self.map_dome(p + pt(0.0, d), extra) - self.map_dome(p - pt(0.0, d), extra);
        let sx = (ax.len() / (2.0 * d)).max(0.2);
        let sy = (ay.len() / (2.0 * d)).max(0.2);
        Affine::translate(o)
            .then_after(Affine::rotate(self.tilt + rotate))
            .then_after(Affine::scale(sx, sy))
    }
}

/// Where everything is for one frame.
#[derive(Clone, Copy, Debug)]
pub struct Rig {
    /// Body space → canvas (lean, lift, tilt and breathing).
    pub body: Affine,
    /// Torso outline space → canvas (adds body width and the Corgi hip wiggle).
    pub torso: Affine,
    pub head: HeadXf,
    body_width: f32,
    leg_length: f32,
}

/// Base head size; the skin's `head_scale` multiplies this. A big head reads as cute.
pub const HEAD_BASE_SCALE: f32 = 1.1;

pub fn head_height(shape: &Shape) -> f32 {
    146.0 - (1.0 - shape.leg_length) * 22.0
}

impl Rig {
    pub fn new(shape: &Shape, pose: &Pose) -> Rig {
        let sq = pose.body_squash.clamp(0.8, 1.25);
        let body = Affine::translate(SEAT + pt(pose.body_lean, pose.body_lift))
            .then_after(Affine::rotate(pose.body_tilt))
            .then_after(Affine::scale(1.0 / sq.sqrt(), sq));
        let sway = pose.hip_sway;
        let skew = Affine { a: 1.0, b: 0.0, c: sway / TORSO_H, d: 1.0, e: sway, f: 0.0 };
        let torso = body.then_after(skew).then_after(Affine::scale(shape.body_width, 1.0));
        let neck = body.map(pt(0.0, -head_height(shape)));
        let head = HeadXf {
            center: neck + pt(0.0, pose.head_drop),
            scale: shape.head_scale * HEAD_BASE_SCALE,
            tilt: pose.head_tilt + pose.body_tilt * 0.5,
            yaw: pose.head_yaw,
            pitch: pose.head_pitch,
        };
        Rig { body, torso, head, body_width: shape.body_width, leg_length: shape.leg_length }
    }

    fn shoulder_local(body_width: f32, leg_length: f32, side: usize) -> Pt {
        // Half-width of the torso outline around shoulder height is ~42.5 (times body width).
        // The pivot sits just inside the flank, so the leg's outer edge runs along the body's
        // silhouette and its inner edge stays clear of the chest fur.
        // Set fairly low, so the front legs stay short and stubby like a corgi's.
        pt(side_sign(side) * (43.0 * body_width - 8.0), -82.0 + (1.0 - leg_length) * 6.0)
    }

    /// Shoulder joint (the pivot of the front leg) for side 0 (viewer's left) or 1 (viewer's
    /// right). It sits just inside the flank under the cheek, like a real shoulder, outside
    /// the chest fur.
    pub fn shoulder(&self, side: usize) -> Pt {
        self.body.map(Rig::shoulder_local(self.body_width, self.leg_length, side))
    }

    /// Point up the flank, under the head, that the front leg grows out of (the shoulder
    /// blade). The leg's root runs along the inside of the flank to it, never across the chest.
    pub fn arm_root(&self, side: usize) -> Pt {
        let j = Rig::shoulder_local(self.body_width, self.leg_length, side);
        self.body.map(j + pt(-side_sign(side) * 5.0, -28.0))
    }

    /// Upper-arm and forearm length. Constant for a skin: slightly longer than the distance
    /// from the shoulder to the paw's resting spot, so a resting leg has a soft bend.
    pub fn arm_bones(&self) -> (f32, f32) {
        let shoulder = SEAT + Rig::shoulder_local(self.body_width, self.leg_length, 1);
        let d = (paw_rest(1) - shoulder).len();
        (0.56 * d, 0.56 * d)
    }

    /// Hip joint of the back leg, right at the keyboard's top edge.
    pub fn hip(&self, side: usize) -> Pt {
        self.torso.map(pt(side_sign(side) * 50.0, -30.0))
    }

    /// Point inside the flank that the back leg grows out of.
    pub fn hip_root(&self, side: usize) -> Pt {
        self.torso.map(pt(side_sign(side) * 30.0, -60.0))
    }

    /// Thigh and shin length of the back legs.
    pub fn leg_bones(&self) -> (f32, f32) {
        (27.0, 27.0)
    }

    /// Whether a canvas point is on the dog's head (for petting).
    pub fn head_contains(&self, p: Pt) -> bool {
        let d = (p - self.head.center).rotate(-self.head.tilt);
        let rx = (HEAD_RX + 2.0) * self.head.scale;
        let ry = (HEAD_RY + 4.0) * self.head.scale;
        (d.x / rx).powi(2) + (d.y / ry).powi(2) <= 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin::BUILTINS;

    #[test]
    fn dome_moves_centre_more_than_edges() {
        let h = HeadXf { center: Pt::ZERO, scale: 1.0, tilt: 0.0, yaw: 0.3, pitch: 0.0 };
        let centre_shift = h.map_dome(Pt::ZERO, 0.0).x;
        let edge_shift = h.map_dome(pt(HEAD_RX, 0.0), 0.0).x - HEAD_RX;
        assert!(centre_shift > 5.0);
        assert!(centre_shift > edge_shift.abs());
    }

    #[test]
    fn neutral_pose_layout_is_sane() {
        for skin in BUILTINS.map(|(_, f)| f()) {
            let rig = Rig::new(&skin.shape, &Pose::neutral());
            let h = rig.head.center;
            assert!(h.y > 100.0 && h.y < 170.0, "{h:?}");
            assert!(rig.shoulder(0).x < rig.shoulder(1).x);
            assert!(rig.shoulder(0).y > h.y);
            assert!(rig.head_contains(h));
            assert!(!rig.head_contains(h + pt(0.0, 200.0)));
        }
    }
}
