//! An open playlist page: its tracks, playable from any row.

use crate::core::action::Action;
use crate::core::state::{Loadable, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::rows;

pub fn view(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    ui.label(theme.label(TextRole::Title, "Playlist"));
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
