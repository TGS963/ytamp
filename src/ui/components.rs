//! Shared default-interface controls. Hover and focus never change geometry.
use crate::theme::{ColorRole, TextRole, Theme};
use egui::{Button, Response, Ui, WidgetText, vec2};
pub const CONTROL_HEIGHT: f32 = 36.;
pub const SECTION_GAP: f32 = 12.;
pub fn quiet(text: impl Into<WidgetText>) -> Button<'static> {
    Button::new(text.into())
        .frame_when_inactive(false)
        .stroke(egui::Stroke::NONE)
        .corner_radius(8)
        .min_size(vec2(0., 32.))
}
pub fn primary(theme: &dyn Theme, text: impl Into<String>) -> Button<'static> {
    Button::new(
        theme
            .label(TextRole::Body, text)
            .color(theme.color(ColorRole::OnAccent)),
    )
    .fill(theme.color(ColorRole::Accent))
    .stroke(egui::Stroke::NONE)
    .corner_radius(8)
    .min_size(vec2(0., CONTROL_HEIGHT))
}
pub fn secondary(theme: &dyn Theme, text: impl Into<String>) -> Button<'static> {
    Button::new(theme.label(TextRole::Body, text))
        .corner_radius(8)
        .min_size(vec2(0., CONTROL_HEIGHT))
}
pub fn heading(ui: &mut Ui, theme: &dyn Theme, title: &str) {
    ui.add(egui::Label::new(theme.label(TextRole::Heading, title).strong()).truncate())
        .on_hover_text(title);
}
pub fn page_title(ui: &mut Ui, theme: &dyn Theme, title: &str) {
    ui.add(egui::Label::new(theme.label(TextRole::Hero, title).strong()).truncate())
        .on_hover_text(title);
}
/// Reserve the action's width before truncating a long heading.
pub fn section(ui: &mut Ui, theme: &dyn Theme, title: &str, link: &str, enabled: bool) -> Response {
    let text = theme.secondary_label(TextRole::Caption, link);
    let text_width = ui
        .painter()
        .layout_no_wrap(
            link.into(),
            theme.font(TextRole::Caption),
            theme.color(ColorRole::TextSecondary),
        )
        .size()
        .x;
    let action_width = (text_width + 24.).min(ui.available_width());
    let response = ui
        .horizontal(|ui| {
            let title_width =
                (ui.available_width() - action_width - ui.spacing().item_spacing.x).max(0.);
            ui.allocate_ui_with_layout(
                vec2(title_width, CONTROL_HEIGHT),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.set_min_width(title_width);
                    heading(ui, theme, title);
                },
            );
            ui.add_enabled_ui(enabled, |ui| {
                ui.add_sized([action_width, CONTROL_HEIGHT], quiet(text))
            })
            .inner
        })
        .inner;
    ui.add_space(SECTION_GAP);
    response
}
/// Paint a focus outline inside the existing hit area, including custom cards.
pub fn focus_outline(ui: &Ui, theme: &dyn Theme) {
    if let Some(id) = ui.ctx().memory(|m| m.focused())
        && let Some(response) = ui.ctx().read_response(id)
        && response.has_focus()
        && ui.is_rect_visible(response.interact_rect)
    {
        ui.ctx()
            .layer_painter(response.layer_id)
            .with_clip_rect(response.interact_rect)
            .rect_stroke(
                response.rect.shrink(1.),
                6,
                egui::Stroke::new(2., theme.color(ColorRole::TextPrimary)),
                egui::StrokeKind::Inside,
            );
    }
}

/// Custom paint over egui's native input/keyboard/accessibility behavior.
pub fn slider<T: egui::emath::Numeric>(
    ui: &mut Ui,
    value: &mut T,
    range: std::ops::RangeInclusive<T>,
    signed: bool,
    enabled: bool,
) -> Response {
    let min = range.start().to_f64();
    let max = range.end().to_f64();
    let response = ui
        .scope(|ui| {
            ui.set_opacity(0.);
            ui.add_enabled(
                enabled,
                egui::Slider::new(value, range)
                    .show_value(false)
                    .handle_shape(egui::style::HandleShape::Circle),
            )
        })
        .inner;
    let rect = response.rect;
    let inset = rect.height() / 2.5;
    let left = rect.left() + inset;
    let right = rect.right() - inset;
    let x = egui::lerp(
        left..=right,
        ((value.to_f64() - min) / (max - min)).clamp(0., 1.) as f32,
    );
    let y = rect.center().y;
    let painter = ui.painter();
    let rail = egui::Rect::from_min_max(egui::pos2(left, y - 2.), egui::pos2(right, y + 2.));
    painter.rect_filled(rail, 2, ui.visuals().widgets.inactive.bg_fill);
    let start = if signed { (left + right) / 2. } else { left };
    if enabled && (start - x).abs() > 0.01 {
        painter.rect_filled(
            egui::Rect::from_min_max(
                egui::pos2(start.min(x), y - 2.),
                egui::pos2(start.max(x), y + 2.),
            ),
            2,
            ui.visuals().selection.bg_fill,
        );
    }
    if signed {
        painter.line_segment(
            [egui::pos2(start, y - 4.), egui::pos2(start, y + 4.)],
            egui::Stroke::new(1., ui.visuals().weak_text_color()),
        );
    }
    painter.circle_filled(
        egui::pos2(x, y),
        if response.hovered() || response.dragged() {
            6.
        } else {
            5.
        },
        if enabled {
            ui.visuals().text_color()
        } else {
            ui.visuals().weak_text_color()
        },
    );
    response
}
pub fn signed_slider(
    ui: &mut Ui,
    value: &mut f32,
    max: f32,
    label: &str,
    suffix: &str,
) -> Response {
    ui.horizontal(|ui| {
        let mut response = slider(ui, value, -max..=max, true, true);
        ui.label(label);
        if ui
            .add(
                egui::DragValue::new(value)
                    .speed(0.1)
                    .range(-max..=max)
                    .fixed_decimals(1)
                    .suffix(suffix),
            )
            .changed()
        {
            response.mark_changed();
        }
        response
    })
    .inner
}

