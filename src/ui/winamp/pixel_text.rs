//! Ported from fastpotify (MIT, Copyright (c) 2026 Carmine Paolino),
//! src/ui/winamp/pixel_text.rs.
//!
//! Pixel-aligned, unsmoothed text for the Winamp playlist window.
//!
//! Each line is drawn once at skin resolution with no anti-aliasing,
//! then cached and nearest-neighbour scaled with the rest of the
//! skin. Inter supplies Latin glyphs; Noto Sans JP supplies Japanese
//! and CJK glyphs using the same fallback as the normal UI.
//!
//! `skrifa` reads the face's outlines the same way `epaint` already
//! does inside egui, and `tiny-skia` fills them with no anti-alias.

use std::collections::HashMap;

use egui::{Color32, ColorImage, TextureHandle, TextureOptions};
use skrifa::MetadataProvider as _;
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, HintingInstance, OutlinePen, Target};

/// Eleven pixels, the size Winamp's own playlist font drew at.
pub const SIZE_PX: u32 = 11;
/// Lines kept before the cache is emptied; a queue is far shorter.
const CACHE_LIMIT: usize = 512;

/// The bundled face's bytes, read once.
static FACE_BYTES: &[u8] = include_bytes!("../../../assets/fonts/InterVariable.ttf");

/// A line of text as a bitmap the size it would be on a screen of
/// skin pixels: white ink on nothing, tinted when drawn.
pub struct Line {
    pub texture: TextureHandle,
    pub width: u32,
    pub height: u32,
}

/// A face opened for drawing, with its hinting programme at the size.
struct Face {
    font: skrifa::FontRef<'static>,
    hinting: Option<HintingInstance>,
}

/// Lines drawn so far, and the face they are drawn with.
#[derive(Default)]
pub struct PixelText {
    lines: HashMap<String, Line>,
    faces: Option<Vec<Face>>,
}

impl PixelText {
    /// Drops every texture, for when the window they belong to is gone.
    pub fn clear(&mut self) {
        self.lines.clear();
    }

    fn faces(&mut self) -> &[Face] {
        self.faces.get_or_insert_with(|| {
            [FACE_BYTES, crate::fonts::JAPANESE]
                .into_iter()
                .filter_map(|bytes| {
                    let font = skrifa::FontRef::new(bytes).ok()?;
                    let hinting = HintingInstance::new(
                        &font.outline_glyphs(),
                        Size::new(SIZE_PX as f32),
                        LocationRef::default(),
                        Target::Mono,
                    )
                    .ok();
                    Some(Face { font, hinting })
                })
                .collect()
        })
    }

    fn glyph_for(faces: &[Face], character: char) -> Option<(usize, skrifa::GlyphId)> {
        faces
            .iter()
            .enumerate()
            .find_map(|(i, face)| face.font.charmap().map(character).map(|g| (i, g)))
            .or_else(|| faces.first()?.font.charmap().map('?').map(|g| (0, g)))
    }

    /// How wide a line would be, in skin pixels, from the face's
    /// advances.
    pub fn width(&mut self, text: &str) -> f32 {
        let size = Size::new(SIZE_PX as f32);
        let faces = self.faces();
        text.chars()
            .filter_map(|character| Self::glyph_for(faces, character))
            .map(|(i, glyph)| {
                faces[i]
                    .font
                    .glyph_metrics(size, LocationRef::default())
                    .advance_width(glyph)
                    .unwrap_or(0.0)
                    .round()
            })
            .sum()
    }

    /// A line, drawn now if it has not been.
    pub fn line(&mut self, ctx: &egui::Context, text: &str) -> &Line {
        if self.lines.len() >= CACHE_LIMIT {
            self.lines.clear();
        }
        if !self.lines.contains_key(text) {
            let image = self.rasterise(text);
            let [width, height] = image.size;
            let texture =
                ctx.load_texture(format!("pledit:{text}"), image, TextureOptions::NEAREST);
            self.lines.insert(
                text.to_string(),
                Line {
                    texture,
                    width: width as u32,
                    height: height as u32,
                },
            );
        }
        &self.lines[text]
    }

    fn rasterise(&mut self, text: &str) -> ColorImage {
        let size = Size::new(SIZE_PX as f32);
        let location = LocationRef::default();
        let faces = self.faces();
        let ascent = faces
            .iter()
            .map(|f| f.font.metrics(size, location).ascent.ceil())
            .fold(0.0f32, f32::max);
        let descent = faces
            .iter()
            .map(|f| -f.font.metrics(size, location).descent.floor())
            .fold(0.0f32, f32::max);
        let height = (ascent + descent).max(1.0) as u32;

        let mut paths = Vec::new();
        let mut x = 0.0f32;
        for character in text.chars() {
            let Some((index, glyph)) = Self::glyph_for(faces, character) else {
                continue;
            };
            let face = &faces[index];
            let mut advance = face
                .font
                .glyph_metrics(size, location)
                .advance_width(glyph)
                .unwrap_or(0.0);
            if let Some(outline) = face.font.outline_glyphs().get(glyph) {
                let mut pen = Pen::new(x, ascent);
                let drawn = match &face.hinting {
                    Some(hinting) => outline.draw(DrawSettings::hinted(hinting, false), &mut pen),
                    None => outline.draw(DrawSettings::unhinted(size, location), &mut pen),
                };
                if let Ok(adjusted) = drawn
                    && let Some(hinted) = adjusted.advance_width
                {
                    advance = hinted;
                }
                if let Some(path) = pen.builder.finish() {
                    paths.push(path);
                }
            }
            x += advance.round();
        }

        let width = (x.ceil() as u32).max(1);
        paint_paths(width, height, &paths)
    }
}

