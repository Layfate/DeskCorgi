//! The Windows app. Two windows share one window procedure:
//! - a hidden *host* window that owns the tray icon, the global hotkey, Raw Input, the frame
//!   timer and session/power notifications;
//! - the visible *pet* window: a topmost, non-activating, per-pixel-alpha layered window.
//!
//! All state lives in a thread-local `App`. Modal UI (menus, dialogs, message boxes) runs
//! *outside* the `App` borrow through a small job queue, so re-entrant messages are safe.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::ptr::{null, null_mut};
use std::time::Instant;

use deskcorgi_core::config::{self, EyeColor, FrameRate, KeyboardTheme, Settings, NAP_MINUTES, SIZE_NAMES, SIZE_SCALES};
use deskcorgi_core::geom::{pt, Pt};
use deskcorgi_core::keymap::{KeyId, KEY_COUNT};
use deskcorgi_core::render::{pixel_size, render_icon};
use deskcorgi_core::skin::{self, Skin, SkinEntry, BUILTINS, BUILTIN_CORGI};
use deskcorgi_core::{mood_config, FrameInput, InputEvent, Mood, MouseButton, Pet, RenderOptions, Renderer};
use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{BeginPaint, EndPaint, InvalidateRect, PAINTSTRUCT};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, WTSUnRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForWindow, GetSystemMetricsForDpi, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, ReleaseCapture, SetCapture, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, VK_MENU,
};
use windows_sys::Win32::UI::Shell::{
    DragFinish, DragQueryFileW, Shell_NotifyIconW, HDROP, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use crate::pacer::Pacer;
use crate::rawinput::{self, Raw};
use crate::surface::{icon_from_pixmap, Surface};
use crate::sys::{self, copy_wide, wide};

const HOST_CLASS: &str = "DeskCorgi.Host";
const PET_CLASS: &str = "DeskCorgi.Pet";
const MUTEX_NAME: &str = "Local\\DeskCorgi.SingleInstance.v1";
const WM_APP_TRAY: u32 = WM_APP + 1;
/// Sent by a second copy of DeskCorgi: turn click-through off and show the menu.
const WM_APP_WAKE: u32 = WM_APP + 2;
const WM_COPYGLOBALDATA: u32 = 0x0049;
const TIMER_FRAME: usize = 1;
const HOTKEY_CLICK_THROUGH: i32 = 1;
const TRAY_ID: u32 = 1;
const GREEN: [u8; 3] = [0, 255, 0];

// Menu command ids
/// Built-in dogs use ID_SKIN_BUILTIN + their index in `skin::BUILTINS`.
const ID_SKIN_BUILTIN: u32 = 100;
const ID_SKIN_IMPORT: u32 = 110;
const ID_SKIN_FOLDER: u32 = 111;
const ID_SKIN_IMPORTED: u32 = 120;
const ID_SKIN_DELETE: u32 = 170;
const MAX_LISTED_SKINS: usize = 40;
const ID_SIZE: u32 = 300;
const ID_EYE: u32 = 320;
const ID_KEYBOARD: u32 = 340;
const ID_KEY_GLOW: u32 = 350;
const ID_SHOW_KEYS: u32 = 351;
const ID_CHASE: u32 = 400;
const ID_ZOOMIES: u32 = 401;
const ID_SLEEP: u32 = 402;
const ID_COUNT_KEYS: u32 = 403;
const ID_YAWN: u32 = 404;
const ID_HUH: u32 = 405;
const ID_FRAME_RATE: u32 = 410;
const ID_NAP: u32 = 420;
const ID_CLICK_THROUGH: u32 = 500;
const ID_LOCK: u32 = 501;
const ID_TOP: u32 = 502;
const ID_STREAM: u32 = 503;
const ID_AUTOSTART: u32 = 504;
const ID_ADMIN: u32 = 505;
const ID_RESET_POS: u32 = 506;
const ID_INFO: u32 = 600;
const ID_QUIT: u32 = 900;

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
    static MODAL: Cell<bool> = const { Cell::new(false) };
}

/// Runs `f` with the app if it exists and is not already borrowed (re-entrant message).
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        let app = guard.as_mut()?;
        Some(f(app))
    })
}

enum Job {
    Menu(POINT),
    Import,
    Message { title: String, text: String, warning: bool },
    Quit,
}

struct Drag {
    grab: POINT,
    start: POINT,
    moved: bool,
    on_head: bool,
    last: POINT,
    last_t: Instant,
}

/// Live reload of the dog's art while it is being drawn: set `DESKCORGI_ART` to the path of an
/// art file (`art/corgi.toml`) and every save shows up on the running dog within half a second.
struct ArtWatch {
    path: PathBuf,
    stamp: Option<std::time::SystemTime>,
    next_check: Instant,
}

impl ArtWatch {
    fn from_env() -> Option<ArtWatch> {
        let path = PathBuf::from(std::env::var_os("DESKCORGI_ART")?);
        Some(ArtWatch { path, stamp: None, next_check: Instant::now() })
    }

    /// The newly saved art, if the file changed since the last look and parses.
    fn poll(&mut self) -> Option<deskcorgi_core::Art> {
        let now = Instant::now();
        if now < self.next_check {
            return None;
        }
        self.next_check = now + std::time::Duration::from_millis(500);
        let stamp = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        if stamp.is_none() || stamp == self.stamp {
            return None;
        }
        self.stamp = stamp;
        let text = std::fs::read_to_string(&self.path).ok()?;
        match deskcorgi_core::art::parse_art(&text) {
            Ok(art) => Some(art),
            Err(e) => {
                // Keep drawing the last good art.
                eprintln!("DESKCORGI_ART {}: {e}", self.path.display());
                None
            }
        }
    }
}

