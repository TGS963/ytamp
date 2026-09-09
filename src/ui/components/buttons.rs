use crate::theme::{ColorRole, TextRole, Theme};
use crate::ui::components::CONTROL_HEIGHT;
use egui::{Button, Response, Ui, WidgetText, vec2};

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
