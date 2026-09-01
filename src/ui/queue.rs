//! The queue side panel: the current track and everything after it.

use egui::Ui;

use crate::core::state::State;
use crate::theme::{MetricRole, TextRole, Theme};

pub fn view(ui: &mut Ui, state: &State, theme: &dyn Theme) {
    egui::Panel::right("queue")
        .exact_size(theme.metric(MetricRole::SidebarWidth) * 1.4)
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
    egui::ScrollArea::vertical().show(ui, |ui| {
        for track in state.playback.queue.upcoming() {
            ui.label(theme.secondary_label(TextRole::Body, &track.title));
        }
    });
}
