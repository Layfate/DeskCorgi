//! The art loop: check and preview `art/corgi.toml` without recompiling anything.
//!
//! ```text
//! cargo run -q -p deskcorgi-core --example art -- check  [--art FILE]
//! cargo run -q -p deskcorgi-core --example art -- sheet  [--art FILE] [--compare OLD] [--view dog|head]
//!                                                        [--skin corgi|all] [--cell PX] [--out FILE]
//!                                                        [--pose idle,happy,focus,sleep] [--turn -1,0,1]
//!   --view arms with --pose type,keys,far,mouse,click,wave shows the legs in real poses.
//! cargo run -q -p deskcorgi-core --example art -- diff OLD [--art FILE]
//! ```
//!
//! `sheet` writes one small contact sheet: rows are poses (idle, happy, focus, sleep), columns
//! are head turns (left, front, right). With `--compare OLD` every cell becomes an old | new
//! pair. `diff` prints how many pixels changed per pose, with no image at all.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use deskcorgi_core::art::{self, parse_art, Art, Shape, TURN_YAW};
use deskcorgi_core::geom::{pt, Pt};
use deskcorgi_core::rig::Rig;
use deskcorgi_core::scene::DOG_X;
use deskcorgi_core::skin::{self, Skin};
use deskcorgi_core::{Pose, RenderOptions, Renderer};
use tiny_skia::{Color, Pixmap, PixmapPaint, Transform};

const POSES: [&str; 4] = ["idle", "happy", "focus", "sleep"];
/// Poses taken from running the real behaviour for a moment, so the paws are exactly where the
/// app puts them (for `--view arms`).
const ARM_POSES: [&str; 6] = ["type", "keys", "far", "mouse", "click", "wave"];
const TURNS: [f32; 3] = [-1.0, 0.0, 1.0];

fn sim(name: &str, skin: &Skin) -> Pose {
    use deskcorgi_core::keymap::key;
    use deskcorgi_core::{FrameInput, InputEvent, MoodConfig, MouseButton, Pet};
    let cfg = MoodConfig { sleep: false, ..Default::default() };
    let mut pet = Pet::new(7);
    let far = Some(pt(-200.0, -120.0));
    let run = |pet: &mut Pet, secs: f32, f: &mut dyn FnMut(&mut Pet, usize) -> FrameInput| {
        for i in 0..(secs * 30.0) as usize {
            let input = f(pet, i);
            pet.update(1.0 / 30.0, &input, skin, &cfg);
        }
    };
    let still = |_: &mut Pet, _: usize| FrameInput { cursor: far, ..Default::default() };
    match name {
        "type" => {
            run(&mut pet, 0.6, &mut |p, i| {
                let k = key(2, 1 + (i / 4) % 9);
                if i % 4 == 0 {
                    p.handle(InputEvent::KeyDown(k));
                }
                if i % 4 == 2 {
                    p.handle(InputEvent::KeyUp(k));
                }
                FrameInput { cursor: far, ..Default::default() }
            });
            pet.handle(InputEvent::KeyDown(key(2, 2)));
            pet.handle(InputEvent::KeyDown(key(1, 9)));
            run(&mut pet, 0.15, &mut { still });
        }
        "keys" => {
            for k in [key(2, 1), key(2, 3), key(2, 4)] {
                pet.handle(InputEvent::KeyDown(k));
            }
            run(&mut pet, 0.6, &mut { still });
        }
        "far" => {
            pet.handle(InputEvent::KeyDown(key(1, 0)));
            pet.handle(InputEvent::KeyDown(key(1, 13)));
            run(&mut pet, 0.6, &mut { still });
        }
        "mouse" | "click" => {
            let cursor = Some(pt(-100.0, 200.0));
            run(&mut pet, 0.6, &mut |p, i| {
                p.handle(InputEvent::MouseMove { dx: if i % 10 < 5 { 25.0 } else { -10.0 }, dy: 6.0 });
                FrameInput { cursor, ..Default::default() }
            });
            if name == "click" {
                pet.handle(InputEvent::MouseButton { button: MouseButton::Left, down: true });
            }
            run(&mut pet, 0.1, &mut |_, _| FrameInput { cursor, ..Default::default() });
        }
        "wave" => {
            run(&mut pet, 0.2, &mut { still });
            pet.wave();
            run(&mut pet, 0.5, &mut { still });
        }
        _ => run(&mut pet, 0.5, &mut { still }),
    }
    pet.pose().clone()
}

