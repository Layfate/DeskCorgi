//! Renders a contact sheet of poses for both built-in skins to `out/poses.png`.
//! Run with `cargo run -p deskcorgi-core --example render_poses [scale]`.

use std::path::Path;

use deskcorgi_core::geom::{pt, Pt};
use deskcorgi_core::keymap::key;
use deskcorgi_core::skin::corgi;
use deskcorgi_core::{FrameInput, InputEvent, MoodConfig, MouseButton, Pet, Pose, RenderOptions, Renderer, Skin};
use tiny_skia::{Color, Pixmap, PixmapPaint, Transform};

const FPS: f32 = 30.0;

fn run(pet: &mut Pet, skin: &Skin, secs: f32, cfg: &MoodConfig, mut frame: impl FnMut(&mut Pet, usize) -> FrameInput) {
    let n = (secs * FPS) as usize;
    for i in 0..n {
        let input = frame(pet, i);
        pet.update(1.0 / FPS, &input, skin, cfg);
    }
}

fn scenario(name: &str, skin: &Skin) -> Pose {
    let cfg = MoodConfig { sleep_after: 4.0, ..Default::default() };
    let mut pet = Pet::new(7);
    let far = Some(pt(-200.0, -120.0));
    let still = |_: &mut Pet, _| FrameInput { cursor: far, ..Default::default() };
    match name {
        "idle" => run(&mut pet, skin, 1.0, &MoodConfig { sleep: false, ..cfg }, still),
        "look right" => run(&mut pet, skin, 1.0, &cfg, |_, _| FrameInput { cursor: Some(pt(520.0, 40.0)), ..Default::default() }),
        "typing" => {
            run(&mut pet, skin, 0.6, &cfg, |p, i| {
                if i % 4 == 0 {
                    // Home-row keys, popping up like they do in the app.
                    const LABELS: [&str; 9] = ["A", "S", "D", "F", "G", "H", "J", "K", "L"];
                    let n = (i / 4) % 9;
                    p.handle(InputEvent::KeyDown(key(2, 1 + n)));
                    p.show_key(key(2, 1 + n), LABELS[n]);
                }
                if i % 4 == 2 {
                    p.handle(InputEvent::KeyUp(key(2, 1 + (i / 4) % 9)));
                }
                FrameInput { cursor: far, ..Default::default() }
            });
            pet.handle(InputEvent::KeyDown(key(2, 2)));
            pet.show_key(key(2, 2), "S");
            pet.handle(InputEvent::KeyDown(key(1, 9)));
            pet.show_key(key(1, 9), "O");
            run(&mut pet, skin, 0.15, &cfg, still);
        }
        "mouse + click" => {
            run(&mut pet, skin, 0.6, &cfg, |p, i| {
                p.handle(InputEvent::MouseMove { dx: if i % 10 < 5 { 25.0 } else { -10.0 }, dy: 6.0 });
                FrameInput { cursor: Some(pt(-100.0, 200.0)), ..Default::default() }
            });
            pet.handle(InputEvent::MouseButton { button: MouseButton::Left, down: true });
            run(&mut pet, skin, 0.1, &cfg, |_, _| FrameInput { cursor: Some(pt(-100.0, 200.0)), ..Default::default() });
        }
        "three keys" => {
            for k in [key(2, 1), key(2, 3), key(2, 4)] {
                pet.handle(InputEvent::KeyDown(k));
            }
            run(&mut pet, skin, 0.6, &cfg, still);
        }
        "chase" => run(&mut pet, skin, 0.8, &cfg, |_, i| {
            let a = i as f32 * 1.3;
            FrameInput { cursor: Some(pt(300.0 + a.cos() * 120.0, 20.0 + a.sin() * 80.0)), ..Default::default() }
        }),
        "pat" => {
            run(&mut pet, skin, 0.1, &cfg, still);
            let c = pet.rig(skin).head.center;
            run(&mut pet, skin, 1.0, &cfg, |_, i| FrameInput {
                cursor: Some(c + pt((i as f32 * 0.7).sin() * 28.0, -14.0)),
                ..Default::default()
            });
        }
        "scruff" => run(&mut pet, skin, 0.5, &cfg, |_, i| FrameInput {
            cursor: far,
            dragging: true,
            drag_velocity: pt(if i < 10 { 600.0 } else { -300.0 }, -150.0),
            ..Default::default()
        }),
        "zoomies" => {
            run(&mut pet, skin, 0.3, &cfg, |p, _| {
                p.handle(InputEvent::MouseButton { button: MouseButton::Left, down: true });
                p.handle(InputEvent::MouseButton { button: MouseButton::Left, down: false });
                FrameInput { cursor: far, ..Default::default() }
            });
            run(&mut pet, skin, 0.25, &cfg, still);
        }
        "sleep" => run(&mut pet, skin, 7.0, &cfg, |_, _| FrameInput::default()),
        _ => {}
    }
    pet.pose().clone()
}

const SCENARIOS: [&str; 10] = ["idle", "look right", "typing", "mouse + click", "three keys", "chase", "pat", "scruff", "zoomies", "sleep"];

fn main() {
    let scale: f32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let skins = [corgi()];
    let (cw, ch) = deskcorgi_core::render::pixel_size(scale);
    let cols = SCENARIOS.len().min(5) as u32;
    let rows_per_skin = (SCENARIOS.len() as u32).div_ceil(cols);
    let mut sheet = Pixmap::new(cw * cols, ch * rows_per_skin * skins.len() as u32).unwrap();
    sheet.fill(Color::from_rgba8(0xE8, 0xEE, 0xF3, 255));
    let mut renderer = Renderer::new(scale);
    let opts = RenderOptions { scale, ..Default::default() };
    for (si, skin) in skins.iter().enumerate() {
        for (i, name) in SCENARIOS.iter().enumerate() {
            let pose = scenario(name, skin);
            let pm = renderer.render(&pose, skin, &opts);
            let col = i as u32 % cols;
            let row = si as u32 * rows_per_skin + i as u32 / cols;
            sheet.draw_pixmap((col * cw) as i32, (row * ch) as i32, pm.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
    }
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out");
    std::fs::create_dir_all(&out).unwrap();
    sheet.save_png(out.join("poses.png")).unwrap();
    // Close-ups of the idle faces.
    for skin in &skins {
        let pose = scenario("idle", skin);
        let mut big = Renderer::new(2.0);
        let pm = big.render(&pose, skin, &RenderOptions { scale: 2.0, ..Default::default() });
        let mut bg = Pixmap::new(pm.width(), pm.height()).unwrap();
        bg.fill(Color::from_rgba8(0xE8, 0xEE, 0xF3, 255));
        bg.draw_pixmap(0, 0, pm.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        bg.save_png(out.join(format!("{}.png", skin.name.to_lowercase().replace(' ', "_")))).unwrap();
        let icon = deskcorgi_core::render::render_icon(skin, 64, deskcorgi_core::config::EyeColor::Brown);
        icon.save_png(out.join(format!("icon_{}.png", skin.name.to_lowercase().replace(' ', "_")))).unwrap();
    }
    let _ = Pt::ZERO;
    println!("wrote out/poses.png");
}
