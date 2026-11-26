//! Renders the frames of the three README demo GIFs: a different corgi coat in each, on a
//! soft grey-green gradient, with a mouse pointer where the cursor is.
//! Run with `cargo run --release -p deskcorgi-core --example demo_gifs`. It writes
//! `out/demo/<name>/f0000.png …`; `tools/demo_gifs.sh` runs this and turns the frames into
//! `assets/demo_<name>.gif` with ffmpeg.

use std::path::Path;

use deskcorgi_core::geom::{pt, Pt};
use deskcorgi_core::keymap::{key_from_set1, label_from_set1};
use deskcorgi_core::skin::{self, Skin};
use deskcorgi_core::tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, PixmapPaint, Point, RadialGradient, Rect, SpreadMode, Stroke,
    Transform,
};
use deskcorgi_core::{FrameInput, InputEvent, MoodConfig, MouseButton, Pet, RenderOptions, Renderer};

const W: u32 = 520;
const H: u32 = 460;
const FPS: f32 = 25.0;
/// Simulation steps per frame, so the springs move as smoothly as in the app.
const SUBSTEPS: usize = 4;
const SCALE: f32 = 1.4;
/// Where the dog's canvas sits in the frame (pixels).
const OFFSET: Pt = pt(8.0, -12.0);

fn to_canvas(px: Pt) -> Pt {
    (px - OFFSET) * (1.0 / SCALE)
}

