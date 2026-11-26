//! User settings, stored as TOML in the OS config folder.

use std::path::PathBuf;

use serde::Deserialize;

use crate::color::{rgb, Rgba};
use crate::skin::BUILTIN_CORGI;

/// Size presets (multiplied by the monitor's DPI scale).
pub const SIZE_SCALES: [f32; 5] = [0.55, 0.7, 0.85, 1.0, 1.2];
pub const SIZE_NAMES: [&str; 5] = ["Tiny", "Small", "Medium", "Large", "Huge"];
pub const DEFAULT_SIZE: usize = 2;
/// Idle minutes before the dog naps, picked from the menu.
pub const NAP_MINUTES: [u32; 4] = [1, 2, 5, 10];
pub const DEFAULT_NAP_MINUTES: u32 = 2;
pub const SLEEP_AFTER_SECS: f64 = DEFAULT_NAP_MINUTES as f64 * 60.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EyeColor {
    Brown,
    Amber,
    Hazel,
    Dark,
    Blue,
    Green,
}

impl EyeColor {
    pub const ALL: [EyeColor; 6] = [EyeColor::Brown, EyeColor::Amber, EyeColor::Hazel, EyeColor::Dark, EyeColor::Blue, EyeColor::Green];

    pub fn name(self) -> &'static str {
        match self {
            EyeColor::Brown => "Brown",
            EyeColor::Amber => "Amber",
            EyeColor::Hazel => "Hazel",
            EyeColor::Dark => "Dark",
            EyeColor::Blue => "Blue",
            EyeColor::Green => "Green",
        }
    }

    pub fn iris(self) -> Rgba {
        match self {
            EyeColor::Brown => rgb(0x7A, 0x48, 0x24),
            EyeColor::Amber => rgb(0xC0, 0x7A, 0x22),
            EyeColor::Hazel => rgb(0x8C, 0x74, 0x34),
            EyeColor::Dark => rgb(0x3C, 0x28, 0x1C),
            EyeColor::Blue => rgb(0x4C, 0x8E, 0xD6),
            EyeColor::Green => rgb(0x58, 0x9C, 0x52),
        }
    }

    fn parse(s: &str) -> Option<EyeColor> {
        EyeColor::ALL.into_iter().find(|e| e.name().eq_ignore_ascii_case(s))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyboardTheme {
    Cream,
    Charcoal,
    Sakura,
}

pub struct KeyboardColors {
    pub base: Rgba,
    pub front: Rgba,
    /// Letter and number keycaps.
    pub key: Rgba,
    pub key_edge: Rgba,
    /// Second keycap tone for modifiers (Shift, Ctrl, Tab…).
    pub modifier: Rgba,
    /// Bright accent keycaps (Esc, Enter).
    pub accent: Rgba,
    /// Space bar.
    pub space: Rgba,
    pub lit: Rgba,
    pub outline: Rgba,
    pub pad: Rgba,
    pub mouse: Rgba,
}

impl KeyboardTheme {
    pub const ALL: [KeyboardTheme; 3] = [KeyboardTheme::Cream, KeyboardTheme::Charcoal, KeyboardTheme::Sakura];

    pub fn name(self) -> &'static str {
        match self {
            KeyboardTheme::Cream => "Cream",
            KeyboardTheme::Charcoal => "Charcoal",
            KeyboardTheme::Sakura => "Sakura",
        }
    }

    pub fn colors(self) -> KeyboardColors {
        match self {
            KeyboardTheme::Cream => KeyboardColors {
                base: rgb(0xEC, 0xE1, 0xD0),
                front: rgb(0xC9, 0xB8, 0x9F),
                key: rgb(0xFF, 0xFD, 0xF8),
                key_edge: rgb(0xD8, 0xCB, 0xB6),
                modifier: rgb(0xF4, 0xE2, 0xC8),
                accent: rgb(0xF6, 0x9F, 0x4E),
                space: rgb(0xFF, 0xDF, 0x96),
                lit: rgb(0xFF, 0xB2, 0x57),
                outline: rgb(0x6E, 0x5A, 0x45),
                pad: rgb(0x7D, 0x8C, 0xA8),
                mouse: rgb(0xFF, 0xFD, 0xF8),
            },
            KeyboardTheme::Charcoal => KeyboardColors {
                base: rgb(0x3D, 0x42, 0x4E),
                front: rgb(0x2A, 0x2E, 0x37),
                key: rgb(0x5A, 0x61, 0x70),
                key_edge: rgb(0x33, 0x38, 0x42),
                modifier: rgb(0x45, 0x4B, 0x58),
                accent: rgb(0xFF, 0x9C, 0x5B),
                space: rgb(0x86, 0xB0, 0xE4),
                lit: rgb(0x7F, 0xD6, 0xFF),
                outline: rgb(0x1E, 0x21, 0x28),
                pad: rgb(0x2E, 0x32, 0x3B),
                mouse: rgb(0x5A, 0x61, 0x70),
            },
            KeyboardTheme::Sakura => KeyboardColors {
                base: rgb(0xF7, 0xC8, 0xD5),
                front: rgb(0xE3, 0x9C, 0xB0),
                key: rgb(0xFF, 0xF2, 0xF6),
                key_edge: rgb(0xEB, 0xB5, 0xC5),
                modifier: rgb(0xF9, 0xD4, 0xE0),
                accent: rgb(0xFF, 0x7E, 0xA9),
                space: rgb(0xCF, 0xBC, 0xF4),
                lit: rgb(0xFF, 0x6F, 0x9C),
                outline: rgb(0x8A, 0x4B, 0x5E),
                pad: rgb(0xB9, 0xA6, 0xE0),
                mouse: rgb(0xFF, 0xF2, 0xF6),
            },
        }
    }

    fn parse(s: &str) -> Option<KeyboardTheme> {
        KeyboardTheme::ALL.into_iter().find(|t| t.name().eq_ignore_ascii_case(s))
    }
}

