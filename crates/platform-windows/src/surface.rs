//! A 32-bit DIB section the renderer's pixels are copied into, plus presenting it to a
//! per-pixel-alpha layered window (or an opaque window in stream mode), and icon creation.

use std::ptr::null_mut;

use deskcorgi_core::tiny_skia::Pixmap;
use windows_sys::Win32::Foundation::{HWND, POINT, SIZE};
use windows_sys::Win32::Graphics::Gdi::{
    BitBlt, CreateBitmap, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject, AC_SRC_ALPHA,
    AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, SRCCOPY,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, UpdateLayeredWindow, HICON, ICONINFO, ULW_ALPHA};

fn dib_info(w: i32, h: i32) -> BITMAPINFO {
    let mut bi: BITMAPINFO = unsafe { std::mem::zeroed() };
    bi.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: -h, // top-down rows, like tiny-skia
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB,
        biSizeImage: 0,
        biXPelsPerMeter: 0,
        biYPelsPerMeter: 0,
        biClrUsed: 0,
        biClrImportant: 0,
    };
    bi
}

pub struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    pub width: i32,
    pub height: i32,
}

impl Surface {
    pub fn new(width: i32, height: i32) -> Option<Surface> {
        if width <= 0 || height <= 0 {
            return None;
        }
        unsafe {
            let screen = GetDC(null_mut());
            let dc = CreateCompatibleDC(screen);
            ReleaseDC(null_mut(), screen);
            if dc.is_null() {
                return None;
            }
            let bi = dib_info(width, height);
            let mut bits: *mut core::ffi::c_void = null_mut();
            let bitmap = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
            if bitmap.is_null() || bits.is_null() {
                if !bitmap.is_null() {
                    DeleteObject(bitmap);
                }
                DeleteDC(dc);
                return None;
            }
            let old = SelectObject(dc, bitmap);
            Some(Surface { dc, bitmap, old, bits: bits.cast(), width, height })
        }
    }

    fn pixels_mut(&mut self) -> &mut [u8] {
        // SAFETY: the DIB section is width*height*4 bytes and lives as long as `self`.
        unsafe { std::slice::from_raw_parts_mut(self.bits, (self.width * self.height * 4) as usize) }
    }

    /// Copies premultiplied RGBA into premultiplied BGRA. With `background`, composites onto
    /// that solid colour and makes every pixel opaque (stream mode).
    pub fn copy_from(&mut self, pixmap: &Pixmap, background: Option<[u8; 3]>) {
        let w = self.width.min(pixmap.width() as i32) as usize;
        let h = self.height.min(pixmap.height() as i32) as usize;
        let src_stride = pixmap.width() as usize * 4;
        let dst_stride = self.width as usize * 4;
        let src = pixmap.data();
        let dst = self.pixels_mut();
        dst.fill(0);
        for y in 0..h {
            let s = &src[y * src_stride..y * src_stride + w * 4];
            let d = &mut dst[y * dst_stride..y * dst_stride + w * 4];
            for (sp, dp) in s.chunks_exact(4).zip(d.chunks_exact_mut(4)) {
                let (r, g, b, a) = (sp[0] as u32, sp[1] as u32, sp[2] as u32, sp[3] as u32);
                match background {
                    None => {
                        dp[0] = b as u8;
                        dp[1] = g as u8;
                        dp[2] = r as u8;
                        dp[3] = a as u8;
                    }
                    Some([br, bg, bb]) => {
                        let inv = 255 - a;
                        dp[0] = (b + (bb as u32 * inv + 127) / 255).min(255) as u8;
                        dp[1] = (g + (bg as u32 * inv + 127) / 255).min(255) as u8;
                        dp[2] = (r + (br as u32 * inv + 127) / 255).min(255) as u8;
                        dp[3] = 255;
                    }
                }
            }
        }
        if background.is_some() {
            // Fill any area the pixmap does not cover.
            if let Some(bg) = background {
                for y in 0..self.height as usize {
                    for x in 0..self.width as usize {
                        if y < h && x < w {
                            continue;
                        }
                        let i = y * dst_stride + x * 4;
                        let px = &mut self.pixels_mut()[i..i + 4];
                        px.copy_from_slice(&[bg[2], bg[1], bg[0], 255]);
                    }
                }
            }
        }
    }

    /// Updates a layered window with per-pixel alpha. `pos` moves the window at the same time.
    pub fn present_layered(&self, hwnd: HWND, pos: Option<POINT>) -> bool {
        let size = SIZE { cx: self.width, cy: self.height };
        let src = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
        let pos_ptr = pos.as_ref().map_or(std::ptr::null(), |p| p as *const POINT);
        unsafe { UpdateLayeredWindow(hwnd, null_mut(), pos_ptr, &size, self.dc, &src, 0, &blend, ULW_ALPHA) != 0 }
    }

    /// Paints an ordinary (non-layered) window, used for the green-screen stream mode.
    pub fn present_opaque(&self, hwnd: HWND, target: Option<HDC>) {
        unsafe {
            let (dc, owned) = match target {
                Some(dc) => (dc, false),
                None => (GetDC(hwnd), true),
            };
            if dc.is_null() {
                return;
            }
            BitBlt(dc, 0, 0, self.width, self.height, self.dc, 0, 0, SRCCOPY);
            if owned {
                ReleaseDC(hwnd, dc);
            }
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            DeleteObject(self.bitmap);
            DeleteDC(self.dc);
        }
    }
}

/// Builds an icon with a real alpha channel from a premultiplied pixmap.
pub fn icon_from_pixmap(pm: &Pixmap) -> HICON {
    let (w, h) = (pm.width() as i32, pm.height() as i32);
    unsafe {
        let screen = GetDC(null_mut());
        let bi = dib_info(w, h);
        let mut bits: *mut core::ffi::c_void = null_mut();
        let color = CreateDIBSection(screen, &bi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        ReleaseDC(null_mut(), screen);
        if color.is_null() || bits.is_null() {
            if !color.is_null() {
                DeleteObject(color);
            }
            return null_mut();
        }
        let dst = std::slice::from_raw_parts_mut(bits.cast::<u8>(), (w * h * 4) as usize);
        for (sp, dp) in pm.pixels().iter().zip(dst.chunks_exact_mut(4)) {
            // Icons want straight (non-premultiplied) alpha.
            let c = sp.demultiply();
            dp[0] = c.blue();
            dp[1] = c.green();
            dp[2] = c.red();
            dp[3] = c.alpha();
        }
        let mask_bytes = vec![0u8; (((w + 15) / 16) * 2 * h) as usize];
        let mask = CreateBitmap(w, h, 1, 1, mask_bytes.as_ptr().cast());
        let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info);
        DeleteObject(color);
        if !mask.is_null() {
            DeleteObject(mask);
        }
        icon
    }
}