/// The soft studio backdrop: light grey-green at the top left, a little deeper at the
/// bottom right, with a gentle glow behind the dog.
fn backdrop() -> Pixmap {
    let mut bg = Pixmap::new(W, H).unwrap();
    let rect = Rect::from_xywh(0.0, 0.0, W as f32, H as f32).unwrap();
    let base = LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(W as f32, H as f32),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(0xDD, 0xE2, 0xDC, 255)),
            GradientStop::new(1.0, Color::from_rgba8(0xB3, 0xBA, 0xB5, 255)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    bg.fill_rect(rect, &Paint { shader: base, ..Paint::default() }, Transform::identity(), None);
    let glow = RadialGradient::new(
        Point::from_xy(W as f32 * 0.48, H as f32 * 0.42),
        Point::from_xy(W as f32 * 0.48, H as f32 * 0.42),
        W as f32 * 0.55,
        vec![GradientStop::new(0.0, Color::from_rgba8(255, 255, 255, 70)), GradientStop::new(1.0, Color::from_rgba8(255, 255, 255, 0))],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .unwrap();
    bg.fill_rect(rect, &Paint { shader: glow, ..Paint::default() }, Transform::identity(), None);
    bg
}

/// A standard arrow pointer with its tip at `tip` (pixels).
fn draw_cursor(pm: &mut Pixmap, tip: Pt) {
    let pts = [(0.0, 0.0), (0.0, 17.0), (4.2, 13.2), (7.0, 19.6), (9.8, 18.4), (7.1, 12.2), (12.4, 12.2)];
    let mut pb = PathBuilder::new();
    for (i, &(x, y)) in pts.iter().enumerate() {
        let (x, y) = (tip.x + x * 1.15, tip.y + y * 1.15);
        if i == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.close();
    let path = pb.finish().unwrap();
    let mut black = Paint::default();
    black.set_color_rgba8(0x14, 0x14, 0x16, 255);
    black.anti_alias = true;
    let mut white = Paint::default();
    white.set_color_rgba8(255, 255, 255, 255);
    white.anti_alias = true;
    let stroke = Stroke { width: 2.2, line_join: deskcorgi_core::tiny_skia::LineJoin::Round, ..Stroke::default() };
    pm.stroke_path(&path, &white, &stroke, Transform::identity(), None);
    pm.fill_path(&path, &black, FillRule::Winding, Transform::identity(), None);
}

/// Set-1 scan code of a character typed in the demos.
fn scan(c: char) -> u16 {
    let rows: [(&str, u16); 3] = [("qwertyuiop", 0x10), ("asdfghjkl", 0x1E), ("zxcvbnm", 0x2C)];
    match c {
        ' ' => 0x39,
        '\u{8}' => 0x0E, // Backspace
        '\n' => 0x1C,    // Enter
        _ => rows.iter().find_map(|(row, base)| row.find(c).map(|i| base + i as u16)).expect("demo character"),
    }
}

/// One demo frame's input, and where to draw the pointer (pixels), if anywhere.
struct Frame {
    input: FrameInput,
    pointer: Option<Pt>,
}

/// Cursor input for a pointer at `px` in the frame. The whole frame stands in for the
/// screen, so the dog's mouse slides around its pad as the pointer moves.
fn at(px: Pt) -> Frame {
    Frame {
        input: FrameInput { cursor: Some(to_canvas(px)), cursor_screen: Some(pt(px.x / W as f32, px.y / H as f32)), ..Default::default() },
        pointer: Some(px),
    }
}

fn press(pet: &mut Pet, c: char) {
    let make = scan(c);
    let id = key_from_set1(make, false).unwrap();
    pet.handle(InputEvent::KeyDown(id));
    if let Some(label) = label_from_set1(make, false) {
        pet.show_key(id, label);
    }
}

fn release(pet: &mut Pet, c: char) {
    pet.handle(InputEvent::KeyUp(key_from_set1(scan(c), false).unwrap()));
}

fn click(pet: &mut Pet, button: MouseButton, down: bool) {
    pet.handle(InputEvent::MouseButton { button, down });
}

struct Demo {
    name: &'static str,
    skin: fn() -> Skin,
    frames: usize,
    step: fn(&mut Pet, usize) -> Frame,
}

/// Typing "hello corgi", a few quick Backspaces (a puzzled head tilt), then "hi".
fn typing(pet: &mut Pet, i: usize) -> Frame {
    let script: &[(usize, char)] = &[
        (4, 'h'), (8, 'e'), (12, 'l'), (15, 'l'), (19, 'o'), (24, ' '),
        (28, 'c'), (31, 'o'), (34, 'r'), (37, 'g'), (40, 'i'),
        (52, '\u{8}'), (56, '\u{8}'), (60, '\u{8}'),
        (96, 'h'), (99, 'i'), (104, '\n'),
    ];
    for &(f, c) in script {
        if i == f {
            press(pet, c);
        }
        if i == f + 2 {
            release(pet, c);
        }
    }
    at(pt(470.0, 70.0))
}

/// Gliding the pointer around (the dog's mouse follows it), a few clicks, then a wave.
fn mouse(pet: &mut Pet, i: usize) -> Frame {
    let t = i as f32 / 116.0 * std::f32::consts::TAU;
    let px = pt(260.0 + 215.0 * t.sin(), 62.0 + 34.0 * (2.0 * t).sin() + 10.0 * t.cos());
    match i {
        34 | 40 => click(pet, MouseButton::Left, true),
        36 | 42 => click(pet, MouseButton::Left, false),
        64 => click(pet, MouseButton::Right, true),
        67 => click(pet, MouseButton::Right, false),
        84 => pet.wave(),
        _ => {}
    }
    at(px)
}

/// Petting the head back and forth (hearts), then clicking fast for zoomies.
fn petting(pet: &mut Pet, i: usize) -> Frame {
    let head = pt(219.0, 148.0) * SCALE + OFFSET;
    let px = match i {
        0..=11 => pt(480.0, 70.0).lerp(head + pt(40.0, -10.0), i as f32 / 11.0),
        12..=62 => head + pt(42.0 * ((i - 12) as f32 * 0.42).cos(), -10.0 + 4.0 * ((i - 12) as f32 * 0.84).sin()),
        63..=72 => (head + pt(42.0, -10.0)).lerp(pt(470.0, 80.0), (i - 63) as f32 / 9.0),
        _ => pt(470.0, 80.0),
    };
    if (76..=100).contains(&i) {
        // Nine quick clicks in about a second.
        match (i - 76) % 3 {
            0 => click(pet, MouseButton::Left, true),
            1 => click(pet, MouseButton::Left, false),
            _ => {}
        }
    }
    at(px)
}

fn main() {
    let demos = [
        Demo { name: "typing", skin: skin::tricolor, frames: 116, step: typing },
        Demo { name: "mouse", skin: skin::blue_merle, frames: 116, step: mouse },
        Demo { name: "pet", skin: skin::fawn, frames: 136, step: petting },
    ];
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out/demo");
    let bg = backdrop();
    let cfg = MoodConfig::default();
    let mut renderer = Renderer::new(SCALE);
    let opts = RenderOptions { scale: SCALE, ..Default::default() };
    for demo in &demos {
        let dir = root.join(demo.name);
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let skin = (demo.skin)();
        let mut pet = Pet::new(7);
        // Settle into the starting pose before recording.
        let first = (demo.step)(&mut Pet::new(7), 0);
        for _ in 0..(FPS as usize * SUBSTEPS) {
            pet.update(1.0 / (FPS * SUBSTEPS as f32), &first.input, &skin, &cfg);
        }
        for i in 0..demo.frames {
            let frame = (demo.step)(&mut pet, i);
            for _ in 0..SUBSTEPS {
                pet.update(1.0 / (FPS * SUBSTEPS as f32), &frame.input, &skin, &cfg);
            }
            let mut pm = bg.clone();
            let dog = renderer.render(pet.pose(), &skin, &opts);
            pm.draw_pixmap(OFFSET.x as i32, OFFSET.y as i32, dog.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
            if let Some(p) = frame.pointer {
                draw_cursor(&mut pm, p);
            }
            pm.save_png(dir.join(format!("f{i:04}.png"))).unwrap();
        }
        println!("wrote out/demo/{}/ ({} frames, {})", demo.name, demo.frames, skin.name);
    }
}
