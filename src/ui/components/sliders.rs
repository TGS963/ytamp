use egui::{Response, Ui};

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