fn pose(name: &str, turn: f32, skin: &Skin) -> Pose {
    if ARM_POSES.contains(&name) {
        let mut p = sim(name, skin);
        p.head_yaw = turn * TURN_YAW;
        return p;
    }
    let mut p = Pose::neutral();
    p.head_yaw = turn * TURN_YAW;
    p.gaze = pt(turn * 0.8, 0.0);
    match name {
        "happy" => {
            p.eye_happy = 1.0;
            p.mouth_open = 0.75;
            p.tongue = 0.9;
            p.blush = 0.9;
            p.ear_perk = 1.0;
        }
        "focus" => {
            p.focus = 1.0;
            p.head_pitch = 0.08;
        }
        "sleep" => {
            p.eye_open = 0.0;
            p.ear_fold = 0.7;
            p.head_drop = 5.0;
            p.head_tilt = 0.12;
            p.blush = 0.5;
        }
        _ => {}
    }
    p
}

struct Args {
    cmd: String,
    art: PathBuf,
    compare: Option<PathBuf>,
    view: String,
    skins: Vec<Skin>,
    cell: Option<u32>,
    out: PathBuf,
    poses: Vec<&'static str>,
    turns: Vec<f32>,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn parse_args() -> Result<Args, String> {
    let mut it = std::env::args().skip(1);
    let cmd = it.next().unwrap_or_else(|| "sheet".into());
    let mut a = Args {
        cmd,
        art: root().join("art/corgi.toml"),
        compare: None,
        view: "dog".into(),
        skins: vec![skin::corgi()],
        cell: None,
        out: root().join("out/art/sheet.png"),
        poses: POSES.to_vec(),
        turns: TURNS.to_vec(),
    };
    while let Some(flag) = it.next() {
        let mut val = || it.next().ok_or(format!("{flag} needs a value"));
        match flag.as_str() {
            "--art" => a.art = PathBuf::from(val()?),
            "--compare" => a.compare = Some(PathBuf::from(val()?)),
            "--view" => a.view = val()?,
            "--cell" => a.cell = Some(val()?.parse().map_err(|_| "--cell takes a number of pixels")?),
            "--out" => a.out = PathBuf::from(val()?),
            "--pose" => {
                let v = val()?;
                a.poses = v
                    .split(',')
                    .map(|n| POSES.iter().chain(&ARM_POSES).copied().find(|p| *p == n.trim()).ok_or(format!("unknown pose {n} ({}, {})", POSES.join(", "), ARM_POSES.join(", "))))
                    .collect::<Result<_, _>>()?;
            }
            "--turn" => {
                let v = val()?;
                a.turns = v.split(',').map(|n| n.trim().parse::<f32>().map(|t| t.clamp(-1.0, 1.0)).map_err(|_| "--turn takes numbers like -1,0,0.5,1".to_string())).collect::<Result<_, _>>()?;
            }
            "--skin" => {
                let v = val()?;
                a.skins = match v.as_str() {
                    "all" => skin::BUILTINS.iter().map(|(_, f)| f()).collect(),
                    id => vec![skin::builtin(id).ok_or(format!("unknown skin {id} (corgi, all)"))?],
                };
            }
            other if a.cmd == "diff" && a.compare.is_none() && !other.starts_with("--") => a.compare = Some(PathBuf::from(other)),
            other => return Err(format!("unknown argument {other}")),
        }
    }
    Ok(a)
}

fn load(path: &Path) -> Result<Art, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_art(&text).map_err(|e| format!("{}: {e}", path.display()))
}

// ---- sheet ------------------------------------------------------------------------------------

/// Canvas rectangle shown in a cell: (x0, y0, width, height).
fn region(view: &str, skin: &Skin, pose: &Pose) -> (f32, f32, f32, f32) {
    match view {
        "head" => {
            let c = Rig::new(&skin.shape, pose).head.center;
            (c.x - 95.0, c.y - 128.0, 190.0, 204.0)
        }
        "arms" => (DOG_X - 172.0, 150.0, 290.0, 172.0),
        _ => (DOG_X - 118.0, 22.0, 236.0, 300.0),
    }
}

