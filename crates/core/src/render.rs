//! Draws a frame into a premultiplied RGBA pixmap with tiny-skia.
//!
//! All geometry is computed in canvas units; one global scale transform turns that into
//! pixels. Parts are drawn back to front: shadow, tail, torso, ears, head, mouse, keyboard,
//! front legs, back legs, particles.

use std::f32::consts::PI;

use tiny_skia::{
    Color, FillRule, GradientStop, LineCap, LineJoin, LinearGradient, Mask, Paint, Path, PathBuilder, Pixmap,
    PixmapPaint, Point, RadialGradient, Shader, SpreadMode, Stroke, Transform,
};

use crate::color::{rgb, rgba, Rgba, WHITE};
use crate::config::{EyeColor, KeyboardColors, KeyboardTheme};
use crate::geom::{clamp01, lerp, pt, smoothstep, Affine, Pt};
use crate::glyphs;
use crate::keymap::{all_keys, key_kind, KeyId, KeyKind, KEY_COUNT};
use crate::limb::{fillets, front_bend, point_in_polygon, skin_limb, solve_elbow, Fillet, LimbShape, Profile};
use crate::pet::{ParticleKind, Pose};
use crate::rig::{HeadXf, Rig, TORSO_H};
use crate::scene::*;
use crate::skin::{Chest, FacePattern, MarkKind, Region, Skin, Space, TailStyle};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RenderOptions {
    /// Pixels per canvas unit.
    pub scale: f32,
    pub eye_color: EyeColor,
    pub keyboard: KeyboardTheme,
    pub key_glow: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions { scale: 1.0, eye_color: EyeColor::Brown, keyboard: KeyboardTheme::Cream, key_glow: false }
    }
}

pub fn pixel_size(scale: f32) -> (u32, u32) {
    let s = if scale.is_finite() { scale.clamp(0.1, 8.0) } else { 1.0 };
    (((CANVAS_W * s).ceil() as u32).max(1), ((CANVAS_H * s).ceil() as u32).max(1))
}

pub struct Renderer {
    pixmap: Pixmap,
    masks: [Mask; 3],
    scale: f32,
    keyboard: Option<Layer>,
    ground: Option<Layer>,
}

/// A static part of the scene pre-rendered once per theme and size, then blitted each frame.
/// The keyboard layer holds every key at rest; only lit or pressed keys are redrawn on top.
struct Layer {
    pixmap: Pixmap,
    x: i32,
    y: i32,
    theme: KeyboardTheme,
    width: u32,
}

impl Layer {
    /// Renders `draw` into a pixmap covering the canvas rectangle `(x0, y0)–(x1, y1)`.
    fn build(theme: KeyboardTheme, s: f32, width: u32, (x0, y0, x1, y1): (f32, f32, f32, f32), draw: impl FnOnce(&mut Painter)) -> Layer {
        let x = (x0 * s).floor() as i32;
        let y = (y0 * s).floor() as i32;
        let w = ((x1 * s).ceil() as i32 - x).max(1) as u32;
        let h = ((y1 * s).ceil() as i32 - y).max(1) as u32;
        let mut pixmap = new_pixmap(w, h);
        let [mut m0, mut m1, mut m2] = new_masks(1, 1);
        let ts = Transform::from_row(s, 0.0, 0.0, s, -x as f32, -y as f32);
        let mut p = Painter { pm: &mut pixmap, ts, head: &mut m0, body: &mut m1, tmp: &mut m2 };
        draw(&mut p);
        Layer { pixmap, x, y, theme, width }
    }

    fn valid(layer: &Option<Layer>, theme: KeyboardTheme, width: u32) -> bool {
        layer.as_ref().is_some_and(|l| l.theme == theme && l.width == width)
    }

    fn blit(&self, p: &mut Painter) {
        p.pm.draw_pixmap(self.x, self.y, self.pixmap.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }
}

impl Renderer {
    pub fn new(scale: f32) -> Renderer {
        let (w, h) = pixel_size(scale);
        Renderer { pixmap: new_pixmap(w, h), masks: new_masks(w, h), scale, keyboard: None, ground: None }
    }

    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }

    pub fn render(&mut self, pose: &Pose, skin: &Skin, opts: &RenderOptions) -> &Pixmap {
        let (w, h) = pixel_size(opts.scale);
        if self.pixmap.width() != w || self.pixmap.height() != h || self.scale != opts.scale {
            self.pixmap = new_pixmap(w, h);
            self.masks = new_masks(w, h);
            self.scale = opts.scale;
        }
        self.pixmap.fill(Color::TRANSPARENT);
        let s = w as f32 / CANVAS_W;
        let theme = opts.keyboard;
        if !Layer::valid(&self.keyboard, theme, w) {
            let rect = (KB_BL.x - 6.0, KB_TL.y - 6.0, KB_BR.x + 6.0, KB_BL.y + KB_FRONT + 6.0);
            self.keyboard = Some(Layer::build(theme, s, w, rect, |p| draw_keyboard_base(p, &theme.colors())));
        }
        if !Layer::valid(&self.ground, theme, w) {
            let rect = (0.0, PAD_CENTER.y - PAD_H, CANVAS_W, CANVAS_H);
            self.ground = Some(Layer::build(theme, s, w, rect, |p| {
                draw_shadow(p);
                draw_mouse_pad(p, &theme.colors());
            }));
        }
        let [m0, m1, m2] = &mut self.masks;
        let mut p = Painter { pm: &mut self.pixmap, ts: Transform::from_scale(s, s), head: m0, body: m1, tmp: m2 };
        draw_scene(&mut p, pose, skin, opts, self.ground.as_ref(), self.keyboard.as_ref());
        &self.pixmap
    }
}

fn new_pixmap(w: u32, h: u32) -> Pixmap {
    Pixmap::new(w.max(1), h.max(1)).unwrap_or_else(|| Pixmap::new(1, 1).expect("1x1 pixmap"))
}

fn new_masks(w: u32, h: u32) -> [Mask; 3] {
    let m = || Mask::new(w.max(1), h.max(1)).unwrap_or_else(|| Mask::new(1, 1).expect("1x1 mask"));
    [m(), m(), m()]
}

/// A square icon of the dog's face (tray icon, window icon).
pub fn render_icon(skin: &Skin, size: u32, eye_color: EyeColor) -> Pixmap {
    let size = size.clamp(8, 512);
    let mut pm = new_pixmap(size, size);
    let [mut m0, mut m1, mut m2] = new_masks(size, size);
    let pose = Pose { blush: 0.5, ..Pose::neutral() };
    let rig = Rig::new(&skin.shape, &pose);
    let c = rig.head.center;
    let k = size as f32 / 168.0;
    let ts = Transform::from_row(k, 0.0, 0.0, k, size as f32 / 2.0 - c.x * k, size as f32 * 0.56 - (c.y) * k);
    let mut p = Painter { pm: &mut pm, ts, head: &mut m0, body: &mut m1, tmp: &mut m2 };
    let opts = RenderOptions { eye_color, ..Default::default() };
    draw_ears(&mut p, &pose, skin, &rig);
    draw_head(&mut p, &pose, skin, &rig, &opts);
    pm
}

// -------------------------------------------------------------------------------------------
// Painting helpers
// -------------------------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum Clip {
    Head,
    Body,
    Tmp,
}

struct Painter<'a> {
    pm: &'a mut Pixmap,
    ts: Transform,
    head: &'a mut Mask,
    body: &'a mut Mask,
    tmp: &'a mut Mask,
}

fn paint(c: Rgba) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color_rgba8(c.r, c.g, c.b, c.a);
    p.anti_alias = true;
    p
}

fn stroke_style(width: f32, cap: LineCap) -> Stroke {
    Stroke { width, line_cap: cap, line_join: LineJoin::Round, ..Stroke::default() }
}

impl Painter<'_> {
    fn fill(&mut self, path: &Option<Path>, c: Rgba) {
        if let Some(path) = path {
            if c.a > 0 {
                self.pm.fill_path(path, &paint(c), FillRule::Winding, self.ts, None);
            }
        }
    }

    fn fill_clipped(&mut self, path: &Option<Path>, c: Rgba, clip: Clip) {
        let Some(path) = path else { return };
        if c.a == 0 {
            return;
        }
        let mask: &Mask = match clip {
            Clip::Head => &*self.head,
            Clip::Body => &*self.body,
            Clip::Tmp => &*self.tmp,
        };
        self.pm.fill_path(path, &paint(c), FillRule::Winding, self.ts, Some(mask));
    }

    fn fill_shader(&mut self, path: &Option<Path>, shader: Option<Shader<'static>>, fallback: Rgba) {
        let Some(path) = path else { return };
        match shader {
            Some(shader) => {
                let p = Paint { shader, anti_alias: true, ..Paint::default() };
                self.pm.fill_path(path, &p, FillRule::Winding, self.ts, None);
            }
            None => self.fill(&Some(path.clone()), fallback),
        }
    }

    fn fill_shader_clipped(&mut self, path: &Option<Path>, shader: Option<Shader<'static>>, fallback: Rgba, clip: Option<Clip>) {
        let Some(clip) = clip else { return self.fill_shader(path, shader, fallback) };
        let Some(path) = path else { return };
        let paint = match shader {
            Some(shader) => Paint { shader, anti_alias: true, ..Paint::default() },
            None => paint(fallback),
        };
        let mask: &Mask = match clip {
            Clip::Head => &*self.head,
            Clip::Body => &*self.body,
            Clip::Tmp => &*self.tmp,
        };
        self.pm.fill_path(path, &paint, FillRule::Winding, self.ts, Some(mask));
    }

    fn stroke(&mut self, path: &Option<Path>, c: Rgba, width: f32) {
        self.stroke_cap(path, c, width, LineCap::Round);
    }

    fn stroke_cap(&mut self, path: &Option<Path>, c: Rgba, width: f32, cap: LineCap) {
        if let Some(path) = path {
            if c.a > 0 && width > 0.0 {
                self.pm.stroke_path(path, &paint(c), &stroke_style(width, cap), self.ts, None);
            }
        }
    }

    fn stroke_clipped(&mut self, path: &Option<Path>, c: Rgba, width: f32, cap: LineCap, clip: Clip) {
        let Some(path) = path else { return };
        let mask: &Mask = match clip {
            Clip::Head => &*self.head,
            Clip::Body => &*self.body,
            Clip::Tmp => &*self.tmp,
        };
        self.pm.stroke_path(path, &paint(c), &stroke_style(width, cap), self.ts, Some(mask));
    }

    fn set_mask(&mut self, clip: Clip, path: &Option<Path>) {
        let ts = self.ts;
        let mask: &mut Mask = match clip {
            Clip::Head => &mut *self.head,
            Clip::Body => &mut *self.body,
            Clip::Tmp => &mut *self.tmp,
        };
        mask.clear();
        if let Some(path) = path {
            mask.fill_path(path, FillRule::Winding, true, ts);
        }
    }
}

fn poly(pts: &[Pt]) -> Option<Path> {
    if pts.len() < 3 || !pts.iter().all(|p| p.is_finite()) {
        return None;
    }
    let mut pb = PathBuilder::with_capacity(pts.len() + 1, pts.len() + 1);
    pb.move_to(pts[0].x, pts[0].y);
    for p in &pts[1..] {
        pb.line_to(p.x, p.y);
    }
    pb.close();
    pb.finish()
}

fn polyline(pts: &[Pt]) -> Option<Path> {
    if pts.len() < 2 || !pts.iter().all(|p| p.is_finite()) {
        return None;
    }
    let mut pb = PathBuilder::with_capacity(pts.len(), pts.len());
    pb.move_to(pts[0].x, pts[0].y);
    for p in &pts[1..] {
        pb.line_to(p.x, p.y);
    }
    pb.finish()
}

fn circle(c: Pt, r: f32) -> Option<Path> {
    if !c.is_finite() || r.is_nan() || r <= 0.0 {
        return None;
    }
    PathBuilder::from_circle(c.x, c.y, r)
}