struct App {
    host: HWND,
    pet_hwnd: HWND,
    settings: Settings,
    skin: Skin,
    skins_dir: Option<PathBuf>,
    imported: Vec<SkinEntry>,
    pet: Pet,
    renderer: Renderer,
    surface: Option<Surface>,
    dpi: u32,
    pos: POINT,
    layered: bool,
    toolwindow: bool,
    transparent: bool,
    start: Instant,
    last_tick: Instant,
    tray_icon: HICON,
    tray_added: bool,
    taskbar_msg: u32,
    drag: Option<Drag>,
    drag_velocity: Pt,
    key_vk: [u16; KEY_COUNT],
    key_seen: [f64; KEY_COUNT],
    key_held: [bool; KEY_COUNT],
    last_raw_key: f64,
    last_watchdog: f64,
    last_keycheck: f64,
    last_topmost: f64,
    save_due: Option<f64>,
    fps_frames: u32,
    work_secs: f64,
    present_secs: f64,
    fps_since: f64,
    fps: f32,
    display_hz: f32,
    autostart: bool,
    elevated: bool,
    jobs: Vec<Job>,
    art_watch: Option<ArtWatch>,
}

fn loword(v: usize) -> u32 {
    (v & 0xFFFF) as u32
}

fn hiword(v: usize) -> u32 {
    ((v >> 16) & 0xFFFF) as u32
}

fn cursor_pos() -> Option<POINT> {
    let mut p = POINT { x: 0, y: 0 };
    (unsafe { GetCursorPos(&mut p) } != 0).then_some(p)
}

fn window_rect(hwnd: HWND) -> Option<RECT> {
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    (unsafe { GetWindowRect(hwnd, &mut r) } != 0).then_some(r)
}

impl App {
    fn secs(&self) -> f64 {
        self.start.elapsed().as_secs_f64()
    }

    fn render_scale(&self) -> f32 {
        self.settings.scale() * self.dpi.max(48) as f32 / 96.0
    }

    fn to_canvas(&self, p: POINT) -> Pt {
        let s = self.render_scale();
        pt((p.x - self.pos.x) as f32 / s, (p.y - self.pos.y) as f32 / s)
    }

    fn push_job(&mut self, job: Job) {
        // Don't stack a second menu/dialog on top of an open one.
        if MODAL.with(|m| m.get()) && matches!(job, Job::Menu(_) | Job::Import) {
            return;
        }
        self.jobs.push(job);
    }

    fn mark_dirty(&mut self) {
        self.save_due = Some(self.secs() + 1.5);
    }

    fn save_now(&mut self) {
        self.save_due = None;
        let _ = self.settings.save();
    }

    // ---- frame ---------------------------------------------------------------------------

