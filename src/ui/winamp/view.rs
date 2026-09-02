//! Ported from fastpotify (MIT, Copyright (c) 2026 Carmine Paolino), src/ui/winamp/mod.rs
//! (the `View` blitter, lines 120-302).
//!
//! Draws a skin's sprites into an egui `Ui` and reads the pointer back
//! against the skin's layout. One `View` covers one window's worth of
//! pixels, at one scale.

use std::collections::HashMap;

use egui::{Color32, Id, Pos2, Rect, Response, Sense, TextureId, Ui, pos2, vec2};

use crate::skin::layout::Area;
use crate::skin::{Sheet, Skin, Sprite, font};

/// What a drag on a slider did this frame.
#[derive(Clone, Copy)]
pub enum SliderEvent {
    None,
    /// The thumb is somewhere between its ends, not yet let go.
    Dragging(f32),
    /// The drag ended, or a click landed straight on a spot: apply it.
    Committed(f32),
}

pub struct View<'a> {
    pub ui: &'a mut Ui,
    pub origin: Pos2,
    pub unit: f32,
    pub skin: &'a Skin,
    pub textures: &'a HashMap<Sheet, TextureId>,
}

impl View<'_> {
    /// A layout area as a screen rectangle, in this window's origin and
    /// scale.
    pub fn rect(&self, area: Area) -> Rect {
        Rect::from_min_size(
            self.origin + vec2(area.x as f32, area.y as f32) * self.unit,
            vec2(area.width as f32, area.height as f32) * self.unit,
        )
    }

    /// A pointer position as a skin x coordinate.
    fn skin_x(&self, pos: Pos2) -> f32 {
        (pos.x - self.origin.x) / self.unit
    }

    fn paint(&self, painter: &egui::Painter, sprite: Sprite, x: u32, y: u32) {
        let Some((bitmap, clipped)) = self.skin.sprite(sprite) else {
            return;
        };
        let Some(&texture) = self.textures.get(&sprite.sheet) else {
            return;
        };
        let (width, height) = (bitmap.width as f32, bitmap.height as f32);
        let uv = Rect::from_min_max(
            pos2(clipped.x as f32 / width, clipped.y as f32 / height),
            pos2(
                (clipped.x + clipped.width) as f32 / width,
                (clipped.y + clipped.height) as f32 / height,
            ),
        );
        let dest = Rect::from_min_size(
            self.origin + vec2(x as f32, y as f32) * self.unit,
            vec2(clipped.width as f32, clipped.height as f32) * self.unit,
        );
        painter.image(texture, dest, uv, Color32::WHITE);
    }

    pub fn sprite_at(&self, sprite: Sprite, x: u32, y: u32) {
        self.paint(self.ui.painter(), sprite, x, y);
    }

    pub fn sprite(&self, sprite: Sprite, area: Area) {
        self.sprite_at(sprite, area.x, area.y);
    }

    /// A line of the skin's bitmap font, cut off at the area's edge.
    pub fn text(&self, text: &str, area: Area) {
        let clip = self.rect(area).intersect(self.ui.clip_rect());
        let painter = self.ui.painter().with_clip_rect(clip);
        for (index, character) in text.chars().enumerate() {
            let x = area.x + 5 * index as u32;
            if x >= area.x + area.width {
                break;
            }
            self.paint(&painter, font::glyph(character), x, area.y);
        }
    }

    pub fn interact(&mut self, area: Area, id: &str, sense: Sense) -> Response {
        let rect = self.rect(area);
        self.ui.interact(rect, Id::new(("winamp", id)), sense)
    }

    /// A button drawn pressed while the pointer holds it down.
    pub fn button(&mut self, area: Area, normal: Sprite, pressed: Sprite, id: &str) -> Response {
        let response = self
            .interact(area, id, Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let sprite = if response.is_pointer_button_down_on() {
            pressed
        } else {
            normal
        };
        self.sprite(sprite, area);
        response
    }

    /// A slider along an area, its thumb `thumb` skin pixels wide. Reports
    /// the pointer's position as a fraction of the thumb's travel.
    pub fn slider(&mut self, area: Area, id: &str, thumb: u32) -> (Response, SliderEvent) {
        let response = self
            .interact(area, id, Sense::click_and_drag())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let memory = Id::new(("winamp-slider", id));
        let dragging = self.ui.data(|data| data.get_temp::<f32>(memory));
        let travel = (area.width - thumb) as f32;
        let pointer = response.interact_pointer_pos().map(|pos| {
            ((self.skin_x(pos) - area.x as f32 - thumb as f32 / 2.0) / travel).clamp(0.0, 1.0)
        });
        let event = self.slider_event(&response, memory, dragging, pointer);
        (response, event)
    }

    /// The event a slider's drag or click produced this frame, and the
    /// bookkeeping a drag needs across frames.
    fn slider_event(
        &mut self,
        response: &Response,
        memory: Id,
        dragging: Option<f32>,
        pointer: Option<f32>,
    ) -> SliderEvent {
        if (response.drag_started() || response.dragged())
            && let Some(value) = pointer
        {
            self.ui.data_mut(|data| data.insert_temp(memory, value));
            return SliderEvent::Dragging(value);
        }
        if response.drag_stopped() {
            self.ui.data_mut(|data| data.remove::<f32>(memory));
            return dragging
                .or(pointer)
                .map_or(SliderEvent::None, SliderEvent::Committed);
        }
        if response.clicked()
            && let Some(value) = pointer
        {
            return SliderEvent::Committed(value);
        }
        SliderEvent::None
    }
}
