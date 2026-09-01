//! The library page: your playlists and your liked songs.

use crate::core::action::Action;
use crate::core::model::Playlist;
use crate::core::state::{Loadable, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::rows;

pub fn view(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    ui.label(theme.label(TextRole::Title, "Library"));
    ui.add_space(theme.metric(MetricRole::GapLarge));
    egui::ScrollArea::vertical().show(ui, |ui| {
        playlists_section(ui, state, theme, out);
        ui.add_space(theme.metric(MetricRole::GapLarge));
        liked_section(ui, state, theme, out);
    });
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
        Loadable::Loaded(playlists) => {
            for playlist in playlists {
                playlist_row(ui, playlist, theme, out);
            }
        }
    }
}

fn playlist_row(ui: &mut egui::Ui, playlist: &Playlist, theme: &dyn Theme, out: &mut Vec<Action>) {
    let text = match playlist.track_count {
        Some(count) => format!("{} · {} tracks", playlist.title, count),
        None => playlist.title.clone(),
    };
    let label = egui::Label::new(theme.label(TextRole::Body, text)).sense(egui::Sense::click());
    if ui.add(label).clicked() {
        out.push(Action::PlaylistOpened(playlist.id.clone()));
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
        Loadable::Loaded(tracks) => rows::track_list(ui, tracks, theme, out),
    }
}