    fn frame(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;
        let t = self.secs();

        let dpi = unsafe { GetDpiForWindow(self.pet_hwnd) };
        if dpi != 0 {
            self.dpi = dpi;
        }
        if let Some(r) = window_rect(self.pet_hwnd) {
            self.pos = POINT { x: r.left, y: r.top };
        }
        let screen_cursor = cursor_pos();
        let cursor = screen_cursor.map(|c| self.to_canvas(c));
        // Where the cursor is on its monitor, 0..1 each way: the dog's mouse mirrors it.
        let cursor_screen = screen_cursor.and_then(|c| {
            let r = sys::monitor_rect_at(c)?;
            let (w, h) = ((r.right - r.left).max(1) as f32, (r.bottom - r.top).max(1) as f32);
            Some(pt((c.x - r.left) as f32 / (w - 1.0).max(1.0), (c.y - r.top) as f32 / (h - 1.0).max(1.0)))
        });
        self.drag_velocity = self.drag_velocity * (-dt.clamp(0.0, 0.5) * 6.0).exp();
        self.update_click_through();
        self.check_stuck_keys(t);

        let input = FrameInput {
            cursor,
            cursor_screen,
            dragging: self.drag.as_ref().is_some_and(|d| d.moved),
            drag_velocity: self.drag_velocity,
        };
        let cfg = mood_config(&self.settings);
        self.pet.update(dt, &input, &self.skin, &cfg);
        let p0 = Instant::now();
        self.present();
        self.present_secs += p0.elapsed().as_secs_f64();

        if t - self.last_topmost > 2.0 {
            self.last_topmost = t;
            if self.settings.always_on_top && self.drag.is_none() {
                unsafe {
                    SetWindowPos(self.pet_hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
                }
            }
        }
        if t - self.last_watchdog > 3.0 {
            self.last_watchdog = t;
            // Some games re-register Raw Input for themselves; take ours back if keys stopped arriving.
            if t - self.last_raw_key > 3.0 && self.foreground_is_fullscreen() {
                rawinput::register(self.host);
            }
        }
        if self.save_due.is_some_and(|due| t >= due) {
            self.save_now();
        }
        self.fps_frames += 1;

        if t - self.fps_since >= 1.0 {
            self.fps = (self.fps_frames as f64 / (t - self.fps_since)) as f32;
            self.fps_frames = 0;
            self.fps_since = t;
            if std::env::var_os("DESKCORGI_DEBUG_FPS").is_some() {
                let n = (self.fps * (t - self.fps_since).max(1.0) as f32).max(1.0) as f64;
                eprintln!(
                    "{:.1} fps (display {:.0} Hz, mood {:?}), render {:.2} ms, present total {:.2} ms, scale {:.2}",
                    self.fps,
                    self.display_hz,
                    self.pet.mood(),
                    self.work_secs / n,
                    self.present_secs * 1000.0 / n,
                    self.render_scale()
                );
            }
            self.work_secs = 0.0;
            self.present_secs = 0.0;
        }
    }

    /// Frames per second wanted right now: full display rate while the dog is moving,
    /// less when idle, even less while it sleeps.
    fn target_fps(&self) -> f32 {
        let (active, idle, asleep) = self.settings.frame_rate.targets();
        if self.pet.mood() == Mood::Sleep && self.drag.is_none() {
            asleep
        } else if self.pet.is_lively() || self.drag.is_some() {
            active
        } else {
            idle
        }
    }

    fn present(&mut self) {
        let scale = self.render_scale();
        let opts = RenderOptions {
            scale,
            eye_color: self.settings.eye_color,
            keyboard: self.settings.keyboard,
            key_glow: self.settings.key_glow,
        };
        if let Some(art) = self.art_watch.as_mut().and_then(ArtWatch::poll) {
            self.renderer.set_art(art);
        }
        let r0 = Instant::now();
        let pm = self.renderer.render(self.pet.pose(), &self.skin, &opts);
        self.work_secs += r0.elapsed().as_secs_f64() * 1000.0;
        let (w, h) = (pm.width() as i32, pm.height() as i32);
        let mut move_to = None;
        if self.surface.as_ref().map(|s| (s.width, s.height)) != Some((w, h)) {
            if let Some(old) = &self.surface {
                // Keep the dog's seat (bottom centre) in place when the size changes.
                self.pos = POINT { x: self.pos.x + (old.width - w) / 2, y: self.pos.y + (old.height - h) };
                move_to = Some(self.pos);
            }
            self.surface = Surface::new(w, h);
            if !self.layered || move_to.is_some() {
                unsafe {
                    SetWindowPos(self.pet_hwnd, null_mut(), self.pos.x, self.pos.y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
                }
            }
        }
        let Some(surface) = self.surface.as_mut() else { return };
        if self.layered {
            surface.copy_from(pm, None);
            surface.present_layered(self.pet_hwnd, move_to);
        } else {
            surface.copy_from(pm, Some(GREEN));
            surface.present_opaque(self.pet_hwnd, None);
        }
    }

    fn update_click_through(&mut self) {
        let alt = unsafe { GetAsyncKeyState(VK_MENU as i32) } < 0;
        let want = self.settings.full_click_through && !alt && self.drag.is_none();
        if want != self.transparent {
            self.transparent = want;
            unsafe {
                let ex = GetWindowLongPtrW(self.pet_hwnd, GWL_EXSTYLE) as u32;
                let ex = if want { ex | WS_EX_TRANSPARENT } else { ex & !WS_EX_TRANSPARENT };
                SetWindowLongPtrW(self.pet_hwnd, GWL_EXSTYLE, ex as isize);
            }
        }
    }

    /// Releases keys whose key-up we never saw (lock screen, UAC prompt, focus games).
    fn check_stuck_keys(&mut self, t: f64) {
        if t - self.last_keycheck < 0.5 {
            return;
        }
        self.last_keycheck = t;
        for i in 0..KEY_COUNT {
            if self.key_held[i] && t - self.key_seen[i] > 1.5 {
                let vk = self.key_vk[i];
                let down = vk != 0 && unsafe { GetAsyncKeyState(vk as i32) } < 0;
                if down {
                    self.key_seen[i] = t;
                } else {
                    self.key_held[i] = false;
                    self.pet.handle(InputEvent::KeyUp(KeyId(i as u8)));
                }
            }
        }
    }

    fn foreground_is_fullscreen(&self) -> bool {
        unsafe {
            let fg = GetForegroundWindow();
            if fg.is_null() || fg == self.pet_hwnd || fg == self.host {
                return false;
            }
            let Some(r) = window_rect(fg) else { return false };
            match sys::monitor_rect_for(&r) {
                Some(m) => r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom,
                None => false,
            }
        }
    }

    fn on_raw(&mut self, raw: Raw) {
        let t = self.secs();
        match raw {
            Raw::Key { id, down, vk, label } => {
                let i = id.0 as usize;
                if i >= KEY_COUNT {
                    return;
                }
                self.last_raw_key = t;
                if down {
                    // A fresh press (not auto-repeat) pops the key up, if the user wants that.
                    if !self.key_held[i] && self.settings.show_keys {
                        if let Some(label) = label {
                            self.pet.show_key(id, label);
                        }
                    }
                    self.key_held[i] = true;
                    self.key_seen[i] = t;
                    self.key_vk[i] = vk;
                    self.pet.handle(InputEvent::KeyDown(id));
                } else {
                    self.key_held[i] = false;
                    self.pet.handle(InputEvent::KeyUp(id));
                }
            }
            Raw::Mouse { dx, dy, relative, buttons } => {
                if relative && (dx != 0 || dy != 0) {
                    self.pet.handle(InputEvent::MouseMove { dx: dx as f32, dy: dy as f32 });
                }
                let table = [
                    (rawinput::LEFT_DOWN, MouseButton::Left, true),
                    (rawinput::LEFT_UP, MouseButton::Left, false),
                    (rawinput::RIGHT_DOWN, MouseButton::Right, true),
                    (rawinput::RIGHT_UP, MouseButton::Right, false),
                    (rawinput::MIDDLE_DOWN, MouseButton::Middle, true),
                    (rawinput::MIDDLE_UP, MouseButton::Middle, false),
                ];
                for (flag, button, down) in table {
                    if buttons & flag != 0 {
                        self.pet.handle(InputEvent::MouseButton { button, down });
                    }
                }
                if buttons & (rawinput::WHEEL | rawinput::HWHEEL) != 0 {
                    self.pet.handle(InputEvent::Wheel(1.0));
                }
            }
        }
    }

    // ---- window styles, tray, repair ----------------------------------------------------------

    fn ex_style(&self) -> u32 {
        let mut ex = WS_EX_NOACTIVATE | WS_EX_ACCEPTFILES;
        if self.layered {
            ex |= WS_EX_LAYERED;
        }
        if self.toolwindow {
            ex |= WS_EX_TOOLWINDOW;
        }
        if self.transparent {
            ex |= WS_EX_TRANSPARENT;
        }
        if self.settings.always_on_top {
            ex |= WS_EX_TOPMOST;
        }
        ex
    }

    /// Applies stream mode, click-through and always-on-top after a settings change.
    fn apply_styles(&mut self) {
        let layered = !self.settings.stream_mode;
        // In stream mode the window shows up in OBS's window list (no tool-window flag).
        let toolwindow = !self.settings.stream_mode;
        self.transparent = self.settings.full_click_through;
        let hide = toolwindow != self.toolwindow;
        self.layered = layered;
        self.toolwindow = toolwindow;
        unsafe {
            if hide {
                ShowWindow(self.pet_hwnd, SW_HIDE);
            }
            SetWindowLongPtrW(self.pet_hwnd, GWL_EXSTYLE, self.ex_style() as isize);
            let after = if self.settings.always_on_top { HWND_TOPMOST } else { HWND_NOTOPMOST };
            SetWindowPos(self.pet_hwnd, after, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_FRAMECHANGED);
            // Force a full redraw in the new mode.
            self.surface = None;
            self.present();
            if hide {
                ShowWindow(self.pet_hwnd, SW_SHOWNOACTIVATE);
            }
            InvalidateRect(self.pet_hwnd, null(), 0);
        }
    }

    /// The tray shows the DeskCorgi logo (embedded in the exe as icon resource 1, at the
    /// size the taskbar wants for this DPI). If the exe has no icon resource (a build without
    /// the MSVC linker), it falls back to a drawing of the current dog's face.
    fn refresh_tray_icon(&mut self) {
        let dpi = unsafe { GetDpiForWindow(self.host) }.max(96);
        let size = unsafe { GetSystemMetricsForDpi(SM_CXSMICON, dpi) }.clamp(16, 64) as u32;
        let logo = unsafe { LoadImageW(GetModuleHandleW(null()), 1 as *const u16, IMAGE_ICON, size as i32, size as i32, LR_DEFAULTCOLOR) };
        let icon = if logo.is_null() { icon_from_pixmap(&render_icon(&self.skin, size, self.settings.eye_color)) } else { logo as HICON };
        let old = std::mem::replace(&mut self.tray_icon, icon);
        self.update_tray();
        if !old.is_null() {
            unsafe {
                DestroyIcon(old);
            }
        }
    }

    fn tray_data(&self) -> NOTIFYICONDATAW {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.host;
        nid.uID = TRAY_ID;
        nid.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        nid.uCallbackMessage = WM_APP_TRAY;
        nid.hIcon = self.tray_icon;
        copy_wide(&mut nid.szTip, &format!("DeskCorgi · {}", self.skin.name));
        nid
    }

    fn update_tray(&mut self) {
        let nid = self.tray_data();
        unsafe {
            if self.tray_added && Shell_NotifyIconW(NIM_MODIFY, &nid) != 0 {
                return;
            }
            self.tray_added = Shell_NotifyIconW(NIM_ADD, &nid) != 0;
        }
    }

    fn remove_tray(&mut self) {
        if self.tray_added {
            let nid = self.tray_data();
            unsafe {
                Shell_NotifyIconW(NIM_DELETE, &nid);
            }
            self.tray_added = false;
        }
    }

    /// After unlock, resume or a display change: take input back and redraw.
    fn repair(&mut self) {
        self.pet.handle(InputEvent::ReleaseAll);
        self.key_held = [false; KEY_COUNT];
        rawinput::register(self.host);
        self.ensure_visible();
        self.surface = None;
        self.last_topmost = 0.0;
        self.update_tray();
    }

    fn default_pos(&self, w: i32, h: i32) -> POINT {
        let wa = sys::work_area_near(POINT { x: 0, y: 0 });
        POINT { x: wa.right - w - 12, y: wa.bottom - h + (h / 40) }
    }

    fn ensure_visible(&mut self) {
        let Some(r) = window_rect(self.pet_hwnd) else { return };
        if !sys::rect_visible(&r) {
            self.move_to_default();
        }
    }

    fn move_to_default(&mut self) {
        let (w, h) = pixel_size(self.render_scale());
        self.pos = self.default_pos(w as i32, h as i32);
        unsafe {
            SetWindowPos(self.pet_hwnd, null_mut(), self.pos.x, self.pos.y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
        }
        self.settings.window_pos = None;
        self.mark_dirty();
    }

    // ---- skins ---------------------------------------------------------------------------------

    fn set_skin(&mut self, id: &str) {
        let loaded = match &self.skins_dir {
            Some(dir) => skin::load(id, dir),
            None => skin::builtin(id),
        };
        match loaded {
            Some(s) => {
                self.skin = s;
                self.settings.skin = id.to_string();
            }
            None => {
                self.skin = skin::corgi();
                self.settings.skin = BUILTIN_CORGI.into();
            }
        }
        self.refresh_tray_icon();
        self.mark_dirty();
    }

    fn import_skin(&mut self, path: &Path) {
        let Some(dir) = self.skins_dir.clone() else {
            self.push_job(Job::Message { title: "DeskCorgi".into(), text: "Could not find the settings folder.".into(), warning: true });
            return;
        };
        match skin::import_file(path, &dir) {
            Ok(entry) => self.set_skin(&entry.id),
            Err(e) => self.push_job(Job::Message { title: "Could not import this skin".into(), text: e, warning: true }),
        }
    }

    // ---- menu ----------------------------------------------------------------------------------

    fn build_menu(&mut self) -> HMENU {
        unsafe fn add(menu: HMENU, id: u32, text: &str, checked: bool) {
            let w = wide(text);
            AppendMenuW(menu, MF_STRING | if checked { MF_CHECKED } else { 0 }, id as usize, w.as_ptr());
        }
        unsafe fn grayed(menu: HMENU, id: u32, text: &str, checked: bool) {
            let w = wide(text);
            AppendMenuW(menu, MF_STRING | MF_GRAYED | if checked { MF_CHECKED } else { 0 }, id as usize, w.as_ptr());
        }
        unsafe fn sep(menu: HMENU) {
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
        }
        unsafe fn sub(menu: HMENU, child: HMENU, text: &str) {
            let w = wide(text);
            AppendMenuW(menu, MF_POPUP, child as usize, w.as_ptr());
        }

        self.imported = self.skins_dir.as_deref().map(skin::list_imported).unwrap_or_default();
        self.imported.truncate(MAX_LISTED_SKINS);
        self.autostart = sys::autostart_enabled();
        let s = &self.settings;
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                return menu;
            }
            grayed(menu, ID_INFO, &format!("DeskCorgi {}", env!("CARGO_PKG_VERSION")), false);
            sep(menu);

            let dogs = CreatePopupMenu();
            for (i, (id, make)) in BUILTINS.iter().enumerate() {
                add(dogs, ID_SKIN_BUILTIN + i as u32, &make().name, s.skin == *id);
            }
            if !self.imported.is_empty() {
                sep(dogs);
                for (i, e) in self.imported.iter().enumerate() {
                    add(dogs, ID_SKIN_IMPORTED + i as u32, &e.name, s.skin == e.id);
                }
            }
            sep(dogs);
            add(dogs, ID_SKIN_IMPORT, "Import skin…", false);
            add(dogs, ID_SKIN_FOLDER, "Open skins folder", false);
            if !self.imported.is_empty() {
                let del = CreatePopupMenu();
                for (i, e) in self.imported.iter().enumerate() {
                    add(del, ID_SKIN_DELETE + i as u32, &e.name, false);
                }
                sub(dogs, del, "Delete imported skin");
            }
            sub(menu, dogs, "Dog");

            let size = CreatePopupMenu();
            for (i, name) in SIZE_NAMES.iter().enumerate().take(SIZE_SCALES.len()) {
                add(size, ID_SIZE + i as u32, name, s.size == i);
            }
            sub(menu, size, "Size");

            let eyes = CreatePopupMenu();
            for (i, e) in EyeColor::ALL.iter().enumerate() {
                add(eyes, ID_EYE + i as u32, e.name(), s.eye_color == *e);
            }
            sub(menu, eyes, "Eye colour");

            let kb = CreatePopupMenu();
            for (i, k) in KeyboardTheme::ALL.iter().enumerate() {
                add(kb, ID_KEYBOARD + i as u32, k.name(), s.keyboard == *k);
            }
            sep(kb);
            add(kb, ID_KEY_GLOW, "Rainbow key sweep", s.key_glow);
            add(kb, ID_SHOW_KEYS, "Show typed keys", s.show_keys);
            sub(menu, kb, "Keyboard");

            let fun = CreatePopupMenu();
            add(fun, ID_CHASE, "Chase a shaking cursor", s.chase);
            add(fun, ID_ZOOMIES, "Zoomies on click spam", s.zoomies);
            add(fun, ID_YAWN, "Yawn when idle", s.yawn);
            add(fun, ID_HUH, "Tilt head at Backspace spam", s.huh);
            let nap = CreatePopupMenu();
            for (i, m) in NAP_MINUTES.iter().enumerate() {
                let label = if *m == 1 { "After 1 idle minute".to_string() } else { format!("After {m} idle minutes") };
                add(nap, ID_NAP + i as u32, &label, s.sleep && s.nap_minutes == *m);
            }
            sep(nap);
            add(nap, ID_SLEEP, "Never", !s.sleep);
            sub(fun, nap, "Nap");
            sub(menu, fun, "Behaviour");

            let fps = CreatePopupMenu();
            for (i, f) in FrameRate::ALL.iter().enumerate() {
                add(fps, ID_FRAME_RATE + i as u32, f.label(), s.frame_rate == *f);
            }
            sep(fps);
            grayed(fps, ID_INFO + 2, &format!("Now: {:.0} fps · display {:.0} Hz", self.fps, self.display_hz), false);
            sub(menu, fps, "Frame rate");

            sep(menu);
            add(menu, ID_CLICK_THROUGH, "Full click-through\tCtrl+Alt+Shift+D", s.full_click_through);
            add(menu, ID_LOCK, "Lock position", s.lock_position);
            add(menu, ID_TOP, "Always on top", s.always_on_top);
            add(menu, ID_STREAM, "Stream mode (green screen)", s.stream_mode);
            add(menu, ID_RESET_POS, "Move back to the corner", false);
            sep(menu);
            add(menu, ID_AUTOSTART, "Start with Windows", self.autostart);
            if self.elevated {
                grayed(menu, ID_ADMIN, "Running as administrator", true);
            } else {
                add(menu, ID_ADMIN, "Restart as administrator", false);
            }
            sep(menu);
            if s.count_keys {
                grayed(menu, ID_INFO + 1, &format!("Keys this session: {}", self.pet.keys_total), false);
            }
            add(menu, ID_COUNT_KEYS, "Count keys (this session only)", s.count_keys);
            add(menu, ID_QUIT, "Quit", false);
            menu
        }
    }

    fn on_command(&mut self, id: u32) {
        let in_range = |base: u32, n: usize| id >= base && id < base + n as u32;
        match id {
            _ if in_range(ID_SKIN_BUILTIN, BUILTINS.len()) => self.set_skin(BUILTINS[(id - ID_SKIN_BUILTIN) as usize].0),
            ID_SKIN_IMPORT => self.push_job(Job::Import),
            ID_SKIN_FOLDER => {
                if let Some(dir) = &self.skins_dir {
                    sys::open_folder(dir);
                }
            }
            _ if in_range(ID_SKIN_IMPORTED, self.imported.len()) => {
                let e = self.imported[(id - ID_SKIN_IMPORTED) as usize].clone();
                self.set_skin(&e.id);
            }
            _ if in_range(ID_SKIN_DELETE, self.imported.len()) => {
                let e = self.imported[(id - ID_SKIN_DELETE) as usize].clone();
                if let Some(dir) = &self.skins_dir {
                    if let Err(err) = skin::delete_imported(&e.id, dir) {
                        self.push_job(Job::Message { title: "DeskCorgi".into(), text: err, warning: true });
                    }
                }
                if self.settings.skin == e.id {
                    self.set_skin(BUILTIN_CORGI);
                }
            }
            _ if in_range(ID_SIZE, SIZE_SCALES.len()) => {
                self.settings.size = (id - ID_SIZE) as usize;
                self.mark_dirty();
            }
            _ if in_range(ID_EYE, EyeColor::ALL.len()) => {
                self.settings.eye_color = EyeColor::ALL[(id - ID_EYE) as usize];
                self.refresh_tray_icon();
                self.mark_dirty();
            }
            _ if in_range(ID_KEYBOARD, KeyboardTheme::ALL.len()) => {
                self.settings.keyboard = KeyboardTheme::ALL[(id - ID_KEYBOARD) as usize];
                self.mark_dirty();
            }
            ID_KEY_GLOW => self.toggle(|s| &mut s.key_glow),
            ID_SHOW_KEYS => self.toggle(|s| &mut s.show_keys),
            ID_CHASE => self.toggle(|s| &mut s.chase),
            ID_ZOOMIES => self.toggle(|s| &mut s.zoomies),
            ID_SLEEP => {
                self.settings.sleep = false;
                self.mark_dirty();
            }
            _ if in_range(ID_NAP, NAP_MINUTES.len()) => {
                self.settings.sleep = true;
                self.settings.nap_minutes = NAP_MINUTES[(id - ID_NAP) as usize];
                self.mark_dirty();
            }
            ID_YAWN => self.toggle(|s| &mut s.yawn),
            ID_HUH => self.toggle(|s| &mut s.huh),
            _ if in_range(ID_FRAME_RATE, FrameRate::ALL.len()) => {
                self.settings.frame_rate = FrameRate::ALL[(id - ID_FRAME_RATE) as usize];
                self.mark_dirty();
            }
            ID_COUNT_KEYS => self.toggle(|s| &mut s.count_keys),
            ID_LOCK => self.toggle(|s| &mut s.lock_position),
            ID_CLICK_THROUGH => {
                self.toggle(|s| &mut s.full_click_through);
                self.apply_styles();
            }
            ID_TOP => {
                self.toggle(|s| &mut s.always_on_top);
                self.apply_styles();
            }
            ID_STREAM => {
                self.toggle(|s| &mut s.stream_mode);
                self.apply_styles();
            }
            ID_RESET_POS => self.move_to_default(),
            ID_AUTOSTART => {
                let want = !sys::autostart_enabled();
                if !sys::set_autostart(want) {
                    self.push_job(Job::Message {
                        title: "DeskCorgi".into(),
                        text: "Could not change the Windows startup setting.".into(),
                        warning: true,
                    });
                }
                self.autostart = sys::autostart_enabled();
            }
            ID_ADMIN => {
                if !self.elevated {
                    self.save_now();
                    if sys::restart_elevated() {
                        self.push_job(Job::Quit);
                    }
                }
            }
            ID_QUIT => self.push_job(Job::Quit),
            _ => {}
        }
    }

    fn toggle(&mut self, field: impl FnOnce(&mut Settings) -> &mut bool) {
        let f = field(&mut self.settings);
        *f = !*f;
        self.mark_dirty();
    }

    fn wake(&mut self) {
        if self.settings.full_click_through {
            self.settings.full_click_through = false;
            self.apply_styles();
            self.mark_dirty();
        }
        self.ensure_visible();
        let r = window_rect(self.pet_hwnd);
        let p = r.map(|r| POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 }).or_else(cursor_pos);
        if let Some(p) = p {
            self.push_job(Job::Menu(p));
        }
    }

