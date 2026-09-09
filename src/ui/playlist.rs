//! An open playlist page: its tracks, playable from any row.

use crate::core::action::Action;
use crate::core::model::Playlist;
use crate::core::state::{Loadable, Page, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::rows::{self, RowContext};

pub fn view(
    ui: &mut egui::Ui,
    state: &State,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    header(ui, state, theme);
    ui.add_space(theme.metric(MetricRole::GapLarge));
    match &state.library.open_playlist {
        Loadable::NotAsked | Loadable::Loading => {
            ui.spinner();
        }
        Loadable::Failed(message) => {
            ui.colored_label(theme.color(ColorRole::Danger), message);
        }
        Loadable::Loaded(tracks) | Loadable::Refreshing(tracks) => {
            rows::track_list(
                ui,
                "playlist_tracks",
                tracks,
                state.library.open_playlist_loading_more,
                theme,
                context,
                out,
            );
        }
    }
}

/// The page title, alongside the open playlist's art when its metadata
/// has already reached the library list.
fn header(ui: &mut egui::Ui, state: &State, theme: &dyn Theme) {
    let art_size = theme.metric(MetricRole::PlayerArtSize);
    let thumbnail_url = open_playlist_metadata(state).and_then(|p| p.thumbnail_url.as_deref());
    ui.horizontal(|ui| {
        rows::artwork(ui, theme, thumbnail_url, art_size);
        let metadata = open_playlist_metadata(state);
        ui.vertical(|ui| {
            ui.add(
                egui::Label::new(theme.label(
                    TextRole::Hero,
                    metadata.map(|p| p.title.as_str()).unwrap_or("Playlist"),
                ))
                .truncate(),
            );
            if let Some(count) = metadata.and_then(|p| p.track_count) {
                ui.label(theme.secondary_label(TextRole::Caption, format!("{count} tracks")));
            }
        });
    });
}

/// The library entry for the playlist the page currently shows, if the
/// library list has loaded it. Holds the title and art the tracks
/// endpoint does not carry.
fn open_playlist_metadata(state: &State) -> Option<&Playlist> {
    let Page::Playlist(id) = &state.page else {
        return None;
    };
    state
        .library
        .playlists
        .loaded()
        .into_iter()
        .flatten()
        .chain(
            state
                .search
                .results
                .loaded()
                .into_iter()
                .flat_map(|r| &r.playlists),
        )
        .find(|playlist| &playlist.id == id)
}