/// How smoothly to animate. Frames are paced by the display's own refresh (vsync).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrameRate {
    /// The monitor's full refresh rate (120/144 Hz…) while moving.
    Smooth,
    /// Up to 60 fps.
    Balanced,
    /// Up to 30 fps, lowest CPU.
    Saver,
}

impl FrameRate {
    pub const ALL: [FrameRate; 3] = [FrameRate::Smooth, FrameRate::Balanced, FrameRate::Saver];

    pub fn name(self) -> &'static str {
        match self {
            FrameRate::Smooth => "Smooth",
            FrameRate::Balanced => "Balanced",
            FrameRate::Saver => "Saver",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FrameRate::Smooth => "Smooth (match the display, up to 240 fps)",
            FrameRate::Balanced => "Balanced (60 fps)",
            FrameRate::Saver => "Power saver (30 fps)",
        }
    }

    /// Target frames per second for (active, idle, asleep). `f32::INFINITY` means every refresh.
    pub fn targets(self) -> (f32, f32, f32) {
        match self {
            FrameRate::Smooth => (f32::INFINITY, 30.0, 20.0),
            FrameRate::Balanced => (60.0, 30.0, 15.0),
            FrameRate::Saver => (30.0, 20.0, 10.0),
        }
    }

    fn parse(s: &str) -> Option<FrameRate> {
        FrameRate::ALL.into_iter().find(|f| f.name().eq_ignore_ascii_case(s))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// A built-in id (`corgi`) or an imported skin id.
    pub skin: String,
    pub size: usize,
    pub eye_color: EyeColor,
    pub keyboard: KeyboardTheme,
    /// Diagonal backlight sweep across the keys.
    pub key_glow: bool,
    /// Pressed keys pop up from the keyboard as little labelled keycaps.
    pub show_keys: bool,
    pub chase: bool,
    pub zoomies: bool,
    pub sleep: bool,
    /// Idle minutes before a nap (one of [`NAP_MINUTES`]).
    pub nap_minutes: u32,
    /// Yawns now and then while idle.
    pub yawn: bool,
    /// Puzzled head tilt at a few quick Backspaces.
    pub huh: bool,
    pub frame_rate: FrameRate,
    pub full_click_through: bool,
    pub lock_position: bool,
    pub always_on_top: bool,
    /// Solid green background for OBS chroma key.
    pub stream_mode: bool,
    /// Keep a key counter for this session (shown in the menu, never saved).
    pub count_keys: bool,
    pub check_updates: bool,
    /// Last window position in physical pixels.
    pub window_pos: Option<(i32, i32)>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            skin: BUILTIN_CORGI.into(),
            size: DEFAULT_SIZE,
            eye_color: EyeColor::Brown,
            keyboard: KeyboardTheme::Cream,
            key_glow: false,
            show_keys: true,
            chase: true,
            zoomies: true,
            sleep: true,
            nap_minutes: DEFAULT_NAP_MINUTES,
            yawn: true,
            huh: true,
            frame_rate: FrameRate::Smooth,
            full_click_through: false,
            lock_position: false,
            always_on_top: true,
            stream_mode: false,
            count_keys: true,
            check_updates: false,
            window_pos: None,
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RawSettings {
    skin: Option<String>,
    size: Option<i64>,
    eye_color: Option<String>,
    keyboard: Option<String>,
    key_glow: Option<bool>,
    show_keys: Option<bool>,
    chase: Option<bool>,
    zoomies: Option<bool>,
    sleep: Option<bool>,
    nap_minutes: Option<i64>,
    yawn: Option<bool>,
    huh: Option<bool>,
    frame_rate: Option<String>,
    full_click_through: Option<bool>,
    lock_position: Option<bool>,
    always_on_top: Option<bool>,
    stream_mode: Option<bool>,
    count_keys: Option<bool>,
    check_updates: Option<bool>,
    window_x: Option<i64>,
    window_y: Option<i64>,
}

fn valid_skin_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 32 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

impl Settings {
    /// Idle seconds before the dog naps.
    pub fn sleep_after(&self) -> f64 {
        let m = if NAP_MINUTES.contains(&self.nap_minutes) { self.nap_minutes } else { DEFAULT_NAP_MINUTES };
        m as f64 * 60.0
    }

    pub fn scale(&self) -> f32 {
        SIZE_SCALES[self.size.min(SIZE_SCALES.len() - 1)]
    }

    /// Parses settings. Any field that is missing or invalid falls back to its default,
    /// so a damaged file never stops the dog from starting.
    pub fn from_toml(text: &str) -> Settings {
        let d = Settings::default();
        let Ok(r) = toml::from_str::<RawSettings>(text) else { return d };
        let to_i32 = |v: Option<i64>| v.and_then(|v| i32::try_from(v).ok());
        Settings {
            skin: r.skin.filter(|s| valid_skin_id(s)).unwrap_or(d.skin),
            size: r.size.and_then(|v| usize::try_from(v).ok()).filter(|&v| v < SIZE_SCALES.len()).unwrap_or(d.size),
            eye_color: r.eye_color.as_deref().and_then(EyeColor::parse).unwrap_or(d.eye_color),
            keyboard: r.keyboard.as_deref().and_then(KeyboardTheme::parse).unwrap_or(d.keyboard),
            key_glow: r.key_glow.unwrap_or(d.key_glow),
            show_keys: r.show_keys.unwrap_or(d.show_keys),
            chase: r.chase.unwrap_or(d.chase),
            zoomies: r.zoomies.unwrap_or(d.zoomies),
            sleep: r.sleep.unwrap_or(d.sleep),
            nap_minutes: r
                .nap_minutes
                .and_then(|m| u32::try_from(m).ok())
                .filter(|m| NAP_MINUTES.contains(m))
                .unwrap_or(d.nap_minutes),
            yawn: r.yawn.unwrap_or(d.yawn),
            huh: r.huh.unwrap_or(d.huh),
            frame_rate: r.frame_rate.as_deref().and_then(FrameRate::parse).unwrap_or(d.frame_rate),
            full_click_through: r.full_click_through.unwrap_or(d.full_click_through),
            lock_position: r.lock_position.unwrap_or(d.lock_position),
            always_on_top: r.always_on_top.unwrap_or(d.always_on_top),
            stream_mode: r.stream_mode.unwrap_or(d.stream_mode),
            count_keys: r.count_keys.unwrap_or(d.count_keys),
            check_updates: r.check_updates.unwrap_or(d.check_updates),
            window_pos: match (to_i32(r.window_x), to_i32(r.window_y)) {
                (Some(x), Some(y)) => Some((x, y)),
                _ => None,
            },
        }
    }

    pub fn to_toml(&self) -> String {
        let mut s = String::from("# DeskCorgi settings. Delete this file to reset everything.\n");
        s += &format!("skin = \"{}\"\n", if valid_skin_id(&self.skin) { self.skin.as_str() } else { BUILTIN_CORGI });
        s += &format!("size = {}\n", self.size.min(SIZE_SCALES.len() - 1));
        s += &format!("eye_color = \"{}\"\n", self.eye_color.name());
        s += &format!("keyboard = \"{}\"\n", self.keyboard.name());
        s += &format!("frame_rate = \"{}\"\n", self.frame_rate.name());
        s += &format!("nap_minutes = {}\n", self.nap_minutes);
        for (k, v) in [
            ("key_glow", self.key_glow),
            ("show_keys", self.show_keys),
            ("chase", self.chase),
            ("zoomies", self.zoomies),
            ("sleep", self.sleep),
            ("yawn", self.yawn),
            ("huh", self.huh),
            ("full_click_through", self.full_click_through),
            ("lock_position", self.lock_position),
            ("always_on_top", self.always_on_top),
            ("stream_mode", self.stream_mode),
            ("count_keys", self.count_keys),
            ("check_updates", self.check_updates),
        ] {
            s += &format!("{k} = {v}\n");
        }
        if let Some((x, y)) = self.window_pos {
            s += &format!("window_x = {x}\nwindow_y = {y}\n");
        }
        s
    }

    pub fn load() -> Settings {
        match config_dir().map(|d| d.join("config.toml")) {
            Some(path) => std::fs::read_to_string(path).map(|t| Settings::from_toml(&t)).unwrap_or_default(),
            None => Settings::default(),
        }
    }

    /// Writes atomically (temp file + rename) so a crash never leaves a half-written file.
    pub fn save(&self) -> std::io::Result<()> {
        let dir = config_dir().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no config folder"))?;
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join("config.toml.tmp");
        std::fs::write(&tmp, self.to_toml())?;
        std::fs::rename(&tmp, dir.join("config.toml"))
    }
}

fn env_dir(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// `%APPDATA%\DeskCorgi`, `~/Library/Application Support/DeskCorgi` or `~/.config/deskcorgi`.
pub fn config_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        env_dir("APPDATA").map(|d| d.join("DeskCorgi"))
    } else if cfg!(target_os = "macos") {
        env_dir("HOME").map(|d| d.join("Library/Application Support/DeskCorgi"))
    } else {
        env_dir("XDG_CONFIG_HOME").or_else(|| env_dir("HOME").map(|h| h.join(".config"))).map(|d| d.join("deskcorgi"))
    }
}

