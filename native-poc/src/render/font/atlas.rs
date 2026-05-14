//! Two-region glyph atlas: Alpha (R8) + Rgba (RGBA8).
//!
//! Phase 2 of font-swash-migration. The atlas is the storage backend behind
//! `render::font::cache::GlyphCache`. Each format owns its own packed 2D
//! page; growth strategy is "double the page width on cap hit"; eviction
//! is not implemented (per IMPLEMENTATION.md "start without eviction, log
//! on cap hit"). Today the atlas is *logical only* — no wgpu textures are
//! created yet because the renderer still routes per-cell drawing through
//! egui's text path. The byte buffers are kept here so the upload path
//! (Phase 3+) can hand them straight to wgpu without reshuffling.

use super::traits::{AtlasFormat, GlyphBitmap};

/// A rectangular region within one of the atlas pages.
///
/// Coordinates are in atlas pixel space. The renderer turns these into
/// UV coordinates by dividing by the page width / height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtlasRegion {
    pub format: AtlasFormat,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl AtlasRegion {
    /// True if this region addresses zero pixels (sentinel for whitespace
    /// / zero-size rasterizer outputs).
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// One packed page backing a single `AtlasFormat`.
///
/// Allocation is a degenerate skyline packer: a single horizontal cursor
/// walks left to right; when the next glyph would overflow the page width
/// the cursor wraps to a new row at the previous row's `max_y`. When the
/// next row would overflow the page height the page grows (doubling the
/// page height up to `MAX_PAGE_DIM`).
#[derive(Debug)]
struct Page {
    format: AtlasFormat,
    width: u32,
    height: u32,
    bytes: Vec<u8>,
    /// Horizontal cursor for the current row.
    cursor_x: u32,
    /// Top of the current row.
    cursor_y: u32,
    /// Height of the tallest glyph in the current row (advances `cursor_y`
    /// on row wrap).
    row_height: u32,
    /// Set on the first allocation failure (cap hit). Read by `cap_hit()`.
    cap_hit: bool,
}

const INITIAL_PAGE_DIM: u32 = 256;
const MAX_PAGE_DIM: u32 = 4096;

impl Page {
    fn new(format: AtlasFormat) -> Self {
        let bpp = match format {
            AtlasFormat::Alpha => 1,
            AtlasFormat::Rgba => 4,
        };
        Self {
            format,
            width: INITIAL_PAGE_DIM,
            height: INITIAL_PAGE_DIM,
            bytes: vec![0; (INITIAL_PAGE_DIM * INITIAL_PAGE_DIM) as usize * bpp],
            cursor_x: 0,
            cursor_y: 0,
            row_height: 0,
            cap_hit: false,
        }
    }

    fn bytes_per_pixel(&self) -> u32 {
        match self.format {
            AtlasFormat::Alpha => 1,
            AtlasFormat::Rgba => 4,
        }
    }

    /// Try to grow the page so that a glyph of `(w, h)` will fit. Returns
    /// `true` on success.
    fn try_grow(&mut self, w: u32, h: u32) -> bool {
        let mut new_w = self.width;
        let mut new_h = self.height;
        while new_w < w.max(self.width) && new_w < MAX_PAGE_DIM {
            new_w *= 2;
        }
        while new_h < (self.cursor_y + h).max(self.height) && new_h < MAX_PAGE_DIM {
            new_h *= 2;
        }
        if new_w > MAX_PAGE_DIM || new_h > MAX_PAGE_DIM {
            return false;
        }
        if new_w == self.width && new_h == self.height {
            return false;
        }
        let bpp = self.bytes_per_pixel() as usize;
        let mut next = vec![0u8; (new_w * new_h) as usize * bpp];
        // Copy old rows into the wider buffer row by row.
        for y in 0..self.height {
            let src = (y * self.width) as usize * bpp;
            let dst = (y * new_w) as usize * bpp;
            let row_len = self.width as usize * bpp;
            next[dst..dst + row_len].copy_from_slice(&self.bytes[src..src + row_len]);
        }
        self.bytes = next;
        self.width = new_w;
        self.height = new_h;
        true
    }

    fn upload(&mut self, bitmap: &GlyphBitmap) -> AtlasRegion {
        debug_assert_eq!(bitmap.format, self.format);
        if bitmap.is_empty() {
            return AtlasRegion {
                format: self.format,
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            };
        }
        let w = bitmap.width;
        let h = bitmap.height;
        // Row wrap.
        if self.cursor_x + w > self.width {
            self.cursor_y += self.row_height;
            self.cursor_x = 0;
            self.row_height = 0;
        }
        // Grow if needed.
        while self.cursor_x + w > self.width || self.cursor_y + h > self.height {
            if !self.try_grow(w, h) {
                self.cap_hit = true;
                log::warn!(
                    "atlas.cap_hit: {:?} page exhausted ({}x{} request at ({}, {}))",
                    self.format,
                    w,
                    h,
                    self.cursor_x,
                    self.cursor_y,
                );
                return AtlasRegion {
                    format: self.format,
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                };
            }
        }
        let x = self.cursor_x;
        let y = self.cursor_y;
        // Blit the bitmap into the page.
        let bpp = self.bytes_per_pixel() as usize;
        for row in 0..h {
            let src = (row * w) as usize * bpp;
            let dst = ((y + row) * self.width + x) as usize * bpp;
            let row_len = w as usize * bpp;
            self.bytes[dst..dst + row_len].copy_from_slice(&bitmap.pixels[src..src + row_len]);
        }
        self.cursor_x += w;
        if h > self.row_height {
            self.row_height = h;
        }
        AtlasRegion {
            format: self.format,
            x,
            y,
            width: w,
            height: h,
        }
    }
}

/// Atlas owning one Alpha page + one Rgba page.
#[derive(Debug)]
pub struct Atlas {
    alpha: Page,
    rgba: Page,
}

impl Default for Atlas {
    fn default() -> Self {
        Self::new()
    }
}

impl Atlas {
    pub fn new() -> Self {
        Self {
            alpha: Page::new(AtlasFormat::Alpha),
            rgba: Page::new(AtlasFormat::Rgba),
        }
    }

    /// Upload a glyph bitmap, routing to the correct format's page.
    pub fn upload(&mut self, bitmap: &GlyphBitmap) -> AtlasRegion {
        match bitmap.format {
            AtlasFormat::Alpha => self.alpha.upload(bitmap),
            AtlasFormat::Rgba => self.rgba.upload(bitmap),
        }
    }

    /// Read-only view of the Alpha page bytes (renderer uses this to
    /// upload to the wgpu R8 texture).
    pub fn alpha_bytes(&self) -> &[u8] {
        &self.alpha.bytes
    }

    /// Read-only view of the Rgba page bytes (renderer uses this to
    /// upload to the wgpu RGBA8 texture).
    pub fn rgba_bytes(&self) -> &[u8] {
        &self.rgba.bytes
    }

    pub fn alpha_dim(&self) -> (u32, u32) {
        (self.alpha.width, self.alpha.height)
    }

    pub fn rgba_dim(&self) -> (u32, u32) {
        (self.rgba.width, self.rgba.height)
    }

    /// True if any page has logged a cap-hit since construction.
    pub fn cap_hit(&self) -> bool {
        self.alpha.cap_hit || self.rgba.cap_hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alpha_bitmap(w: u32, h: u32, fill: u8) -> GlyphBitmap {
        GlyphBitmap {
            format: AtlasFormat::Alpha,
            width: w,
            height: h,
            bearing: (0, 0),
            advance: w as f32,
            pixels: vec![fill; (w * h) as usize],
        }
    }

    fn rgba_bitmap(w: u32, h: u32, fill: u8) -> GlyphBitmap {
        GlyphBitmap {
            format: AtlasFormat::Rgba,
            width: w,
            height: h,
            bearing: (0, 0),
            advance: w as f32,
            pixels: vec![fill; (w * h * 4) as usize],
        }
    }

    /// TS-font-6: Atlas upload routes Alpha → R8 page; Rgba → RGBA8 page.
    #[test]
    fn upload_routes_to_correct_page() {
        let mut atlas = Atlas::new();
        let a = atlas.upload(&alpha_bitmap(4, 4, 0xAA));
        let r = atlas.upload(&rgba_bitmap(4, 4, 0xCC));
        assert_eq!(a.format, AtlasFormat::Alpha);
        assert_eq!(r.format, AtlasFormat::Rgba);
        // Pixel-presence check: byte at the alpha origin equals 0xAA;
        // byte at the rgba origin (first channel) equals 0xCC.
        let (aw, _) = atlas.alpha_dim();
        let (rw, _) = atlas.rgba_dim();
        let a_idx = (a.y * aw + a.x) as usize;
        let r_idx = (r.y * rw + r.x) as usize * 4;
        assert_eq!(atlas.alpha_bytes()[a_idx], 0xAA);
        assert_eq!(atlas.rgba_bytes()[r_idx], 0xCC);
    }

    #[test]
    fn empty_bitmap_returns_empty_region() {
        let mut atlas = Atlas::new();
        let r = atlas.upload(&alpha_bitmap(0, 4, 0));
        assert!(r.is_empty());
        assert!(!atlas.cap_hit());
    }

    #[test]
    fn row_wrap_advances_cursor_y() {
        let mut atlas = Atlas::new();
        // 256 / 64 = 4 glyphs per row. Upload 5; the 5th must wrap onto
        // a fresh row.
        let mut last = AtlasRegion {
            format: AtlasFormat::Alpha,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
        for _ in 0..5 {
            last = atlas.upload(&alpha_bitmap(64, 16, 0xFF));
        }
        assert!(
            last.y >= 16,
            "fifth upload must wrap below row 0 (y={})",
            last.y
        );
    }

    #[test]
    fn grows_when_row_overflows_height() {
        let mut atlas = Atlas::new();
        // Force enough uploads to climb past the initial 256-px page
        // height. Each glyph is 64×64 → 16 fit per page; 20 forces growth.
        for _ in 0..20 {
            atlas.upload(&alpha_bitmap(64, 64, 0xFF));
        }
        let (_, ah) = atlas.alpha_dim();
        assert!(
            ah > 256,
            "alpha page must have grown beyond 256 (got {})",
            ah
        );
        assert!(!atlas.cap_hit());
    }
}
