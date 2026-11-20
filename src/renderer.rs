//! Direct2D + DirectWrite grayscale-antialiased renderer with DIB double-buffering.
//! Compatible with BitBlt, PrintWindow(PW_RENDERFULLCONTENT), and Windows Graphics Capture.

use std::ffi::c_void;

use windows::Win32::Foundation::RECT;
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_COLOR_F, D2D1_PIXEL_FORMAT,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_RENDER_TARGET_PROPERTIES,
    D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE, D2D1CreateFactory,
    ID2D1DCRenderTarget, ID2D1Factory, ID2D1SolidColorBrush,
};
use windows::Win32::Graphics::DirectWrite::{
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_BOLD, DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER,
    DWRITE_TEXT_METRICS, DWRITE_WORD_WRAPPING_NO_WRAP, DWriteCreateFactory, IDWriteFactory,
    IDWriteTextFormat,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC, HGDIOBJ, SRCCOPY, SelectObject,
};
use windows::core::{Result, w};

pub struct DirectWriteRenderer {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
    dc_rt: ID2D1DCRenderTarget,
    white_brush: ID2D1SolidColorBrush,
    fitted_px: f32,
    last_char_len: usize,
    text_format: Option<IDWriteTextFormat>,
    mem_dc: HDC,
    dib: HBITMAP,
    old_bmp: HGDIOBJ,
    buf_w: i32,
    buf_h: i32,
}