    fn shutdown(&mut self) {
        unsafe {
            KillTimer(self.host, TIMER_FRAME);
            UnregisterHotKey(self.host, HOTKEY_CLICK_THROUGH);
            WTSUnRegisterSessionNotification(self.host);
        }
        self.save_now();
        self.remove_tray();
        unsafe {
            DestroyWindow(self.pet_hwnd);
            DestroyWindow(self.host);
        }
    }

    // ---- messages ------------------------------------------------------------------------------

    fn handle(&mut self, hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        if hwnd == self.pet_hwnd {
            return self.handle_pet(msg, wp, lp);
        }
        if hwnd != self.host {
            return None;
        }
        match msg {
            WM_INPUT => {
                if let Some(raw) = rawinput::read(lp) {
                    self.on_raw(raw);
                }
                None // DefWindowProc does the required cleanup
            }
            WM_TIMER if wp == TIMER_FRAME => {
                // Normally the main loop paces frames. While a menu or dialog runs its own
                // modal loop, this slower timer keeps the dog animating.
                if MODAL.with(|m| m.get()) {
                    self.frame();
                }
                Some(0)
            }
            WM_APP_TRAY => {
                let ev = loword(lp as usize);
                if ev == WM_LBUTTONUP || ev == WM_RBUTTONUP || ev == WM_CONTEXTMENU {
                    if let Some(p) = cursor_pos() {
                        self.push_job(Job::Menu(p));
                    }
                }
                Some(0)
            }
            WM_APP_WAKE => {
                self.wake();
                Some(0)
            }
            WM_HOTKEY if wp as i32 == HOTKEY_CLICK_THROUGH => {
                self.settings.full_click_through = !self.settings.full_click_through;
                self.apply_styles();
                self.mark_dirty();
                Some(0)
            }
            WM_WTSSESSION_CHANGE => {
                match wp as u32 {
                    WTS_SESSION_LOCK => {
                        self.pet.handle(InputEvent::ReleaseAll);
                        self.key_held = [false; KEY_COUNT];
                    }
                    WTS_SESSION_UNLOCK => self.repair(),
                    _ => {}
                }
                Some(0)
            }
            WM_POWERBROADCAST => {
                if wp as u32 == PBT_APMRESUMEAUTOMATIC {
                    self.repair();
                }
                Some(1)
            }
            WM_DISPLAYCHANGE => {
                self.repair();
                None
            }
            WM_QUERYENDSESSION => {
                self.save_now();
                Some(1)
            }
            WM_ENDSESSION => {
                if wp != 0 {
                    self.save_now();
                    self.remove_tray();
                }
                Some(0)
            }
            WM_CLOSE => {
                self.push_job(Job::Quit);
                Some(0)
            }
            m if m == self.taskbar_msg && m != 0 => {
                // Explorer restarted: the tray icon is gone.
                self.tray_added = false;
                self.update_tray();
                Some(0)
            }
            _ => None,
        }
    }

