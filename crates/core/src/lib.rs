//! DeskCorgi core: everything that is the same on every platform.
//!
//! The platform layer feeds [`input::InputEvent`]s into a [`pet::Pet`], calls
//! [`pet::Pet::update`] once per frame and draws the result with [`render::Renderer`] into
//! an RGBA buffer, which it then copies to a transparent window.

pub mod art;
pub mod color;
pub mod config;
pub mod geom;
pub mod glyphs;
pub mod input;
pub mod keymap;
pub mod limb;
pub mod pet;
pub mod render;
pub mod rig;
pub mod scene;
pub mod skin;
pub mod spring;
pub mod state;

/// Re-exported so platform crates use the exact same version.
pub use tiny_skia;

pub use art::Art;
pub use config::Settings;
pub use input::{InputEvent, MouseButton};
pub use pet::{FrameInput, Pet, Pose};
pub use render::{RenderOptions, Renderer};
pub use skin::Skin;
pub use state::{Mood, MoodConfig};

/// Mood settings derived from the user's settings.
pub fn mood_config(s: &Settings) -> MoodConfig {
    MoodConfig { chase: s.chase, zoomies: s.zoomies, sleep: s.sleep, sleep_after: config::SLEEP_AFTER_SECS, yawn: s.yawn, huh: s.huh }
}
