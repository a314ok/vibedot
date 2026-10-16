//! Signed-distance-field text.
//!
//! Glyphs are rasterized once at a base size, converted to a distance field
//! and packed into an atlas. The fragment shader reconstructs crisp edges at
//! any scale or camera zoom, and can embolden text by shifting the threshold.

use std::collections::HashMap;
use std::sync::OnceLock;

use ab_glyph::{Font, FontRef, GlyphId, ScaleFont, point};
use vd_core::{Vec2, vec2};

/// Size at which glyphs are rasterized into the atlas.
pub const BASE_PX: f32 = 48.0;
/// Distance-field spread in atlas pixels.
pub const SPREAD: i32 = 6;
pub const ATLAS_SIZE: u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontFace {
    Sans,
    Mono,
}

struct Fonts {
    sans: FontRef<'static>,
    mono: FontRef<'static>,
}

fn fonts() -> &'static Fonts {
    static FONTS: OnceLock<Fonts> = OnceLock::new();
    FONTS.get_or_init(|| Fonts {
        sans: FontRef::try_from_slice(epaint_default_fonts::UBUNTU_LIGHT).expect("embedded font is valid"),
        mono: FontRef::try_from_slice(epaint_default_fonts::HACK_REGULAR).expect("embedded font is valid"),
    })
}