    fn end_drag(&mut self, boop: bool) {
        let Some(d) = self.drag.take() else { return };
        if d.moved {
            self.settings.window_pos = Some((self.pos.x, self.pos.y));
            self.mark_dirty();
        } else if boop && d.on_head {
            self.pet.boop();
        }
    }

    fn handle_pet(&mut self, msg: u32, wp: WPARAM, lp: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_MOUSEACTIVATE => Some(MA_NOACTIVATE as LRESULT),
            WM_LBUTTONDOWN => {
                let c = cursor_pos()?;
                let on_head = self.pet.head_hit(self.to_canvas(c), &self.skin);
                self.drag = Some(Drag {
                    grab: POINT { x: c.x - self.pos.x, y: c.y - self.pos.y },
                    start: c,
                    moved: false,
                    on_head,
                    last: c,
                    last_t: Instant::now(),
                });
                unsafe {
                    SetCapture(self.pet_hwnd);
                }
                Some(0)
            }
            WM_MOUSEMOVE => {
                let lock = self.settings.lock_position;
                let scale = self.render_scale();
                let c = cursor_pos()?;
                let Some(d) = self.drag.as_mut() else { return Some(0) };
                if !d.moved && !lock && (c.x - d.start.x).abs() + (c.y - d.start.y).abs() > 4 {
                    d.moved = true;
                }
                if d.moved {
                    let (x, y) = (c.x - d.grab.x, c.y - d.grab.y);
                    let dt = d.last_t.elapsed().as_secs_f32();
                    if dt > 0.004 {
                        let v = pt((c.x - d.last.x) as f32, (c.y - d.last.y) as f32) * (1.0 / (dt * scale));
                        self.drag_velocity = self.drag_velocity.lerp(v.clamp_len(4000.0), 0.5);
                        d.last = c;
                        d.last_t = Instant::now();
                    }
                    self.pos = POINT { x, y };
                    unsafe {
                        SetWindowPos(self.pet_hwnd, null_mut(), x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
                    }
                }
                Some(0)
            }
            WM_LBUTTONUP => {
                self.end_drag(true);
                unsafe {
                    ReleaseCapture();
                }
                Some(0)
            }
            WM_CAPTURECHANGED => {
                if lp as HWND != self.pet_hwnd {
                    self.end_drag(false);
                }
                Some(0)
            }
            WM_RBUTTONUP => {
                if let Some(p) = cursor_pos() {
                    self.push_job(Job::Menu(p));
                }
                Some(0)
            }
            WM_DROPFILES => {
                let hdrop = wp as HDROP;
                let mut buf = vec![0u16; 4096];
                let n = unsafe { DragQueryFileW(hdrop, 0, buf.as_mut_ptr(), buf.len() as u32) } as usize;
                unsafe {
                    DragFinish(hdrop);
                }
                if n > 0 && n < buf.len() {
                    use std::os::windows::ffi::OsStringExt;
                    let path = PathBuf::from(std::ffi::OsString::from_wide(&buf[..n]));
                    self.import_skin(&path);
                }
                Some(0)
            }
            WM_PAINT => {
                unsafe {
                    let mut ps: PAINTSTRUCT = std::mem::zeroed();
                    let dc = BeginPaint(self.pet_hwnd, &mut ps);
                    if !self.layered && !dc.is_null() {
                        if let Some(s) = &self.surface {
                            s.present_opaque(self.pet_hwnd, Some(dc));
                        }
                    }
                    EndPaint(self.pet_hwnd, &ps);
                }
                Some(0)
            }
            WM_ERASEBKGND => Some(1),
            WM_DPICHANGED => {
                let dpi = hiword(wp);
                if dpi != 0 {
                    self.dpi = dpi;
                }
                Some(0)
            }
            WM_CLOSE => {
                // Task Manager / taskkill / sign-out ask the visible window to close.
                self.push_job(Job::Quit);
                Some(0)
            }
            _ => None,
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if !self.tray_icon.is_null() {
            unsafe {
                DestroyIcon(self.tray_icon);
            }
        }
    }
}

// ---- window procedure & jobs ----------------------------------------------------------------

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    let handled = with_app(|app| app.handle(hwnd, msg, wp, lp)).flatten();
    if !MODAL.with(|m| m.get()) {
        run_jobs();
    }
    match handled {
        Some(r) => r,
        None => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn run_jobs() {
    loop {
        let job = with_app(|a| if a.jobs.is_empty() { None } else { Some(a.jobs.remove(0)) }).flatten();
        let Some(job) = job else { break };
        MODAL.with(|m| m.set(true));
        match job {
            Job::Menu(p) => show_menu(p),
            Job::Import => {
                let owner = with_app(|a| a.host).unwrap_or(null_mut());
                if let Some(path) = sys::pick_skin_file(owner) {
                    with_app(|a| a.import_skin(&path));
                }
            }
            Job::Message { title, text, warning } => {
                let owner = with_app(|a| a.host).unwrap_or(null_mut());
                sys::message(owner, &title, &text, warning);
            }
            Job::Quit => {
                with_app(|a| a.shutdown());
                unsafe {
                    PostQuitMessage(0);
                }
            }
        }
        MODAL.with(|m| m.set(false));
    }
}

fn show_menu(p: POINT) {
    let Some((menu, host)) = with_app(|a| (a.build_menu(), a.host)) else { return };
    if menu.is_null() {
        return;
    }
    let cmd = unsafe {
        SetForegroundWindow(host);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY, p.x, p.y, 0, host, null());
        PostMessageW(host, WM_NULL, 0, 0);
        DestroyMenu(menu);
        cmd
    };
    if cmd > 0 {
        with_app(|a| a.on_command(cmd as u32));
    }
}

// ---- startup ----------------------------------------------------------------------------------

struct InstanceLock(HANDLE);

impl Drop for InstanceLock {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Takes the single-instance lock. With `wait`, keeps trying for a few seconds (used when
/// restarting as administrator while the old copy is still closing).
fn lock_instance(wait: bool) -> Option<InstanceLock> {
    let name = wide(MUTEX_NAME);
    let tries = if wait { 60 } else { 1 };
    for _ in 0..tries {
        unsafe {
            let h = CreateMutexW(null(), 0, name.as_ptr());
            let err = GetLastError();
            if !h.is_null() && err != ERROR_ALREADY_EXISTS {
                return Some(InstanceLock(h));
            }
            if !h.is_null() {
                CloseHandle(h);
            }
        }
        if wait {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    None
}

fn register_class(name: &[u16], hinst: HINSTANCE, cursor: bool) -> bool {
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style: 0,
        lpfnWndProc: Some(wndproc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: hinst,
        // The logo (shared handles, owned by the system), for Alt+Tab and dialogs.
        hIcon: unsafe { LoadImageW(hinst, 1 as *const u16, IMAGE_ICON, 0, 0, LR_DEFAULTSIZE | LR_SHARED) } as HICON,
        hCursor: if cursor { unsafe { LoadCursorW(null_mut(), IDC_ARROW) } } else { null_mut() },
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: name.as_ptr(),
        hIconSm: unsafe { LoadImageW(hinst, 1 as *const u16, IMAGE_ICON, GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON), LR_SHARED) } as HICON,
    };
    unsafe { RegisterClassExW(&wc) != 0 }
}

pub fn run() -> i32 {
    let wait_for_old = std::env::args().any(|a| a == "--replace");
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    let host_class = wide(HOST_CLASS);
    let pet_class = wide(PET_CLASS);
    let Some(lock) = lock_instance(wait_for_old) else {
        // Already running: ask that copy to come back (click-through off + menu).
        unsafe {
            let other = FindWindowW(host_class.as_ptr(), null());
            if !other.is_null() {
                PostMessageW(other, WM_APP_WAKE, 0, 0);
            }
        }
        return 0;
    };

    let hinst: HINSTANCE = unsafe { GetModuleHandleW(null()) };
    if !register_class(&host_class, hinst, false) || !register_class(&pet_class, hinst, true) {
        sys::message(null_mut(), "DeskCorgi", "Could not start (window class registration failed).", true);
        return 1;
    }
    let title = wide("DeskCorgi");
    let host = unsafe {
        CreateWindowExW(WS_EX_TOOLWINDOW, host_class.as_ptr(), title.as_ptr(), WS_POPUP, 0, 0, 0, 0, null_mut(), null_mut(), hinst, null())
    };
    if host.is_null() {
        sys::message(null_mut(), "DeskCorgi", "Could not create the main window.", true);
        return 1;
    }
    unsafe {
        // Let a non-elevated second copy reach an elevated first copy.
        ChangeWindowMessageFilterEx(host, WM_APP_WAKE, MSGFLT_ALLOW, null_mut());
    }

    let settings = Settings::load();
    let skins_dir = config::skins_dir();
    let skin = skins_dir
        .as_deref()
        .and_then(|d| skin::load(&settings.skin, d))
        .or_else(|| skin::builtin(&settings.skin))
        .unwrap_or_else(skin::corgi);
    let mut settings = settings;
    if skin::builtin(&settings.skin).is_none() && skins_dir.as_deref().and_then(|d| skin::load(&settings.skin, d)).is_none() {
        settings.skin = BUILTIN_CORGI.into();
    }

    let layered = !settings.stream_mode;
    let toolwindow = !settings.stream_mode;
    let transparent = settings.full_click_through;
    let mut app = App {
        host,
        pet_hwnd: null_mut(),
        renderer: Renderer::new(settings.scale()),
        settings,
        skin,
        skins_dir,
        imported: Vec::new(),
        pet: Pet::new(std::process::id()),
        surface: None,
        dpi: 96,
        pos: POINT { x: 0, y: 0 },
        layered,
        toolwindow,
        transparent,
        start: Instant::now(),
        last_tick: Instant::now(),
        tray_icon: null_mut(),
        tray_added: false,
        taskbar_msg: unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) },
        drag: None,
        drag_velocity: Pt::ZERO,
        key_vk: [0; KEY_COUNT],
        key_seen: [0.0; KEY_COUNT],
        key_held: [false; KEY_COUNT],
        last_raw_key: 0.0,
        last_watchdog: 0.0,
        last_keycheck: 0.0,
        last_topmost: 0.0,
        save_due: None,
        fps_frames: 0,
        work_secs: 0.0,
        present_secs: 0.0,
        fps_since: 0.0,
        fps: 0.0,
        display_hz: 60.0,
        autostart: sys::autostart_enabled(),
        elevated: sys::is_elevated(),
        jobs: Vec::new(),
        art_watch: ArtWatch::from_env(),
    };

    let pet_hwnd = unsafe {
        CreateWindowExW(app.ex_style(), pet_class.as_ptr(), title.as_ptr(), WS_POPUP, 0, 0, 1, 1, null_mut(), null_mut(), hinst, null())
    };
    if pet_hwnd.is_null() {
        sys::message(null_mut(), "DeskCorgi", "Could not create the dog window.", true);
        return 1;
    }
    app.pet_hwnd = pet_hwnd;
    unsafe {
        for m in [WM_DROPFILES, WM_COPYDATA, WM_COPYGLOBALDATA] {
            ChangeWindowMessageFilterEx(pet_hwnd, m, MSGFLT_ALLOW, null_mut());
        }
    }

    // Place the window: saved position if it is still on a monitor, else the bottom-right corner.
    app.dpi = unsafe { GetDpiForWindow(pet_hwnd) }.max(96);
    let (w, h) = pixel_size(app.render_scale());
    let (w, h) = (w as i32, h as i32);
    let pos = match app.settings.window_pos {
        Some((x, y)) if sys::rect_visible(&RECT { left: x, top: y, right: x + w, bottom: y + h }) => POINT { x, y },
        _ => app.default_pos(w, h),
    };
    app.pos = pos;
    unsafe {
        SetWindowPos(pet_hwnd, null_mut(), pos.x, pos.y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
    }
    // The second instance of this lookup happens after the move, in case it changed monitors.
    app.dpi = unsafe { GetDpiForWindow(pet_hwnd) }.max(96);

    app.refresh_tray_icon();
    app.pet.update(0.0, &FrameInput::default(), &app.skin, &mood_config(&app.settings));
    app.present();
    unsafe {
        ShowWindow(pet_hwnd, SW_SHOWNOACTIVATE);
        if app.settings.always_on_top {
            SetWindowPos(pet_hwnd, HWND_TOPMOST, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        rawinput::register(host);
        RegisterHotKey(host, HOTKEY_CLICK_THROUGH, MOD_CONTROL | MOD_ALT | MOD_SHIFT | MOD_NOREPEAT, 'D' as u32);
        WTSRegisterSessionNotification(host, NOTIFY_FOR_THIS_SESSION);
        SetTimer(host, TIMER_FRAME, 33, None);
    }
    app.last_tick = Instant::now();
    APP.with(|cell| *cell.borrow_mut() = Some(app));

    // Main loop: handle every pending message (input first), render when a frame is due,
    // then sleep until the next frame or the next message.
    let mut pacer = Pacer::new();
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        'run: loop {
            while PeekMessageW(&mut msg, null_mut(), 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    break 'run;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if pacer.due() {
                let hz = pacer.refresh_hz();
                let Some(target) = with_app(|a| {
                    a.display_hz = hz;
                    a.frame();
                    a.target_fps()
                }) else {
                    break 'run;
                };
                pacer.after_frame(target);
            } else {
                pacer.wait();
            }
        }
    }
    // Make sure everything is saved even if the loop ended some other way.
    let app = APP.with(|cell| cell.borrow_mut().take());
    if let Some(mut app) = app {
        app.save_now();
        app.remove_tray();
    }
    drop(lock);
    0
}