pub fn play_button(ui: &mut Ui, theme: &dyn Theme, playing: bool, enabled: bool) -> Response {
    let response = ui.add_enabled(
        enabled,
        primary(theme, "")
            .min_size(vec2(44., 44.))
            .corner_radius(22),
    );
    let c = response.rect.center();
    let color = if enabled {
        theme.color(ColorRole::OnAccent)
    } else {
        theme.color(ColorRole::TextSecondary)
    };
    if playing {
        for x in [-5., 5.] {
            ui.painter().rect_filled(
                egui::Rect::from_center_size(c + vec2(x, 0.), vec2(4., 16.)),
                1,
                color,
            );
        }
    } else {
        // Center the triangle's visual mass, independently of font bearings.
        ui.painter().add(egui::Shape::convex_polygon(
            vec![c + vec2(-5., -9.), c + vec2(-5., 9.), c + vec2(10., 0.)],
            color,
            egui::Stroke::NONE,
        ));
    }
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            enabled,
            if playing { "Pause" } else { "Play" },
        )
    });
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::DefaultTheme;
    #[test]
    fn slider_trail_stops_at_solid_thumb_and_keyboard_changes_value() {
        let ctx = egui::Context::default();
        let mut value = 0.5_f64;
        let mut run = |events, focus| {
            let mut response = None;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        vec2(400., 200.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    super::super::apply_page_style(ui, &DefaultTheme);
                    response = Some(slider(ui, &mut value, 0.0..=1.0, false, true));
                    if focus {
                        response.as_ref().unwrap().request_focus();
                    }
                },
            );
            output.textures_delta.clear();
            (output, response.unwrap())
        };
        run(vec![], false);
        let (output, response) = run(vec![], true);
        let thumb = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Circle(c) if c.fill == DefaultTheme.color(ColorRole::TextPrimary) => {
                    Some(c)
                }
                _ => None,
            })
            .unwrap();
        let trail = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill == DefaultTheme.color(ColorRole::Accent) => Some(r),
                _ => None,
            })
            .unwrap();
        assert!((trail.rect.right() - thumb.center.x).abs() < 0.01);
        assert!((thumb.radius - 5.).abs() < 0.01);
        assert!(response.rect.contains(thumb.center));
        let (_, changed) = run(
            vec![egui::Event::Key {
                key: egui::Key::ArrowRight,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Default::default(),
            }],
            false,
        );
        assert!(changed.changed());
        assert!(value > 0.5);
    }
    #[test]
    fn section_reserves_action_space_and_keyboard_activation_keeps_layout() {
        for width in [260., 494., 850., 1460.] {
            let ctx = egui::Context::default();
            let frame = |events, focus, enabled| {
                let mut result = None;
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(width, 300.),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        super::super::apply_page_style(ui, &DefaultTheme);
                        let available = ui.available_rect_before_wrap();
                        let response = section(
                            ui,
                            &DefaultTheme,
                            "A very long collection heading that needs to make room for its action",
                            "View library →",
                            enabled,
                        );
                        if focus {
                            response.request_focus();
                        }
                        focus_outline(ui, &DefaultTheme);
                        let below = ui.label("Content").rect;
                        result = Some((response.rect, below, response.clicked(), available));
                    },
                );
                output.textures_delta.clear();
                result.unwrap()
            };
            frame(vec![], false, true);
            let (button, below, _, available) = frame(vec![], false, true);
            assert!(
                available.contains_rect(button),
                "action escaped at width {width}"
            );
            assert!(
                (button.right() - available.right()).abs() < 1.,
                "action must align to right edge"
            );
            let hovered = frame(
                vec![egui::Event::PointerMoved(button.center())],
                false,
                true,
            );
            assert_eq!((hovered.0, hovered.1), (button, below));
            frame(vec![egui::Event::PointerGone], true, true);
            let activated = frame(
                vec![egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                }],
                false,
                true,
            );
            assert!(activated.2, "Enter must activate focused action");
            assert_eq!((activated.0, activated.1), (button, below));
            let disabled = frame(vec![], false, false);
            assert_eq!((disabled.0, disabled.1), (button, below));
        }
    }
}
