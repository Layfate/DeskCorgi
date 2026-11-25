//! Measures how long one frame takes to simulate and render.
//! Run with `cargo run --release -p deskcorgi-core --example bench [scale]`.

use std::time::Instant;

use deskcorgi_core::geom::pt;
use deskcorgi_core::keymap::key;
use deskcorgi_core::skin::{corgi, corgi_merle, corgi_tricolor};
use deskcorgi_core::{FrameInput, InputEvent, MoodConfig, Pet, RenderOptions, Renderer};

fn main() {
    let scale: f32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1.275);
    for skin in [corgi(), corgi_tricolor(), corgi_merle()] {
        let mut pet = Pet::new(1);
        let mut r = Renderer::new(scale);
        let opts = RenderOptions { scale, ..Default::default() };
        let cfg = MoodConfig::default();
        let frames = 600;
        let start = Instant::now();
        for i in 0..frames {
            if i % 3 == 0 {
                pet.handle(InputEvent::KeyDown(key(2, 1 + i % 9)));
            } else {
                pet.handle(InputEvent::KeyUp(key(2, 1 + (i - 1) % 9)));
            }
            let input = FrameInput { cursor: Some(pt(100.0 + i as f32, 50.0)), ..Default::default() };
            pet.update(1.0 / 30.0, &input, &skin, &cfg);
            std::hint::black_box(r.render(pet.pose(), &skin, &opts));
        }
        let per = start.elapsed().as_secs_f64() * 1000.0 / frames as f64;
        println!("{}: {:.2} ms/frame at scale {scale} ({} x {} px)", skin.name, per, r.pixmap().width(), r.pixmap().height());
    }
}