/// Where imported skins are kept.
pub fn skins_dir() -> Option<PathBuf> {
    if cfg!(windows) || cfg!(target_os = "macos") {
        config_dir().map(|d| d.join("skins"))
    } else {
        env_dir("XDG_DATA_HOME")
            .or_else(|| env_dir("HOME").map(|h| h.join(".local/share")))
            .map(|d| d.join("deskcorgi/skins"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let s = Settings {
            skin: "0123456789abcdef".into(),
            size: 4,
            eye_color: EyeColor::Green,
            keyboard: KeyboardTheme::Sakura,
            key_glow: true,
            show_keys: false,
            frame_rate: FrameRate::Saver,
            nap_minutes: 10,
            stream_mode: true,
            window_pos: Some((-1200, 340)),
            ..Settings::default()
        };
        assert_eq!(Settings::from_toml(&s.to_toml()), s);
        assert_eq!(Settings::from_toml(&Settings::default().to_toml()), Settings::default());
    }

    #[test]
    fn bad_values_fall_back_to_defaults() {
        let s = Settings::from_toml("skin = \"../../evil\"\nsize = 99\neye_color = \"purple\"\nchase = false\nwindow_x = 5\n");
        assert_eq!(s.skin, BUILTIN_CORGI);
        assert_eq!(s.size, DEFAULT_SIZE);
        assert_eq!(s.eye_color, EyeColor::Brown);
        assert!(!s.chase);
        assert_eq!(s.window_pos, None);
        let s = Settings::from_toml("nap_minutes = 7\n");
        assert_eq!(s.nap_minutes, DEFAULT_NAP_MINUTES);
        assert_eq!(s.sleep_after(), 120.0);
        assert_eq!(Settings::from_toml("nap_minutes = 1\n").sleep_after(), 60.0);
        assert_eq!(Settings::from_toml("this is [not toml"), Settings::default());
        assert_eq!(Settings::from_toml("size = \"big\""), Settings::default());
    }
}