fn quad_path(a: Pt, ctrl: Pt, b: Pt) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    pb.quad_to(ctrl.x, ctrl.y, b.x, b.y);
    pb.finish()
}

fn ellipse_pts(c: Pt, rx: f32, ry: f32, n: usize) -> Vec<Pt> {
    (0..n).map(|i| {
        let a = i as f32 / n as f32 * 2.0 * PI;
        c + pt(a.cos() * rx, a.sin() * ry)
    })
    .collect()
}

fn cubic(p0: Pt, p1: Pt, p2: Pt, p3: Pt, t: f32) -> Pt {
    let u = 1.0 - t;
    p0 * (u * u * u) + p1 * (3.0 * u * u * t) + p2 * (3.0 * u * t * t) + p3 * (t * t * t)
}

fn quad(p0: Pt, p1: Pt, p2: Pt, t: f32) -> Pt {
    let u = 1.0 - t;
    p0 * (u * u) + p1 * (2.0 * u * t) + p2 * (t * t)
}

/// Tessellates a chain of cubic segments `[start, (c1, c2, end)...]` into points.
fn cubic_chain(start: Pt, segs: &[(Pt, Pt, Pt)], n: usize) -> Vec<Pt> {
    let mut out = Vec::with_capacity(segs.len() * n + 1);
    let mut p0 = start;
    out.push(p0);
    for &(c1, c2, p3) in segs {
        for i in 1..=n {
            out.push(cubic(p0, c1, c2, p3, i as f32 / n as f32));
        }
        p0 = p3;
    }
    out
}

/// Closed Catmull-Rom spline through `pts`, tessellated.
fn smooth_closed(pts: &[Pt], n: usize) -> Vec<Pt> {
    let len = pts.len();
    let mut out = Vec::with_capacity(len * n);
    if len < 3 {
        return pts.to_vec();
    }
    for i in 0..len {
        let p0 = pts[(i + len - 1) % len];
        let p1 = pts[i];
        let p2 = pts[(i + 1) % len];
        let p3 = pts[(i + 2) % len];
        let c1 = p1 + (p2 - p0) * (1.0 / 6.0);
        let c2 = p2 - (p3 - p1) * (1.0 / 6.0);
        for k in 0..n {
            out.push(cubic(p1, c1, c2, p2, k as f32 / n as f32));
        }
    }
    out
}

/// Polygon with rounded corners.
fn rounded_poly(pts: &[Pt], r: f32) -> Option<Path> {
    let n = pts.len();
    if n < 3 {
        return None;
    }
    let mut pb = PathBuilder::new();
    for i in 0..n {
        let prev = pts[(i + n - 1) % n];
        let cur = pts[i];
        let next = pts[(i + 1) % n];
        let r1 = r.min((prev - cur).len() * 0.45);
        let r2 = r.min((next - cur).len() * 0.45);
        let a = cur + (prev - cur).normalized() * r1;
        let b = cur + (next - cur).normalized() * r2;
        if i == 0 {
            pb.move_to(a.x, a.y);
        } else {
            pb.line_to(a.x, a.y);
        }
        pb.quad_to(cur.x, cur.y, b.x, b.y);
    }
    pb.close();
    pb.finish()
}

fn map_pts(pts: &[Pt], f: impl Fn(Pt) -> Pt) -> Vec<Pt> {
    pts.iter().map(|&p| f(p)).collect()
}

fn affine_path(path: Option<Path>, a: Affine) -> Option<Path> {
    path?.transform(a.to_skia())
}

fn linear(a: Pt, b: Pt, c0: Rgba, c1: Rgba) -> Option<Shader<'static>> {
    LinearGradient::new(
        Point::from_xy(a.x, a.y),
        Point::from_xy(b.x, b.y),
        vec![GradientStop::new(0.0, c0.to_skia()), GradientStop::new(1.0, c1.to_skia())],
        SpreadMode::Pad,
        Transform::identity(),
    )
}

// -------------------------------------------------------------------------------------------
// Scene
// -------------------------------------------------------------------------------------------

fn draw_scene(p: &mut Painter, pose: &Pose, skin: &Skin, o: &RenderOptions, ground: Option<&Layer>, keyboard: Option<&Layer>) {
    let rig = Rig::new(&skin.shape, pose);
    let kb = o.keyboard.colors();
    match ground {
        Some(g) => g.blit(p),
        None => {
            draw_shadow(p);
            draw_mouse_pad(p, &kb);
        }
    }
    draw_tail(p, pose, skin, &rig);
    let legs = Legs::new(pose, skin, &rig);
    draw_body(p, skin, &rig, &legs);
    draw_ears(p, pose, skin, &rig);
    draw_head(p, pose, skin, &rig, o);
    draw_mouse(p, pose, &kb);
    match keyboard {
        Some(k) => k.blit(p),
        None => draw_keyboard_base(p, &kb),
    }
    draw_key_overlay(p, pose, &kb, o.key_glow);
    draw_legs_over_keyboard(p, pose, &legs, skin);
    draw_key_popups(p, pose, &kb);
    draw_particles(p, pose);
}

fn draw_shadow(p: &mut Painter) {
    let c = pt(DOG_X - 22.0, KB_BL.y + KB_FRONT + 4.0);
    let (rx, ry) = (178.0, 16.0);
    let k = ry / rx;
    let shader = RadialGradient::new(
        Point::from_xy(c.x, c.y),
        Point::from_xy(c.x, c.y),
        rx,
        vec![
            GradientStop::new(0.0, Color::from_rgba8(40, 24, 10, 70)),
            GradientStop::new(0.6, Color::from_rgba8(40, 24, 10, 34)),
            GradientStop::new(1.0, Color::from_rgba8(40, 24, 10, 0)),
        ],
        SpreadMode::Pad,
        Transform::from_row(1.0, 0.0, 0.0, k, 0.0, c.y * (1.0 - k)),
    );
    let path = poly(&ellipse_pts(c, rx, ry, 48));
    p.fill_shader(&path, shader, rgba(0, 0, 0, 0));
}

// ---- torso ----------------------------------------------------------------------------------

fn torso_pts(skin: &Skin) -> Vec<Pt> {
    let bf = skin.shape.butt_fluff;
    let top = -TORSO_H;
    let hip = 60.0 + 16.0 * (bf - 1.0);
    let right = [
        (pt(24.0, top), pt(40.0, top + 12.0), pt(42.0, -100.0)),
        (pt(44.0, -78.0), pt(hip - 5.0, -56.0), pt(hip, -28.0)),
        (pt(hip + 3.0, -6.0), pt(50.0, 8.0), pt(0.0, 8.0)),
    ];
    let mut pts = cubic_chain(pt(0.0, top), &right, 18);
    let left: Vec<(Pt, Pt, Pt)> = vec![
        (pt(-50.0, 8.0), pt(-hip - 3.0, -6.0), pt(-hip, -28.0)),
        (pt(-hip + 5.0, -56.0), pt(-44.0, -78.0), pt(-42.0, -100.0)),
        (pt(-40.0, top + 12.0), pt(-24.0, top), pt(0.0, top)),
    ];
    let more = cubic_chain(pt(0.0, 8.0), &left, 18);
    pts.extend_from_slice(&more[1..more.len() - 1]);
    // A soft fur fringe along the flanks for fluffy builds.
    if bf > 1.05 {
        let amp = (bf - 1.0) * 6.0;
        for q in pts.iter_mut() {
            if q.y > -90.0 && q.y < -20.0 {
                let env = smoothstep(-90.0, -70.0, q.y) * (1.0 - smoothstep(-40.0, -20.0, q.y));
                let bump = (0.5 - 0.5 * (q.y * 0.32).cos()).powf(0.8);
                q.x += q.x.signum() * amp * env * bump;
            }
        }
    }
    pts
}

/// The chest fur marking in body space, if the skin has one.
fn chest_pts(skin: &Skin) -> Option<Vec<Pt>> {
    match skin.shape.chest {
        Chest::Ruff => {
            let mut pts = vec![pt(-31.0, -140.0), pt(31.0, -140.0)];
            pts.extend(cubic_chain(pt(31.0, -140.0), &[(pt(37.0, -116.0), pt(35.0, -92.0), pt(29.0, -70.0))], 10).into_iter().skip(1));
            // scalloped bottom edge
            let n = 4;
            for i in 0..n {
                let x0 = 29.0 - 58.0 * i as f32 / n as f32;
                let x1 = 29.0 - 58.0 * (i + 1) as f32 / n as f32;
                let ctrl = pt((x0 + x1) / 2.0, -50.0);
                for k in 1..=6 {
                    pts.push(quad(pt(x0, -70.0), ctrl, pt(x1, -70.0), k as f32 / 6.0));
                }
            }
            pts.extend(cubic_chain(pt(-29.0, -70.0), &[(pt(-35.0, -92.0), pt(-37.0, -116.0), pt(-31.0, -140.0))], 10).into_iter().skip(1));
            Some(pts)
        }
        Chest::Bib => Some(cubic_chain(
            pt(-27.0, -142.0),
            &[
                (pt(-4.0, -142.0), pt(4.0, -142.0), pt(27.0, -142.0)),
                (pt(32.0, -108.0), pt(24.0, -66.0), pt(0.0, -44.0)),
                (pt(-24.0, -66.0), pt(-32.0, -108.0), pt(-27.0, -142.0)),
            ],
            14,
        )),
        Chest::None => None,
    }
}

/// Visible outline width used everywhere on the body and legs.
const INK: f32 = 2.4;

/// Torso and legs as one piece. The outline is "inked" first as a thick stroke under every
/// fill, so it only survives on the outside of the union of torso and legs: shoulders and
/// hips have no seams. Where a leg passes in front of the body it gets an occlusion line
/// (a band just outside its edge) that fades in below the joint.
fn draw_body(p: &mut Painter, skin: &Skin, rig: &Rig, legs: &Legs) {
    let pal = &skin.palette;
    let torso = poly(&legs.torso);

    // 1. Ink for the silhouette of torso ∪ legs.
    p.stroke(&torso, pal.outline, INK * 2.0);
    for leg in &legs.items {
        p.stroke(&poly(&leg.outline), pal.outline, INK * 2.0);
        ink_fillets(p, leg, skin, None);
    }

    // 2. Torso fill and markings.
    let (top, bottom) = legs.shade;
    let shade = pal.body_shade.unwrap_or(pal.body);
    p.fill_shader(&torso, linear(top, bottom, pal.body, shade), pal.body);
    p.set_mask(Clip::Body, &torso);
    if let Some(pts) = chest_pts(skin) {
        p.fill_clipped(&poly(&map_pts(&pts, |q| rig.body.map(q))), pal.belly, Clip::Body);
    }
    draw_custom(p, skin, Space::Body, rig, None);

    // 3. Legs, back to front: fill, thin contour where in front of the body, then the
    //    fillets that merge the leg's silhouette into the body's.
    for leg in &legs.items {
        fill_leg(p, legs, leg, skin, None, 0);
        edge_bands(p, leg, skin, None, 0, [&leg.contour[0], &leg.contour[1]]);
        fill_fillets(p, legs, leg, skin, None);
    }
}

// ---- tail -------------------------------------------------------------------------------------

struct TailGeom {
    center: Vec<Pt>,
    normal: Vec<Pt>,
    half: Vec<f32>,
}

impl TailGeom {
    fn outline(&self) -> Vec<Pt> {
        let n = self.center.len();
        let mut pts = Vec::with_capacity(n * 2 + 8);
        for i in 0..n {
            pts.push(self.center[i] + self.normal[i] * self.half[i]);
        }
        // round tip
        let last = n - 1;
        let tangent = (self.center[last] - self.center[last.saturating_sub(1)]).normalized();
        for k in 1..8 {
            let a = k as f32 / 8.0 * PI;
            let dir = self.normal[last] * a.cos() + tangent * a.sin();
            pts.push(self.center[last] + dir * self.half[last]);
        }
        for i in (0..n).rev() {
            pts.push(self.center[i] - self.normal[i] * self.half[i]);
        }
        pts
    }

