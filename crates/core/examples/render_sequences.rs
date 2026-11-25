//! Renders film strips of *intermediate* animation frames (not just start and end poses) for
//! every kind of limb motion, to check that legs stay attached and outlines stay clean.
//! Run with `cargo run --release -p deskcorgi-core --example render_sequences`.
//! Writes `out/seq_<skin>_<n>.png`; each row is one sequence, left to right in time.

use std::path::Path;

use deskcorgi_core::geom::pt;
use deskcorgi_core::keymap::key;
use deskcorgi_core::skin::{corgi, corgi_merle, corgi_tricolor};
use deskcorgi_core::tiny_skia::{Color, IntRect, Pixmap, PixmapPaint, Transform};
use deskcorgi_core::{FrameInput, InputEvent, MoodConfig, MouseButton, Pet, Pose, RenderOptions, Renderer, Skin};

const FPS: f32 = 60.0;
const FRAMES_PER_ROW: usize = 10;

struct Seq {
    name: &'static str,
    /// Frames simulated before recording starts.
    warmup: usize,
    /// Simulated frames between recorded frames.
    every: usize,
    step: fn(&mut Pet, usize) -> FrameInput,
    setup: fn(&mut Pet),
}

fn far() -> FrameInput {
    FrameInput { cursor: Some(pt(-200.0, -100.0)), ..Default::default() }
}

fn none(_: &mut Pet) {}

const SEQS: [Seq; 11] = [
    Seq { name: "idle", warmup: 30, every: 9, setup: none, step: |_, _| far() },
    Seq { name: "wave hello", warmup: 0, every: 7, setup: |p| p.wave(), step: |_, _| far() },
    Seq {
        name: "reach for mouse",
        warmup: 30,
        every: 3,
        setup: none,
        step: |p, i| {
            if i >= 30 {
                p.handle(InputEvent::MouseMove { dx: 12.0, dy: 3.0 });
            }
            FrameInput { cursor: Some(pt(-150.0, 250.0)), ..Default::default() }
        },
    },
    Seq {
        name: "back to keys",
        warmup: 60,
        every: 2,
        setup: none,
        step: |p, i| {
            if i < 60 {
                p.handle(InputEvent::MouseMove { dx: 12.0, dy: 3.0 });
            } else if i == 60 {
                p.handle(InputEvent::KeyDown(key(2, 10)));
            }
            far()
        },
    },
    Seq {
        name: "two keys, one side",
        warmup: 20,
        every: 2,
        setup: none,
        step: |p, i| {
            if i == 10 {
                p.handle(InputEvent::KeyDown(key(2, 2)));
            }
            if i == 20 {
                p.handle(InputEvent::KeyDown(key(2, 4)));
            }
            far()
        },
    },
    Seq {
        name: "alternating paws",
        warmup: 10,
        every: 2,
        setup: none,
        step: |p, i| {
            let keys = [key(2, 2), key(2, 8), key(1, 3), key(1, 7)];
            if i % 6 == 0 {
                p.handle(InputEvent::KeyDown(keys[(i / 6) % 4]));
            }
            if i % 6 == 3 {
                p.handle(InputEvent::KeyUp(keys[(i / 6) % 4]));
            }
            far()
        },
    },
    Seq {
        name: "held keys: back paw",
        warmup: 0,
        every: 3,
        setup: |p| {
            for k in [key(1, 2), key(2, 1), key(2, 2)] {
                p.handle(InputEvent::KeyDown(k));
            }
        },
        step: |_, _| far(),
    },
    Seq {
        name: "zoomies",
        warmup: 0,
        every: 6,
        setup: |p| {
            for _ in 0..9 {
                p.handle(InputEvent::MouseButton { button: MouseButton::Left, down: true });
                p.handle(InputEvent::MouseButton { button: MouseButton::Left, down: false });
            }
        },
        step: |_, _| far(),
    },
    Seq {
        name: "dangling (drag)",
        warmup: 0,
        every: 4,
        setup: none,
        step: |_, i| FrameInput {
            cursor: Some(pt(-200.0, -100.0)),
            dragging: i < 30,
            drag_velocity: pt(((i as f32) * 0.25).sin() * 900.0, -100.0),
        },
    },
    Seq {
        name: "petting",
        warmup: 0,
        every: 6,
        setup: none,
        step: |p, i| {
            let c = p.rig(&corgi()).head.center;
            FrameInput { cursor: Some(c + pt((i as f32 * 0.35).sin() * 28.0, -14.0)), ..Default::default() }
        },
    },
    Seq {
        name: "sleep -> wake",
        warmup: 60 * 7,
        every: 6,
        setup: none,
        step: |p, i| {
            if i == 60 * 7 + 6 {
                p.handle(InputEvent::MouseMove { dx: 2.0, dy: 0.0 });
            }
            if i < 60 * 7 + 6 {
                FrameInput::default()
            } else {
                far()
            }
        },
    },
];

