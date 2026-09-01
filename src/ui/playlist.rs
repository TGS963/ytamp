//! An open playlist page: its tracks, playable from any row.

use crate::core::action::Action;
use crate::core::model::Playlist;
use crate::core::state::{Loadable, Page, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::rows;

pub fn view(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
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
            rows::track_list(ui, "playlist_tracks", tracks, theme, out);
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
        ui.label(theme.label(TextRole::Title, "Playlist"));
    });
}

/// The library entry for the playlist the page currently shows, if the
/// library list has loaded it. Holds the title and art the tracks
/// endpoint does not carry.
fn open_playlist_metadata(state: &State) -> Option<&Playlist> {
    let Page::Playlist(id) = &state.page else {
        return None;
    };
    let playlists = state.library.playlists.loaded()?;
    playlists.iter().find(|playlist| &playlist.id == id)
}
