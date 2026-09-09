//! The search page: one query box, results grouped by type.

use crate::core::action::Action;
use crate::core::model::{Album, Artist, SearchResults};
use crate::core::state::{Loadable, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::rows::{self, RowContext};

/// How many song rows show before the songs panel scrolls internally.
/// Caps the panel so albums and artists, drawn below it, stay reachable.
const SONGS_VISIBLE_ROWS: f32 = 10.0;

pub fn view(
    ui: &mut egui::Ui,
    state: &State,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    super::components::page_title(ui, theme, "Search");
    ui.label(theme.secondary_label(
        TextRole::Body,
        "Songs, artists, albums, and playlists from YouTube Music.",
    ));
    ui.add_space(16.);
    query_box(ui, state, out);
    ui.add_space(theme.metric(MetricRole::GapLarge));
    match &state.search.results {
        Loadable::NotAsked => {
            ui.label(theme.secondary_label(
                TextRole::Body,
                "Search for songs, albums, artists, and playlists.",
            ));
        }
        Loadable::Loading => {
            ui.spinner();
        }
        Loadable::Failed(message) => {
            ui.colored_label(theme.color(ColorRole::Danger), message);
        }
        Loadable::Loaded(results) | Loadable::Refreshing(results) => {
            results_view(ui, results, theme, context, out)
        }
    }
}

fn query_box(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    let mut input = state.search.input.clone();
    let edit = egui::TextEdit::singleline(&mut input)
        .hint_text("What do you want to listen to?")
        .margin(egui::vec2(12., 10.))
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
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    egui::ScrollArea::vertical()
        .id_salt("search_results")
        .show(ui, |ui| {
            songs_section(ui, results, theme, context, out);
            albums_section(ui, results, theme, out);
            artists_section(ui, results, theme, out);
            playlists_section(ui, results, theme, out);
        });
}

fn songs_section(
    ui: &mut egui::Ui,
    results: &SearchResults,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    if results.songs.is_empty() {
        return;
    }
    ui.label(theme.label(TextRole::Heading, "Songs"));
    let max_height = theme.metric(MetricRole::RowHeight) * SONGS_VISIBLE_ROWS;
    rows::track_list_capped(
        ui,
        "search_songs",
        &results.songs,
        max_height,
        theme,
        context,
        out,
    );
}

fn albums_section(
    ui: &mut egui::Ui,
    results: &SearchResults,
    theme: &dyn Theme,
    out: &mut Vec<Action>,
) {
    if results.albums.is_empty() {
        return;
    }
    ui.add_space(theme.metric(MetricRole::GapLarge));
    ui.label(theme.label(TextRole::Heading, "Albums"));
    for album in &results.albums {
        album_row(ui, album, theme, out);
    }
}

fn album_row(ui: &mut egui::Ui, album: &Album, theme: &dyn Theme, out: &mut Vec<Action>) {
    let response = rows::row_frame(ui, theme, |ui| {
        let art_size = theme.metric(MetricRole::RowArtSize);
        rows::artwork(ui, theme, album.thumbnail_url.as_deref(), art_size);
        ui.label(theme.label(TextRole::Body, &album.title));
        ui.label(theme.secondary_label(TextRole::Caption, album.artists.join(", ")));
    });
    if response.clicked() {
        out.push(Action::AlbumOpened(album.id.clone()));
    }
}

fn artists_section(
    ui: &mut egui::Ui,
    results: &SearchResults,
    theme: &dyn Theme,
    out: &mut Vec<Action>,
) {
    if results.artists.is_empty() {
        return;
    }
    ui.add_space(theme.metric(MetricRole::GapLarge));
    ui.label(theme.label(TextRole::Heading, "Artists"));
    for artist in &results.artists {
        artist_row(ui, artist, theme, out);
    }
}

fn artist_row(ui: &mut egui::Ui, artist: &Artist, theme: &dyn Theme, out: &mut Vec<Action>) {
    let response = rows::row_frame(ui, theme, |ui| {
        let art_size = theme.metric(MetricRole::RowArtSize);
        rows::artwork(ui, theme, artist.thumbnail_url.as_deref(), art_size);
        ui.label(theme.label(TextRole::Body, &artist.name));
    });
    if response.clicked() {
        out.push(Action::ArtistOpened(artist.id.clone()));
    }
}

fn playlists_section(
    ui: &mut egui::Ui,
    results: &SearchResults,
    theme: &dyn Theme,
    out: &mut Vec<Action>,
) {
    if results.playlists.is_empty() {
        return;
    }
    ui.add_space(theme.metric(MetricRole::GapLarge));
    ui.label(theme.label(TextRole::Heading, "Playlists"));
    for playlist in &results.playlists {
        let response = rows::row_frame(ui, theme, |ui| {
            rows::artwork(
                ui,
                theme,
                playlist.thumbnail_url.as_deref(),
                theme.metric(MetricRole::RowArtSize),
            );
            ui.add(egui::Label::new(theme.label(TextRole::Body, &playlist.title)).truncate());
        });
        if response.clicked() {
            out.push(Action::PlaylistOpened(playlist.id.clone()));
        }
    }
}
