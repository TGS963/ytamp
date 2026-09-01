//! The search page: one query box, results grouped by type.

use crate::core::action::Action;
use crate::core::model::SearchResults;
use crate::core::state::{Loadable, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::rows;

pub fn view(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    query_box(ui, state, out);
    ui.add_space(theme.metric(MetricRole::GapLarge));
    match &state.search.results {
        Loadable::NotAsked => {
            ui.label(theme.secondary_label(TextRole::Body, "Search for songs, albums, artists."));
        }
        Loadable::Loading => {
            ui.spinner();
        }
        Loadable::Failed(message) => {
            ui.colored_label(theme.color(ColorRole::Danger), message);
        }
        Loadable::Loaded(results) => results_view(ui, results, theme, out),
    }
}

fn query_box(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    let mut input = state.search.input.clone();
    let edit = egui::TextEdit::singleline(&mut input)
        .hint_text("Search YouTube Music")
        .desired_width(f32::INFINITY);
    let response = ui.add(edit);
    if response.changed() {
        out.push(Action::SearchInputChanged(input));
    }
    if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        out.push(Action::SearchSubmitted);
    }
}

fn results_view(
    ui: &mut egui::Ui,
    results: &SearchResults,
    theme: &dyn Theme,
    out: &mut Vec<Action>,
) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        if !results.songs.is_empty() {
            ui.label(theme.label(TextRole::Heading, "Songs"));
            rows::track_list(ui, &results.songs, theme, out);
        }
        albums_section(ui, results, theme);
        artists_section(ui, results, theme);
    });
}

fn albums_section(ui: &mut egui::Ui, results: &SearchResults, theme: &dyn Theme) {
    if results.albums.is_empty() {
        return;
    }
    ui.add_space(theme.metric(MetricRole::GapLarge));
    ui.label(theme.label(TextRole::Heading, "Albums"));
    for album in &results.albums {
        let line = format!("{} — {}", album.title, album.artists.join(", "));
        ui.label(theme.secondary_label(TextRole::Body, line));
    }
}

fn artists_section(ui: &mut egui::Ui, results: &SearchResults, theme: &dyn Theme) {
    if results.artists.is_empty() {
        return;
    }
    ui.add_space(theme.metric(MetricRole::GapLarge));
    ui.label(theme.label(TextRole::Heading, "Artists"));
    for artist in &results.artists {
        ui.label(theme.secondary_label(TextRole::Body, &artist.name));
    }
}
