//! The dog's behaviour: turns input and the current mood into spring targets, steps the
//! springs and produces a `Pose` for the renderer.

use std::f32::consts::PI;

use crate::geom::{pt, Pt, Rng};
use crate::input::{wpm, InputEvent, MouseButton, RateCounter};
use crate::keymap::{KeyId, KEY_COUNT};
use crate::rig::Rig;
use crate::scene::{key_center, paw_rest, preferred_side, side_sign, MOUSE_REST, MOUSE_SIDE, SEAT, WAVE_SIDE};
use crate::skin::{Skin, TailStyle};
use crate::spring::{Spring, Spring2, MAX_DT};
use crate::state::{Mood, MoodConfig, MoodMachine, Signals};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PawPose {
    pub pos: Pt,
    /// 0 = resting, 1 = pushing a key down.
    pub press: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HindPose {
    pub pos: Pt,
    /// 0 = hidden behind the keyboard, 1 = fully raised onto a key.
    pub up: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParticleKind {
    Heart,
    Zzz,
    Star,
}

/// A key the user just pressed, popping up from the dog's keyboard as a little keycap and
/// fading away. Only the label printed on the key is kept, and only for about a second.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KeyPopup {
    pub label: &'static str,
    /// Where it starts: the matching key on the dog's keyboard.
    pub from: Pt,
    pub age: f32,
    pub life: f32,
    /// Sideways drift and tilt, a little different for every pop-up.
    pub drift: f32,
    pub tilt: f32,
}

/// Closest the two front paws may come side by side (a paw is ~26 wide).
const PAW_GAP: f32 = 22.0;

/// Keeps the viewer-left paw to the left of the viewer-right paw so the arms never cross.
/// A paw that is only hovering or resting steps aside for one that is on a key or the
/// mouse; two pinned paws (keys stacked above each other) spread apart around their middle.
fn keep_paws_apart(targets: &mut [(Pt, f32); 2], pinned: [bool; 2]) {
    let (l, r) = (targets[0].0.x, targets[1].0.x);
    if l <= r - PAW_GAP {
        return;
    }
    match pinned {
        [true, false] => targets[1].0.x = l + PAW_GAP,
        [false, true] => targets[0].0.x = r - PAW_GAP,
        _ => {
            let mid = (l + r) / 2.0;
            targets[0].0.x = mid - PAW_GAP / 2.0;
            targets[1].0.x = mid + PAW_GAP / 2.0;
        }
    }
}

/// How long a key pop-up stays, and how many can be on screen at once.
pub const KEY_POPUP_LIFE: f32 = 1.1;
const MAX_KEY_POPUPS: usize = 6;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Particle {
    pub kind: ParticleKind,
    pub pos: Pt,
    pub vel: Pt,
    pub age: f32,
    pub life: f32,
    pub size: f32,
    pub spin: f32,
}

/// Everything the renderer needs for one frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    pub time: f32,
    pub mood: Mood,
    pub body_lift: f32,
    pub body_lean: f32,
    pub body_tilt: f32,
    pub body_squash: f32,
    pub hip_sway: f32,
    pub head_yaw: f32,
    pub head_pitch: f32,
    pub head_tilt: f32,
    pub head_drop: f32,
    pub ear_perk: f32,
    pub ear_fold: f32,
    pub ear_twitch: [f32; 2],
    pub eye_open: f32,
    pub eye_happy: f32,
    pub gaze: Pt,
    pub pupil: f32,
    pub sparkle: f32,
    /// 0..1: the focused "in the zone" face while typing fast (slanted lids, 💢 mark).
    pub focus: f32,
    pub mouth_open: f32,
    pub tongue: f32,
    pub blush: f32,
    pub tail_wag: f32,
    pub paws: [PawPose; 2],
    pub hind: [HindPose; 2],
    /// Elbow of each front leg and knee of each back leg, solved by IK (NaN = not solved yet).
    pub elbows: [Pt; 2],
    pub knees: [Pt; 2],
    pub mouse_offset: Pt,
    /// Pressed amount of the user's left / right mouse button.
    pub mouse_buttons: [f32; 2],
    pub key_glow: [f32; KEY_COUNT],
    pub key_down: [bool; KEY_COUNT],
    pub marquee: f32,
    pub marquee_boost: f32,
    pub particles: Vec<Particle>,
    pub key_popups: Vec<KeyPopup>,
}

impl Pose {
    pub fn neutral() -> Pose {
        Pose {
            time: 0.0,
            mood: Mood::Idle,
            body_lift: 0.0,
            body_lean: 0.0,
            body_tilt: 0.0,
            body_squash: 1.0,
            hip_sway: 0.0,
            head_yaw: 0.0,
            head_pitch: 0.0,
            head_tilt: 0.0,
            head_drop: 0.0,
            ear_perk: 0.0,
            ear_fold: 0.0,
            ear_twitch: [0.0; 2],
            eye_open: 1.0,
            eye_happy: 0.0,
            gaze: Pt::ZERO,
            pupil: 1.0,
            sparkle: 0.0,
            focus: 0.0,
            mouth_open: 0.0,
            tongue: 0.0,
            blush: 0.35,
            tail_wag: 0.0,
            paws: [PawPose { pos: paw_rest(0), press: 0.0 }, PawPose { pos: paw_rest(1), press: 0.0 }],
            hind: [HindPose { pos: SEAT, up: 0.0 }, HindPose { pos: SEAT, up: 0.0 }],
            elbows: [pt(f32::NAN, f32::NAN); 2],
            knees: [pt(f32::NAN, f32::NAN); 2],
            mouse_offset: Pt::ZERO,
            mouse_buttons: [0.0; 2],
            key_glow: [0.0; KEY_COUNT],
            key_down: [false; KEY_COUNT],
            marquee: 0.0,
            marquee_boost: 0.0,
            particles: Vec::new(),
            key_popups: Vec::new(),
        }
    }
}

/// Per-frame input from the platform layer.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameInput {
    /// Cursor position in canvas units (may be far outside the canvas).
    pub cursor: Option<Pt>,
    /// The user is dragging the dog around.
    pub dragging: bool,
    /// Drag velocity in canvas units per second.
    pub drag_velocity: Pt,
}