    /// Tail space: x = 0..100 along the tail, y = offset across it.
    fn map(&self, q: Pt) -> Pt {
        let n = self.center.len();
        let f = (q.x / 100.0).clamp(0.0, 1.0) * (n - 1) as f32;
        let i = (f.floor() as usize).min(n - 2);
        let t = f - i as f32;
        let c = self.center[i].lerp(self.center[i + 1], t);
        let nn = self.normal[i].lerp(self.normal[i + 1], t);
        c + nn * q.y
    }
}

fn tail_geom(pose: &Pose, skin: &Skin, rig: &Rig) -> Option<TailGeom> {
    let l = skin.shape.tail_length;
    let wag = pose.tail_wag;
    let n = 28;
    let (center, half): (Vec<Pt>, Vec<f32>) = match skin.shape.tail_style {
        TailStyle::Curl => {
            let base = rig.torso.map(pt(50.0, -72.0));
            let c = base + pt(15.0, -27.0) * l;
            let r0 = (base - c).len();
            let a0 = (base.y - c.y).atan2(base.x - c.x);
            let pts: Vec<Pt> = (0..=n)
                .map(|i| {
                    let t = i as f32 / n as f32;
                    let a = a0 - t * 4.7;
                    let r = lerp(r0, 11.0 * l, t);
                    c + pt(a.cos(), a.sin()) * r
                })
                .map(|q| base + (q - base).rotate(wag * 0.55))
                .collect();
            let half = (0..=n).map(|i| lerp(12.0, 7.5, i as f32 / n as f32) * l.powf(0.3)).collect();
            (pts, half)
        }
        TailStyle::Plume => {
            let base = rig.torso.map(pt(54.0, -26.0));
            let c1 = base + pt(36.0, 10.0) * l;
            let c2 = base + pt(72.0, -16.0) * l;
            let end = base + pt(62.0, -70.0) * l;
            let pts = (0..=n)
                .map(|i| cubic(base, c1, c2, end, i as f32 / n as f32))
                .map(|q| base + (q - base).rotate(wag * 0.7))
                .collect();
            let half = (0..=n)
                .map(|i| {
                    let t = i as f32 / n as f32;
                    (9.0 + 6.0 * (PI * t).sin() - 4.0 * t) * l.powf(0.3)
                })
                .collect();
            (pts, half)
        }
        TailStyle::Stub => return None,
    };
    let normal = (0..center.len())
        .map(|i| {
            let a = center[i.saturating_sub(1)];
            let b = center[(i + 1).min(center.len() - 1)];
            (b - a).normalized().perp()
        })
        .collect();
    Some(TailGeom { center, normal, half })
}

fn draw_tail(p: &mut Painter, pose: &Pose, skin: &Skin, rig: &Rig) {
    let pal = &skin.palette;
    if skin.shape.tail_style == TailStyle::Stub {
        // A round fluffy puff peeking out behind the hip; it wiggles with the rear end.
        let r = 13.0 * skin.shape.tail_length.powf(0.5) * 1.2;
        let base = rig.torso.map(pt(52.0 + 16.0 * (skin.shape.butt_fluff - 1.0), -44.0));
        let c = base + pt(8.0, -5.0).rotate(pose.tail_wag * 1.4);
        let pts: Vec<Pt> = (0..72)
            .map(|i| {
                let a = i as f32 / 72.0 * 2.0 * PI;
                let rr = r * (1.0 + 0.045 * (a * 7.0 + pose.tail_wag * 3.0).cos());
                c + pt(a.cos(), a.sin()) * rr
            })
            .collect();
        let path = poly(&pts);
        p.fill_shader(&path, linear(c + pt(-r, -r), c + pt(r, r), pal.tail, pal.tail_tip), pal.tail);
        p.stroke(&path, pal.outline, 2.2);
        return;
    }
    let Some(g) = tail_geom(pose, skin, rig) else { return };
    let outline = poly(&g.outline());
    p.fill(&outline, pal.tail);
    p.set_mask(Clip::Tmp, &outline);
    let n = g.center.len();
    if skin.shape.tail_style == TailStyle::Curl {
        // cream underside on the inside of the curl
        let mid = g.center[n / 2];
        let pivot = g.center.iter().fold(Pt::ZERO, |acc, &q| acc + q) * (1.0 / n as f32);
        let inward = if (pivot - mid).x * g.normal[n / 2].x + (pivot - mid).y * g.normal[n / 2].y > 0.0 { 1.0 } else { -1.0 };
        let mut band: Vec<Pt> = (0..n).map(|i| g.center[i] + g.normal[i] * (g.half[i] * 0.15 * inward)).collect();
        band.extend((0..n).rev().map(|i| g.center[i] + g.normal[i] * (g.half[i] * 1.2 * inward)));
        p.fill_clipped(&poly(&band), pal.tail_tip, Clip::Tmp);
    }
    // tip colour on the last 20%
    let tip_start = (n as f32 * 0.8) as usize;
    let mut tip: Vec<Pt> = (tip_start..n).map(|i| g.center[i] + g.normal[i] * (g.half[i] * 1.3)).collect();
    tip.push(g.center[n - 1] + (g.center[n - 1] - g.center[n - 2]).normalized() * (g.half[n - 1] * 1.4));
    tip.extend((tip_start..n).rev().map(|i| g.center[i] - g.normal[i] * (g.half[i] * 1.3)));
    p.fill_clipped(&poly(&tip), pal.tail_tip, Clip::Tmp);
    draw_custom(p, skin, Space::Tail, rig, Some(&g));
    p.stroke(&outline, pal.outline, 2.2);
}

// ---- ears -------------------------------------------------------------------------------------

fn ear_frame(pose: &Pose, skin: &Skin, head: &HeadXf, side: usize) -> Affine {
    let sh = &skin.shape;
    let s = side_sign(side);
    let e = sh.ear_size;
    let base_w = 40.0 * (0.5 + 0.5 * e);
    let inner = pt(s * 12.0, -43.0);
    let outer = pt(s * (12.0 + base_w), -43.0 + base_w * 0.45);
    let mid = inner.lerp(outer, 0.5);
    let height = 50.0 * e * (1.0 + pose.ear_perk * 0.06 - pose.ear_fold * 0.3);
    let spread = sh.ear_spread + pose.ear_fold * 0.75 - pose.ear_perk * 0.1 + pose.ear_twitch[side];
    let tip = mid + pt(0.0, -height).rotate(s * spread);
    let mut ci = head.map_flat(inner, 9.0);
    let mut co = head.map_flat(outer, 9.0);
    let ct = head.map_flat(tip, 5.0);
    // The ear on the side the head turns towards is seen more edge-on.
    let turn = clamp01(head.yaw * s / 0.42);
    co = ci + (co - ci) * (1.0 - 0.32 * turn);
    ci = ci + (co - ci) * (0.06 * turn);
    let o = ci.lerp(co, 0.5);
    Affine::from_axes(o, (co - ci) * (1.0 / 100.0), (o - ct) * (1.0 / 100.0))
}

fn ear_outer_path(pointy: f32) -> Option<Path> {
    let tw = 5.5 / pointy.max(0.5);
    let mut pb = PathBuilder::new();
    pb.move_to(-50.0, 0.0);
    pb.cubic_to(-47.0, -38.0, -24.0, -78.0, -tw, -97.0);
    pb.quad_to(0.0, -101.5, tw, -97.0);
    pb.cubic_to(24.0, -78.0, 47.0, -38.0, 50.0, 0.0);
    pb.cubic_to(20.0, 9.0, -20.0, 9.0, -50.0, 0.0);
    pb.close();
    pb.finish()
}

fn ear_inner_path(pointy: f32) -> Option<Path> {
    let tw = 3.5 / pointy.max(0.5);
    let mut pb = PathBuilder::new();
    pb.move_to(-30.0, -4.0);
    pb.cubic_to(-27.0, -34.0, -13.0, -62.0, -tw, -77.0);
    pb.quad_to(0.0, -81.0, tw, -77.0);
    pb.cubic_to(13.0, -62.0, 27.0, -34.0, 30.0, -4.0);
    pb.cubic_to(12.0, 1.0, -12.0, 1.0, -30.0, -4.0);
    pb.close();
    pb.finish()
}

fn draw_ears(p: &mut Painter, pose: &Pose, skin: &Skin, rig: &Rig) {
    let pal = &skin.palette;
    for side in 0..2 {
        let f = ear_frame(pose, skin, &rig.head, side);
        let outer = affine_path(ear_outer_path(skin.shape.ear_pointiness), f);
        let inner = affine_path(ear_inner_path(skin.shape.ear_pointiness), f);
        p.fill(&outer, pal.ear_outer);
        p.set_mask(Clip::Tmp, &outer);
        p.fill_clipped(&inner, pal.ear_inner, Clip::Tmp);
        // a little fur line inside the ear
        let a = f.map(pt(0.0, -18.0));
        let b = f.map(pt(4.0, -52.0));
        p.stroke_clipped(&polyline(&[a, b]), pal.ear_outer.fade(0.35), 1.4, LineCap::Round, Clip::Tmp);
        let space = if side == VIEWER_LEFT { Space::EarL } else { Space::EarR };
        draw_custom_ear(p, skin, space, f);
        p.stroke(&outer, pal.outline, 2.3);
    }
}

// ---- head -------------------------------------------------------------------------------------

fn head_space_outline(skin: &Skin) -> Vec<Pt> {
    let segs = [
        (pt(32.0, -50.0), pt(57.0, -38.0), pt(60.0, -12.0)),
        (pt(63.0, 10.0), pt(52.0, 34.0), pt(30.0, 42.0)),
        (pt(16.0, 47.0), pt(-16.0, 47.0), pt(-30.0, 42.0)),
        (pt(-52.0, 34.0), pt(-63.0, 10.0), pt(-60.0, -12.0)),
        (pt(-57.0, -38.0), pt(-32.0, -50.0), pt(0.0, -49.0)),
    ];
    let mut pts = cubic_chain(pt(0.0, -49.0), &segs, 32);
    pts.pop(); // closed: last == first
    let amp = 3.6 * skin.shape.cheek_fluff;
    if amp > 0.05 {
        // Fluffy cheek tufts on both sides of the jaw.
        let (a0, a1) = (0.05f32, 1.0f32);
        let tufts = 3.0;
        for q in pts.iter_mut() {
            let ang = q.y.atan2(q.x.abs());
            if ang > a0 && ang < a1 {
                let u = (ang - a0) / (a1 - a0);
                let env = (PI * u).sin();
                let bump = (tufts * PI * u).sin().abs().powf(0.7);
                let dir = q.normalized();
                *q += dir * (amp * env * bump);
            }
        }
    }
    pts
}

