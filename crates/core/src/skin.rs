//! Skins: colours, body-shape numbers and optional vector markings.
//!
//! A skin is data, not images: the renderer draws the dog from it every frame, so every
//! dog shares one rig and one renderer. The built-in is the red Corgi; the other shape options (curled tail, bib, urajiro, brows) stay available
//! for imported skins. Rules (same as KuroCat's skin format):
//! numbers are clamped to their legal range, unknown fields are ignored, and type errors,
//! bad colours or missing required fields reject the whole file with a clear message.

use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::color::{rgb, Rgba};
use crate::geom::{pt, Pt};

pub const FORMAT_VERSION: i64 = 1;
pub const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_NAME_CHARS: usize = 40;
pub const MAX_REGIONS: usize = 64;
pub const MAX_ANCHORS: usize = 128;
pub const MAX_MARKS: usize = 256;
pub const MAX_MARK_POINTS: usize = 512;
pub const COORD_LIMIT: f32 = 400.0;

pub const BUILTIN_CORGI: &str = "corgi";

#[derive(Clone, Debug, PartialEq)]
pub struct Skin {
    pub name: String,
    pub author: String,
    pub palette: Palette,
    pub shape: Shape,
    pub regions: Vec<Region>,
    pub marks: Vec<Mark>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Palette {
    pub body: Rgba,
    /// Optional lower colour of a top-to-bottom body gradient.
    pub body_shade: Option<Rgba>,
    /// Chest, face blaze / urajiro, muzzle.
    pub belly: Rgba,
    /// Upper front legs.
    pub legs: Rgba,
    /// Socks and paws.
    pub paws: Rgba,
    /// Toe beans.
    pub pads: Rgba,
    pub ear_outer: Rgba,
    pub ear_inner: Rgba,
    pub nose: Rgba,
    pub mouth: Rgba,
    pub tongue: Rgba,
    pub tail: Rgba,
    /// Tail tip and underside.
    pub tail_tip: Rgba,
    /// Sticker-style outline around every part.
    pub outline: Rgba,
    pub blush: Rgba,
    /// Default colour for `marks` without their own colour.
    pub markings: Rgba,
    /// Overrides the eye colour chosen in the menu.
    pub iris: Option<Rgba>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TailStyle {
    /// Corgi: a fluffy puff, the whole rear end wiggles.
    Stub,
    /// Shiba: curled over the back.
    Curl,
    /// Long fluffy tail.
    Plume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FacePattern {
    None,
    /// Corgi: white stripe down the forehead into a white muzzle.
    Blaze,
    /// Shiba: cream cheeks and muzzle.
    Urajiro,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Chest {
    None,
    /// Fluffy scalloped chest ruff.
    Ruff,
    /// Smooth cream bib.
    Bib,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub body_width: f32,
    pub leg_length: f32,
    pub head_scale: f32,
    pub muzzle_length: f32,
    pub ear_size: f32,
    pub ear_spread: f32,
    pub ear_pointiness: f32,
    pub eye_size: f32,
    pub eye_lid: f32,
    pub eye_slant: f32,
    pub cheek_fluff: f32,
    pub butt_fluff: f32,
    pub tail_style: TailStyle,
    pub tail_length: f32,
    pub face: FacePattern,
    pub chest: Chest,
    /// Shiba "mayu" eyebrow dots.
    pub brows: bool,
    /// How far up the front legs the paw colour reaches (0..1).
    pub sock_length: f32,
}

/// (name, min, max) for every numeric shape field, used for clamping and docs.
pub const SHAPE_RANGES: [(&str, f32, f32); 14] = [
    ("body_width", 0.85, 1.35),
    ("leg_length", 0.5, 1.2),
    ("head_scale", 0.85, 1.2),
    ("muzzle_length", 0.6, 1.4),
    ("ear_size", 0.6, 1.5),
    ("ear_spread", -0.2, 0.8),
    ("ear_pointiness", 0.5, 1.5),
    ("eye_size", 0.7, 1.4),
    ("eye_lid", 0.0, 0.5),
    ("eye_slant", -0.3, 0.3),
    ("cheek_fluff", 0.0, 1.5),
    ("butt_fluff", 0.8, 1.4),
    ("tail_length", 0.3, 1.4),
    ("sock_length", 0.0, 1.0),
];

fn range_of(name: &str) -> (f32, f32) {
    SHAPE_RANGES.iter().find(|r| r.0 == name).map(|r| (r.1, r.2)).unwrap_or((f32::MIN, f32::MAX))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Space {
    /// Face, projected onto a dome so it turns with the head.
    Head,
    /// Head without 3D turning (big top-of-head patches).
    Skull,
    /// Like `Head` but sits further forward (moves most when turning).
    Muzzle,
    Body,
    /// x = 0 (root) … 100 (tip) along the tail, y = offset across it.
    Tail,
    /// Ear on the viewer's left. (-50,0) inner base, (50,0) outer base, (0,-100) tip.
    EarL,
    /// Ear on the viewer's right, mirrored automatically.
    EarR,
}

impl Space {
    pub fn name(self) -> &'static str {
        match self {
            Space::Head => "head",
            Space::Skull => "skull",
            Space::Muzzle => "muzzle",
            Space::Body => "body",
            Space::Tail => "tail",
            Space::EarL => "ear_l",
            Space::EarR => "ear_r",
        }
    }

    fn parse(s: &str) -> Option<Space> {
        Some(match s {
            "head" => Space::Head,
            "skull" => Space::Skull,
            "muzzle" => Space::Muzzle,
            "body" => Space::Body,
            "tail" => Space::Tail,
            "ear_l" => Space::EarL,
            "ear_r" => Space::EarR,
            _ => return None,
        })
    }
}

/// One anchor of a closed cubic Bézier region. Handles are absolute coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Anchor {
    pub p: Pt,
    pub handle_in: Pt,
    pub handle_out: Pt,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Region {
    pub space: Space,
    pub fill: Rgba,
    pub anchors: Vec<Anchor>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkKind {
    Polygon,
    Stroke,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub space: Space,
    pub kind: MarkKind,
    pub color: Rgba,
    pub width: f32,
    pub points: Vec<Pt>,
}

// ---------------------------------------------------------------------------------------------
// Built-in skins
// ---------------------------------------------------------------------------------------------

/// Pembroke Welsh Corgi: red and white, big ears, short legs, fluffy loaf with a stub tail.
pub fn corgi() -> Skin {
    let body = rgb(0xE9, 0x93, 0x3F);
    let white = rgb(0xFF, 0xF8, 0xEE);
    Skin {
        name: "Corgi".into(),
        author: "DeskCorgi".into(),
        palette: Palette {
            body,
            body_shade: Some(rgb(0xD9, 0x7C, 0x2C)),
            belly: white,
            legs: body,
            paws: white,
            pads: rgb(0xF2, 0x9C, 0xA3),
            ear_outer: rgb(0xE3, 0x8A, 0x36),
            ear_inner: rgb(0xF9, 0xC9, 0xB9),
            nose: rgb(0x2E, 0x1F, 0x1A),
            mouth: rgb(0x4A, 0x2B, 0x22),
            tongue: rgb(0xFF, 0x86, 0x9E),
            tail: body,
            tail_tip: white,
            outline: rgb(0x5C, 0x34, 0x1E),
            blush: rgb(0xFF, 0x8F, 0x8F),
            markings: rgb(0xB8, 0x62, 0x22),
            iris: None,
        },
        shape: Shape {
            body_width: 1.16,
            leg_length: 0.6,
            head_scale: 1.14,
            muzzle_length: 0.85,
            ear_size: 1.08,
            ear_spread: 0.45,
            ear_pointiness: 0.58,
            eye_size: 1.4,
            eye_lid: 0.0,
            eye_slant: 0.0,
            cheek_fluff: 0.9,
            butt_fluff: 1.25,
            tail_style: TailStyle::Stub,
            tail_length: 0.7,
            face: FacePattern::Blaze,
            chest: Chest::Ruff,
            brows: false,
            sock_length: 0.5,
        },
        regions: Vec::new(),
        marks: Vec::new(),
    }
}

/// A smooth, slightly wobbly blob (a merle patch or a tan point) as a polygon, rounded to
/// 0.1 so it writes to a skin file and reads back exactly. `wobble` (0..) makes it uneven.
fn blob(c: Pt, rx: f32, ry: f32, rot: f32, wobble: f32, seed: f32) -> Vec<Pt> {
    let n = 28;
    (0..n)
        .map(|i| {
            let a = i as f32 / n as f32 * std::f32::consts::TAU;
            let k = 1.0 + wobble * (0.6 * (3.0 * a + seed).sin() + 0.4 * (5.0 * a + 2.1 * seed).sin());
            let q = pt(a.cos() * rx * k, a.sin() * ry * k).rotate(rot);
            pt(((c.x + q.x) * 10.0).round() / 10.0, ((c.y + q.y) * 10.0).round() / 10.0)
        })
        .collect()
}

fn patch(space: Space, color: Rgba, points: Vec<Pt>) -> Mark {
    Mark { space, kind: MarkKind::Polygon, color, width: 2.0, points }
}

/// Tricolor: a black saddle and head with tan eyebrow dots and tan cheeks, white blaze, socks
/// and bib.
pub fn tricolor() -> Skin {
    let black = rgb(0x3D, 0x35, 0x39);
    let tan = rgb(0xDC, 0x95, 0x58);
    let mut s = corgi();
    s.name = "Tricolor Corgi".into();
    let p = &mut s.palette;
    p.body = black;
    p.body_shade = Some(rgb(0x31, 0x2A, 0x2E));
    p.legs = black;
    p.ear_outer = rgb(0x45, 0x3B, 0x40);
    p.ear_inner = rgb(0xF4, 0xBF, 0xB4);
    p.nose = rgb(0x1F, 0x18, 0x1A);
    p.mouth = rgb(0x3A, 0x2A, 0x2A);
    p.tail = black;
    p.outline = rgb(0x24, 0x1C, 0x1F);
    p.blush = rgb(0xFF, 0x94, 0xA4);
    p.markings = tan;
    s.marks = vec![
        // Eyebrow dots above each eye.
        patch(Space::Head, tan, blob(pt(-17.0, -14.5), 5.6, 3.6, 0.25, 0.0, 0.0)),
        patch(Space::Head, tan, blob(pt(17.0, -14.5), 5.6, 3.6, -0.25, 0.0, 0.0)),
        // Tan cheeks running out past the edge of the face.
        patch(Space::Head, tan, blob(pt(-56.0, 9.0), 15.0, 10.0, 0.3, 0.0, 0.0)),
        patch(Space::Head, tan, blob(pt(56.0, 9.0), 15.0, 10.0, -0.3, 0.0, 0.0)),
    ];
    s
}

/// Blue merle: soft grey-blue with darker merle patches (one around an eye) and blue eyes.
pub fn blue_merle() -> Skin {
    let blue = rgb(0xAB, 0xB8, 0xC8);
    let dark = rgb(0x5E, 0x69, 0x7A);
    let mut s = corgi();
    s.name = "Blue Merle Corgi".into();
    let p = &mut s.palette;
    p.body = blue;
    p.body_shade = Some(rgb(0x98, 0xA6, 0xB8));
    p.legs = blue;
    p.ear_outer = rgb(0x9E, 0xAB, 0xBC);
    p.ear_inner = rgb(0xF8, 0xCB, 0xC6);
    p.nose = rgb(0x2C, 0x2A, 0x31);
    p.mouth = rgb(0x3C, 0x35, 0x40);
    p.tail = blue;
    p.outline = rgb(0x46, 0x4F, 0x5D);
    p.blush = rgb(0xFF, 0x9D, 0xAE);
    p.markings = dark;
    p.iris = Some(rgb(0x5C, 0xB2, 0xE6));
    s.marks = vec![
        // A patch around the viewer-right eye, and one on the far side of the crown.
        patch(Space::Head, dark, blob(pt(29.0, 2.0), 15.5, 14.0, 0.2, 0.08, 1.0)),
        patch(Space::Head, dark, blob(pt(-38.0, -40.0), 17.0, 11.0, -0.4, 0.1, 2.0)),
        patch(Space::Head, dark, blob(pt(-8.0, -45.0), 6.5, 4.5, 0.1, 0.12, 3.0)),
        // Merle spots on the body.
        patch(Space::Body, dark, blob(pt(-46.0, -96.0), 11.0, 15.0, 0.3, 0.12, 4.0)),
        patch(Space::Body, dark, blob(pt(50.0, -46.0), 13.0, 10.0, -0.5, 0.12, 5.0)),
        patch(Space::Body, dark, blob(pt(-54.0, -28.0), 8.0, 6.0, 0.2, 0.12, 6.0)),
        patch(Space::Body, dark, blob(pt(40.0, -112.0), 6.0, 7.5, 0.0, 0.12, 7.0)),
    ];
    s
}

/// Fawn: a pale honey-gold coat.
pub fn fawn() -> Skin {
    let honey = rgb(0xF2, 0xBD, 0x7C);
    let mut s = corgi();
    s.name = "Fawn Corgi".into();
    let p = &mut s.palette;
    p.body = honey;
    p.body_shade = Some(rgb(0xE6, 0xA6, 0x62));
    p.legs = honey;
    p.ear_outer = rgb(0xEC, 0xB0, 0x6C);
    p.ear_inner = rgb(0xFB, 0xCF, 0xC2);
    p.tail = honey;
    p.outline = rgb(0x7A, 0x4C, 0x2C);
    p.markings = rgb(0xC9, 0x8A, 0x4A);
    s
}

/// Sable: rich red-brown with darker tipped fur on the back and a soft widow's peak.
pub fn sable() -> Skin {
    let red = rgb(0xC9, 0x6E, 0x37);
    let dark = rgb(0x8E, 0x4C, 0x27);
    let mut s = corgi();
    s.name = "Sable Corgi".into();
    let p = &mut s.palette;
    p.body = red;
    // Darker on the back, lighter towards the seat.
    p.body_shade = Some(rgb(0xE2, 0x93, 0x52));
    p.legs = red;
    p.ear_outer = rgb(0xA8, 0x5A, 0x2E);
    p.ear_inner = rgb(0xF6, 0xC2, 0xB2);
    p.tail = red;
    p.outline = rgb(0x4E, 0x2A, 0x17);
    p.markings = dark;
    // Widow's peak: a darker cap over the crown, dipping to a point above the blaze.
    let mut cap: Vec<Pt> = (0..=24)
        .map(|i| {
            let x = -70.0 + 140.0 * i as f32 / 24.0;
            let u = (x / 70.0).abs();
            let y = -40.0 + 12.0 * u * u - 4.0 * (1.0 - u).powi(3);
            pt(x, (y * 10.0).round() / 10.0)
        })
        .collect();
    cap.extend([pt(70.0, -70.0), pt(-70.0, -70.0)]);
    s.marks = vec![
        patch(Space::Head, dark.fade(0.55), cap),
        // Darker saddle along the back.
        patch(Space::Body, dark.fade(0.45), blob(pt(0.0, -150.0), 70.0, 48.0, 0.0, 0.0, 0.0)),
    ];
    s
}

/// Builds a built-in skin.
pub type SkinFn = fn() -> Skin;

/// The built-in dogs, in menu order: (id, constructor).
pub const BUILTINS: [(&str, SkinFn); 5] =
    [(BUILTIN_CORGI, corgi), ("tricolor", tricolor), ("blue_merle", blue_merle), ("fawn", fawn), ("sable", sable)];

pub fn builtin(id: &str) -> Option<Skin> {
    BUILTINS.iter().find(|(b, _)| *b == id).map(|(_, f)| f())
}

// ---------------------------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------------------------

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSkin {
    format_version: Option<i64>,
    name: Option<String>,
    author: Option<String>,
    palette: Option<RawPalette>,
    shape: RawShape,
    regions: Vec<RawRegion>,
    marks: Vec<RawMark>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawPalette {
    body: Option<String>,
    body_shade: Option<String>,
    belly: Option<String>,
    legs: Option<String>,
    paws: Option<String>,
    pads: Option<String>,
    ear_outer: Option<String>,
    ear_inner: Option<String>,
    nose: Option<String>,
    mouth: Option<String>,
    tongue: Option<String>,
    tail: Option<String>,
    tail_tip: Option<String>,
    outline: Option<String>,
    blush: Option<String>,
    markings: Option<String>,
    iris: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawShape {
    body_width: Option<f32>,
    leg_length: Option<f32>,
    head_scale: Option<f32>,
    muzzle_length: Option<f32>,
    ear_size: Option<f32>,
    ear_spread: Option<f32>,
    ear_pointiness: Option<f32>,
    eye_size: Option<f32>,
    eye_lid: Option<f32>,
    eye_slant: Option<f32>,
    cheek_fluff: Option<f32>,
    butt_fluff: Option<f32>,
    tail_style: Option<String>,
    tail_length: Option<f32>,
    face: Option<String>,
    chest: Option<String>,
    brows: Option<bool>,
    sock_length: Option<f32>,
}

#[derive(Deserialize)]
struct RawAnchor {
    x: f32,
    y: f32,
    in_x: Option<f32>,
    in_y: Option<f32>,
    out_x: Option<f32>,
    out_y: Option<f32>,
}

#[derive(Deserialize)]
struct RawRegion {
    space: String,
    fill: String,
    #[serde(default)]
    alpha: Option<f32>,
    anchors: Vec<RawAnchor>,
}

#[derive(Deserialize)]
struct RawMark {
    space: String,
    kind: String,
    #[serde(default)]
    color: Option<String>,
    #[serde(default)]
    alpha: Option<f32>,
    #[serde(default)]
    width: Option<f32>,
    points: Vec<[f32; 2]>,
}

fn color_field(field: &str, value: &Option<String>) -> Result<Option<Rgba>, String> {
    match value {
        None => Ok(None),
        Some(s) => Rgba::parse_hex(s)
            .map(Some)
            .ok_or_else(|| format!("{field}: colour must be #RRGGBB or #RRGGBBAA, got \"{s}\"")),
    }
}

fn clamp_field(name: &str, v: Option<f32>, default: f32) -> f32 {
    let (lo, hi) = range_of(name);
    match v {
        Some(x) if x.is_finite() => x.clamp(lo, hi),
        _ => default,
    }
}

fn clamp_coord(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(-COORD_LIMIT, COORD_LIMIT)
    } else {
        0.0
    }
}

fn opacity(field: &str, alpha: Option<f32>) -> Result<f32, String> {
    match alpha {
        None => Ok(1.0),
        Some(a) if a.is_finite() => Ok(a.clamp(0.0, 1.0)),
        Some(_) => Err(format!("{field}: alpha must be a number from 0 to 1")),
    }
}

/// Parses and validates a skin file. Errors are short, human-readable sentences.
pub fn parse_skin(text: &str) -> Result<Skin, String> {
    if text.len() > MAX_FILE_BYTES {
        return Err(format!("the skin file is larger than {} MB", MAX_FILE_BYTES / (1024 * 1024)));
    }
    let raw: RawSkin = toml::from_str(text).map_err(|e| format!("not a valid skin file: {}", e.message()))?;

    match raw.format_version {
        None => return Err("format_version is missing (it should be 1)".into()),
        Some(v) if v > FORMAT_VERSION => {
            return Err(format!("this skin needs a newer DeskCorgi (format_version {v}); please update DeskCorgi"))
        }
        Some(v) if v < 1 => return Err(format!("format_version must be 1, got {v}")),
        Some(_) => {}
    }

    let name: String = raw.name.as_deref().unwrap_or("").trim().chars().take(MAX_NAME_CHARS).collect();
    if name.is_empty() {
        return Err("name is missing or empty".into());
    }
    let author: String = raw.author.as_deref().unwrap_or("").trim().chars().take(MAX_NAME_CHARS).collect();

    let rp = raw.palette.ok_or("the [palette] table is missing")?;
    let body = color_field("palette.body", &rp.body)?.ok_or("palette.body is required")?;
    let belly = color_field("palette.belly", &rp.belly)?.unwrap_or_else(|| body.mix(rgb(255, 250, 242), 0.82));
    let legs = color_field("palette.legs", &rp.legs)?.unwrap_or(body);
    let ear_outer = color_field("palette.ear_outer", &rp.ear_outer)?.unwrap_or(body);
    let tail = color_field("palette.tail", &rp.tail)?.unwrap_or(body);
    let palette = Palette {
        body,
        body_shade: color_field("palette.body_shade", &rp.body_shade)?,
        belly,
        legs,
        paws: color_field("palette.paws", &rp.paws)?.unwrap_or(belly),
        pads: color_field("palette.pads", &rp.pads)?.unwrap_or(rgb(0xF2, 0x9C, 0xA3)),
        ear_outer,
        ear_inner: color_field("palette.ear_inner", &rp.ear_inner)?
            .unwrap_or_else(|| ear_outer.mix(rgb(0xF9, 0xC9, 0xB9), 0.65)),
        nose: color_field("palette.nose", &rp.nose)?.unwrap_or(rgb(0x2E, 0x1F, 0x1A)),
        mouth: color_field("palette.mouth", &rp.mouth)?.unwrap_or(rgb(0x4A, 0x2B, 0x22)),
        tongue: color_field("palette.tongue", &rp.tongue)?.unwrap_or(rgb(0xFF, 0x86, 0x9E)),
        tail,
        tail_tip: color_field("palette.tail_tip", &rp.tail_tip)?.unwrap_or(tail),
        outline: color_field("palette.outline", &rp.outline)?.unwrap_or_else(|| body.darken(0.62)),
        blush: color_field("palette.blush", &rp.blush)?.unwrap_or(rgb(0xFF, 0x8F, 0x8F)),
        markings: color_field("palette.markings", &rp.markings)?.unwrap_or_else(|| body.darken(0.25)),
        iris: color_field("palette.iris", &rp.iris)?,
    };

    let d = corgi().shape;
    let s = raw.shape;
    let tail_style = match s.tail_style.as_deref() {
        None => d.tail_style,
        Some("stub") => TailStyle::Stub,
        Some("curl") => TailStyle::Curl,
        Some("plume") => TailStyle::Plume,
        Some(other) => return Err(format!("shape.tail_style must be \"stub\", \"curl\" or \"plume\", got \"{other}\"")),
    };
    let face = match s.face.as_deref() {
        None => d.face,
        Some("none") => FacePattern::None,
        Some("blaze") => FacePattern::Blaze,
        Some("urajiro") => FacePattern::Urajiro,
        Some(other) => return Err(format!("shape.face must be \"none\", \"blaze\" or \"urajiro\", got \"{other}\"")),
    };
    let chest = match s.chest.as_deref() {
        None => d.chest,
        Some("none") => Chest::None,
        Some("ruff") => Chest::Ruff,
        Some("bib") => Chest::Bib,
        Some(other) => return Err(format!("shape.chest must be \"none\", \"ruff\" or \"bib\", got \"{other}\"")),
    };
    let shape = Shape {
        body_width: clamp_field("body_width", s.body_width, d.body_width),
        leg_length: clamp_field("leg_length", s.leg_length, d.leg_length),
        head_scale: clamp_field("head_scale", s.head_scale, d.head_scale),
        muzzle_length: clamp_field("muzzle_length", s.muzzle_length, d.muzzle_length),
        ear_size: clamp_field("ear_size", s.ear_size, d.ear_size),
        ear_spread: clamp_field("ear_spread", s.ear_spread, d.ear_spread),
        ear_pointiness: clamp_field("ear_pointiness", s.ear_pointiness, d.ear_pointiness),
        eye_size: clamp_field("eye_size", s.eye_size, d.eye_size),
        eye_lid: clamp_field("eye_lid", s.eye_lid, d.eye_lid),
        eye_slant: clamp_field("eye_slant", s.eye_slant, d.eye_slant),
        cheek_fluff: clamp_field("cheek_fluff", s.cheek_fluff, d.cheek_fluff),
        butt_fluff: clamp_field("butt_fluff", s.butt_fluff, d.butt_fluff),
        tail_style,
        tail_length: clamp_field("tail_length", s.tail_length, d.tail_length),
        face,
        chest,
        brows: s.brows.unwrap_or(d.brows),
        sock_length: clamp_field("sock_length", s.sock_length, d.sock_length),
    };

    if raw.regions.len() > MAX_REGIONS {
        return Err(format!("too many regions ({}, the limit is {MAX_REGIONS})", raw.regions.len()));
    }
    let mut regions = Vec::with_capacity(raw.regions.len());
    for (i, r) in raw.regions.iter().enumerate() {
        let field = format!("regions[{i}]");
        let space = Space::parse(&r.space).ok_or_else(|| format!("{field}.space \"{}\" is not a known part", r.space))?;
        let fill = color_field(&format!("{field}.fill"), &Some(r.fill.clone()))?.unwrap_or(body);
        let fill = fill.fade(opacity(&field, r.alpha)?);
        if r.anchors.len() < 2 || r.anchors.len() > MAX_ANCHORS {
            return Err(format!("{field} needs 2 to {MAX_ANCHORS} anchors, got {}", r.anchors.len()));
        }
        let anchors = r
            .anchors
            .iter()
            .map(|a| {
                let p = pt(clamp_coord(a.x), clamp_coord(a.y));
                let handle = |hx: Option<f32>, hy: Option<f32>| match (hx, hy) {
                    (Some(x), Some(y)) => pt(clamp_coord(x), clamp_coord(y)),
                    _ => p,
                };
                Anchor { p, handle_in: handle(a.in_x, a.in_y), handle_out: handle(a.out_x, a.out_y) }
            })
            .collect();
        regions.push(Region { space, fill, anchors });
    }

    if raw.marks.len() > MAX_MARKS {
        return Err(format!("too many marks ({}, the limit is {MAX_MARKS})", raw.marks.len()));
    }
    let mut marks = Vec::with_capacity(raw.marks.len());
    for (i, m) in raw.marks.iter().enumerate() {
        let field = format!("marks[{i}]");
        let space = Space::parse(&m.space).ok_or_else(|| format!("{field}.space \"{}\" is not a known part", m.space))?;
        let kind = match m.kind.as_str() {
            "polygon" => MarkKind::Polygon,
            "stroke" => MarkKind::Stroke,
            other => return Err(format!("{field}.kind must be \"polygon\" or \"stroke\", got \"{other}\"")),
        };
        let min_points = if kind == MarkKind::Polygon { 3 } else { 2 };
        if m.points.len() < min_points || m.points.len() > MAX_MARK_POINTS {
            return Err(format!("{field} needs {min_points} to {MAX_MARK_POINTS} points, got {}", m.points.len()));
        }
        let color = color_field(&format!("{field}.color"), &m.color)?.unwrap_or(palette.markings);
        let color = color.fade(opacity(&field, m.alpha)?);
        let width = match m.width {
            Some(w) if w.is_finite() => w.clamp(0.2, 24.0),
            _ => 2.0,
        };
        let points = m.points.iter().map(|p| pt(clamp_coord(p[0]), clamp_coord(p[1]))).collect();
        marks.push(Mark { space, kind, color, width, points });
    }

    Ok(Skin { name, author, palette, shape, regions, marks })
}

// ---------------------------------------------------------------------------------------------
// Writing (used for the template files in `skins/`)
// ---------------------------------------------------------------------------------------------

fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7F => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Formats a float so it parses back to exactly the same value and always reads as a float.
fn toml_f32(v: f32) -> String {
    let s = format!("{v:?}");
    if s.contains('.') || s.contains('e') || s.contains("inf") || s.contains("NaN") {
        s
    } else {
        format!("{s}.0")
    }
}

impl Skin {
    pub fn to_toml(&self) -> String {
        use std::fmt::Write;
        let mut o = String::new();
        let p = &self.palette;
        let s = &self.shape;
        let _ = writeln!(o, "# DeskCorgi skin. Every value is optional except format_version, name and palette.body.");
        let _ = writeln!(o, "format_version = {FORMAT_VERSION}");
        let _ = writeln!(o, "name = {}", toml_string(&self.name));
        let _ = writeln!(o, "author = {}", toml_string(&self.author));
        let _ = writeln!(o, "\n[palette]");
        let mut color = |k: &str, c: Option<Rgba>| {
            if let Some(c) = c {
                let _ = writeln!(o, "{k} = \"{}\"", c.to_hex());
            }
        };
        color("body", Some(p.body));
        color("body_shade", p.body_shade);
        color("belly", Some(p.belly));
        color("legs", Some(p.legs));
        color("paws", Some(p.paws));
        color("pads", Some(p.pads));
        color("ear_outer", Some(p.ear_outer));
        color("ear_inner", Some(p.ear_inner));
        color("nose", Some(p.nose));
        color("mouth", Some(p.mouth));
        color("tongue", Some(p.tongue));
        color("tail", Some(p.tail));
        color("tail_tip", Some(p.tail_tip));
        color("outline", Some(p.outline));
        color("blush", Some(p.blush));
        color("markings", Some(p.markings));
        color("iris", p.iris);
        let _ = writeln!(o, "\n[shape]");
        let nums = [
            ("body_width", s.body_width),
            ("leg_length", s.leg_length),
            ("head_scale", s.head_scale),
            ("muzzle_length", s.muzzle_length),
            ("ear_size", s.ear_size),
            ("ear_spread", s.ear_spread),
            ("ear_pointiness", s.ear_pointiness),
            ("eye_size", s.eye_size),
            ("eye_lid", s.eye_lid),
            ("eye_slant", s.eye_slant),
            ("cheek_fluff", s.cheek_fluff),
            ("butt_fluff", s.butt_fluff),
            ("tail_length", s.tail_length),
            ("sock_length", s.sock_length),
        ];
        for (k, v) in nums {
            let (lo, hi) = range_of(k);
            let _ = writeln!(o, "{k} = {}  # {} to {}", toml_f32(v), toml_f32(lo), toml_f32(hi));
        }
        let tail = match s.tail_style {
            TailStyle::Stub => "stub",
            TailStyle::Curl => "curl",
            TailStyle::Plume => "plume",
        };
        let face = match s.face {
            FacePattern::None => "none",
            FacePattern::Blaze => "blaze",
            FacePattern::Urajiro => "urajiro",
        };
        let chest = match s.chest {
            Chest::None => "none",
            Chest::Ruff => "ruff",
            Chest::Bib => "bib",
        };
        let _ = writeln!(o, "tail_style = \"{tail}\"  # stub, curl or plume");
        let _ = writeln!(o, "face = \"{face}\"  # none, blaze or urajiro");
        let _ = writeln!(o, "chest = \"{chest}\"  # none, ruff or bib");
        let _ = writeln!(o, "brows = {}", s.brows);
        for r in &self.regions {
            let _ = writeln!(o, "\n[[regions]]\nspace = \"{}\"\nfill = \"{}\"\nanchors = [", r.space.name(), r.fill.to_hex());
            for a in &r.anchors {
                let _ = writeln!(
                    o,
                    "  {{ x = {}, y = {}, in_x = {}, in_y = {}, out_x = {}, out_y = {} }},",
                    toml_f32(a.p.x),
                    toml_f32(a.p.y),
                    toml_f32(a.handle_in.x),
                    toml_f32(a.handle_in.y),
                    toml_f32(a.handle_out.x),
                    toml_f32(a.handle_out.y)
                );
            }
            let _ = writeln!(o, "]");
        }
        for m in &self.marks {
            let kind = if m.kind == MarkKind::Polygon { "polygon" } else { "stroke" };
            let _ = writeln!(
                o,
                "\n[[marks]]\nspace = \"{}\"\nkind = \"{kind}\"\ncolor = \"{}\"\nwidth = {}",
                m.space.name(),
                m.color.to_hex(),
                toml_f32(m.width)
            );
            let pts: Vec<String> = m.points.iter().map(|p| format!("[{}, {}]", toml_f32(p.x), toml_f32(p.y))).collect();
            let _ = writeln!(o, "points = [{}]", pts.join(", "));
        }
        o
    }
}

// ---------------------------------------------------------------------------------------------
// Imported skins on disk
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct SkinEntry {
    /// A built-in id (`corgi`) or the 16-hex-digit content hash of an
    /// imported file.
    pub id: String,
    pub name: String,
}

pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

fn is_hash_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| b.is_ascii_hexdigit())
}

fn imported_path(dir: &Path, id: &str) -> Option<PathBuf> {
    is_hash_id(id).then(|| dir.join(format!("{id}.toml")))
}

/// Lists valid imported skins in `dir`, sorted by name. Broken files are skipped.
pub fn list_imported(dir: &Path) -> Vec<SkinEntry> {
    let mut out = Vec::new();
    let Ok(read) = std::fs::read_dir(dir) else { return out };
    for entry in read.flatten() {
        let path = entry.path();
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        if path.extension().and_then(|e| e.to_str()) != Some("toml") || !is_hash_id(stem) {
            continue;
        }
        if let Ok(text) = std::fs::read_to_string(&path) {
            if let Ok(skin) = parse_skin(&text) {
                out.push(SkinEntry { id: stem.to_ascii_lowercase(), name: skin.name });
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
    out
}

/// Validates `src` and copies it into `dir` under its content hash.
pub fn import_file(src: &Path, dir: &Path) -> Result<SkinEntry, String> {
    let meta = std::fs::metadata(src).map_err(|e| format!("could not open the file: {e}"))?;
    if meta.len() > MAX_FILE_BYTES as u64 {
        return Err(format!("the skin file is larger than {} MB", MAX_FILE_BYTES / (1024 * 1024)));
    }
    let bytes = std::fs::read(src).map_err(|e| format!("could not read the file: {e}"))?;
    let text = String::from_utf8(bytes).map_err(|_| "the skin file is not UTF-8 text".to_string())?;
    let skin = parse_skin(&text)?;
    let id = format!("{:016x}", fnv1a64(text.as_bytes()));
    std::fs::create_dir_all(dir).map_err(|e| format!("could not create the skins folder: {e}"))?;
    let dest = dir.join(format!("{id}.toml"));
    let tmp = dir.join(format!("{id}.tmp"));
    std::fs::write(&tmp, text.as_bytes()).map_err(|e| format!("could not save the skin: {e}"))?;
    std::fs::rename(&tmp, &dest).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("could not save the skin: {e}")
    })?;
    Ok(SkinEntry { id, name: skin.name })
}

/// Loads a built-in or imported skin by id.
pub fn load(id: &str, dir: &Path) -> Option<Skin> {
    if let Some(s) = builtin(id) {
        return Some(s);
    }
    let text = std::fs::read_to_string(imported_path(dir, id)?).ok()?;
    parse_skin(&text).ok()
}

pub fn delete_imported(id: &str, dir: &Path) -> Result<(), String> {
    let path = imported_path(dir, id).ok_or("built-in skins cannot be deleted")?;
    std::fs::remove_file(path).map_err(|e| format!("could not delete the skin: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str = "format_version = 1\nname = \"Mini\"\n[palette]\nbody = \"#336699\"\n";

    #[test]
    fn builtins_round_trip_through_toml() {
        for (_, f) in BUILTINS {
            let skin = f();
            let text = skin.to_toml();
            let back = parse_skin(&text).unwrap_or_else(|e| panic!("{}: {e}\n{text}", skin.name));
            assert_eq!(back, skin);
        }
    }

    #[test]
    fn builtin_shapes_are_within_their_ranges() {
        for (_, f) in BUILTINS {
            let skin = f();
            let text = skin.to_toml();
            // Clamping must not change anything for the built-ins.
            assert_eq!(parse_skin(&text).unwrap().shape, skin.shape);
        }
    }

    /// Head markings of the built-ins are painted on the face (`head` space), like the blaze
    /// and the eyes, so they can never slide against the face when the head turns. Each one
    /// either sits inside the face or runs clearly past its edge (the outline clips it), so
    /// no thin sliver of fur can open up at the rim.
    #[test]
    fn builtin_head_markings_are_painted_on_the_face() {
        use crate::rig::{HEAD_RX, HEAD_RY};
        for (_, f) in BUILTINS {
            let skin = f();
            for m in &skin.marks {
                assert!(m.space != Space::Skull && m.space != Space::Muzzle, "{}: head marks belong in `head` space", skin.name);
                if m.space != Space::Head {
                    continue;
                }
                let reach = m.points.iter().map(|q| ((q.x / HEAD_RX).powi(2) + (q.y / HEAD_RY).powi(2)).sqrt()).fold(0.0f32, f32::max);
                assert!(reach < 0.97 || reach > 1.08, "{}: a head mark ends right at the rim ({reach})", skin.name);
            }
        }
    }

    #[test]
    fn minimal_skin_fills_defaults() {
        let s = parse_skin(MINIMAL).unwrap();
        assert_eq!(s.name, "Mini");
        assert_eq!(s.palette.body, rgb(0x33, 0x66, 0x99));
        assert_eq!(s.palette.legs, s.palette.body);
        assert_eq!(s.shape, corgi().shape);
    }

    #[test]
    fn numbers_are_clamped_and_unknown_fields_ignored() {
        let text = format!("{MINIMAL}future_field = 3\n[shape]\nbody_width = 9\near_size = -4\nwhatever = \"x\"\n");
        let s = parse_skin(&text).unwrap();
        assert_eq!(s.shape.body_width, 1.35);
        assert_eq!(s.shape.ear_size, 0.6);
    }

    #[test]
    fn rejections_have_clear_messages() {
        let cases: &[(&str, &str)] = &[
            ("name = \"x\"\n[palette]\nbody = \"#000000\"\n", "format_version is missing"),
            ("format_version = 2\nname = \"x\"\n[palette]\nbody = \"#000000\"\n", "newer DeskCorgi"),
            ("format_version = 1\nname = \"  \"\n[palette]\nbody = \"#000000\"\n", "name is missing"),
            ("format_version = 1\nname = \"x\"\n", "[palette] table is missing"),
            ("format_version = 1\nname = \"x\"\n[palette]\nbelly = \"#000000\"\n", "palette.body is required"),
            ("format_version = 1\nname = \"x\"\n[palette]\nbody = \"pink\"\n", "palette.body: colour must be"),
            ("format_version = 1\nname = \"x\"\n[palette]\nbody = 5\n", "not a valid skin file"),
            ("format_version = 1\nname = \"x\"\n[palette]\nbody = \"#000000\"\n[shape]\ntail_style = \"long\"\n", "shape.tail_style"),
            ("format_version = 1\nname = [", "not a valid skin file"),
        ];
        for (text, needle) in cases {
            let err = parse_skin(text).unwrap_err();
            assert!(err.contains(needle), "expected {needle:?} in {err:?}");
        }
    }

    #[test]
    fn regions_and_marks_parse_and_validate() {
        let ok = format!(
            "{MINIMAL}[[regions]]\nspace = \"head\"\nfill = \"#FFFFFF\"\nalpha = 0.5\nanchors = [{{ x = 0, y = -30 }}, {{ x = 900, y = 10, in_x = 4, in_y = -5, out_x = 12, out_y = 20 }}]\n\
             [[marks]]\nspace = \"tail\"\nkind = \"stroke\"\npoints = [[0, 0], [50, 2]]\n"
        );
        let s = parse_skin(&ok).unwrap();
        assert_eq!(s.regions.len(), 1);
        assert_eq!(s.regions[0].fill.a, 128);
        assert_eq!(s.regions[0].anchors[1].p.x, COORD_LIMIT);
        assert_eq!(s.regions[0].anchors[0].handle_in, s.regions[0].anchors[0].p);
        assert_eq!(s.marks[0].color, s.palette.markings);
        assert_eq!(s.marks[0].width, 2.0);

        let bad_space = format!("{MINIMAL}[[marks]]\nspace = \"wing\"\nkind = \"stroke\"\npoints = [[0, 0], [1, 1]]\n");
        assert!(parse_skin(&bad_space).unwrap_err().contains("not a known part"));
        let few = format!("{MINIMAL}[[marks]]\nspace = \"head\"\nkind = \"polygon\"\npoints = [[0, 0], [1, 1]]\n");
        assert!(parse_skin(&few).unwrap_err().contains("needs 3"));
    }

    #[test]
    fn name_is_trimmed_and_truncated() {
        let long = "é".repeat(60);
        let text = format!("format_version = 1\nname = \"  {long}  \"\n[palette]\nbody = \"#000000\"\n");
        assert_eq!(parse_skin(&text).unwrap().name.chars().count(), MAX_NAME_CHARS);
    }

    #[test]
    fn import_list_load_delete() {
        let dir = std::env::temp_dir().join(format!("deskcorgi-skin-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src = std::env::temp_dir().join(format!("deskcorgi-src-{}.toml", std::process::id()));
        std::fs::write(&src, MINIMAL).unwrap();
        let entry = import_file(&src, &dir).unwrap();
        assert_eq!(entry.name, "Mini");
        assert_eq!(list_imported(&dir), vec![entry.clone()]);
        assert_eq!(load(&entry.id, &dir).unwrap().name, "Mini");
        assert!(load("../etc", &dir).is_none());
        assert_eq!(load(BUILTIN_CORGI, &dir).unwrap(), corgi());
        assert!(load("shiba", &dir).is_none(), "the Shiba is no longer built in");
        delete_imported(&entry.id, &dir).unwrap();
        assert!(list_imported(&dir).is_empty());
        assert!(delete_imported(BUILTIN_CORGI, &dir).is_err());
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&src);
    }

    #[test]
    fn template_files_match_builtins() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skins");
        for (id, f) in BUILTINS {
            let file = format!("{id}.toml");
            let text = std::fs::read_to_string(root.join(&file)).expect("template file exists");
            assert_eq!(text.replace("\r\n", "\n"), f().to_toml(), "{file} is out of date: run `cargo run -p deskcorgi-core --example export_skins`");
        }
    }
}
