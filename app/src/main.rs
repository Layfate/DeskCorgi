//! DeskCorgi: a tiny desktop dog that mirrors your keyboard and mouse.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(windows)]
fn main() {
    std::process::exit(deskcorgi_windows::run());
}

#[cfg(not(windows))]
fn main() {
    // Linux (X11/Wayland) and macOS layers are later milestones (M4–M6 in IMPLEMENTATION_PLAN.md).
    let _ = deskcorgi_core::skin::corgi();
    eprintln!("DeskCorgi currently runs on Windows only. Linux and macOS support is on the roadmap.");
    std::process::exit(1);
}