fn render_cell(r: &mut Renderer, skin: &Skin, pose: &Pose, view: &str, cell_w: u32) -> Pixmap {
    let (x0, y0, w, h) = region(view, skin, pose);
    let scale = cell_w as f32 / w;
    let pm = r.render(pose, skin, &RenderOptions { scale, ..Default::default() });
    let ch = (h * scale).round() as u32;
    let mut cell = Pixmap::new(cell_w, ch).expect("cell");
    cell.fill(Color::from_rgba8(0xE8, 0xEE, 0xF3, 255));
    cell.draw_pixmap((-x0 * scale).round() as i32, (-y0 * scale).round() as i32, pm.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    cell
}

fn sheet(a: &Args) -> Result<(), String> {
    let new = load(&a.art)?;
    let old = a.compare.as_deref().map(load).transpose()?;
    let arts: Vec<&Art> = old.iter().chain(std::iter::once(&new)).collect();
    let cell_w = a.cell.unwrap_or(if arts.len() > 1 { 150 } else { 180 });
    let gap = 4u32;
    let mut cells: Vec<Vec<Pixmap>> = Vec::new();
    for skin in &a.skins {
        for &name in &a.poses {
            let mut row = Vec::new();
            for &turn in &a.turns {
                for art in &arts {
                    let mut r = Renderer::new(1.0);
                    r.set_art((*art).clone());
                    row.push(render_cell(&mut r, skin, &pose(name, turn, skin), &a.view, cell_w));
                }
            }
            cells.push(row);
        }
    }
    let ch = cells[0][0].height();
    let per_turn = arts.len() as u32;
    let nt = a.turns.len() as u32;
    let cols = nt * per_turn;
    // Pairs sit tight together; a wider gap separates the turns.
    let pair_gap = if per_turn > 1 { 1 } else { gap };
    let width = cols * cell_w + (nt - 1) * gap * 2 + (per_turn - 1) * nt * pair_gap;
    let height = cells.len() as u32 * (ch + gap) - gap;
    let mut sheet = Pixmap::new(width, height).ok_or("sheet too large")?;
    sheet.fill(Color::from_rgba8(0x9A, 0xA4, 0xB0, 255));
    for (ri, row) in cells.iter().enumerate() {
        let mut x = 0u32;
        for (ci, cell) in row.iter().enumerate() {
            if ci > 0 {
                x += if ci as u32 % per_turn == 0 { gap * 2 } else { pair_gap };
            }
            sheet.draw_pixmap(x as i32, (ri as u32 * (ch + gap)) as i32, cell.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
            x += cell_w;
        }
    }
    if let Some(dir) = a.out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    sheet.save_png(&a.out).map_err(|e| e.to_string())?;
    let skins: Vec<&str> = a.skins.iter().map(|s| s.name.as_str()).collect();
    println!("wrote {} ({}x{})", a.out.display(), width, height);
    let turns: Vec<String> = a.turns.iter().map(|t| format!("{t}")).collect();
    println!("rows: {} per skin [{}]; cols: turn {}{}", a.poses.join(", "), skins.join(", "), turns.join(", "), if per_turn > 1 { " (each: old | new)" } else { "" });
    Ok(())
}

// ---- diff -------------------------------------------------------------------------------------

fn diff(a: &Args) -> Result<(), String> {
    let new = load(&a.art)?;
    let old = load(a.compare.as_deref().ok_or("diff needs the old art file")?)?;
    let mut total = 0usize;
    for skin in &a.skins {
        for &name in &a.poses {
            for &turn in &a.turns {
                let p = pose(name, turn, skin);
                let mut r_old = Renderer::new(1.0);
                r_old.set_art(old.clone());
                let mut r_new = Renderer::new(1.0);
                r_new.set_art(new.clone());
                let pa = r_old.render(&p, skin, &RenderOptions::default()).clone();
                let pb = r_new.render(&p, skin, &RenderOptions::default());
                let w = pa.width();
                let (mut n, mut lo, mut hi) = (0usize, (u32::MAX, u32::MAX), (0u32, 0u32));
                for (i, (x, y)) in pa.pixels().iter().zip(pb.pixels()).enumerate() {
                    let d = (x.red() as i32 - y.red() as i32).abs().max((x.green() as i32 - y.green() as i32).abs()).max((x.blue() as i32 - y.blue() as i32).abs()).max((x.alpha() as i32 - y.alpha() as i32).abs());
                    if d > 8 {
                        n += 1;
                        let (px, py) = (i as u32 % w, i as u32 / w);
                        lo = (lo.0.min(px), lo.1.min(py));
                        hi = (hi.0.max(px), hi.1.max(py));
                    }
                }
                total += n;
                let area = if n > 0 { format!("  in x {}..{}, y {}..{}", lo.0, hi.0, lo.1, hi.1) } else { String::new() };
                println!("{:<18} {:<6} turn {:>4}: {:>6} px changed{area}", skin.name, name, turn, n);
            }
        }
    }
    println!("total changed pixels: {total}");
    Ok(())
}

// ---- check ------------------------------------------------------------------------------------

fn seg_dist(p: Pt, a: Pt, b: Pt) -> f32 {
    let ab = b - a;
    let t = (((p.x - a.x) * ab.x + (p.y - a.y) * ab.y) / (ab.x * ab.x + ab.y * ab.y).max(1e-9)).clamp(0.0, 1.0);
    (p - (a + ab * t)).len()
}

fn ring_dist(p: Pt, ring: &[Pt], closed: bool) -> f32 {
    let n = ring.len();
    let m = if closed { n } else { n - 1 };
    (0..m).map(|i| seg_dist(p, ring[i], ring[(i + 1) % n])).fold(f32::MAX, f32::min)
}

fn cross(o: Pt, a: Pt, b: Pt) -> f32 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

fn crossings(ring: &[Pt]) -> usize {
    let n = ring.len();
    let mut count = 0;
    for i in 0..n {
        for j in i + 2..n {
            if i == 0 && j == n - 1 {
                continue;
            }
            let (a, b, c, d) = (ring[i], ring[(i + 1) % n], ring[j], ring[(j + 1) % n]);
            if cross(a, b, c) * cross(a, b, d) < 0.0 && cross(c, d, a) * cross(c, d, b) < 0.0 {
                count += 1;
            }
        }
    }
    count
}

fn check_shape(issues: &mut Vec<String>, id: &str, label: &str, s: &Shape) {
    for (k, (ring, sub)) in s.polys(1.5).iter().zip(&s.subs).enumerate() {
        if sub.closed && ring.len() > 3 {
            let x = crossings(ring);
            if x > 0 {
                issues.push(format!("{id} ({label}, subpath {k}): outline crosses itself {x} time(s)"));
            }
        }
    }
}

fn check(a: &Args) -> Result<bool, String> {
    let art = load(&a.art)?;
    let mut issues = Vec::new();
    println!("{} parts, style {:?}", art.parts.len(), art.style);
    for part in &art.parts {
        let segs: usize = part.front.subs.iter().map(|s| s.segs.len()).sum();
        let (lo, hi) = part.front.bounds();
        let mut line = format!("  {:<13} {:>2} segs  x {:>6.1}..{:<6.1} y {:>6.1}..{:<6.1}", part.id, segs, lo.x, hi.x, lo.y, hi.y);
        check_shape(&mut issues, &part.id, "front", &part.front);
        if part.symmetric {
            let ring = part.front.outline();
            let closed = part.front.subs[0].closed;
            let mirrored: Vec<Pt> = ring.iter().map(|q| pt(-q.x, q.y)).collect();
            let dev = ring.iter().map(|q| ring_dist(*q, &mirrored, closed)).fold(0.0f32, f32::max);
            line += &format!("  sym dev {dev:.2}");
            if dev > 0.75 {
                issues.push(format!("{}: marked symmetric but is {dev:.2} units off its mirror image", part.id));
            }
        }
        if part.turn.is_some() {
            let f = part.at(0.0).outline();
            let t = part.at(1.0).outline();
            let travel = f.iter().zip(&t).map(|(x, y)| (*x - *y).len()).fold(0.0f32, f32::max);
            line += &format!("  turn travel {travel:.1}");
            for (label, k) in [("turn", 1.0), ("half turn", 0.5), ("turn left", -1.0)] {
                check_shape(&mut issues, &part.id, label, &part.at(k));
            }
        }
        println!("{line}");
        if !art::REQUIRED.contains(&part.id.as_str()) {
            issues.push(format!("{}: not used by the renderer (typo?)", part.id));
        }
    }
    // Render sanity: every skin, every turn.
    for (_, f) in skin::BUILTINS {
        let skin = f();
        for turn in TURNS {
            let p = pose("idle", turn, &skin);
            let mut r = Renderer::new(1.0);
            r.set_art(art.clone());
            let pm = r.render(&p, &skin, &RenderOptions::default());
            let filled = pm.pixels().iter().filter(|q| q.alpha() > 200).count() as f32 / (pm.width() * pm.height()) as f32;
            if !(0.15..0.7).contains(&filled) {
                issues.push(format!("{} turn {turn}: {:.0}% of the canvas is filled (expected 15-70%)", skin.name, filled * 100.0));
            }
            let c = Rig::new(&skin.shape, &p).head.center;
            if pm.pixel(c.x as u32, c.y as u32).map_or(true, |q| q.alpha() < 255) {
                issues.push(format!("{} turn {turn}: the head centre is not opaque", skin.name));
            }
        }
    }
    if issues.is_empty() {
        println!("OK: no issues");
        Ok(true)
    } else {
        for i in &issues {
            println!("ISSUE: {i}");
        }
        Ok(false)
    }
}

fn main() -> ExitCode {
    let result = parse_args().and_then(|a| match a.cmd.as_str() {
        "sheet" => sheet(&a).map(|_| true),
        "diff" => diff(&a).map(|_| true),
        "check" => check(&a),
        other => Err(format!("unknown command {other} (check, sheet, diff)")),
    });
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
