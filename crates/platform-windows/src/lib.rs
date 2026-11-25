//! Windows platform layer for DeskCorgi: a per-pixel-alpha layered window, Raw Input
//! (works while games have focus), a tray icon with the settings menu, and robustness
//! fixes for lock screen, sleep/resume, display changes and elevated games.

#[cfg(windows)]
mod app;
#[cfg(windows)]
mod pacer;
#[cfg(windows)]
mod rawinput;
#[cfg(windows)]
mod surface;
#[cfg(windows)]
mod sys;

/// Runs the app until the user quits. Returns the process exit code.
#[cfg(windows)]
pub fn run() -> i32 {
    app::run()
}
