//! Frame pacing. At full rate, frames are locked to the display with `DwmFlush` (one frame
//! per refresh: 60, 120, 144 Hz…). Lower rates sleep on a high-resolution waitable timer.
//! Either way the thread wakes immediately when a window message (input) arrives.

use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Graphics::Dwm::{DwmFlush, DwmGetCompositionTimingInfo, DWM_TIMING_INFO};
use windows_sys::Win32::System::Threading::{CreateWaitableTimerExW, SetWaitableTimer, CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS};
use windows_sys::Win32::UI::WindowsAndMessaging::{MsgWaitForMultipleObjects, QS_ALLINPUT};

/// Highest frame rate we ever render at.
pub const MAX_FPS: f32 = 240.0;

pub struct Pacer {
    timer: HANDLE,
    next_due: Instant,
    refresh_hz: f32,
    last_refresh_query: Instant,
    vsync_ok: bool,
    vsync_misses: u32,
}

impl Pacer {
    pub fn new() -> Pacer {
        let mut timer = unsafe { CreateWaitableTimerExW(null(), null(), CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, TIMER_ALL_ACCESS) };
        if timer.is_null() {
            // Older Windows 10 without high-resolution timers: a normal timer still works.
            timer = unsafe { CreateWaitableTimerExW(null(), null(), 0, TIMER_ALL_ACCESS) };
        }
        let now = Instant::now();
        Pacer { timer, next_due: now, refresh_hz: query_refresh_hz().unwrap_or(60.0), last_refresh_query: now, vsync_ok: true, vsync_misses: 0 }
    }

    pub fn refresh_hz(&self) -> f32 {
        self.refresh_hz
    }

    /// Frame rate actually used for a requested target (`INFINITY` = every refresh).
    pub fn effective_fps(&self, target: f32) -> f32 {
        target.min(self.refresh_hz).clamp(5.0, MAX_FPS)
    }

    /// True when it is time to render a frame. Call `after_frame` once the frame is presented.
    pub fn due(&mut self) -> bool {
        Instant::now() >= self.next_due
    }

    /// Schedules the next frame. At (or near) the display rate this waits for the next
    /// vertical blank so motion is perfectly even.
    pub fn after_frame(&mut self, target: f32) {
        let now = Instant::now();
        if now.duration_since(self.last_refresh_query) > Duration::from_secs(2) {
            self.last_refresh_query = now;
            if let Some(hz) = query_refresh_hz() {
                self.refresh_hz = hz;
            }
        }
        let fps = self.effective_fps(target);
        let full_rate = fps >= self.refresh_hz * 0.95;
        if full_rate && self.vsync_ok {
            let before = Instant::now();
            let hr = unsafe { DwmFlush() };
            let waited = before.elapsed();
            // DwmFlush can return at once (no compositor, remote desktop). Fall back to the timer.
            if hr != 0 || waited < Duration::from_micros(300) {
                self.vsync_misses += 1;
                if self.vsync_misses > 30 {
                    self.vsync_ok = false;
                }
            } else {
                self.vsync_misses = 0;
                self.next_due = Instant::now();
                return;
            }
        }
        let interval = Duration::from_secs_f32(1.0 / fps);
        self.next_due += interval;
        // Don't try to catch up after a stall: just continue from now.
        if self.next_due + interval < now || self.next_due > now + interval {
            self.next_due = now + interval;
        }
    }

    /// Sleeps until the next frame is due or a message arrives.
    pub fn wait(&mut self) {
        let now = Instant::now();
        if now >= self.next_due {
            return;
        }
        let left = self.next_due - now;
        let ms = (left.as_secs_f64() * 1000.0).ceil().min(1000.0) as u32;
        unsafe {
            if !self.timer.is_null() {
                // Negative due time = relative, in 100 ns units.
                let due = -((left.as_nanos() / 100).max(1) as i64);
                if SetWaitableTimer(self.timer, &due, 0, None, null(), 0) != 0 {
                    MsgWaitForMultipleObjects(1, &self.timer, 0, ms + 1, QS_ALLINPUT);
                    return;
                }
            }
            MsgWaitForMultipleObjects(0, null_mut(), 0, ms, QS_ALLINPUT);
        }
    }
}

impl Drop for Pacer {
    fn drop(&mut self) {
        if !self.timer.is_null() {
            unsafe {
                CloseHandle(self.timer);
            }
        }
    }
}

/// The compositor's refresh rate in Hz.
fn query_refresh_hz() -> Option<f32> {
    let mut info: DWM_TIMING_INFO = unsafe { std::mem::zeroed() };
    info.cbSize = std::mem::size_of::<DWM_TIMING_INFO>() as u32;
    let hr = unsafe { DwmGetCompositionTimingInfo(null_mut(), &mut info) };
    let r = info.rateRefresh;
    if hr != 0 || r.uiDenominator == 0 || r.uiNumerator == 0 {
        return None;
    }
    let hz = r.uiNumerator as f32 / r.uiDenominator as f32;
    (20.0..=500.0).contains(&hz).then_some(hz)
}
