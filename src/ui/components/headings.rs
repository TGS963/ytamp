use crate::theme::{ColorRole, TextRole, Theme};
use crate::ui::components::{CONTROL_HEIGHT, SECTION_GAP, quiet};
use egui::{Response, Ui, vec2};

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