fn record(seq: &Seq, skin: &Skin) -> Vec<Pose> {
    let cfg = MoodConfig { sleep_after: 6.0, ..Default::default() };
    let mut pet = Pet::new(11);
    (seq.setup)(&mut pet);
    let mut frames = Vec::new();
    let total = seq.warmup + seq.every * FRAMES_PER_ROW;
    for i in 0..total {
        let input = (seq.step)(&mut pet, i);
        pet.update(1.0 / FPS, &input, skin, &cfg);
        if i >= seq.warmup && (i - seq.warmup) % seq.every == 0 {
            frames.push(pet.pose().clone());
        }
    }
    frames
}

fn main() {
    let scale = 1.0;
    let crop = IntRect::from_xywh(40, 20, 290, 300).unwrap();
    let out = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../out");
    std::fs::create_dir_all(&out).unwrap();
    let mut renderer = Renderer::new(scale);
    let opts = RenderOptions { scale, ..Default::default() };
    for skin in [corgi(), corgi_tricolor(), corgi_merle()] {
        for (sheet_no, chunk) in SEQS.chunks(6).enumerate() {
            let cw = crop.width();
            let ch = crop.height();
            let mut sheet = Pixmap::new(cw * FRAMES_PER_ROW as u32, ch * chunk.len() as u32).unwrap();
            sheet.fill(Color::from_rgba8(0xE8, 0xEE, 0xF3, 255));
            for (row, seq) in chunk.iter().enumerate() {
                for (col, pose) in record(seq, &skin).iter().enumerate() {
                    let pm = renderer.render(pose, &skin, &opts);
                    let part = pm.clone_rect(crop).unwrap();
                    sheet.draw_pixmap((col as u32 * cw) as i32, (row as u32 * ch) as i32, part.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
                }
            }
            let name = format!("seq_{}_{}.png", skin.name.to_lowercase().replace(' ', "_"), sheet_no + 1);
            sheet.save_png(out.join(&name)).unwrap();
            println!("wrote out/{name}: {}", chunk.iter().map(|s| s.name).collect::<Vec<_>>().join(" | "));
        }
        // Close-ups of the hardest intermediate frames.
        let z = 2.5;
        let mut big = Renderer::new(z);
        let zopts = RenderOptions { scale: z, ..Default::default() };
        let picks = [(1usize, 4usize), (2, 5), (3, 2), (4, 7), (5, 3), (7, 3)];
        let zc = IntRect::from_xywh((120.0 * z) as i32, (95.0 * z) as i32, (230.0 * z) as u32, (225.0 * z) as u32).unwrap();
        let mut zs = Pixmap::new(zc.width() * 3, zc.height() * 2).unwrap();
        zs.fill(Color::from_rgba8(0xE8, 0xEE, 0xF3, 255));
        for (n, (seq, frame)) in picks.iter().enumerate() {
            let poses = record(&SEQS[*seq], &skin);
            let pm = big.render(&poses[*frame], &skin, &zopts);
            let part = pm.clone_rect(zc).unwrap();
            zs.draw_pixmap(((n % 3) as u32 * zc.width()) as i32, ((n / 3) as u32 * zc.height()) as i32, part.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
        }
        if let Ok(spec) = std::env::var("FRAME") {
            // FRAME=seq,frame,x,y,w,h renders one frame at 4x for inspection.
            let v: Vec<f32> = spec.split(',').filter_map(|x| x.parse().ok()).collect();
            if v.len() == 6 {
                let poses = record(&SEQS[v[0] as usize], &skin);
                let mut r4 = Renderer::new(4.0);
                let pm = r4.render(&poses[v[1] as usize], &skin, &RenderOptions { scale: 4.0, ..Default::default() });
                let c = IntRect::from_xywh((v[2] * 4.0) as i32, (v[3] * 4.0) as i32, (v[4] * 4.0) as u32, (v[5] * 4.0) as u32).unwrap();
                let mut bg = Pixmap::new(c.width(), c.height()).unwrap();
                bg.fill(Color::from_rgba8(0xE8, 0xEE, 0xF3, 255));
                bg.draw_pixmap(0, 0, pm.clone_rect(c).unwrap().as_ref(), &PixmapPaint::default(), Transform::identity(), None);
                bg.save_png(out.join(format!("frame_{}.png", skin.name.to_lowercase().replace(' ', "_")))).unwrap();
            }
        }
        let name = format!("zoom_{}.png", skin.name.to_lowercase().replace(' ', "_"));
        zs.save_png(out.join(&name)).unwrap();
        println!("wrote out/{name}");
    }
}
