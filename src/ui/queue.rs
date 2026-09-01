//! The queue side panel: the current track and everything after it.

use egui::Ui;

use crate::core::model::Track;
use crate::core::state::State;
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::panel_frame;
use super::rows;

pub fn view(ui: &mut Ui, state: &State, theme: &dyn Theme) {
    let frame = panel_frame(ui, theme, ColorRole::PanelBackground);
    egui::Panel::right("queue")
        .exact_size(theme.metric(MetricRole::SidebarWidth) * 1.4)
        .frame(frame)
        .show(ui, |ui| {
            ui.add_space(theme.metric(MetricRole::PagePadding));
            ui.label(theme.label(TextRole::Heading, "Queue"));
            ui.add_space(theme.metric(MetricRole::GapSmall));
            now_playing_entry(ui, state, theme);
            upcoming_entries(ui, state, theme);
        });
}

fn now_playing_entry(ui: &mut Ui, state: &State, theme: &dyn Theme) {
    let Some(track) = state.playback.queue.current() else {
        ui.label(theme.secondary_label(TextRole::Body, "The queue is empty."));
        return;
    };
    ui.label(theme.label(TextRole::Body, format!("▶ {}", track.title)));
}

fn upcoming_entries(ui: &mut Ui, state: &State, theme: &dyn Theme) {
    let upcoming: Vec<&Track> = state.playback.queue.upcoming().collect();
    let row_height = theme.metric(MetricRole::RowHeight);
    egui::ScrollArea::vertical()
        .id_salt("queue_upcoming")
        .show_rows(ui, row_height, upcoming.len(), |ui, row_range| {
            for index in row_range {
                rows::row_frame(ui, theme, |ui| {
                    ui.label(theme.secondary_label(TextRole::Body, &upcoming[index].title));
                });
            }
        });
}
