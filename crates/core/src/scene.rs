//! Fixed scene layout on the logical canvas (y down). Everything is scaled at render time.
//!
//! The dog faces the viewer. The keyboard faces the dog, so from the viewer's side it is
//! rotated 180°: the space bar is nearest the dog (top). The mouse sits under the dog's right
//! paw (the viewer's left) and also faces the dog: its buttons and cable point towards the
//! viewer. It slides around its pad following the user's cursor (see [`mouse_spot`]).

use crate::geom::{pt, Pt};
use crate::keymap::{cell, KeyId, ROWS, ROW_UNITS};

pub const CANVAS_W: f32 = 360.0;
pub const CANVAS_H: f32 = 340.0;

/// Horizontal centre of the dog and of the keyboard.
pub const DOG_X: f32 = 219.0;
/// Seat anchor: bottom centre of the dog's body (hidden behind the keyboard).
pub const SEAT: Pt = pt(DOG_X, 282.0);

/// Keyboard top face corners (a gentle trapezoid).
pub const KB_TL: Pt = pt(DOG_X - 72.0, 254.0);
pub const KB_TR: Pt = pt(DOG_X + 72.0, 254.0);
pub const KB_BR: Pt = pt(DOG_X + 81.0, 309.0);
pub const KB_BL: Pt = pt(DOG_X - 81.0, 309.0);
/// Height of the keyboard's front edge.
pub const KB_FRONT: f32 = 7.0;
/// Inset of the key area inside the top face, as fractions of the face.
const KEY_U0: f32 = 0.035;
const KEY_U1: f32 = 0.965;
const KEY_V0: f32 = 0.1;
const KEY_V1: f32 = 0.9;

/// Mouse pad centre and size.
pub const PAD_CENTER: Pt = pt(95.0, 284.0);
pub const PAD_W: f32 = 74.0;
pub const PAD_H: f32 = 52.0;
/// Resting position of the mouse body (the cursor in the middle of its screen).
pub const MOUSE_REST: Pt = pt(95.0, 282.0);
/// How far the mouse body travels from `MOUSE_REST` when the cursor is at a screen edge.
/// It keeps the whole mouse on its pad.
pub const MOUSE_RANGE: Pt = pt(17.0, 6.5);

/// Where the mouse sits on its pad for a cursor at `screen` (its position on the monitor,
/// (0, 0) top-left to (1, 1) bottom-right), as an offset from `MOUSE_REST`. It moves the same
/// way as the cursor on screen: cursor right → mouse right, cursor up → mouse up the pad.
pub fn mouse_spot(screen: Pt) -> Pt {
    let u = screen.x.clamp(0.0, 1.0) * 2.0 - 1.0;
    let v = screen.y.clamp(0.0, 1.0) * 2.0 - 1.0;
    pt(u * MOUSE_RANGE.x, v * MOUSE_RANGE.y)
}

/// Keeps a mouse offset on the pad.
pub fn clamp_mouse(off: Pt) -> Pt {
    pt(off.x.clamp(-MOUSE_RANGE.x, MOUSE_RANGE.x), off.y.clamp(-MOUSE_RANGE.y, MOUSE_RANGE.y))
}

/// Index 0 = viewer's left (the dog's right paw); 1 = viewer's right (the dog's left paw).
pub const VIEWER_LEFT: usize = 0;
pub const VIEWER_RIGHT: usize = 1;

/// Resting spot of each front paw on the keyboard.
/// The paw that works the mouse (the dog's right paw, on the viewer's left). The other paw is
/// the one that waves hello.
pub const MOUSE_SIDE: usize = VIEWER_LEFT;
pub const WAVE_SIDE: usize = VIEWER_RIGHT;

pub fn paw_rest(side: usize) -> Pt {
    let s = side_sign(side);
    pt(DOG_X + s * 30.0, 263.0)
}

pub fn side_sign(side: usize) -> f32 {
    if side == VIEWER_LEFT {
        -1.0
    } else {
        1.0
    }
}

/// Point on the keyboard's top face from face coordinates (u: 0 left → 1 right, v: 0 top → 1 bottom).
pub fn kb_point(u: f32, v: f32) -> Pt {
    let top = KB_TL.lerp(KB_TR, u);
    let bottom = KB_BL.lerp(KB_BR, u);
    top.lerp(bottom, v)
}

/// The four corners of a key cap (top-left, top-right, bottom-right, bottom-left) in canvas space.
pub fn key_quad(id: KeyId) -> [Pt; 4] {
    let c = cell(id);
    // Rotate the user's layout 180°: their left is our right, their top row is our bottom row.
    let gap_u = 0.09;
    let x0 = ROW_UNITS - (c.x0 + c.width) + gap_u;
    let x1 = ROW_UNITS - c.x0 - gap_u;
    let row_from_top = (ROWS - 1 - c.row as usize) as f32;
    let gap_v = 0.11;
    let y0 = row_from_top + gap_v;
    let y1 = row_from_top + 1.0 - gap_v;
    let u = |x: f32| KEY_U0 + (KEY_U1 - KEY_U0) * (x / ROW_UNITS);
    let v = |y: f32| KEY_V0 + (KEY_V1 - KEY_V0) * (y / ROWS as f32);
    [kb_point(u(x0), v(y0)), kb_point(u(x1), v(y0)), kb_point(u(x1), v(y1)), kb_point(u(x0), v(y1))]
}

pub fn key_center(id: KeyId) -> Pt {
    let q = key_quad(id);
    pt((q[0].x + q[1].x + q[2].x + q[3].x) / 4.0, (q[0].y + q[1].y + q[2].y + q[3].y) / 4.0)
}

/// Which front paw naturally presses this key (by which side of the dog it is on).
pub fn preferred_side(id: KeyId) -> usize {
    if key_center(id).x < DOG_X {
        VIEWER_LEFT
    } else {
        VIEWER_RIGHT
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::{all_keys, key};

    const _: () = assert!(KB_BL.y + KB_FRONT < CANVAS_H, "keyboard must fit on the canvas");

    #[test]
    fn keys_sit_inside_keyboard_and_canvas() {
        for id in all_keys() {
            for p in key_quad(id) {
                assert!(p.x > KB_BL.x && p.x < KB_BR.x, "{id:?} {p:?}");
                assert!(p.y > KB_TL.y && p.y < KB_BL.y, "{id:?} {p:?}");
            }
        }
    }

    #[test]
    fn layout_is_mirrored_for_the_dog() {
        // The user's Q (left side) is on the viewer's right, pressed by the dog's left paw.
        let q = key(1, 1);
        let p = key(1, 10);
        assert!(key_center(q).x > key_center(p).x);
        assert_eq!(preferred_side(q), VIEWER_RIGHT);
        assert_eq!(preferred_side(p), VIEWER_LEFT);
        // The space bar row is nearest the dog (top), the number row nearest the viewer.
        assert!(key_center(key(4, 3)).y < key_center(key(0, 5)).y);
    }
}