fn draw_head(p: &mut Painter, pose: &Pose, skin: &Skin, rig: &Rig, o: &RenderOptions) {
    let pal = &skin.palette;
    let sh = &skin.shape;
    let h = &rig.head;
    let silhouette = poly(&map_pts(&head_space_outline(skin), |q| h.map_flat(q, 4.0)));
    p.fill(&silhouette, pal.body);
    p.set_mask(Clip::Head, &silhouette);
    let dome = |q: Pt| h.map_dome(q, 0.0);

    // Face pattern
    match sh.face {
        FacePattern::Blaze => {
            let pts = [
                pt(-4.0, -58.0),
                pt(4.0, -58.0),
                pt(5.5, -32.0),
                pt(7.5, -12.0),
                pt(14.0, 3.0),
                pt(32.0, 9.0),
                pt(52.0, 20.0),
                pt(68.0, 40.0),
                pt(0.0, 72.0),
                pt(-68.0, 40.0),
                pt(-52.0, 20.0),
                pt(-32.0, 9.0),
                pt(-14.0, 3.0),
                pt(-7.5, -12.0),
                pt(-5.5, -32.0),
            ];
            p.fill_clipped(&poly(&map_pts(&smooth_closed(&pts, 8), dome)), pal.belly, Clip::Head);
        }
        FacePattern::Urajiro => {
            let pts = [
                pt(-66.0, 8.0),
                pt(-46.0, 7.0),
                pt(-31.0, 13.0),
                pt(-17.0, 10.0),
                pt(-9.0, 6.0),
                pt(0.0, 8.0),
                pt(9.0, 6.0),
                pt(17.0, 10.0),
                pt(31.0, 13.0),
                pt(46.0, 7.0),
                pt(66.0, 8.0),
                pt(70.0, 44.0),
                pt(0.0, 74.0),
                pt(-70.0, 44.0),
            ];
            p.fill_clipped(&poly(&map_pts(&smooth_closed(&pts, 8), dome)), pal.belly, Clip::Head);
        }
        FacePattern::None => {}
    }

    // Soft highlight on the crown for volume.
    let sc = h.map_dome(pt(-16.0, -30.0), 0.0);
    let sr = 26.0 * h.scale;
    let shine_shader = RadialGradient::new(
        Point::from_xy(sc.x, sc.y),
        Point::from_xy(sc.x, sc.y),
        sr,
        vec![GradientStop::new(0.0, Color::from_rgba8(255, 255, 255, 46)), GradientStop::new(1.0, Color::from_rgba8(255, 255, 255, 0))],
        SpreadMode::Pad,
        Transform::identity(),
    );
    if let (Some(path), Some(shader)) = (circle(sc, sr), shine_shader) {
        let paint = Paint { shader, anti_alias: true, ..Paint::default() };
        p.pm.fill_path(&path, &paint, FillRule::Winding, p.ts, Some(&*p.head));
    }

    // Muzzle
    let mw = 21.0 * (0.9 + 0.1 * sh.muzzle_length);
    let muzzle_pts = map_pts(&ellipse_pts(pt(0.0, 20.0), mw, 14.5, 40), |q| h.map_dome(q, 5.0));
    let muzzle_color = if sh.face == FacePattern::None { pal.body.lighten(0.25) } else { pal.belly };
    p.fill_clipped(&poly(&muzzle_pts), muzzle_color, Clip::Head);

    draw_custom(p, skin, Space::Skull, rig, None);
    draw_custom(p, skin, Space::Head, rig, None);
    draw_custom(p, skin, Space::Muzzle, rig, None);

    // Blush
    let blush_a = 0.16 + 0.42 * pose.blush;
    for s in [-1.0f32, 1.0] {
        let pts = map_pts(&ellipse_pts(pt(s * 37.0, 16.0), 9.0, 5.4, 24), dome);
        p.fill_clipped(&poly(&pts), pal.blush.fade(blush_a), Clip::Head);
    }

    // Shiba "mayu" brows
    if sh.brows {
        for s in [-1.0f32, 1.0] {
            let f = h.feature_frame(pt(s * 16.0, -21.0), 1.0, s * 0.22);
            p.fill(&affine_path(poly(&ellipse_pts(Pt::ZERO, 5.6, 3.3, 20)), f), pal.belly);
        }
    }

    draw_eyes(p, pose, skin, h, o);
    draw_nose_mouth(p, pose, skin, h);

    // whisker dots
    for s in [-1.0f32, 1.0] {
        for q in [pt(s * 10.0, 22.0), pt(s * 14.5, 18.5), pt(s * 14.0, 25.0)] {
            p.fill(&circle(h.map_dome(q, 5.0), 0.95 * h.scale), pal.markings.fade(0.5));
        }
    }

    p.stroke(&silhouette, pal.outline, 2.5);
    draw_focus_mark(p, pose, h);
}

fn draw_eyes(p: &mut Painter, pose: &Pose, skin: &Skin, h: &HeadXf, o: &RenderOptions) {
    let sh = &skin.shape;
    let pal = &skin.palette;
    let iris = pal.iris.unwrap_or_else(|| o.eye_color.iris());
    let dark = rgb(0x24, 0x16, 0x0F);
    let rx = 10.2 * sh.eye_size;
    let ry = 11.0 * sh.eye_size * (1.0 - sh.eye_lid * 0.25);
    for s in [-1.0f32, 1.0] {
        let f = h.feature_frame(pt(s * 22.0, -1.0), 2.0, -s * sh.eye_slant);
        if pose.eye_happy > 0.5 {
            // ∩ happy eyes
            let pts: Vec<Pt> = (0..=14)
                .map(|i| {
                    let u = i as f32 / 14.0;
                    f.map(pt(lerp(-rx, rx, u), 2.5 - 7.0 * (PI * u).sin()))
                })
                .collect();
            p.stroke(&polyline(&pts), dark, 3.0 * h.scale);
            continue;
        }
        if pose.eye_open < 0.22 {
            // ∪ closed eyes (sleep, blink)
            let pts: Vec<Pt> = (0..=14)
                .map(|i| {
                    let u = i as f32 / 14.0;
                    f.map(pt(lerp(-rx, rx, u), 1.0 + 3.6 * (PI * u).sin()))
                })
                .collect();
            p.stroke(&polyline(&pts), dark, 2.6 * h.scale);
            continue;
        }
        let open = pose.eye_open;
        let ery = ry * open;
        let eye = affine_path(poly(&ellipse_pts(Pt::ZERO, rx, ery, 36)), f);
        p.fill(&eye, dark);
        // iris with a lighter lower half
        let ir = 0.84;
        let iris_path = affine_path(poly(&ellipse_pts(Pt::ZERO, rx * ir, ery * ir, 36)), f);
        let top = f.map(pt(0.0, -ery));
        let bottom = f.map(pt(0.0, ery));
        p.fill_shader(&iris_path, linear(top, bottom, iris.darken(0.25), iris.lighten(0.22)), iris);
        // pupil follows the cursor
        let pr = 0.56 * pose.pupil.clamp(0.6, 1.5);
        let max_off = (rx * ir - rx * pr).max(0.0) * 0.9;
        let g = pose.gaze.clamp_len(1.0);
        let off = pt(g.x * max_off, g.y * max_off * 0.7 * open);
        let pupil = affine_path(poly(&ellipse_pts(off, rx * pr, ery * pr * 1.04, 32)), f);
        p.fill(&pupil, rgb(0x16, 0x0D, 0x08));
        // glints
        let big = 0.31 * rx * (0.92 + 0.22 * pose.sparkle);
        p.fill(&affine_path(circle(pt(-0.33 * rx, -0.36 * ery), big), f), WHITE);
        p.fill(&affine_path(circle(pt(0.33 * rx, 0.32 * ery), 0.13 * rx), f), WHITE.fade(0.9));
        if pose.sparkle > 0.15 {
            let c = pt(0.36 * rx, -0.42 * ery);
            let k = 0.17 * rx * pose.sparkle;
            p.fill(&affine_path(poly(&star_pts(c, k, 0.3)), f), WHITE);
        }
        // Focused eyelid: a lid slanting down towards the nose, like a determined squint.
        if pose.focus > 0.02 {
            let k = pose.focus;
            let inner = -s; // +x points towards the nose for the viewer-left eye
            let lid_y = |x: f32| -ery * 1.04 + k * ery * (0.72 + 0.5 * (x * inner / rx));
            let ring = ellipse_pts(Pt::ZERO, rx * 1.06, ery * 1.06, 48);
            let cap = clip_above(&ring, &lid_y);
            if cap.len() >= 3 {
                p.fill(&affine_path(poly(&cap), f), pal.body);
                let a = pt(-rx * 1.08, lid_y(-rx * 1.08));
                let b = pt(rx * 1.08, lid_y(rx * 1.08));
                p.stroke(&affine_path(polyline(&[a, b]), f), dark, (1.6 + 0.8 * k) * h.scale);
            }
        }
        // smug eyelid
        if sh.eye_lid > 0.01 {
            let lid_y = -ery + 2.0 * ery * sh.eye_lid;
            let mut lid: Vec<Pt> = ellipse_pts(Pt::ZERO, rx * 1.02, ery * 1.02, 48).into_iter().filter(|q| q.y < lid_y).collect();
            if lid.len() >= 2 {
                // ellipse points run clockwise from +x; rotate so the lid arc is contiguous
                lid.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
                let half_w = rx * 1.02 * (1.0 - (lid_y / (ery * 1.02)).powi(2)).max(0.0).sqrt();
                let mut shape = vec![pt(-half_w - 0.6, lid_y)];
                shape.extend(lid);
                shape.push(pt(half_w + 0.6, lid_y));
                p.fill(&affine_path(poly(&shape), f), pal.body);
                let tip = 0.6 * s;
                let line = affine_path(polyline(&[pt(-half_w - 0.8, lid_y + tip), pt(half_w + 0.8, lid_y - tip)]), f);
                p.stroke(&line, dark, 1.9 * h.scale);
            }
        }
    }
}