fn font(face: FontFace) -> &'static FontRef<'static> {
    match face {
        FontFace::Sans => &fonts().sans,
        FontFace::Mono => &fonts().mono,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AtlasGlyph {
    /// Atlas rect in pixels (min x, min y, w, h).
    pub rect: [u32; 4],
    /// Offset of the padded bitmap's top-left from the pen position at BASE_PX.
    pub offset: Vec2,
}

/// One positioned glyph quad, relative to the text block's top-left, in
/// pixels at the requested font size.
#[derive(Clone, Copy, Debug)]
pub struct GlyphQuad {
    pub min: Vec2,
    pub size: Vec2,
    pub uv: [f32; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// CPU side of the glyph atlas (alpha channel = distance).
pub struct GlyphAtlas {
    pub pixels: Vec<u8>,
    pub dirty: bool,
    glyphs: HashMap<(FontFace, GlyphId), Option<AtlasGlyph>>,
    shelf_x: u32,
    shelf_y: u32,
    shelf_h: u32,
}

impl Default for GlyphAtlas {
    fn default() -> Self {
        Self::new()
    }
}

impl GlyphAtlas {
    pub fn new() -> Self {
        let mut pixels = vec![255u8; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize];
        for a in pixels.iter_mut().skip(3).step_by(4) {
            *a = 0;
        }
        Self { pixels, dirty: true, glyphs: HashMap::new(), shelf_x: 1, shelf_y: 1, shelf_h: 0 }
    }

    /// Rasterizes (if needed) and returns a glyph's atlas entry. `None` for
    /// whitespace or when the atlas is full.
    fn glyph(&mut self, face: FontFace, id: GlyphId) -> Option<AtlasGlyph> {
        if let Some(g) = self.glyphs.get(&(face, id)) {
            return *g;
        }
        let entry = self.rasterize(face, id);
        self.glyphs.insert((face, id), entry);
        entry
    }

    fn rasterize(&mut self, face: FontFace, id: GlyphId) -> Option<AtlasGlyph> {
        let f = font(face);
        let glyph = id.with_scale_and_position(BASE_PX, point(0.0, 0.0));
        let outlined = f.outline_glyph(glyph)?;
        let b = outlined.px_bounds();
        let (gw, gh) = (b.width().ceil() as i32, b.height().ceil() as i32);
        if gw <= 0 || gh <= 0 {
            return None;
        }
        let (w, h) = (gw + SPREAD * 2, gh + SPREAD * 2);
        let mut cov = vec![0.0f32; (w * h) as usize];
        outlined.draw(|x, y, c| {
            let (x, y) = (x as i32 + SPREAD, y as i32 + SPREAD);
            if x < w && y < h {
                cov[(y * w + x) as usize] = c;
            }
        });
        let sdf = coverage_to_sdf(&cov, w, h);

        // Shelf packing.
        let (uw, uh) = (w as u32, h as u32);
        if self.shelf_x + uw + 1 > ATLAS_SIZE {
            self.shelf_x = 1;
            self.shelf_y += self.shelf_h + 1;
            self.shelf_h = 0;
        }
        if self.shelf_y + uh + 1 > ATLAS_SIZE {
            log::warn!("glyph atlas full");
            return None;
        }
        let (ax, ay) = (self.shelf_x, self.shelf_y);
        self.shelf_x += uw + 1;
        self.shelf_h = self.shelf_h.max(uh);
        for y in 0..uh {
            for x in 0..uw {
                let i = (((ay + y) * ATLAS_SIZE + ax + x) * 4 + 3) as usize;
                self.pixels[i] = sdf[(y * uw + x) as usize];
            }
        }
        self.dirty = true;
        Some(AtlasGlyph { rect: [ax, ay, uw, uh], offset: vec2(b.min.x - SPREAD as f32, b.min.y - SPREAD as f32) })
    }

    /// Lays out `text` at `size` pixels. Returns glyph quads and the block size.
    pub fn layout(&mut self, text: &str, size: f32, face: FontFace, align: Align) -> (Vec<GlyphQuad>, Vec2) {
        let f = font(face);
        let scaled = f.as_scaled(BASE_PX);
        let k = size / BASE_PX;
        let line_h = (scaled.ascent() - scaled.descent() + scaled.line_gap()) * k;
        let (widths, total) = measure_lines(text, size, face);
        let mut quads = Vec::with_capacity(text.len());
        for (li, line) in text.split('\n').enumerate() {
            let lw = widths[li];
            let start_x = match align {
                Align::Left => 0.0,
                Align::Center => (total.x - lw) * 0.5,
                Align::Right => total.x - lw,
            };
            let baseline = li as f32 * line_h + scaled.ascent() * k;
            let mut pen = start_x;
            let mut prev: Option<GlyphId> = None;
            for ch in line.chars() {
                let gid = f.glyph_id(ch);
                if let Some(p) = prev {
                    pen += scaled.kern(p, gid) * k;
                }
                if let Some(g) = self.glyph(face, gid) {
                    let a = ATLAS_SIZE as f32;
                    quads.push(GlyphQuad {
                        min: vec2(pen, baseline) + g.offset * k,
                        size: vec2(g.rect[2] as f32, g.rect[3] as f32) * k,
                        uv: [
                            g.rect[0] as f32 / a,
                            g.rect[1] as f32 / a,
                            (g.rect[0] + g.rect[2]) as f32 / a,
                            (g.rect[1] + g.rect[3]) as f32 / a,
                        ],
                    });
                }
                pen += scaled.h_advance(gid) * k;
                prev = Some(gid);
            }
        }
        (quads, total)
    }
}

/// Measures each line and the whole block (no rasterization).
pub fn measure_lines(text: &str, size: f32, face: FontFace) -> (Vec<f32>, Vec2) {
    let f = font(face);
    let scaled = f.as_scaled(size);
    let line_h = scaled.ascent() - scaled.descent() + scaled.line_gap();
    let mut widths = Vec::new();
    for line in text.split('\n') {
        let mut w = 0.0;
        let mut prev = None;
        for ch in line.chars() {
            let gid = f.glyph_id(ch);
            if let Some(p) = prev {
                w += scaled.kern(p, gid);
            }
            w += scaled.h_advance(gid);
            prev = Some(gid);
        }
        widths.push(w);
    }
    let max_w = widths.iter().copied().fold(0.0, f32::max);
    let n = widths.len() as f32;
    (widths, vec2(max_w, line_h * n - scaled.line_gap()))
}

pub fn measure(text: &str, size: f32, face: FontFace) -> Vec2 {
    measure_lines(text, size, face).1
}

/// Brute-force signed distance transform within `SPREAD` pixels.
fn coverage_to_sdf(cov: &[f32], w: i32, h: i32) -> Vec<u8> {
    let inside = |x: i32, y: i32| x >= 0 && y >= 0 && x < w && y < h && cov[(y * w + x) as usize] >= 0.5;
    let mut out = vec![0u8; (w * h) as usize];
    let r = SPREAD;
    for y in 0..h {
        for x in 0..w {
            let me = inside(x, y);
            let mut best = (r * r * 2) as f32;
            for dy in -r..=r {
                for dx in -r..=r {
                    let d2 = (dx * dx + dy * dy) as f32;
                    if d2 < best && inside(x + dx, y + dy) != me {
                        best = d2;
                    }
                }
            }
            // Distance to the boundary sits halfway between the two pixel centers.
            let d = best.sqrt() - 0.5;
            let signed = if me { d } else { -d };
            let v = 0.5 + signed / (2.0 * r as f32);
            out[(y * w + x) as usize] = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_and_measure_agree() {
        let mut atlas = GlyphAtlas::new();
        let (quads, size) = atlas.layout("Hello\nVibedot", 32.0, FontFace::Sans, Align::Center);
        assert_eq!(quads.len(), 12);
        assert!(size.x > 50.0 && size.y > 60.0, "{size}");
        assert_eq!(size, measure("Hello\nVibedot", 32.0, FontFace::Sans));
        // Glyphs stay inside the (padded) block.
        for q in &quads {
            assert!(q.min.x > -10.0 && q.min.x + q.size.x < size.x + 10.0);
        }
        assert!(atlas.dirty);
        // Cached: laying out again does not add glyphs.
        let n = atlas.glyphs.len();
        atlas.layout("Hello", 12.0, FontFace::Sans, Align::Left);
        assert_eq!(atlas.glyphs.len(), n);
    }

    #[test]
    fn sdf_is_monotonic_across_edge() {
        // A filled square in a 20x20 field.
        let w = 20;
        let mut cov = vec![0.0; 400];
        for y in 6..14 {
            for x in 6..14 {
                cov[y * 20 + x] = 1.0;
            }
        }
        let sdf = coverage_to_sdf(&cov, w, w);
        let row: Vec<u8> = (0..20).map(|x| sdf[10 * 20 + x]).collect();
        assert!(row[10] > 128 && row[2] < 128, "{row:?}");
        assert!(row[3] <= row[4] && row[4] <= row[5] && row[5] <= row[7], "{row:?}");
    }
}
