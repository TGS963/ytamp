//! Ported from fastpotify (MIT, Copyright (c) 2026 Carmine Paolino), src/ui/winamp/mod.rs
//! (the `View` blitter, lines 120-302).
//!
//! Draws a skin's sprites into an egui `Ui` and reads the pointer back
//! against the skin's layout. One `View` covers one window's worth of
//! pixels, at one scale.

use std::collections::HashMap;

use egui::{Color32, Id, Pos2, Rect, Response, Sense, TextureId, Ui, pos2, vec2};

use crate::skin::layout::Area;
use crate::skin::{Mask, Sheet, Skin, Sprite, font};

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
    pub mask: Option<&'a Mask>,
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
        let mut mesh = egui::Mesh::with_texture(texture);
        self.visible_rects(Area::new(x, y, clipped.width, clipped.height), |area| {
            let sx = clipped.x + area.x - x;
            let sy = clipped.y + area.y - y;
            let uv = Rect::from_min_max(
                pos2(
                    sx as f32 / bitmap.width as f32,
                    sy as f32 / bitmap.height as f32,
                ),
                pos2(
                    (sx + area.width) as f32 / bitmap.width as f32,
                    (sy + area.height) as f32 / bitmap.height as f32,
                ),
            );
            mesh.add_rect_with_uv(self.rect(area), uv, Color32::WHITE);
        });
        painter.add(egui::Shape::mesh(mesh));
    }

    fn visible_rects(&self, area: Area, mut visit: impl FnMut(Area)) {
        if let Some(mask) = self.mask {
            mask.visit_rects(area, visit);
        } else {
            visit(area);
        }
    }

    pub fn sprite_at(&self, sprite: Sprite, x: u32, y: u32) {
        self.paint(self.ui.painter(), sprite, x, y);
    }

    pub fn sprite(&self, sprite: Sprite, area: Area) {
        self.sprite_at(sprite, area.x, area.y);
    }

    /// A sprite, clipped to `clip`'s screen rectangle. Used to tile a
    /// repeating sprite across an area whose far edge cuts a tile
    /// short, as the playlist window's frame does.
    pub fn sprite_clipped(&self, sprite: Sprite, x: u32, y: u32, clip: Area) {
        let clip_rect = self.rect(clip).intersect(self.ui.clip_rect());
        let painter = self.ui.painter().with_clip_rect(clip_rect);
        self.paint(&painter, sprite, x, y);
    }

    /// A block of skin pixels in one flat colour.
    pub fn fill(&self, x: u32, y: u32, width: u32, height: u32, color: Color32) {
        let mut mesh = egui::Mesh::default();
        self.visible_rects(Area::new(x, y, width, height), |area| {
            mesh.add_colored_rect(self.rect(area), color);
        });
        self.ui.painter().add(egui::Shape::mesh(mesh));
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
        let id = Id::new(("winamp", id));
        let outside = self.mask.is_some_and(|mask| {
            self.ui.ctx().pointer_hover_pos().is_some_and(|pos| {
                let local = (pos - self.origin) / self.unit;
                local.x < 0.
                    || local.y < 0.
                    || !mask.contains(local.x.floor() as u32, local.y.floor() as u32)
            })
        });
        let rect = if outside && !self.ui.ctx().is_being_dragged(id) {
            Rect::NOTHING
        } else {
            self.rect(area)
        };
        self.ui.interact(rect, id, sense)
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

    /// A button whose unpressed look is already baked into the
    /// background it sits on: this only draws `pressed` while held,
    /// the way the playlist window's close and shade buttons work.
    pub fn lamp_button(&mut self, area: Area, pressed: Sprite, id: &str) -> Response {
        let response = self
            .interact(area, id, Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        if response.is_pointer_button_down_on() {
            self.sprite(pressed, area);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mask_rectangles_preserve_every_visible_pixel_without_overlap() {
        let mask = Mask::from_polygons(
            40,
            20,
            &[
                vec![(0, 0), (20, 0), (0, 20)],
                vec![(25, 2), (40, 2), (40, 18), (25, 18)],
            ],
        );
        let area = Area::new(3, 4, 30, 12);
        let mut coverage = [0u8; 800];
        mask.visit_rects(area, |rect| {
            for y in rect.y..rect.y + rect.height {
                for x in rect.x..rect.x + rect.width {
                    coverage[(y * 40 + x) as usize] += 1;
                }
            }
        });
        for y in 0..20 {
            for x in 0..40 {
                assert_eq!(
                    coverage[(y * 40 + x) as usize],
                    u8::from(area.contains(x, y) && mask.contains(x, y)),
                    "pixel {x},{y}"
                );
            }
        }
    }
    #[test]
    fn transparent_part_of_a_control_does_not_accept_clicks() {
        let ctx = egui::Context::default();
        let mask = Mask::from_polygons(20, 20, &[vec![(0, 0), (5, 0), (5, 20), (0, 20)]]);
        let skin = Skin::builtin();
        let textures = HashMap::new();
        let frame = |events| {
            let mut clicked = false;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(100., 100.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let mut view = View {
                        ui,
                        origin: Pos2::ZERO,
                        unit: 1.,
                        skin: &skin,
                        mask: Some(&mask),
                        textures: &textures,
                    };
                    clicked = view
                        .interact(Area::new(0, 0, 20, 20), "masked", Sense::click())
                        .clicked();
                },
            );
            output.textures_delta.clear();
            clicked
        };
        frame(vec![]);
        for (x, expected) in [(12., false), (2., true)] {
            let pos = pos2(x, 10.);
            frame(vec![egui::Event::PointerMoved(pos)]);
            frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }]);
            let clicked = frame(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }]);
            assert_eq!(clicked, expected);
        }
    }
}