/// Fills a set of glyph outlines into a white-on-transparent bitmap.
fn paint_paths(width: u32, height: u32, paths: &[tiny_skia::Path]) -> ColorImage {
    let mut image = ColorImage::filled([width as usize, height as usize], Color32::TRANSPARENT);
    let Some(mut pixmap) = tiny_skia::Pixmap::new(width, height) else {
        return image;
    };
    let mut paint = tiny_skia::Paint {
        anti_alias: false,
        ..tiny_skia::Paint::default()
    };
    paint.set_color_rgba8(255, 255, 255, 255);
    for path in paths {
        pixmap.fill_path(
            path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::identity(),
            None,
        );
    }
    for y in 0..height {
        for x in 0..width {
            if pixmap.pixel(x, y).is_some_and(|pixel| pixel.alpha() > 0) {
                image[(x as usize, y as usize)] = Color32::WHITE;
            }
        }
    }
    image
}

/// Collects an outline into a path, moved to its place on the line
/// and turned the right way up.
struct Pen {
    builder: tiny_skia::PathBuilder,
    x: f32,
    baseline: f32,
}

impl Pen {
    fn new(x: f32, baseline: f32) -> Self {
        Self {
            builder: tiny_skia::PathBuilder::new(),
            x,
            baseline,
        }
    }

    fn at(&self, x: f32, y: f32) -> (f32, f32) {
        (self.x + x, self.baseline - y)
    }
}

impl OutlinePen for Pen {
    fn move_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.builder.move_to(x, y);
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (x, y) = self.at(x, y);
        self.builder.line_to(x, y);
    }

    fn quad_to(&mut self, cx0: f32, cy0: f32, x: f32, y: f32) {
        let (cx0, cy0) = self.at(cx0, cy0);
        let (x, y) = self.at(x, y);
        self.builder.quad_to(cx0, cy0, x, y);
    }

    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        let (cx0, cy0) = self.at(cx0, cy0);
        let (cx1, cy1) = self.at(cx1, cy1);
        let (x, y) = self.at(x, y);
        self.builder.cubic_to(cx0, cy0, cx1, cy1, x, y);
    }

    fn close(&mut self) {
        self.builder.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn japanese_titles_use_real_fallback_glyphs() {
        let mut text = PixelText::default();
        let faces = text.faces();
        for c in "初音ミク".chars() {
            let (face, glyph) = PixelText::glyph_for(faces, c).unwrap();
            assert_eq!(face, 1);
            assert_ne!(Some(glyph), faces[face].font.charmap().map('?'));
        }
        let japanese = text.rasterise("初音ミク");
        let missing = text.rasterise("????");
        assert_ne!(japanese, missing);
    }

    #[test]
    fn a_line_is_ink_on_nothing_at_the_skin_size() {
        let mut text = PixelText::default();
        let image = text.rasterise("Bonobo - Rosewood");
        assert!(image.size[1] >= SIZE_PX as usize && image.size[1] <= 2 * SIZE_PX as usize);
        let ink = image
            .pixels
            .iter()
            .filter(|pixel| **pixel == Color32::WHITE)
            .count();
        assert!(ink > 40, "only {ink} pixels of ink");
        assert!(
            image
                .pixels
                .iter()
                .all(|pixel| *pixel == Color32::WHITE || *pixel == Color32::TRANSPARENT),
            "a pixel is neither ink nor nothing"
        );
        let blank = text.rasterise("");
        assert_eq!(blank.size[0], 1);
        assert!(
            blank
                .pixels
                .iter()
                .all(|pixel| *pixel == Color32::TRANSPARENT)
        );
    }

    #[test]
    fn width_grows_with_the_text() {
        let mut text = PixelText::default();
        let short = text.width("Otomo");
        let long = text.width("Khruangbin - Otomo");
        assert!(long > short && short > 0.0);
    }

    #[test]
    fn an_uncovered_character_falls_back_to_the_question_mark_glyph() {
        let mut text = PixelText::default();
        let face = text.faces();
        let question = PixelText::glyph_for(face, '?');
        // The last codepoint of the supplementary private-use area B:
        // no distributed face maps a glyph to it.
        let uncovered = char::from_u32(0x10FFFD).expect("a valid scalar");
        assert_eq!(PixelText::glyph_for(face, uncovered), question);
    }
}
