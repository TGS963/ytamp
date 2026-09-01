//! The library page: your playlists and your liked songs.

use crate::core::action::Action;
use crate::core::model::Playlist;
use crate::core::state::{Loadable, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::rows;

/// How many playlist rows show before the playlists panel scrolls
/// internally. Caps the panel so liked songs, drawn below it, still gets
/// room on the page.
const PLAYLISTS_VISIBLE_ROWS: f32 = 6.0;

pub fn view(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    ui.label(theme.label(TextRole::Title, "Library"));
    ui.add_space(theme.metric(MetricRole::GapLarge));
    playlists_section(ui, state, theme, out);
    ui.add_space(theme.metric(MetricRole::GapLarge));
    liked_section(ui, state, theme, out);
}

fn playlists_section(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    ui.label(theme.label(TextRole::Heading, "Playlists"));
    match &state.library.playlists {
        Loadable::NotAsked | Loadable::Loading => {
            ui.spinner();
        }
        Loadable::Failed(message) => {
            ui.colored_label(theme.color(ColorRole::Danger), message);
        }
        Loadable::Loaded(playlists) | Loadable::Refreshing(playlists) if playlists.is_empty() => {
            ui.label(theme.secondary_label(TextRole::Body, "No playlists in your library yet."));
        }
        Loadable::Loaded(playlists) | Loadable::Refreshing(playlists) => {
            playlists_list(ui, playlists, theme, out)
        }
    }
}

fn playlists_list(
    ui: &mut egui::Ui,
    playlists: &[Playlist],
    theme: &dyn Theme,
    out: &mut Vec<Action>,
) {
    let row_height = theme.metric(MetricRole::RowHeight);
    egui::ScrollArea::vertical()
        .id_salt("library_playlists")
        .max_height(row_height * PLAYLISTS_VISIBLE_ROWS)
        .show_rows(ui, row_height, playlists.len(), |ui, row_range| {
            for index in row_range {
                playlist_row(ui, &playlists[index], theme, out);
            }
        });
}

fn playlist_row(ui: &mut egui::Ui, playlist: &Playlist, theme: &dyn Theme, out: &mut Vec<Action>) {
    let response = rows::row_frame(ui, theme, |ui| {
        ui.label(theme.label(TextRole::Body, playlist_label(playlist)));
    });
    if response.clicked() {
        out.push(Action::PlaylistOpened(playlist.id.clone()));
    }
}

fn playlist_label(playlist: &Playlist) -> String {
    match playlist.track_count {
        Some(count) => format!("{} · {} tracks", playlist.title, count),
        None => playlist.title.clone(),
    }
}

fn liked_section(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    ui.label(theme.label(TextRole::Heading, "Liked songs"));
    match &state.library.liked {
        Loadable::NotAsked | Loadable::Loading => {
            ui.spinner();
        }
        Loadable::Failed(message) => {
            ui.colored_label(theme.color(ColorRole::Danger), message);
        }
        Loadable::Loaded(tracks) | Loadable::Refreshing(tracks) if tracks.is_empty() => {
            ui.label(theme.secondary_label(TextRole::Body, "No liked songs yet."));
        }
        Loadable::Loaded(tracks) | Loadable::Refreshing(tracks) => {
            rows::track_list(ui, "library_liked", tracks, theme, out)
        }
    }
}
