//! The queue side panel: the current track and everything after it.

use egui::Ui;

use crate::core::action::Action;
use crate::core::model::Track;
use crate::core::state::State;
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::panel_frame;
use super::queue_edit::Editor;

pub fn view(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let frame = panel_frame(ui, theme, ColorRole::PanelBackground);
    egui::Panel::right("queue")
        .exact_size(theme.metric(MetricRole::SidebarWidth) * 1.4)
        .frame(frame)
        .show(ui, |ui| {
            ui.add_space(theme.metric(MetricRole::PagePadding));
            ui.label(theme.label(TextRole::Heading, "Queue"));
            ui.add_space(theme.metric(MetricRole::GapSmall));
            now_playing_entry(ui, state, theme);
            if ui.button("Clear queued tracks").clicked() {
                out.push(Action::QueueCleared);
            }
            upcoming_entries(ui, state, theme, out);
        });
}

fn now_playing_entry(ui: &mut Ui, state: &State, theme: &dyn Theme) {
    let Some(track) = state.playback.queue.current() else {
        ui.label(theme.secondary_label(TextRole::Body, "The queue is empty."));
        return;
    };
    ui.label(theme.label(TextRole::Body, format!("▶ {}", track.title)));
}

pub(super) fn upcoming_entries(
    ui: &mut Ui,
    state: &State,
    theme: &dyn Theme,
    out: &mut Vec<Action>,
) {
    let upcoming: Vec<&Track> = state.playback.queue.upcoming().collect();
    let mut editor = Editor::load(ui.ctx(), &state.playback.queue);
    let row_height = theme.metric(MetricRole::RowHeight);
    ui.label(theme.secondary_label(
        TextRole::Body,
        "Select · double-click to play · drag to move",
    ));
    egui::ScrollArea::vertical()
        .id_salt("queue_upcoming")
        .show_rows(ui, row_height, upcoming.len(), |ui, range| {
            for index in range {
                ui.push_id(index, |ui| {
                    let label = theme.secondary_label(TextRole::Body, &upcoming[index].title);
                    let response = ui.add_sized(
                        [ui.available_width(), row_height],
                        egui::Button::new(label)
                            .right_text("")
                            .frame_when_inactive(false)
                            .selected(editor.selected.contains(&index))
                            .sense(egui::Sense::click_and_drag())
                            .truncate(),
                    );
                    editor.row(ui, &response, index, out);
                    response.context_menu(|ui| editor.menu(ui, index, out));
                });
            }
        });
    editor.keyboard(ui, upcoming.len(), out);
    editor.store(ui.ctx());
}