const MAX_HELD: usize = 16;
const MAX_PARTICLES: usize = 48;
const MOUSE_PAW_HOLD: f64 = 1.6;
/// A key must be held this long before a back paw comes up for it. Normal fast typing
/// overlaps keys for a few dozen milliseconds; that must not summon extra paws.
pub const HIND_PAW_DELAY: f64 = 0.2;

fn soft_sat(x: f32) -> f32 {
    x / (1.0 + x * x).sqrt()
}

pub struct Pet {
    now: f64,
    rng: Rng,
    mood: MoodMachine,
    held: Vec<KeyId>,
    key_down: [bool; KEY_COUNT],
    key_glow: [f32; KEY_COUNT],
    key_presses: RateCounter,
    clicks: RateCounter,
    key_pressed_frame: bool,
    mouse_events: bool,
    raw_mouse: Pt,
    buttons: [bool; 3],
    last_mouse: f64,
    last_key_down: f64,
    key_down_at: [f64; KEY_COUNT],
    prev_cursor: Option<Pt>,
    boops: f32,
    wpm: f32,
    pub keys_total: u64,
    pub clicks_total: u64,

    paw: [Spring2; 2],
    press: [Spring; 2],
    paw_key: [Option<KeyId>; 2],
    paw_release: [f64; 2],
    paw_last: [Pt; 2],
    hind: [Spring2; 2],
    hind_up: [Spring; 2],
    elbow: [Spring2; 2],
    knee: [Spring2; 2],
    joints_ready: bool,
    wave_until: f64,

    lift: Spring,
    lean: Spring,
    tilt: Spring,
    squash: Spring,
    yaw: Spring,
    pitch: Spring,
    head_tilt: Spring,
    drop: Spring,
    perk: Spring,
    fold: Spring,
    twitch: [Spring; 2],
    gaze: Spring2,
    pupil: Spring,
    sparkle: Spring,
    focus: Spring,
    happy: Spring,
    sleepy: Spring,
    mouth: Spring,
    tongue: Spring,
    blush: Spring,
    wag_speed: Spring,
    wag_amp: Spring,
    wag_phase: f32,
    mouse_off: Spring2,
    mouse_target: Pt,
    mouse_btn: [Spring; 2],

    next_blink: f64,
    blink_start: f64,
    next_twitch: f64,
    next_curious: f64,
    curious_until: f64,
    curious_dir: f32,
    next_blep: f64,
    blep_until: f64,
    next_heart: f64,
    next_z: f64,
    next_hop: f64,
    hops_left: u8,
    breath_phase: f32,
    marquee: f32,
    particles: Vec<Particle>,
    popups: Vec<KeyPopup>,
    pose: Pose,
}

impl Pet {
    pub fn new(seed: u32) -> Pet {
        let rest = [paw_rest(0), paw_rest(1)];
        let paw_spring = |p: Pt| Spring2::new(p, 800.0, 42.0);
        Pet {
            now: 0.0,
            rng: Rng::new(seed ^ 0x9E37_79B9),
            mood: MoodMachine::new(0.0),
            held: Vec::with_capacity(MAX_HELD),
            key_down: [false; KEY_COUNT],
            key_glow: [0.0; KEY_COUNT],
            key_presses: RateCounter::default(),
            clicks: RateCounter::default(),
            key_pressed_frame: false,
            mouse_events: false,
            raw_mouse: Pt::ZERO,
            buttons: [false; 3],
            last_mouse: f64::NEG_INFINITY,
            last_key_down: f64::NEG_INFINITY,
            key_down_at: [0.0; KEY_COUNT],
            prev_cursor: None,
            boops: 0.0,
            wpm: 0.0,
            keys_total: 0,
            clicks_total: 0,
            paw: [paw_spring(rest[0]), paw_spring(rest[1])],
            press: [Spring::new(0.0, 1200.0, 50.0); 2],
            paw_key: [None; 2],
            paw_release: [f64::NEG_INFINITY; 2],
            paw_last: rest,
            hind: [Spring2::new(SEAT, 500.0, 34.0); 2],
            hind_up: [Spring::new(0.0, 380.0, 30.0); 2],
            elbow: [Spring2::new(Pt::ZERO, 900.0, 50.0); 2],
            knee: [Spring2::new(Pt::ZERO, 900.0, 50.0); 2],
            joints_ready: false,
            wave_until: f64::NEG_INFINITY,
            lift: Spring::bouncy(0.0),
            lean: Spring::soft(0.0),
            tilt: Spring::soft(0.0),
            squash: Spring::new(1.0, 200.0, 16.0),
            yaw: Spring::soft(0.0),
            pitch: Spring::soft(0.0),
            head_tilt: Spring::soft(0.0),
            drop: Spring::new(0.0, 90.0, 14.0),
            perk: Spring::new(0.0, 220.0, 16.0),
            fold: Spring::new(0.0, 160.0, 18.0),
            twitch: [Spring::new(0.0, 520.0, 13.0); 2],
            gaze: Spring2::new(Pt::ZERO, 320.0, 30.0),
            pupil: Spring::new(1.0, 260.0, 20.0),
            sparkle: Spring::soft(0.0),
            focus: Spring::soft(0.0),
            happy: Spring::new(0.0, 300.0, 26.0),
            sleepy: Spring::new(0.0, 60.0, 14.0),
            mouth: Spring::new(0.0, 300.0, 24.0),
            tongue: Spring::new(0.0, 200.0, 20.0),
            blush: Spring::soft(0.35),
            wag_speed: Spring::new(2.6, 40.0, 12.0),
            wag_amp: Spring::new(0.16, 40.0, 12.0),
            wag_phase: 0.0,
            mouse_off: Spring2::new(Pt::ZERO, 300.0, 30.0),
            mouse_target: Pt::ZERO,
            mouse_btn: [Spring::new(0.0, 1200.0, 50.0); 2],
            next_blink: 1.5,
            blink_start: f64::NEG_INFINITY,
            next_twitch: 4.0,
            next_curious: 8.0,
            curious_until: f64::NEG_INFINITY,
            curious_dir: 1.0,
            next_blep: 20.0,
            blep_until: f64::NEG_INFINITY,
            next_heart: 0.0,
            next_z: 0.0,
            next_hop: 0.0,
            hops_left: 0,
            breath_phase: 0.0,
            marquee: 0.0,
            particles: Vec::new(),
            popups: Vec::new(),
            pose: Pose::neutral(),
        }
    }