/// The part of the closed polygon `pts` above the line `y = line(x)` (Sutherland–Hodgman
/// against one half-plane).
fn clip_above(pts: &[Pt], line: &dyn Fn(f32) -> f32) -> Vec<Pt> {
    let inside = |q: Pt| q.y <= line(q.x);
    let mut out = Vec::with_capacity(pts.len() + 2);
    for i in 0..pts.len() {
        let a = pts[i];
        let b = pts[(i + 1) % pts.len()];
        let (ia, ib) = (inside(a), inside(b));
        if ia {
            out.push(a);
        }
        if ia != ib {
            // Bisect for the crossing; the line is straight, a few steps are plenty.
            let (mut lo, mut hi) = (0.0f32, 1.0f32);
            for _ in 0..12 {
                let mid = (lo + hi) / 2.0;
                if inside(a.lerp(b, mid)) == ia {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            out.push(a.lerp(b, (lo + hi) / 2.0));
        }
    }
    out
}

/// The anime "💢" anger mark beside the head while the dog is focused: four little arcs
/// bowing in towards a centre, red with a white rim, pulsing gently.
fn draw_focus_mark(p: &mut Painter, pose: &Pose, h: &HeadXf) {
    let a = smoothstep(0.3, 0.85, pose.focus);
    if a <= 0.01 {
        return;
    }
    let pulse = 1.0 + 0.1 * (pose.time * 9.0).sin();
    let size = 6.8 * h.scale * pulse * (0.6 + 0.4 * a);
    // On the head itself, just inside its top-right edge.
    let c = h.map_flat(pt(41.0, -36.0), 6.0);
    let rot = h.tilt + 0.2;
    let arcs: Vec<Vec<Pt>> = [(1.0f32, 1.0f32), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)]
        .iter()
        .map(|&(sx, sy)| {
            (0..=8)
                .map(|i| {
                    let t = i as f32 / 8.0;
                    let q = quad(pt(sx * 0.32, sy), pt(sx * 0.3, sy * 0.3), pt(sx, sy * 0.32), t);
                    c + (q * size).rotate(rot)
                })
                .collect()
        })
        .collect();
    for arc in &arcs {
        p.stroke(&polyline(arc), WHITE.fade(0.9 * a), 4.2 * h.scale);
    }
    for arc in &arcs {
        p.stroke(&polyline(arc), rgb(0xE8, 0x3F, 0x4A).fade(a), 2.3 * h.scale);
    }
}

fn draw_nose_mouth(p: &mut Painter, pose: &Pose, skin: &Skin, h: &HeadXf) {
    let pal = &skin.palette;
    let f = h.feature_frame(pt(0.0, 12.0), 7.0 + skin.shape.muzzle_length * 6.0, 0.0);
    let o = pose.mouth_open;
    let mouth_line = pal.mouth;
    if o > 0.12 {
        // Open, happy mouth with a tongue.
        let depth = 15.0 * o;
        let mut pb = PathBuilder::new();
        pb.move_to(-9.5, 8.0);
        pb.quad_to(-4.5, 11.5, 0.0, 8.5);
        pb.quad_to(4.5, 11.5, 9.5, 8.0);
        pb.cubic_to(9.5, 8.0 + depth * 0.8, 4.5, 9.0 + depth, 0.0, 9.0 + depth);
        pb.cubic_to(-4.5, 9.0 + depth, -9.5, 8.0 + depth * 0.8, -9.5, 8.0);
        pb.close();
        let mouth = affine_path(pb.finish(), f);
        p.fill(&mouth, pal.mouth.mix(rgb(0x9A, 0x30, 0x3A), 0.55));
        if pose.tongue > 0.1 {
            let tl = 9.0 + depth * 0.45;
            let hang = 9.0 + depth + 4.5 * pose.tongue;
            let mut tb = PathBuilder::new();
            tb.move_to(-5.2, tl);
            tb.quad_to(0.0, tl - 2.5, 5.2, tl);
            tb.line_to(5.2, hang - 4.0);
            tb.quad_to(5.2, hang + 0.5, 0.0, hang + 0.5);
            tb.quad_to(-5.2, hang + 0.5, -5.2, hang - 4.0);
            tb.close();
            let tongue = affine_path(tb.finish(), f);
            p.fill(&tongue, pal.tongue);
            p.stroke(&affine_path(polyline(&[pt(0.0, tl + 1.0), pt(0.0, hang - 3.0)]), f), pal.tongue.darken(0.22), 1.1 * h.scale);
            p.stroke(&tongue, pal.tongue.darken(0.35), 1.1 * h.scale);
        }
        p.stroke(&mouth, mouth_line, 1.7 * h.scale);
    } else {
        if pose.tongue > 0.2 {
            // A little "blep".
            let t = pose.tongue;
            let mut tb = PathBuilder::new();
            tb.move_to(-3.3, 9.6);
            tb.line_to(-3.3, 10.0 + 2.5 * t);
            tb.quad_to(-3.3, 14.0 + 3.5 * t, 0.0, 14.0 + 3.5 * t);
            tb.quad_to(3.3, 14.0 + 3.5 * t, 3.3, 10.0 + 2.5 * t);
            tb.line_to(3.3, 9.6);
            tb.close();
            let tongue = affine_path(tb.finish(), f);
            p.fill(&tongue, pal.tongue);
            p.stroke(&tongue, pal.tongue.darken(0.35), 1.0 * h.scale);
        }
        let mut pb = PathBuilder::new();
        pb.move_to(-9.0, 8.0);
        pb.quad_to(-4.5, 13.0, 0.0, 8.8);
        pb.quad_to(4.5, 13.0, 9.0, 8.0);
        p.stroke(&affine_path(pb.finish(), f), mouth_line, 1.9 * h.scale);
    }
    // philtrum
    p.stroke(&affine_path(polyline(&[pt(0.0, 4.5), pt(0.0, 8.8)]), f), mouth_line, 1.8 * h.scale);
    // nose
    let mut nb = PathBuilder::new();
    nb.move_to(-8.5, -3.5);
    nb.cubic_to(-8.5, -7.8, 8.5, -7.8, 8.5, -3.5);
    nb.cubic_to(8.5, 0.5, 3.5, 4.6, 0.0, 5.0);
    nb.cubic_to(-3.5, 4.6, -8.5, 0.5, -8.5, -3.5);
    nb.close();
    let nose = affine_path(nb.finish(), f);
    p.fill(&nose, pal.nose);
    p.fill(&affine_path(poly(&ellipse_pts(pt(-2.8, -3.9), 2.8, 1.35, 16)), f), WHITE.fade(0.7));
}

// ---- mouse & keyboard -------------------------------------------------------------------------

fn draw_mouse_pad(p: &mut Painter, kb: &KeyboardColors) {
    let c = PAD_CENTER;
    let (hw, hh) = (PAD_W / 2.0, PAD_H / 2.0);
    let pad = rounded_poly(&[c + pt(-hw + 4.0, -hh), c + pt(hw - 4.0, -hh), c + pt(hw, hh), c + pt(-hw, hh)], 8.0);
    p.fill(&rounded_poly(&[c + pt(-hw + 4.0, -hh + 3.0), c + pt(hw - 4.0, -hh + 3.0), c + pt(hw, hh + 3.0), c + pt(-hw, hh + 3.0)], 8.0), kb.pad.darken(0.3));
    p.fill(&pad, kb.pad);
    p.stroke(&pad, kb.outline, 1.8);
}

fn draw_mouse(p: &mut Painter, pose: &Pose, kb: &KeyboardColors) {
    let m = MOUSE_REST + pose.mouse_offset;
    let (rx, ry) = (13.5, 17.5);
    // The mouse faces the dog, which sits behind it: buttons, wheel and cable point towards
    // the viewer. `f` maps the mouse's own frame (front = -y) onto the canvas, turned 180°.
    let f = |q: Pt| m - q;
    // Cable out of the front, curling round to the back edge of the keyboard (which faces the
    // viewer too) and tucking under it.
    let cable = quad_path(f(pt(0.0, -ry)), pt(m.x + 4.0, KB_BL.y + KB_FRONT + 10.0), pt(KB_BL.x + 14.0, KB_BL.y + 2.0));
    p.stroke(&cable, kb.outline.fade(0.55), 1.8);
    let body = poly(&ellipse_pts(m, rx, ry, 40));
    p.fill(&poly(&ellipse_pts(m + pt(0.0, 2.2), rx, ry, 40)), kb.outline.fade(0.35));
    p.fill(&body, kb.mouse);
    // Buttons: seen from the dog's side, the user's left button is on the viewer's right.
    for (i, sgn) in [(0usize, -1.0f32), (1, 1.0)] {
        let press = clamp01(pose.mouse_buttons[i]);
        if press > 0.02 {
            let pts: Vec<Pt> = (0..=12)
                .map(|k| {
                    let a = -PI / 2.0 + sgn * (k as f32 / 12.0) * (PI / 2.0);
                    f(pt(a.cos() * rx, a.sin() * ry))
                })
                .chain([f(pt(sgn * rx * 0.98, -2.0)), f(pt(0.0, -2.0))])
                .collect();
            p.fill(&poly(&pts), kb.lit.fade(0.35 + 0.55 * press));
        }
    }
    p.stroke(&polyline(&[f(pt(0.0, -ry)), f(pt(0.0, -2.0))]), kb.outline.fade(0.7), 1.3);
    p.stroke(&polyline(&[f(pt(-rx * 0.96, -2.0)), f(pt(rx * 0.96, -2.0))]), kb.outline.fade(0.45), 1.1);
    p.fill(&rounded_poly(&[f(pt(-1.8, -13.0)), f(pt(1.8, -13.0)), f(pt(1.8, -6.0)), f(pt(-1.8, -6.0))], 1.6), kb.outline.fade(0.6));
    p.stroke(&body, kb.outline, 1.8);
}

fn hue(h: f32) -> Rgba {
    let h = h.rem_euclid(1.0) * 6.0;
    let x = 1.0 - ((h % 2.0) - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    let soft = |v: f32| (90.0 + 165.0 * v) as u8;
    rgb(soft(r), soft(g), soft(b))
}

fn kb_base_color(kb: &KeyboardColors, y: f32) -> Rgba {
    kb.base.lighten(0.08).mix(kb.base, clamp01((y - KB_TL.y) / (KB_BL.y - KB_TL.y)))
}

/// Resting cap colour of a key and the colour of its side, by key kind. Coloured keys get a
/// side in a darker shade of their own colour so they still read as 3D keycaps.
fn key_colors(kb: &KeyboardColors, id: KeyId) -> (Rgba, Rgba) {
    match key_kind(id) {
        KeyKind::Alpha => (kb.key, kb.key_edge),
        KeyKind::Modifier => (kb.modifier, kb.modifier.mix(kb.key_edge, 0.5).darken(0.08)),
        KeyKind::Accent => (kb.accent, kb.accent.darken(0.22)),
        KeyKind::Space => (kb.space, kb.space.darken(0.2)),
    }
}

fn draw_key(p: &mut Painter, q: &[Pt; 4], kb: &KeyboardColors, color: Rgba, side: Rgba, sink: f32, glow: f32) {
    let edge: Vec<Pt> = q.iter().map(|&v| v + pt(0.0, 1.6)).collect();
    p.fill(&rounded_poly(&edge, 1.8), side);
    let cap: Vec<Pt> = q.iter().map(|&v| v + pt(0.0, sink)).collect();
    let cap_path = rounded_poly(&cap, 1.8);
    p.fill(&cap_path, color);
    if glow > 0.02 {
        p.stroke(&cap_path, kb.lit.fade(0.55 * glow), 2.4);
    }
}

/// Keyboard body and every key at rest.
fn draw_keyboard_base(p: &mut Painter, kb: &KeyboardColors) {
    let front = rounded_poly(&[KB_BL + pt(0.0, -6.0), KB_BR + pt(0.0, -6.0), KB_BR + pt(-2.0, KB_FRONT), KB_BL + pt(2.0, KB_FRONT)], 5.0);
    p.fill(&front, kb.front);
    p.stroke(&front, kb.outline, 2.0);
    let top = rounded_poly(&[KB_TL, KB_TR, KB_BR, KB_BL], 6.0);
    p.fill_shader(&top, linear(KB_TL, KB_BL, kb.base.lighten(0.08), kb.base), kb.base);
    p.stroke(&top, kb.outline, 2.0);
    for id in all_keys() {
        let (cap, side) = key_colors(kb, id);
        draw_key(p, &key_quad(id), kb, cap, side, 0.0, 0.0);
    }
}

/// Redraws only the keys that differ from the cached keyboard (lit, pressed, or the sweep).
fn draw_key_overlay(p: &mut Painter, pose: &Pose, kb: &KeyboardColors, sweep: bool) {
    let width = KB_TR.x - KB_TL.x;
    let height = KB_BL.y - KB_TL.y;
    for id in all_keys() {
        let i = id.0 as usize;
        if i >= KEY_COUNT {
            continue;
        }
        let glow = clamp01(pose.key_glow[i]);
        let down = pose.key_down[i];
        if !(sweep || down || glow > 0.02) {
            continue;
        }
        let q = key_quad(id);
        let (mut color, side) = key_colors(kb, id);
        if sweep {
            let c = key_center(id);
            let pos = (c.x - KB_TL.x) / width * 0.75 + (c.y - KB_TL.y) / height * 0.25;
            let d = (pose.marquee - pos).rem_euclid(1.0);
            let comet = (-d * 6.0).exp() * (0.55 + 0.45 * pose.marquee_boost);
            color = color.mix(hue(pos + pose.marquee), 0.6 * comet);
        }
        color = color.mix(kb.lit, glow);
        if down {
            // Hide the resting cap so the key looks pushed in.
            let c = key_center(id);
            p.fill(&rounded_poly(&q, 1.8), kb_base_color(kb, c.y));
        }
        draw_key(p, &q, kb, color, side, if down { 1.2 } else { 0.0 }, glow);
    }
}

// ---- legs -------------------------------------------------------------------------------------

/// Where the keyboard starts covering the body. Legs are drawn in two passes: everything
/// with the body (before the head, so the head covers the shoulders), and again on top of the
/// keyboard for the part below this line.
const LEG_SPLIT_Y: f32 = KB_TL.y - 5.0;

/// One leg ready to draw: its skinned shape, where the sock and the inner contour start, and
/// the fillets that blend it into the body's silhouette.
struct LegDraw {
    shape: LimbShape,
    sock_from: f32,
    line_from: f32,
    fillets: Vec<Fillet>,
    /// Closed outline of the whole leg, root included, and its bounding box.
    outline: Vec<Pt>,
    bbox: (Pt, Pt),
    /// Per edge sample (outer, inner): whether it lies over the torso or another leg, and the
    /// contour width drawn there in the body pass. Filled in once all legs are known.
    behind: [Vec<bool>; 2],
    contour: [Vec<f32>; 2],
}

impl LegDraw {
    fn new(shape: LimbShape, sock_from: f32, line_from: f32, torso: &[Pt], fillet_r: f32) -> LegDraw {
        let fillets = fillets(&shape, torso, fillet_r, LEG_SPLIT_Y);
        let outline = shape.polygon(f32::MIN, 1.0);
        let bbox = outline.iter().fold((pt(f32::MAX, f32::MAX), pt(f32::MIN, f32::MIN)), |(lo, hi), q| {
            (pt(lo.x.min(q.x), lo.y.min(q.y)), pt(hi.x.max(q.x), hi.y.max(q.y)))
        });
        LegDraw { shape, sock_from, line_from, fillets, outline, bbox, behind: [Vec::new(), Vec::new()], contour: [Vec::new(), Vec::new()] }
    }
}

struct Legs {
    /// Back to front: back legs, then front legs (a leg crossing the chest is drawn last).
    items: Vec<LegDraw>,
    /// Index into `items` of each front leg, and of each raised back leg.
    front: [usize; 2],
    hind: [Option<usize>; 2],
    /// The torso's shading gradient (top and bottom points), shared by the legs so a leg's
    /// root is pixel-identical to the body under it.
    shade: (Pt, Pt),
    /// Torso outline in canvas space.
    torso: Vec<Pt>,
}

/// Width of the thin contour drawn where a leg passes in front of the body. Much lighter than
/// the silhouette ink, so the leg reads as part of the body rather than a cut-out on top.
const CONTOUR: f32 = 1.35;
/// Arm length of the fillet curve where a leg grows out of the silhouette.
const FILLET_R: f32 = 10.0;

impl Legs {
    fn new(pose: &Pose, skin: &Skin, rig: &Rig) -> Legs {
        let sh = &skin.shape;
        let torso = map_pts(&torso_pts(skin), |q| rig.torso.map(q));
        let w = 12.3 * (0.9 + 0.1 * sh.body_width);
        // Organic front-leg profile: the outer side (towards the flank) carries the shoulder,
        // the inner side tapers into the body at the armpit so it never reaches the chest fur.
        let arm = Profile {
            outer: [w * 0.84, w * 0.9, w * 0.86, w * 0.82, w * 0.8],
            inner: [w * 0.37, w * 0.7, w * 0.78, w * 0.78, w * 0.76],
            root: w * 0.62,
        };
        let leg = Profile { outer: [16.0, 14.0, 12.0, 11.0, 10.5], inner: [12.0, 11.0, 10.0, 10.0, 9.5], root: 15.0 };
        let (upper, lower) = rig.arm_bones();
        let (thigh, shin) = rig.leg_bones();

        let mut items = Vec::with_capacity(4);
        let mut hind = [None, None];
        for side in 0..2 {
            let up = clamp01(pose.hind[side].up);
            if up < 0.12 {
                continue;
            }
            let s = side_sign(side);
            let hip = rig.hip(side);
            let foot = hip.lerp(pose.hind[side].pos, up);
            let knee = if pose.knees[side].is_finite() { pose.knees[side] } else { solve_elbow(hip, foot, thigh, shin, pt(s, 0.2)) };
            let shape = skin_limb(rig.hip_root(side), hip, knee, foot, &leg);
            hind[side] = Some(items.len());
            items.push(LegDraw::new(shape, 0.55, 0.2, &torso, FILLET_R * 0.8));
        }

        let fronts = [0, 1].map(|side| {
            let s = side_sign(side);
            let paw = pose.paws[side].pos + pt(0.0, pose.paws[side].press * 1.8);
            let joint = rig.shoulder(side);
            let elbow = if pose.elbows[side].is_finite() { pose.elbows[side] } else { solve_elbow(joint, paw, upper, lower, front_bend(s)) };
            let shape = skin_limb(rig.arm_root(side), joint, elbow, paw, &arm);
            LegDraw::new(shape, 1.0 - sh.sock_length, 0.15, &torso, FILLET_R)
        });
        // A paw reaching across to the other side of the chest is drawn in front.
        let crossing = |side: usize| (fronts[side].shape.wrist.x - DOG_X) * side_sign(side) < -4.0;
        let order = if crossing(0) && !crossing(1) { [1, 0] } else { [0, 1] };
        let mut front = [0usize; 2];
        let [a, b] = fronts;
        let mut fronts = [Some(a), Some(b)];
        for side in order {
            front[side] = items.len();
            if let Some(f) = fronts[side].take() {
                items.push(f);
            }
        }
        let mut legs = Legs { items, front, hind, shade: (rig.body.map(pt(0.0, -TORSO_H)), rig.body.map(pt(0.0, 0.0))), torso };
        for idx in 0..legs.items.len() {
            let behind = [true, false].map(|outer| {
                let s = &legs.items[idx].shape;
                let edge = if outer { &s.outer } else { &s.inner };
                edge.iter().map(|&q| legs.behind(idx, q)).collect::<Vec<bool>>()
            });
            legs.items[idx].behind = behind;
            let contour = [contour_widths(&legs.items[idx], true), contour_widths(&legs.items[idx], false)];
            legs.items[idx].contour = contour;
        }
        legs
    }

    /// The fur gradient shared by torso, legs and fillets.
    fn fur(&self, skin: &Skin) -> Option<Shader<'static>> {
        let pal = &skin.palette;
        let bottom = if pal.legs == pal.body { pal.body_shade.unwrap_or(pal.body) } else { pal.legs.darken(0.06) };
        linear(self.shade.0, self.shade.1, pal.legs, bottom)
    }

    /// Whether `p` lies on something the leg `idx` is drawn in front of: the torso or another leg.
    fn behind(&self, idx: usize, p: Pt) -> bool {
        point_in_polygon(p, &self.torso)
            || self.items.iter().enumerate().any(|(k, l)| {
                k != idx && p.x >= l.bbox.0.x && p.x <= l.bbox.1.x && p.y >= l.bbox.0.y && p.y <= l.bbox.1.y && point_in_polygon(p, &l.outline)
            })
    }
}

fn clip_fill(p: &mut Painter, path: &Option<Path>, c: Rgba, clip: Option<Clip>) {
    match clip {
        Some(cl) => p.fill_clipped(path, c, cl),
        None => p.fill(path, c),
    }
}

/// First arc parameter at which the leg reaches the keyboard split line.
fn split_u(leg: &LegDraw) -> f32 {
    let s = &leg.shape;
    for i in 0..s.center.len() {
        if s.center[i].y.max(s.outer[i].y).max(s.inner[i].y) >= LEG_SPLIT_Y - 1.0 {
            return s.u[i.saturating_sub(1)];
        }
    }
    1.0
}

/// How far (in arc parameter) the sock edge sits from `sock_from` at `t` across the leg
/// (0 = inner edge, 1 = outer edge). Negative is higher up the leg. The paw colour rises in
/// two soft rounded tufts with a small dip between them, echoing the scallops of the chest
/// fur, so the colour change reads as fur rather than a straight cut.
fn sock_edge(t: f32) -> f32 {
    0.03 - 0.035 * (PI * t).sin() - 0.04 * (2.0 * PI * t).sin().abs()
}

/// Closed outline of the sock: the fur-like boundary across the leg, then down the outer edge
/// to the wrist and back up the inner edge.
fn sock_pts(s: &LimbShape, from: f32) -> Vec<Pt> {
    const K: usize = 24;
    let mut pts = Vec::with_capacity(K + s.center.len() * 2);
    for k in 0..=K {
        let t = k as f32 / K as f32;
        let u = (from + sock_edge(t)).clamp(0.02, 0.98);
        pts.push(s.edge_at(u, false).lerp(s.edge_at(u, true), t));
    }
    let u_out = (from + sock_edge(1.0)).clamp(0.02, 0.98);
    let u_in = (from + sock_edge(0.0)).clamp(0.02, 0.98);
    pts.extend((0..s.center.len()).filter(|&i| s.u[i] > u_out).map(|i| s.outer[i]));
    pts.extend((0..s.center.len()).rev().filter(|&i| s.u[i] > u_in).map(|i| s.inner[i]));
    pts
}

fn fill_leg(p: &mut Painter, legs: &Legs, leg: &LegDraw, skin: &Skin, clip: Option<Clip>, from: usize) {
    let pal = &skin.palette;
    let s = &leg.shape;
    let start = s.u[from.min(s.u.len() - 1)];
    // Fur with the body's own shading (so the root is identical to the torso under it), down
    // past the lowest point of the sock edge; then the sock on top.
    p.fill_shader_clipped(&poly(&s.polygon(start, leg.sock_from + sock_edge(0.0) + 0.02)), legs.fur(skin), pal.legs, clip);
    clip_fill(p, &poly(&sock_pts(s, leg.sock_from)), pal.paws, clip);
}

/// Visible line width along one edge of a leg in the body pass: a thin contour where the edge
/// is in front of the body (fading in below the joint and out where the edge merges into the
/// silhouette at a fillet), nothing where it is the silhouette (the ink is already there).
fn contour_widths(leg: &LegDraw, outer: bool) -> Vec<f32> {
    let s = &leg.shape;
    let edge = if outer { &s.outer } else { &s.inner };
    let behind = &leg.behind[if outer { 0 } else { 1 }];
    let exit = leg.fillets.iter().find(|f| f.outer == outer);
    (0..edge.len())
        .map(|i| {
            if !behind[i] {
                return 0.0;
            }
            let mut k = smoothstep(leg.line_from, leg.line_from + 0.3, s.u[i]);
            if let Some(f) = exit {
                if s.u[i] <= f.exit_u {
                    k *= smoothstep(2.0, 16.0, (edge[i] - f.exit).len());
                }
            }
            CONTOUR * k
        })
        .collect()
}

/// Draws a band of the outline colour just outside each edge with the given widths. It
/// overlaps the fill slightly so anti-aliasing leaves no seam.
fn edge_bands(p: &mut Painter, leg: &LegDraw, skin: &Skin, clip: Option<Clip>, from: usize, widths: [&[f32]; 2]) {
    let s = &leg.shape;
    let n = s.center.len();
    let start = from.max(s.first_index_from(0.0));
    if start + 1 >= n {
        return;
    }
    for (edge, w) in [(&s.outer, widths[0]), (&s.inner, widths[1])] {
        // Split into runs where the band is visible, so it can vanish completely in between.
        let mut i = start;
        while i < n {
            if w[i] <= 0.01 {
                i += 1;
                continue;
            }
            let a = i.saturating_sub(1).max(start);
            let mut b = i;
            while b + 1 < n && w[b + 1] > 0.01 {
                b += 1;
            }
            let b_end = (b + 1).min(n - 1);
            i = b + 1;
            // A run only a few pixels long (a fold of the arm brushing the body) would read as
            // a stray dash, not a contour.
            let run: f32 = (a..b_end).map(|j| (edge[j + 1] - edge[j]).len()).sum();
            if run < 7.0 {
                continue;
            }
            let mut band: Vec<Pt> = Vec::with_capacity((b_end - a + 1) * 2);
            let mut back: Vec<Pt> = Vec::with_capacity(b_end - a + 1);
            for j in a..=b_end {
                let out = (edge[j] - s.center[j]).normalized();
                band.push(edge[j] - out * 0.5);
                back.push(edge[j] + out * w[j]);
            }
            band.extend(back.into_iter().rev());
            clip_fill(p, &poly(&band), skin.palette.outline, clip);
        }
    }
}

/// Inks the silhouette curve of each fillet (under the fills, like the rest of the outline).
fn ink_fillets(p: &mut Painter, leg: &LegDraw, skin: &Skin, clip: Option<Clip>) {
    for f in &leg.fillets {
        let path = polyline(&f.curve);
        match clip {
            Some(cl) => p.stroke_clipped(&path, skin.palette.outline, INK * 2.0, LineCap::Round, cl),
            None => p.stroke(&path, skin.palette.outline, INK * 2.0),
        }
    }
}

/// Fills each fillet with the shared fur gradient, covering the sharp corner of ink that the
/// separate outlines of body and leg leave there.
fn fill_fillets(p: &mut Painter, legs: &Legs, leg: &LegDraw, skin: &Skin, clip: Option<Clip>) {
    for f in &leg.fillets {
        if f.exit_u >= leg.sock_from {
            clip_fill(p, &poly(&f.region), skin.palette.paws, clip);
        } else {
            p.fill_shader_clipped(&poly(&f.region), legs.fur(skin), skin.palette.legs, clip);
        }
    }
}

/// Second pass, after the keyboard: the parts of the legs on or over the keyboard (same
/// ink → fill → contour order, clipped below the split line), then the paws. Over the
/// keyboard a leg is outlined in full; between the split line and the keyboard's edge the
/// line widths blend from the body pass to that, so there is no step.
fn draw_legs_over_keyboard(p: &mut Painter, pose: &Pose, legs: &Legs, skin: &Skin) {
    let pal = &skin.palette;
    let below = poly(&[pt(-50.0, LEG_SPLIT_Y), pt(CANVAS_W + 50.0, LEG_SPLIT_Y), pt(CANVAS_W + 50.0, CANVAS_H + 50.0), pt(-50.0, CANVAS_H + 50.0)]);
    p.set_mask(Clip::Tmp, &below);
    for leg in &legs.items {
        let su = split_u(leg);
        if su >= 1.0 {
            continue;
        }
        let from = leg.shape.first_index_from(su);
        let s = &leg.shape;
        let widths = [0, 1].map(|e| {
            let edge = if e == 0 { &s.outer } else { &s.inner };
            (0..edge.len())
                .map(|i| {
                    let base = if leg.behind[e][i] { leg.contour[e][i] } else { INK };
                    lerp(base, INK, smoothstep(LEG_SPLIT_Y, KB_TL.y + 3.0, edge[i].y))
                })
                .collect::<Vec<f32>>()
        });
        ink_fillets(p, leg, skin, Some(Clip::Tmp));
        fill_leg(p, legs, leg, skin, Some(Clip::Tmp), from);
        edge_bands(p, leg, skin, Some(Clip::Tmp), from, [&widths[0], &widths[1]]);
        fill_fillets(p, legs, leg, skin, Some(Clip::Tmp));
    }
    for leg in &legs.items {
        let is_front = legs.front.iter().position(|&i| std::ptr::eq(&legs.items[i], leg));
        let is_hind = legs.hind.iter().position(|h| h.is_some_and(|i| std::ptr::eq(&legs.items[i], leg)));
        if let Some(side) = is_front {
            draw_front_paw(p, leg, pose.paws[side].press, skin);
        } else if let Some(side) = is_hind {
            let k = 0.6 + 0.4 * clamp01(pose.hind[side].up);
            draw_paw(p, leg.shape.wrist, 13.5 * k, 11.0 * k, pal.paws, pal.outline, true, pal.pads);
        }
    }
}

/// Shape of a front paw in its own frame (+y points from the wrist towards the toes): a plump
/// round pad with three toe bumps along its tip.
const PAW_MAIN: (Pt, f32, f32) = (pt(0.0, -1.0), 12.8, 10.0);
const PAW_TOES: [(Pt, f32); 3] = [(pt(-7.6, 5.2), 4.7), (pt(0.0, 6.6), 4.7), (pt(7.6, 5.2), 4.7)];

/// Distance from the paw's origin to the edge of the union of its parts along direction `u`.
/// Every part overlaps the main pad, which contains the origin, so the union is star-shaped
/// and one ray per direction finds its outline exactly (toe notches included).
fn paw_radius(u: Pt, sy: f32) -> f32 {
    let (mc, rx, ry) = PAW_MAIN;
    let (cx, cy, ry) = (mc.x, mc.y * sy, ry * sy);
    let a = u.x * u.x / (rx * rx) + u.y * u.y / (ry * ry);
    let b = -2.0 * (u.x * cx / (rx * rx) + u.y * cy / (ry * ry));
    let c = cx * cx / (rx * rx) + cy * cy / (ry * ry) - 1.0;
    let mut r = (-b + (b * b - 4.0 * a * c).max(0.0).sqrt()) / (2.0 * a);
    for (q, tr) in PAW_TOES {
        let q = pt(q.x, q.y * sy);
        let bq = u.x * q.x + u.y * q.y;
        let disc = bq * bq - (q.x * q.x + q.y * q.y - tr * tr);
        if disc >= 0.0 {
            r = r.max(bq + disc.sqrt());
        }
    }
    r
}

/// A front paw that grows out of the end of the leg: it turns with the forearm, has no line
/// where it meets the leg, little toe creases while it rests on the keys, and pink toe beans
/// when it is lifted and its palm faces the viewer (waving, zoomies).
fn draw_front_paw(p: &mut Painter, leg: &LegDraw, press: f32, skin: &Skin) {
    let pal = &skin.palette;
    let s = &leg.shape;
    let wrist = s.wrist;
    let lift = 1.0 - smoothstep(KB_TL.y - 40.0, KB_TL.y - 8.0, wrist.y);
    // Point the toes along the forearm; a paw resting on the keys turns only part way so it
    // still looks planted.
    let d = (wrist - s.point_at(0.8)).normalized();
    let full = d.y.atan2(d.x) - PI / 2.0;
    let rot = full * lerp(0.3, 1.0, lift);
    let sy = 1.0 - 0.07 * clamp01(press);
    let c = wrist + d * 2.0;
    let map = |q: Pt| c + q.rotate(rot);

    const N: usize = 96;
    let outline: Vec<Pt> = (0..N)
        .map(|i| {
            let a = i as f32 / N as f32 * 2.0 * PI;
            let u = pt(a.cos(), a.sin());
            map(u * paw_radius(u, sy))
        })
        .collect();
    p.fill(&poly(&outline), pal.paws);

    // Outline everything except where the paw joins the forearm, so the two read as one.
    let forearm = s.polygon(0.7, 1.0);
    let open: Vec<bool> = outline.iter().map(|q| !point_in_polygon(*q, &forearm)).collect();
    if open.iter().all(|&o| o) {
        p.stroke(&poly(&outline), pal.outline, 2.2);
    } else if let Some(first_closed) = open.iter().position(|&o| !o) {
        // Walk the ring starting at a hidden point so every visible run is contiguous.
        let mut run: Vec<Pt> = Vec::new();
        for k in 1..=N {
            let i = (first_closed + k) % N;
            if open[i] {
                run.push(outline[i]);
            } else if !run.is_empty() {
                p.stroke(&polyline(&run), pal.outline, 2.2);
                run.clear();
            }
        }
        if run.len() > 1 {
            p.stroke(&polyline(&run), pal.outline, 2.2);
        }
    }

    // Toe creases between the bumps, fading out as the palm turns to the viewer.
    let crease = pal.outline.fade(0.6 * (1.0 - lift));
    for x in [-3.8f32, 3.8] {
        let a = map(pt(x, 9.3 * sy));
        let b = map(pt(x * 0.92, 5.0 * sy));
        p.stroke(&polyline(&[a, b]), crease, 1.3);
    }
    // Toe beans and the main pad.
    if lift > 0.02 {
        let pad = pal.pads.fade(lift);
        let main: Vec<Pt> = ellipse_pts(Pt::ZERO, 5.4, 4.0, 24).into_iter().map(|q| map(q + pt(0.0, -1.2 * sy))).collect();
        p.fill(&poly(&main), pad);
        for (q, _) in PAW_TOES {
            let bean: Vec<Pt> = ellipse_pts(Pt::ZERO, 2.3, 2.0, 16).into_iter().map(|e| map(e + pt(q.x * 0.94, q.y * sy + 0.6))).collect();
            p.fill(&poly(&bean), pad);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_paw(p: &mut Painter, c: Pt, rx: f32, ry: f32, fill: Rgba, outline: Rgba, beans: bool, pads: Rgba) {
    let path = poly(&ellipse_pts(c, rx, ry, 32));
    p.fill(&path, fill);
    p.stroke(&path, outline, 2.1);
    if beans {
        p.fill(&poly(&ellipse_pts(c + pt(0.0, ry * 0.25), rx * 0.42, ry * 0.36, 20)), pads);
        for (x, y) in [(-0.45f32, -0.32f32), (0.0, -0.5), (0.45, -0.32)] {
            p.fill(&circle(c + pt(x * rx, y * ry), rx * 0.17), pads);
        }
    } else {
        for x in [-0.32f32, 0.32] {
            let a = c + pt(x * rx, ry * 0.15);
            let b = c + pt(x * rx * 1.08, ry * 0.85);
            p.stroke(&polyline(&[a, b]), outline.fade(0.55), 1.4);
        }
    }
}

// ---- key pop-ups ---------------------------------------------------------------------------

/// `1 + c3·(x−1)³ + c1·(x−1)²`: rises past 1 and settles back, for a springy pop.
fn ease_out_back(x: f32) -> f32 {
    let c1 = 1.70158;
    let c3 = c1 + 1.0;
    let x = clamp01(x) - 1.0;
    1.0 + c3 * x * x * x + c1 * x * x
}

/// Pressed keys pop out of the keyboard as little keycaps in the keyboard's own colours,
/// with the key's label on top, then float up and fade.
fn draw_key_popups(p: &mut Painter, pose: &Pose, kb: &KeyboardColors) {
    let label_color = if kb.key.luminance() > 0.55 { kb.outline } else { rgb(0xF4, 0xF6, 0xFA) };
    for k in &pose.key_popups {
        let life = k.life.max(0.01);
        let t = clamp01(k.age / life);
        let rise = 1.0 - (1.0 - clamp01(k.age / 0.42)).powi(3);
        let float = smoothstep(0.4, 1.0, t) * 10.0;
        let c = k.from + pt(k.drift * rise, -(44.0 * rise + float));
        let scale = 0.45 + 0.55 * ease_out_back(k.age / 0.24);
        let alpha = smoothstep(0.0, 0.05, k.age) * (1.0 - smoothstep(0.72, 1.0, t));
        if alpha <= 0.01 {
            continue;
        }
        let rot = k.tilt * rise;
        let f = |q: Pt| c + (q * scale).rotate(rot);

        let (strokes, w) = glyphs::layout(k.label);
        let len = k.label.chars().count();
        let gs = if len >= 3 && strokes.len() > 1 && w > glyphs::GLYPH_W * 1.5 { 1.15 } else if w > glyphs::GLYPH_W * 1.5 { 1.3 } else { 1.55 };
        let lw = w * gs;
        let (hw, hh, lip) = (((lw + 11.0).max(21.0)) / 2.0, 10.5, 4.0);
        let rect = |x0: f32, y0: f32, x1: f32, y1: f32| [f(pt(x0, y0)), f(pt(x1, y0)), f(pt(x1, y1)), f(pt(x0, y1))];
        let r = 4.0 * scale;

        // Soft shadow, outline (inked under the fills), the key's side, then its top face.
        p.fill(&rounded_poly(&rect(-hw + 1.0, -hh + 3.0, hw + 1.0, hh + lip + 3.0), r), kb.outline.fade(0.16 * alpha));
        let whole = rounded_poly(&rect(-hw, -hh, hw, hh + lip), r);
        p.stroke(&whole, kb.outline.fade(alpha), 2.6 * scale);
        p.fill(&whole, kb.key_edge.fade(alpha));
        let fresh = 1.0 - smoothstep(0.0, 0.3, k.age);
        let top_color = kb.key.mix(kb.lit, 0.35 * fresh);
        let top = rounded_poly(&rect(-hw + 0.6, -hh + 0.6, hw - 0.6, hh - 0.4), r * 0.85);
        let (a, b) = (f(pt(0.0, -hh)), f(pt(0.0, hh)));
        p.fill_shader(&top, linear(a, b, top_color.lighten(0.06).fade(alpha), top_color.fade(alpha)), top_color.fade(alpha));
        p.stroke(&top, kb.outline.fade(0.3 * alpha), 0.9 * scale);

        // The label, centred on the top face.
        let ink = label_color.fade(alpha);
        for st in &strokes {
            let pts: Vec<Pt> = st.iter().map(|q| f(pt(q.x * gs - lw / 2.0, (q.y - glyphs::GLYPH_H / 2.0) * gs - 0.3))).collect();
            if pts.len() == 1 || (pts.len() == 2 && (pts[0] - pts[1]).len() < 0.2) {
                p.fill(&circle(pts[0], 0.95 * gs * scale), ink);
            } else {
                p.stroke(&polyline(&pts), ink, 1.6 * gs.min(1.2) * scale);
            }
        }
    }
}

// ---- particles ------------------------------------------------------------------------------

fn star_pts(c: Pt, r: f32, inner: f32) -> Vec<Pt> {
    (0..8)
        .map(|i| {
            let a = i as f32 / 8.0 * 2.0 * PI - PI / 2.0;
            let rr = if i % 2 == 0 { r } else { r * inner };
            c + pt(a.cos(), a.sin()) * rr
        })
        .collect()
}

fn heart_path(c: Pt, s: f32, rot: f32) -> Option<Path> {
    let m = |x: f32, y: f32| c + pt(x * s, y * s).rotate(rot);
    let a = m(0.0, 0.9);
    let mut pb = PathBuilder::new();
    pb.move_to(a.x, a.y);
    let (c1, c2, e) = (m(-1.2, 0.1), m(-0.95, -0.95), m(0.0, -0.45));
    pb.cubic_to(c1.x, c1.y, c2.x, c2.y, e.x, e.y);
    let (c1, c2, e) = (m(0.95, -0.95), m(1.2, 0.1), a);
    pb.cubic_to(c1.x, c1.y, c2.x, c2.y, e.x, e.y);
    pb.close();
    pb.finish()
}

fn draw_particles(p: &mut Painter, pose: &Pose) {
    for part in &pose.particles {
        let life = part.life.max(0.01);
        let k = clamp01(part.age / life);
        let fade = (1.0 - k).powf(0.6) * smoothstep(0.0, 0.12, k).max(0.25);
        match part.kind {
            ParticleKind::Heart => {
                let s = part.size * (0.7 + 0.3 * smoothstep(0.0, 0.2, k));
                let path = heart_path(part.pos, s, (part.age * part.spin).sin() * 0.3);
                p.stroke(&path, WHITE.fade(fade), 2.4);
                p.fill(&path, rgb(0xFF, 0x6F, 0x91).fade(fade));
            }
            ParticleKind::Zzz => {
                let s = part.size;
                let c = part.pos;
                let pts = [c + pt(-s, -s), c + pt(s, -s), c + pt(-s, s), c + pt(s, s)];
                let path = polyline(&pts);
                p.stroke(&path, WHITE.fade(fade * 0.9), s * 0.55 + 2.2);
                p.stroke(&path, rgb(0x6F, 0x7B, 0xE0).fade(fade), s * 0.4);
            }
            ParticleKind::Star => {
                let path = poly(&star_pts(part.pos, part.size, 0.42));
                p.fill(&path, rgb(0xFF, 0xD3, 0x4D).fade(fade));
                p.stroke(&path, rgb(0xC0, 0x8A, 0x10).fade(fade), 1.0);
            }
        }
    }
}

// ---- custom regions & marks ---------------------------------------------------------------

fn region_points(r: &Region, map: &dyn Fn(Pt) -> Pt) -> Vec<Pt> {
    let n = r.anchors.len();
    let mut out = Vec::with_capacity(n * 16);
    for i in 0..n {
        let a = &r.anchors[i];
        let b = &r.anchors[(i + 1) % n];
        for k in 0..16 {
            out.push(map(cubic(a.p, a.handle_out, b.handle_in, b.p, k as f32 / 16.0)));
        }
    }
    out
}

fn draw_custom(p: &mut Painter, skin: &Skin, space: Space, rig: &Rig, tail: Option<&TailGeom>) {
    if skin.regions.iter().all(|r| r.space != space) && skin.marks.iter().all(|m| m.space != space) {
        return;
    }
    let h = rig.head;
    let torso = rig.torso;
    let map: Box<dyn Fn(Pt) -> Pt + '_> = match space {
        Space::Head => Box::new(move |q| h.map_dome(q, 0.0)),
        Space::Skull => Box::new(move |q| h.map_flat(q, 2.0)),
        Space::Muzzle => Box::new(move |q| h.map_dome(q, 8.0)),
        Space::Body => Box::new(move |q| torso.map(q)),
        Space::Tail => match tail {
            Some(g) => Box::new(move |q| g.map(q)),
            None => return,
        },
        Space::EarL | Space::EarR => return,
    };
    let clip = match space {
        Space::Body => Clip::Body,
        Space::Tail => Clip::Tmp,
        _ => Clip::Head,
    };
    for r in skin.regions.iter().filter(|r| r.space == space) {
        p.fill_clipped(&poly(&region_points(r, &*map)), r.fill, clip);
    }
    let cap = if space == Space::Tail { LineCap::Butt } else { LineCap::Round };
    for m in skin.marks.iter().filter(|m| m.space == space) {
        let pts = map_pts(&m.points, &*map);
        match m.kind {
            MarkKind::Polygon => p.fill_clipped(&poly(&pts), m.color, clip),
            MarkKind::Stroke => p.stroke_clipped(&polyline(&pts), m.color, m.width, cap, clip),
        }
    }
}

fn draw_custom_ear(p: &mut Painter, skin: &Skin, space: Space, f: Affine) {
    for r in skin.regions.iter().filter(|r| r.space == space) {
        p.fill_clipped(&poly(&region_points(r, &|q| f.map(q))), r.fill, Clip::Tmp);
    }
    for m in skin.marks.iter().filter(|m| m.space == space) {
        let pts = map_pts(&m.points, |q| f.map(q));
        match m.kind {
            MarkKind::Polygon => p.fill_clipped(&poly(&pts), m.color, Clip::Tmp),
            MarkKind::Stroke => p.stroke_clipped(&polyline(&pts), m.color, m.width, LineCap::Round, Clip::Tmp),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skin::{corgi, parse_skin, BUILTINS};

    fn opaque_pixels(pm: &Pixmap) -> usize {
        pm.pixels().iter().filter(|p| p.alpha() > 200).count()
    }

    /// At rest, no part of a front leg (root included) may cover the chest fur, and the
    /// shoulders must sit inside the body.
    #[test]
    fn resting_front_legs_stay_off_the_chest_fur() {
        for skin in BUILTINS.map(|(_, f)| f()) {
            let pose = Pose::neutral();
            let rig = Rig::new(&skin.shape, &pose);
            let legs = Legs::new(&pose, &skin, &rig);
            let chest = map_pts(&chest_pts(&skin).unwrap(), |q| rig.body.map(q));
            for side in 0..2 {
                assert!(point_in_polygon(rig.shoulder(side), &legs.torso), "{} shoulder {side}", skin.name);
                let leg = &legs.items[legs.front[side]];
                // The head is drawn over both, so only the visible part counts.
                for q in leg.outline.iter().filter(|q| !rig.head_contains(**q)) {
                    assert!(!point_in_polygon(*q, &chest), "{}: leg {side} covers the chest at {q:?}", skin.name);
                }
                for q in chest.iter().filter(|q| !rig.head_contains(**q)) {
                    assert!(!point_in_polygon(*q, &leg.outline), "{}: chest point {q:?} under leg {side}", skin.name);
                }
            }
        }
    }

    /// Where a front leg leaves the silhouette the outline is rounded by a fillet, in rest and
    /// in a raised pose; every fillet curve stays outside the torso.
    #[test]
    fn shoulders_blend_into_the_silhouette() {
        let skin = corgi();
        let mut pose = Pose::neutral();
        let rig = Rig::new(&skin.shape, &pose);
        pose.paws[1].pos = rig.shoulder(1) + pt(50.0, -44.0);
        pose.elbows = [pt(f32::NAN, f32::NAN); 2];
        let legs = Legs::new(&pose, &skin, &rig);
        let raised = &legs.items[legs.front[1]];
        assert!(raised.fillets.len() == 2, "raised arm should blend on both sides: {}", raised.fillets.len());
        for f in &raised.fillets {
            let mid = f.curve[f.curve.len() / 2];
            assert!(!point_in_polygon(mid, &legs.torso), "fillet curve inside the torso at {mid:?}");
            assert!(!point_in_polygon(mid, &raised.outline), "fillet curve inside the leg at {mid:?}");
        }
    }

    #[test]
    fn renders_both_builtins_at_several_sizes() {
        for skin in BUILTINS.map(|(_, f)| f()) {
            for scale in [0.5, 1.0, 1.7] {
                let mut r = Renderer::new(scale);
                let pm = r.render(&Pose::neutral(), &skin, &RenderOptions { scale, ..Default::default() });
                assert_eq!((pm.width(), pm.height()), pixel_size(scale));
                let filled = opaque_pixels(pm) as f32 / (pm.width() * pm.height()) as f32;
                assert!(filled > 0.15 && filled < 0.7, "{} at {scale}: {filled}", skin.name);
                // Corners stay transparent so clicks pass through.
                assert_eq!(pm.pixel(0, 0).unwrap().alpha(), 0);
                assert_eq!(pm.pixel(pm.width() - 1, 0).unwrap().alpha(), 0);
            }
        }
    }

    #[test]
    fn head_centre_is_opaque_for_hit_testing() {
        let skin = corgi();
        let mut r = Renderer::new(1.0);
        let pose = Pose::neutral();
        let pm = r.render(&pose, &skin, &RenderOptions::default());
        let c = Rig::new(&skin.shape, &pose).head.center;
        assert!(pm.pixel(c.x as u32, (c.y - 30.0) as u32).unwrap().alpha() == 255);
    }

    #[test]
    fn custom_regions_and_marks_render() {
        let text = format!(
            "{}[[regions]]\nspace = \"body\"\nfill = \"#0000FF\"\nanchors = [{{ x = -80, y = -60 }}, {{ x = 80, y = -60 }}, {{ x = 80, y = 20 }}, {{ x = -80, y = 20 }}]\n\
             [[marks]]\nspace = \"ear_l\"\nkind = \"polygon\"\ncolor = \"#00FF00\"\npoints = [[-60, 10], [60, 10], [0, -120]]\n",
            "format_version = 1\nname = \"Blue\"\n[palette]\nbody = \"#E9933F\"\n"
        );
        let skin = parse_skin(&text).unwrap();
        let mut r = Renderer::new(1.0);
        let pm = r.render(&Pose::neutral(), &skin, &RenderOptions::default());
        let blue = pm.pixels().iter().filter(|p| p.blue() > 200 && p.red() < 40 && p.green() < 40).count();
        let green = pm.pixels().iter().filter(|p| p.green() > 200 && p.red() < 40 && p.blue() < 40).count();
        assert!(blue > 200, "blue {blue}");
        assert!(green > 50, "green {green}");
    }

    /// The Shiba-style shape options are no longer used by a built-in, but imported skins can
    /// still use them: curled tail, smooth bib, cream cheeks and eyebrow dots.
    #[test]
    fn shiba_style_options_still_render() {
        let text = "format_version = 1\nname = \"Shiba-ish\"\n[palette]\nbody = \"#E07E3C\"\n\
                    [shape]\ntail_style = \"curl\"\nface = \"urajiro\"\nchest = \"bib\"\nbrows = true\n";
        let skin = parse_skin(text).unwrap();
        assert!(chest_pts(&skin).is_some());
        let mut r = Renderer::new(1.0);
        let pm = r.render(&Pose::neutral(), &skin, &RenderOptions::default());
        let filled = opaque_pixels(pm) as f32 / (pm.width() * pm.height()) as f32;
        assert!(filled > 0.15 && filled < 0.7, "{filled}");
    }

    /// The variants really look different: their coats are far apart in colour.
    #[test]
    fn corgi_variants_have_distinct_coats() {
        let coats: Vec<_> = BUILTINS.iter().map(|(_, f)| f().palette.body).collect();
        for i in 0..coats.len() {
            for j in i + 1..coats.len() {
                let (a, b) = (coats[i], coats[j]);
                let d = (a.r as i32 - b.r as i32).abs() + (a.g as i32 - b.g as i32).abs() + (a.b as i32 - b.b as i32).abs();
                assert!(d > 120, "coats {i} and {j} are too similar");
            }
        }
    }

    #[test]
    fn icon_is_mostly_dog() {
        for skin in BUILTINS.map(|(_, f)| f()) {
            let pm = render_icon(&skin, 32, EyeColor::Brown);
            assert_eq!(pm.width(), 32);
            let filled = opaque_pixels(&pm);
            assert!(filled > 32 * 32 / 3, "{} icon {filled}", skin.name);
        }
    }
}
