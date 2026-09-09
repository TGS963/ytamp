use crate::theme::{ColorRole, Theme};
use egui::Ui;

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