    pub fn pose(&self) -> &Pose {
        &self.pose
    }

    pub fn mood(&self) -> Mood {
        self.mood.mood
    }

    pub fn now(&self) -> f64 {
        self.now
    }

    pub fn seconds_since_input(&self) -> f64 {
        self.mood.seconds_since_input(self.now)
    }

    /// True while something is visibly moving fast enough to deserve the higher frame rate.
    pub fn is_lively(&self) -> bool {
        self.seconds_since_input() < 2.0
            || !self.particles.is_empty()
            || !self.popups.is_empty()
            || !matches!(self.mood.mood, Mood::Idle | Mood::Sleep)
            || !self.lift.is_settled()
    }

    pub fn rig(&self, skin: &Skin) -> Rig {
        Rig::new(&skin.shape, &self.pose)
    }

    /// Is this canvas point on the dog's head?
    pub fn head_hit(&self, p: Pt, skin: &Skin) -> bool {
        self.rig(skin).head_contains(p)
    }

    /// Waves hello with the viewer-right paw for a moment.
    pub fn wave(&mut self) {
        self.wave_until = self.now + 1.8;
    }

    /// A click on the head: counts as a big pat.
    pub fn boop(&mut self) {
        self.boops = (self.boops + 60.0).min(120.0);
    }