impl DirectWriteRenderer {
    pub fn new() -> Result<Self> {
        unsafe {
            let d2d: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let dwrite: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;

            let rt_props = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_IGNORE,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let dc_rt = d2d.CreateDCRenderTarget(&rt_props)?;
            dc_rt.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);

            let white = D2D1_COLOR_F {
                r: 1.0,
                g: 1.0,
                b: 1.0,
                a: 1.0,
            };
            let white_brush = dc_rt.CreateSolidColorBrush(&white, None)?;

            let mut renderer = Self {
                d2d,
                dwrite,
                dc_rt,
                white_brush,
                fitted_px: 54.0,
                last_char_len: 8,
                text_format: None,
                mem_dc: HDC::default(),
                dib: HBITMAP::default(),
                old_bmp: HGDIOBJ::default(),
                buf_w: 0,
                buf_h: 0,
            };
            renderer.recompute_font_size(440, 140);
            Ok(renderer)
        }
    }

    pub fn recompute_font_size(&mut self, width: i32, height: i32) {
        self.recompute_font_size_for(width, height, self.last_char_len);
    }

    pub fn recompute_font_size_for(&mut self, width: i32, height: i32, char_len: usize) {
        let char_len = char_len.max(8);
        self.last_char_len = char_len;

        let w = width.max(64) as f32;
        let h = height.max(32) as f32;
        let max_w = (w * 0.86).max(40.0);
        let max_h = (h * 0.78).max(20.0);

        let hour_digits = char_len.saturating_sub(6).max(2);
        let probe_str = format!("{}:88:88", "8".repeat(hour_digits));
        let probe_text: Vec<u16> = probe_str.encode_utf16().collect();
        let mut lo = 4i32;
        let mut hi = height.max(16);
        let mut best = lo;

        unsafe {
            while lo <= hi {
                let mid = (lo + hi) / 2;
                if let Ok(fmt) = self.dwrite.CreateTextFormat(
                    w!("Consolas"),
                    None,
                    DWRITE_FONT_WEIGHT_BOLD,
                    DWRITE_FONT_STYLE_NORMAL,
                    DWRITE_FONT_STRETCH_NORMAL,
                    mid as f32,
                    w!("en-us"),
                ) {
                    let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                    if let Ok(layout) =
                        self.dwrite
                            .CreateTextLayout(&probe_text, &fmt, 10_000.0, 10_000.0)
                    {
                        let mut metrics = DWRITE_TEXT_METRICS::default();
                        if layout.GetMetrics(&mut metrics).is_ok()
                            && metrics.width <= max_w
                            && metrics.height <= max_h
                        {
                            best = mid;
                            lo = mid + 1;
                            continue;
                        }
                    }
                }
                hi = mid - 1;
            }

            self.fitted_px = best as f32;
            if let Ok(fmt) = self.dwrite.CreateTextFormat(
                w!("Consolas"),
                None,
                DWRITE_FONT_WEIGHT_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                self.fitted_px,
                w!("en-us"),
            ) {
                let _ = fmt.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER);
                let _ = fmt.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
                let _ = fmt.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
                self.text_format = Some(fmt);
            }
        }
    }

    fn ensure_backbuffer(&mut self, width: i32, height: i32) -> bool {
        if width <= 0 || height <= 0 {
            return false;
        }
        if self.buf_w == width && self.buf_h == height && !self.mem_dc.is_invalid() {
            return true;
        }
        self.release_backbuffer();

        unsafe {
            let mem_dc = CreateCompatibleDC(None);
            if mem_dc.is_invalid() {
                return false;
            }
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height, // top-down DIB
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let Ok(dib) = CreateDIBSection(Some(mem_dc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            else {
                let _ = DeleteDC(mem_dc);
                return false;
            };
            let old_bmp = SelectObject(mem_dc, dib.into());
            self.mem_dc = mem_dc;
            self.dib = dib;
            self.old_bmp = old_bmp;
            self.buf_w = width;
            self.buf_h = height;
            true
        }
    }

    fn release_backbuffer(&mut self) {
        unsafe {
            if !self.mem_dc.is_invalid() {
                if !self.old_bmp.is_invalid() {
                    let _ = SelectObject(self.mem_dc, self.old_bmp);
                    self.old_bmp = HGDIOBJ::default();
                }
                if !self.dib.is_invalid() {
                    let _ = DeleteObject(self.dib.into());
                    self.dib = HBITMAP::default();
                }
                let _ = DeleteDC(self.mem_dc);
                self.mem_dc = HDC::default();
            }
            self.buf_w = 0;
            self.buf_h = 0;
        }
    }

    /// Render `text` centered on pure `#000000` into `target_hdc` via DIB + `BitBlt`.
    pub fn render_to_hdc(&mut self, target_hdc: HDC, width: i32, height: i32, text: &str) {
        if !self.ensure_backbuffer(width, height) {
            return;
        }
        let char_len = text.chars().count().max(8);
        if char_len != self.last_char_len {
            self.recompute_font_size_for(width, height, char_len);
        }
        let Some(fmt) = &self.text_format else {
            return;
        };

        let text_u16: Vec<u16> = text.encode_utf16().collect();
        let rc = RECT {
            left: 0,
            top: 0,
            right: width,
            bottom: height,
        };

        unsafe {
            if self.dc_rt.BindDC(self.mem_dc, &rc).is_err() {
                return;
            }
            let _ = &self.d2d; // keeps factory alive for lifetime of render target
            self.dc_rt.BeginDraw();
            let black = D2D1_COLOR_F {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 1.0,
            };
            self.dc_rt.Clear(Some(&black));

            if let Ok(layout) =
                self.dwrite
                    .CreateTextLayout(&text_u16, fmt, width as f32, height as f32)
            {
                self.dc_rt.DrawTextLayout(
                    Default::default(),
                    &layout,
                    &self.white_brush,
                    Default::default(),
                );
            }

            if self.dc_rt.EndDraw(None, None).is_ok() {
                let _ = BitBlt(
                    target_hdc,
                    0,
                    0,
                    width,
                    height,
                    Some(self.mem_dc),
                    0,
                    0,
                    SRCCOPY,
                );
            }
        }
    }
}

impl Drop for DirectWriteRenderer {
    fn drop(&mut self) {
        self.release_backbuffer();
    }
}