    pub fn handle(&mut self, ev: InputEvent) {
        match ev {
            InputEvent::KeyDown(k) => {
                let i = k.0 as usize;
                if i >= KEY_COUNT || self.key_down[i] {
                    return; // out of range or auto-repeat
                }
                self.key_down[i] = true;
                self.held.retain(|&h| h != k);
                if self.held.len() >= MAX_HELD {
                    let old = self.held.remove(0);
                    self.key_down[old.0 as usize] = false;
                }
                self.held.push(k);
                self.key_presses.push(self.now);
                self.key_pressed_frame = true;
                self.key_down_at[i] = self.now;
                self.last_key_down = self.now;
                self.keys_total += 1;
                self.key_glow[i] = 1.0;
            }
            InputEvent::KeyUp(k) => {
                let i = k.0 as usize;
                if i < KEY_COUNT {
                    self.key_down[i] = false;
                }
                self.held.retain(|&h| h != k);
            }
            InputEvent::MouseMove { dx, dy } => {
                if dx.is_finite() && dy.is_finite() {
                    self.raw_mouse += pt(dx, dy);
                }
            }
            InputEvent::MouseButton { button, down } => {
                let i = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                };
                if down && !self.buttons[i] {
                    self.clicks.push(self.now);
                    self.clicks_total += 1;
                }
                self.buttons[i] = down;
                self.mouse_events = true;
            }
            InputEvent::Wheel(_) => self.mouse_events = true,
            InputEvent::ReleaseAll => {
                self.held.clear();
                self.key_down = [false; KEY_COUNT];
                self.buttons = [false; 3];
            }
        }
    }

    /// Front paws [viewer-left, viewer-right] and back paws for the keys being held.
    fn assign_keys(&self, mousing: bool, waving: bool, now: f64) -> ([Option<KeyId>; 2], [Option<KeyId>; 2]) {
        let mut front = [None; 2];
        let mut hind = [None; 2];
        let mut free = [true; 2];
        free[MOUSE_SIDE] &= !mousing;
        free[WAVE_SIDE] &= !waving;
        for &k in self.held.iter().rev() {
            let p = preferred_side(k);
            let o = 1 - p;
            if free[p] && front[p].is_none() {
                front[p] = Some(k);
            } else if free[o] && front[o].is_none() {
                front[o] = Some(k);
            } else if now - self.key_down_at[k.0 as usize] < HIND_PAW_DELAY {
                // A quick overlap while typing: no extra paw.
            } else if hind[p].is_none() {
                hind[p] = Some(k);
            } else if hind[o].is_none() {
                hind[o] = Some(k);
            }
        }
        // Never cross the arms: with a key under each paw, the viewer-left paw takes the
        // key further left.
        if let (Some(a), Some(b)) = (front[0], front[1]) {
            if free[0] && free[1] && key_center(a).x > key_center(b).x {
                front = [Some(b), Some(a)];
            }
        }
        (front, hind)
    }

    /// Shows a pressed key as a keycap popping up from key `id` with `label` printed on it.
    /// The platform layer calls this on a fresh key press (not on auto-repeat), and only when
    /// the user has key pop-ups turned on.
    pub fn show_key(&mut self, id: KeyId, label: &'static str) {
        if self.popups.len() >= MAX_KEY_POPUPS {
            self.popups.remove(0);
        }
        let drift = self.rng.range(-7.0, 7.0);
        let tilt = self.rng.range(-0.14, 0.14);
        self.popups.push(KeyPopup { label, from: key_center(id), age: 0.0, life: KEY_POPUP_LIFE, drift, tilt });
    }

    fn spawn(&mut self, kind: ParticleKind, pos: Pt, vel: Pt, life: f32, size: f32) {
        if self.particles.len() >= MAX_PARTICLES {
            self.particles.remove(0);
        }
        let spin = self.rng.range(-1.5, 1.5);
        self.particles.push(Particle { kind, pos, vel, age: 0.0, life, size, spin });
    }

    fn on_enter(&mut self, mood: Mood, prev: Mood, rig: &Rig) {
        match mood {
            Mood::Zoomies => {
                self.hops_left = 3;
                self.next_hop = self.now;
                let c = rig.body.map(pt(0.0, -80.0));
                for i in 0..8 {
                    let a = i as f32 / 8.0 * 2.0 * PI + self.rng.range(-0.2, 0.2);
                    let v = pt(a.cos(), a.sin()) * self.rng.range(70.0, 110.0);
                    let size = self.rng.range(4.0, 6.5);
                    self.spawn(ParticleKind::Star, c + v * 0.3, v, 0.8, size);
                }
            }
            Mood::Pat => self.next_heart = self.now,
            Mood::Sleep => self.next_z = self.now + 0.8,
            _ => {}
        }
        if prev == Mood::Sleep {
            self.wave_until = self.now + 1.8;
            // Waking up: a quick double blink.
            self.blink_start = self.now + 0.12;
            self.next_blink = self.now + 0.5;
        }
    }

    pub fn update(&mut self, dt: f32, input: &FrameInput, skin: &Skin, cfg: &MoodConfig) {
        let dt = if dt.is_finite() { dt.clamp(0.0, MAX_DT) } else { 0.0 };
        self.now += dt as f64;
        let now = self.now;
        let t = now as f32;
        let rig = self.rig(skin);

        // ---- signals -------------------------------------------------------------------
        let mut cursor_speed = 0.0;
        let mut stroke = 0.0;
        let mut cursor_moved = false;
        if let (Some(c), Some(p)) = (input.cursor, self.prev_cursor) {
            let d = (c - p).len();
            if d.is_finite() {
                cursor_moved = d > 0.3;
                if dt > 0.0 {
                    cursor_speed = d / dt;
                }
                if !input.dragging && rig.head_contains(c) && rig.head_contains(p) {
                    stroke = d.min(40.0);
                }
            }
        }
        self.prev_cursor = input.cursor.filter(|c| c.is_finite());
        stroke += std::mem::take(&mut self.boops);
        let raw = std::mem::take(&mut self.raw_mouse);
        let mouse_active = cursor_moved || raw.len() > 0.5 || self.mouse_events;
        if mouse_active {
            self.last_mouse = now;
        }
        let wpm_now = wpm(&self.key_presses, now);
        self.wpm += (wpm_now - self.wpm) * (1.0 - (-dt * 3.0).exp());
        let signals = Signals {
            key_pressed: self.key_pressed_frame,
            mouse_active,
            wpm: self.wpm,
            cursor_speed,
            click_burst: self.clicks.count_since(now - 1.2) >= 8,
            stroke,
            dragging: input.dragging,
        };
        self.key_pressed_frame = false;
        self.mouse_events = false;

        let prev = self.mood.mood;
        let mood = self.mood.update(now, dt, &signals, cfg);
        if mood != prev {
            self.on_enter(mood, prev, &rig);
        }

        // ---- head look -----------------------------------------------------------------
        let head_c = rig.head.center;
        let (mut yaw_t, mut pitch_t, mut gaze_t) = (0.0, 0.0, Pt::ZERO);
        if let Some(c) = input.cursor.filter(|c| c.is_finite()) {
            let d = c - head_c;
            yaw_t = 0.42 * soft_sat(d.x / 240.0);
            pitch_t = 0.24 * soft_sat(d.y / 280.0);
            gaze_t = (d * (1.0 / 150.0)).clamp_len(1.0);
            if self.seconds_since_input() > 8.0 {
                yaw_t *= 0.5;
                pitch_t *= 0.5;
                gaze_t = gaze_t * 0.5;
            }
        }
        let drag_v = if input.drag_velocity.is_finite() { input.drag_velocity } else { Pt::ZERO };
        match mood {
            Mood::Sleep => {
                yaw_t = 0.0;
                pitch_t = 0.3;
                gaze_t = Pt::ZERO;
            }
            Mood::Pat => {
                yaw_t *= 0.25;
                pitch_t = -0.06;
            }
            Mood::Scruff => {
                yaw_t = (-drag_v.x * 0.0006).clamp(-0.3, 0.3);
                pitch_t = 0.05;
                gaze_t = pt((-drag_v.x * 0.003).clamp(-1.0, 1.0), 0.3);
            }
            _ => {}
        }
        self.yaw.target = yaw_t;
        self.pitch.target = pitch_t;
        self.gaze.set_target(gaze_t);

        if mood == Mood::Idle && now >= self.next_curious {
            self.curious_until = now + self.rng.range(1.2, 2.2) as f64;
            self.curious_dir = if self.rng.f32() < 0.5 { -1.0 } else { 1.0 };
            self.next_curious = now + self.rng.range(7.0, 14.0) as f64;
        }
        self.head_tilt.target = match mood {
            Mood::Idle if now < self.curious_until => 0.14 * self.curious_dir,
            Mood::Pat => 0.1 * (t * 3.5).sin(),
            Mood::Sleep => 0.1,
            Mood::Scruff => (drag_v.x * 0.0004).clamp(-0.2, 0.2),
            Mood::Zoomies => 0.12 * (t * 9.0).sin(),
            _ => 0.0,
        };
        self.drop.target = match mood {
            Mood::Sleep => 18.0,
            Mood::Chase => 6.0,
            Mood::Pat => 2.0,
            _ => 0.0,
        };

        // ---- paws ----------------------------------------------------------------------
        // The mouse paw stays on the mouse only while the mouse was used more recently than
        // the keyboard; as soon as you type, both front paws go back to the keys.
        let mousing = now - self.last_mouse < MOUSE_PAW_HOLD
            && self.last_mouse > self.last_key_down
            && matches!(mood, Mood::Idle | Mood::Typing | Mood::Excited | Mood::Chase);
        let waving = now < self.wave_until && matches!(mood, Mood::Idle | Mood::Typing | Mood::Excited);
        let (front, hind) = self.assign_keys(mousing, waving, now);
        let mouse_pos = MOUSE_REST + self.mouse_off.value();
        let swing = pt((-drag_v.x * 0.03).clamp(-16.0, 16.0), (-drag_v.y * 0.02).clamp(-10.0, 10.0));
        let mut targets = [(Pt::ZERO, 0.0f32); 2];
        // Whether each paw is pinned to something (a key or the mouse) rather than hovering.
        let mut pinned = [false; 2];
        for side in 0..2 {
            let s = side_sign(side);
            // Remember where a paw let go of a key so it can hover there for a moment.
            if let (Some(prev_key), None) = (self.paw_key[side], front[side]) {
                self.paw_release[side] = now;
                self.paw_last[side] = key_center(prev_key);
            }
            self.paw_key[side] = front[side];
            let rest = paw_rest(side);
            let (target, press) = match mood {
                Mood::Scruff => (rig.shoulder(side) + pt(s * 6.0, 50.0) + swing, 0.0),
                Mood::Zoomies => {
                    let wave = (t * 16.0 + side as f32 * PI).sin() * 10.0;
                    (rig.shoulder(side) + pt(s * 22.0, 30.0 + wave), 0.0)
                }
                Mood::Sleep => (rest + pt(-s * 7.0, 3.0), 0.0),
                _ if waving && side == WAVE_SIDE => {
                    // Paw raised beside the cheek, swaying side to side.
                    (rig.shoulder(side) + pt(s * (46.0 + (t * 12.0).sin() * 8.0), -44.0 + (t * 24.0).sin() * 2.0), 0.0)
                }
                _ => {
                    if side == MOUSE_SIDE && mousing {
                        let b = self.mouse_btn[0].value.max(self.mouse_btn[1].value);
                        pinned[side] = true;
                        (mouse_pos + pt(-side_sign(MOUSE_SIDE) * 2.0, -10.0), b)
                    } else if let Some(k) = front[side] {
                        pinned[side] = true;
                        (key_center(k) + pt(0.0, -2.0), 1.0)
                    } else if now - self.paw_release[side] < 0.28 {
                        (self.paw_last[side] + pt(0.0, -6.0), 0.0)
                    } else {
                        (rest, 0.0)
                    }
                }
            };
            targets[side] = (target, press);

            let hind_key = if matches!(mood, Mood::Scruff | Mood::Zoomies | Mood::Sleep) { None } else { hind[side] };
            match hind_key {
                Some(k) => {
                    self.hind[side].set_target(key_center(k) + pt(0.0, -1.0));
                    self.hind_up[side].target = 1.0;
                }
                None => {
                    self.hind[side].set_target(rig.hip(side));
                    self.hind_up[side].target = 0.0;
                }
            }
        }

        if matches!(mood, Mood::Idle | Mood::Typing | Mood::Excited | Mood::Chase | Mood::Pat) && !waving {
            keep_paws_apart(&mut targets, pinned);
        }
        for side in 0..2 {
            self.paw[side].set_target(targets[side].0);
            self.press[side].target = targets[side].1;
        }

        // ---- mouse on its pad (mirrored: it faces the dog) --------------------------------
        let mt = self.mouse_target - raw * 0.045;
        self.mouse_target = pt(mt.x.clamp(-12.0, 12.0), mt.y.clamp(-7.0, 7.0)) * (-dt * 2.2).exp();
        self.mouse_off.set_target(self.mouse_target);
        for i in 0..2 {
            self.mouse_btn[i].target = if self.buttons[i] { 1.0 } else { 0.0 };
        }

        // ---- body ----------------------------------------------------------------------
        let cursor_side = input.cursor.map(|c| soft_sat((c.x - head_c.x) / 200.0)).unwrap_or(0.0);
        self.lean.target = match mood {
            Mood::Chase => cursor_side * 6.0,
            Mood::Scruff => (-drag_v.x * 0.01).clamp(-6.0, 6.0),
            _ if mousing => side_sign(MOUSE_SIDE) * 10.0,
            _ => 0.0,
        };
        self.tilt.target = match mood {
            Mood::Scruff => (drag_v.x * 0.00025).clamp(-0.15, 0.15),
            Mood::Chase => cursor_side * 0.04,
            _ if mousing => side_sign(MOUSE_SIDE) * 0.05,
            _ => 0.0,
        };
        self.lift.target = if mood == Mood::Scruff { -18.0 } else { 0.0 };
        self.squash.target = if mood == Mood::Scruff { 1.07 } else { 1.0 };
        if mood == Mood::Zoomies && self.hops_left > 0 && now >= self.next_hop {
            self.lift.kick(-300.0);
            self.squash.kick(1.6);
            self.hops_left -= 1;
            self.next_hop = now + 0.38;
        }

        // ---- ears ----------------------------------------------------------------------
        self.perk.target = match mood {
            Mood::Excited | Mood::Chase | Mood::Scruff | Mood::Zoomies => 1.0,
            Mood::Typing => 0.35,
            _ => 0.0,
        };
        self.fold.target = match mood {
            Mood::Pat => 0.85,
            Mood::Sleep => 0.5,
            _ => 0.0,
        };
        if matches!(mood, Mood::Idle | Mood::Typing) && now >= self.next_twitch {
            let side = (self.rng.next_u32() % 2) as usize;
            let kick = self.rng.range(4.0, 7.0);
            self.twitch[side].kick(kick);
            self.next_twitch = now + self.rng.range(4.0, 10.0) as f64;
        }

        // ---- eyes & face ---------------------------------------------------------------
        if mood != Mood::Sleep && now >= self.next_blink {
            self.blink_start = now;
            let double = self.rng.f32() < 0.2;
            self.next_blink = now + if double { 0.3 } else { self.rng.range(2.4, 6.0) as f64 };
        }
        self.happy.target = if matches!(mood, Mood::Pat | Mood::Zoomies) { 1.0 } else { 0.0 };
        self.sleepy.target = if mood == Mood::Sleep { 1.0 } else { 0.0 };
        self.pupil.target = match mood {
            Mood::Chase => 1.38,
            Mood::Zoomies => 1.2,
            // Typing fast: concentrating, pupils a little tighter.
            Mood::Excited => 0.9,
            Mood::Scruff => 0.92,
            _ => 1.0,
        };
        self.sparkle.target = if matches!(mood, Mood::Chase | Mood::Zoomies) { 1.0 } else { 0.0 };
        self.focus.target = if mood == Mood::Excited { 1.0 } else { 0.0 };
        if mood == Mood::Idle && now >= self.next_blep {
            self.blep_until = now + 1.6;
            self.next_blep = now + self.rng.range(14.0, 30.0) as f64;
        }
        self.mouth.target = match mood {
            Mood::Zoomies => 1.0,
            Mood::Chase => 0.85,
            Mood::Pat => 0.75,
            Mood::Scruff => 0.4,
            _ => 0.0,
        };
        self.tongue.target = match mood {
            Mood::Zoomies | Mood::Chase | Mood::Pat => 1.0,
            // Concentration tongue: a little blep while typing hard.
            Mood::Excited => 0.55,
            Mood::Idle if now < self.blep_until => 0.6,
            _ => 0.0,
        };
        self.blush.target = match mood {
            Mood::Pat => 1.0,
            Mood::Zoomies => 0.8,
            Mood::Scruff => 0.6,
            Mood::Excited => 0.55,
            Mood::Sleep => 0.25,
            _ => 0.35,
        };

        // ---- tail: wag speed follows typing speed -------------------------------------
        let (speed, amp) = match mood {
            Mood::Sleep => (0.9, 0.04),
            Mood::Idle => (2.6, 0.16),
            Mood::Typing => (3.5 + self.wpm * 0.09, 0.27),
            Mood::Excited => (10.0 + self.wpm * 0.03, 0.36),
            Mood::Chase | Mood::Pat | Mood::Zoomies => (15.0, 0.46),
            Mood::Scruff => (7.0, 0.3),
        };
        self.wag_speed.target = speed.min(17.0);
        self.wag_amp.target = amp;

        // ---- step ----------------------------------------------------------------------
        for s in self.paw.iter_mut().chain(self.hind.iter_mut()) {
            s.step(dt);
        }
        // Inverse kinematics: elbows and knees follow the paws. A stiff spring on top smooths
        // the rare moment where the bend flips sides, so limbs never snap.
        let (upper, lower) = rig.arm_bones();
        let (thigh, shin) = rig.leg_bones();
        for side in 0..2 {
            let s = side_sign(side);
            let prefer = if waving && side == WAVE_SIDE { pt(s * 0.6, 1.0) } else { crate::limb::front_bend(s) };
            let elbow = crate::limb::solve_elbow(rig.shoulder(side), self.paw[side].value(), upper, lower, prefer);
            let hip = rig.hip(side);
            let foot = hip.lerp(self.hind[side].value(), self.hind_up[side].value.clamp(0.0, 1.0));
            let knee = crate::limb::solve_elbow(hip, foot, thigh, shin, pt(s, 0.2));
            if self.joints_ready {
                self.elbow[side].set_target(elbow);
                self.knee[side].set_target(knee);
                self.elbow[side].step(dt);
                self.knee[side].step(dt);
            } else {
                self.elbow[side].snap(elbow);
                self.knee[side].snap(knee);
            }
        }
        self.joints_ready = true;
        self.gaze.step(dt);
        self.mouse_off.step(dt);
        let paired = self
            .press
            .iter_mut()
            .chain(self.hind_up.iter_mut())
            .chain(self.twitch.iter_mut())
            .chain(self.mouse_btn.iter_mut());
        for s in paired.chain([
            &mut self.lift,
            &mut self.lean,
            &mut self.tilt,
            &mut self.squash,
            &mut self.yaw,
            &mut self.pitch,
            &mut self.head_tilt,
            &mut self.drop,
            &mut self.perk,
            &mut self.fold,
            &mut self.pupil,
            &mut self.sparkle,
            &mut self.focus,
            &mut self.happy,
            &mut self.sleepy,
            &mut self.mouth,
            &mut self.tongue,
            &mut self.blush,
            &mut self.wag_speed,
            &mut self.wag_amp,
        ]) {
            s.step(dt);
        }
        self.wag_phase = (self.wag_phase + self.wag_speed.value * dt) % (2.0 * PI);
        let sleeping = mood == Mood::Sleep;
        self.breath_phase = (self.breath_phase + dt * if sleeping { 1.3 } else { 2.2 }) % (2.0 * PI);

        for i in 0..KEY_COUNT {
            self.key_glow[i] = if self.key_down[i] { 1.0 } else { (self.key_glow[i] - dt * 3.5).max(0.0) };
        }
        let boost = (self.held.len() as f32 / 4.0 + self.wpm / 140.0).clamp(0.0, 1.0);
        self.marquee = (self.marquee + dt * (0.22 + 0.5 * boost)).fract();

        // ---- particles -----------------------------------------------------------------
        if mood == Mood::Pat && now >= self.next_heart {
            let p = head_c + pt(self.rng.range(-34.0, 34.0), -46.0);
            let v = pt(self.rng.range(-12.0, 12.0), -46.0);
            let size = self.rng.range(5.0, 7.0);
            self.spawn(ParticleKind::Heart, p, v, 1.3, size);
            self.next_heart = now + 0.3;
        }
        if sleeping && now >= self.next_z {
            self.spawn(ParticleKind::Zzz, head_c + pt(44.0, -40.0), pt(10.0, -16.0), 2.8, 5.0);
            self.next_z = now + 1.7;
        }
        if mood == Mood::Zoomies && now >= self.next_heart {
            let p = rig.body.map(pt(self.rng.range(-50.0, 50.0), -90.0));
            let v = pt(self.rng.range(-40.0, 40.0), -50.0);
            let size = self.rng.range(3.5, 5.5);
            self.spawn(ParticleKind::Star, p, v, 0.7, size);
            self.next_heart = now + 0.12;
        }
        for p in &mut self.particles {
            p.age += dt;
            p.pos += p.vel * dt;
            p.vel.x *= 1.0 - dt * 0.8;
            if p.kind == ParticleKind::Zzz {
                p.size += dt * 1.6;
            }
        }
        self.particles.retain(|p| p.age < p.life);
        for k in &mut self.popups {
            k.age += dt;
        }
        self.popups.retain(|k| k.age < k.life);

        // ---- pose ----------------------------------------------------------------------
        let blink_t = ((now - self.blink_start) / 0.16) as f32;
        let blink_open = if (0.0..1.0).contains(&blink_t) { 1.0 - (PI * blink_t).sin() } else { 1.0 };
        let breath = self.breath_phase.sin() * if sleeping { 0.022 } else { 0.012 };
        let wag = self.wag_phase.sin() * self.wag_amp.value;
        let sway = match skin.shape.tail_style {
            TailStyle::Stub => wag * 9.0,
            _ => wag * 2.0,
        };
        let p = &mut self.pose;
        p.time = t;
        p.mood = mood;
        p.body_lift = self.lift.value;
        p.body_lean = self.lean.value;
        p.body_tilt = self.tilt.value;
        p.body_squash = self.squash.value + breath;
        p.hip_sway = sway;
        p.head_yaw = self.yaw.value;
        p.head_pitch = self.pitch.value;
        p.head_tilt = self.head_tilt.value;
        p.head_drop = self.drop.value - breath * 40.0;
        p.ear_perk = self.perk.value;
        p.ear_fold = self.fold.value;
        p.ear_twitch = [self.twitch[0].value * 0.04, self.twitch[1].value * 0.04];
        p.eye_open = (blink_open * (1.0 - self.sleepy.value)).clamp(0.0, 1.0);
        p.eye_happy = self.happy.value.clamp(0.0, 1.0);
        p.gaze = self.gaze.value();
        p.pupil = self.pupil.value;
        p.sparkle = self.sparkle.value.clamp(0.0, 1.0);
        p.focus = self.focus.value.clamp(0.0, 1.0);
        p.mouth_open = self.mouth.value.clamp(0.0, 1.0);
        p.tongue = self.tongue.value.clamp(0.0, 1.0);
        p.blush = self.blush.value.clamp(0.0, 1.0);
        p.tail_wag = wag;
        for side in 0..2 {
            p.paws[side] = PawPose { pos: self.paw[side].value(), press: self.press[side].value.clamp(0.0, 1.2) };
            p.hind[side] = HindPose { pos: self.hind[side].value(), up: self.hind_up[side].value.clamp(0.0, 1.1) };
            p.elbows[side] = self.elbow[side].value();
            p.knees[side] = self.knee[side].value();
        }
        p.mouse_offset = self.mouse_off.value();
        p.mouse_buttons = [self.mouse_btn[0].value, self.mouse_btn[1].value];
        p.key_glow = self.key_glow;
        p.key_down = self.key_down;
        p.marquee = self.marquee;
        p.marquee_boost = boost;
        p.particles.clear();
        p.particles.extend_from_slice(&self.particles);
        p.key_popups.clear();
        p.key_popups.extend_from_slice(&self.popups);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::key;
    use crate::skin::{corgi, corgi_merle, corgi_tricolor};

    fn step(pet: &mut Pet, skin: &Skin, secs: f32, input: FrameInput) {
        let n = (secs * 30.0).round().max(1.0) as usize;
        for _ in 0..n {
            pet.update(1.0 / 30.0, &input, skin, &MoodConfig::default());
        }
    }

    #[test]
    fn paw_reaches_pressed_key() {
        let skin = corgi();
        let mut pet = Pet::new(1);
        let q = key(1, 1); // user's Q: viewer's right
        pet.handle(InputEvent::KeyDown(q));
        step(&mut pet, &skin, 0.3, FrameInput::default());
        let paw = pet.pose().paws[1].pos;
        assert!((paw - (key_center(q) + pt(0.0, -2.0))).len() < 1.0, "{paw:?}");
        assert!(pet.pose().paws[1].press > 0.9);
        assert!(pet.pose().key_down[q.0 as usize]);
        pet.handle(InputEvent::KeyUp(q));
        step(&mut pet, &skin, 1.0, FrameInput::default());
        assert!((pet.pose().paws[1].pos - paw_rest(1)).len() < 1.0);
        assert_eq!(pet.keys_total, 1);
    }

    #[test]
    fn third_held_key_raises_a_back_paw() {
        let skin = corgi_tricolor();
        let mut pet = Pet::new(2);
        for k in [key(2, 1), key(2, 2), key(2, 3)] {
            pet.handle(InputEvent::KeyDown(k));
        }
        step(&mut pet, &skin, 0.5, FrameInput::default());
        let up = pet.pose().hind[0].up.max(pet.pose().hind[1].up);
        assert!(up > 0.9, "{:?}", pet.pose().hind);
        pet.handle(InputEvent::ReleaseAll);
        step(&mut pet, &skin, 1.0, FrameInput::default());
        assert!(pet.pose().hind.iter().all(|h| h.up < 0.05));
    }

    #[test]
    fn fast_typing_never_shows_back_paws() {
        // Rolling over keys: each key goes down before the previous two are released.
        let skin = corgi();
        let mut pet = Pet::new(9);
        let keys = [key(2, 1), key(2, 2), key(2, 3), key(2, 4), key(1, 5), key(1, 6), key(2, 7)];
        let mut max_up: f32 = 0.0;
        for i in 0..90 {
            let k = keys[i % keys.len()];
            pet.handle(InputEvent::KeyDown(k));
            if i >= 2 {
                pet.handle(InputEvent::KeyUp(keys[(i - 2) % keys.len()]));
            }
            // A little mouse jiggle while typing must not steal a front paw either.
            if i % 10 == 0 {
                pet.handle(InputEvent::MouseMove { dx: 3.0, dy: 0.0 });
            }
            step(&mut pet, &skin, 1.0 / 30.0, FrameInput::default());
            step(&mut pet, &skin, 1.0 / 30.0, FrameInput::default());
            max_up = max_up.max(pet.pose().hind[0].up).max(pet.pose().hind[1].up);
        }
        assert!(max_up < 0.05, "back paw came up while typing: {max_up}");
    }

    #[test]
    fn typing_takes_the_paw_off_the_mouse() {
        let skin = corgi();
        let mut pet = Pet::new(10);
        for _ in 0..10 {
            pet.handle(InputEvent::MouseMove { dx: 20.0, dy: 0.0 });
            step(&mut pet, &skin, 1.0 / 30.0, FrameInput::default());
        }
        let p = key(2, 10); // a key on the viewer's left, the mouse paw's side
        assert_eq!(preferred_side(p), MOUSE_SIDE);
        pet.handle(InputEvent::KeyDown(p));
        step(&mut pet, &skin, 0.3, FrameInput::default());
        let paw = pet.pose().paws[MOUSE_SIDE].pos;
        assert!((paw - (key_center(p) + pt(0.0, -2.0))).len() < 2.0, "{paw:?}");
        assert!(pet.pose().hind.iter().all(|h| h.up < 0.05));
    }

    #[test]
    fn auto_repeat_does_not_count() {
        let mut pet = Pet::new(3);
        for _ in 0..10 {
            pet.handle(InputEvent::KeyDown(key(4, 3)));
        }
        assert_eq!(pet.keys_total, 1);
    }

    #[test]
    fn petting_makes_happy_eyes_and_hearts() {
        let skin = corgi();
        let mut pet = Pet::new(4);
        step(&mut pet, &skin, 0.1, FrameInput::default());
        let c = pet.rig(&skin).head.center;
        for i in 0..60 {
            let x = c.x + ((i as f32) * 0.8).sin() * 30.0;
            step(&mut pet, &skin, 1.0 / 30.0, FrameInput { cursor: Some(pt(x, c.y - 10.0)), ..Default::default() });
        }
        assert_eq!(pet.mood(), Mood::Pat);
        assert!(pet.pose().eye_happy > 0.8);
        assert!(pet.pose().particles.iter().any(|p| p.kind == ParticleKind::Heart));
    }

    #[test]
    fn sleeps_and_stays_finite_for_a_long_time() {
        let skin = corgi_merle();
        let mut pet = Pet::new(5);
        let cfg = MoodConfig { sleep_after: 5.0, ..Default::default() };
        for _ in 0..(30 * 20) {
            pet.update(1.0 / 30.0, &FrameInput::default(), &skin, &cfg);
        }
        assert_eq!(pet.mood(), Mood::Sleep);
        let p = pet.pose();
        assert!(p.eye_open < 0.1);
        assert!(p.head_drop.is_finite() && p.body_squash.is_finite());
        assert!(p.particles.iter().any(|p| p.kind == ParticleKind::Zzz));
    }

    /// Fast two-handed typing: overlapping presses on both sides, in every order, must never
    /// put the viewer-left paw to the right of the viewer-right paw.
    #[test]
    fn typing_never_crosses_the_arms() {
        let skin = corgi();
        let keys = [key(2, 8), key(2, 9), key(1, 8), key(2, 2), key(2, 3), key(1, 3), key(3, 7), key(3, 2), key(2, 5), key(2, 6)];
        for seed in 0..4u32 {
            let mut pet = Pet::new(seed);
            let mut held: Vec<KeyId> = Vec::new();
            for i in 0..240usize {
                let k = keys[(i * 7 + seed as usize * 3) % keys.len()];
                if i % 3 == 0 && !held.contains(&k) {
                    pet.handle(InputEvent::KeyDown(k));
                    held.push(k);
                }
                if held.len() > 2 || (i % 5 == 0 && !held.is_empty()) {
                    let up = held.remove(0);
                    pet.handle(InputEvent::KeyUp(up));
                }
                step(&mut pet, &skin, 1.0 / 60.0, FrameInput::default());
                let p = pet.pose();
                assert!(p.paws[0].pos.x < p.paws[1].pos.x, "seed {seed}, frame {i}: arms crossed {:?} / {:?}", p.paws[0].pos, p.paws[1].pos);
            }
        }
    }

    /// Typing fast puts on the focused face (and takes off the sparkly one); it fades again
    /// when typing stops.
    #[test]
    fn fast_typing_makes_the_dog_focus() {
        let skin = corgi();
        let mut pet = Pet::new(8);
        for i in 0..150usize {
            let k = key(1 + i % 3, 1 + (i * 5) % 9);
            pet.handle(InputEvent::KeyDown(k));
            step(&mut pet, &skin, 1.0 / 60.0, FrameInput::default());
            pet.handle(InputEvent::KeyUp(k));
            step(&mut pet, &skin, 1.0 / 60.0, FrameInput::default());
        }
        assert_eq!(pet.mood(), Mood::Excited);
        assert!(pet.pose().focus > 0.8, "focus {}", pet.pose().focus);
        assert!(pet.pose().sparkle < 0.2);
        step(&mut pet, &skin, 6.0, FrameInput::default());
        assert!(pet.pose().focus < 0.1, "focus stays after typing stopped: {}", pet.pose().focus);
    }

    #[test]
    fn key_popups_rise_and_go_away() {
        let skin = corgi();
        let mut pet = Pet::new(3);
        for i in 0..9 {
            pet.show_key(key(2, 1 + i), "K");
        }
        // Capped, and gone after their life.
        assert_eq!(pet.pose().key_popups.len(), 0);
        step(&mut pet, &skin, 0.1, FrameInput::default());
        assert_eq!(pet.pose().key_popups.len(), MAX_KEY_POPUPS);
        assert!(pet.is_lively());
        step(&mut pet, &skin, KEY_POPUP_LIFE + 0.2, FrameInput::default());
        assert!(pet.pose().key_popups.is_empty());
    }

    #[test]
    fn mouse_paw_goes_to_the_mouse() {
        let skin = corgi();
        let mut pet = Pet::new(6);
        for _ in 0..15 {
            pet.handle(InputEvent::MouseMove { dx: 30.0, dy: 0.0 });
            step(&mut pet, &skin, 1.0 / 30.0, FrameInput::default());
        }
        let paw = pet.pose().paws[MOUSE_SIDE].pos;
        let mouse = MOUSE_REST + pet.pose().mouse_offset;
        assert!(mouse.x < crate::scene::DOG_X, "the mouse is on the viewer's left");
        assert!((paw - mouse).len() < 16.0, "{paw:?} vs {mouse:?}");
        // The mouse faces the dog, so moving right moves it to the viewer's left.
        assert!(pet.pose().mouse_offset.x < -3.0);
    }
}
