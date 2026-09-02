//! An album's browse page: header with a Play button, then its tracks.

use crate::core::action::Action;
use crate::core::model::AlbumPage;
use crate::core::state::{Loadable, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::rows::{self, RowContext};

pub fn view(
    ui: &mut egui::Ui,
    state: &State,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    match &state.browse.album {
        Loadable::NotAsked | Loadable::Loading => {
            ui.spinner();
        }
        Loadable::Failed(message) => {
            ui.colored_label(theme.color(ColorRole::Danger), message);
        }
        Loadable::Loaded(page) | Loadable::Refreshing(page) => {
            page_view(ui, page, theme, context, out)
        }
    }
}

fn page_view(
    ui: &mut egui::Ui,
    page: &AlbumPage,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    header(ui, page, theme, out);
    ui.add_space(theme.metric(MetricRole::GapLarge));
    rows::track_list(ui, "album_tracks", &page.tracks, false, theme, context, out);
}

fn header(ui: &mut egui::Ui, page: &AlbumPage, theme: &dyn Theme, out: &mut Vec<Action>) {
    let art_size = theme.metric(MetricRole::PlayerArtSize);
    ui.horizontal(|ui| {
        rows::artwork(ui, theme, page.album.thumbnail_url.as_deref(), art_size);
        ui.vertical(|ui| {
            ui.label(theme.label(TextRole::Title, &page.album.title));
            ui.label(theme.secondary_label(TextRole::Caption, album_subtitle(page)));
            if ui.button("Play").clicked() {
                out.push(Action::ContextPlayed {
                    tracks: page.tracks.clone(),
                    start: 0,
                });
            }
        });
    });
}

/// The artists and, when known, the year: "Artist A, Artist B · 2019".
fn album_subtitle(page: &AlbumPage) -> String {
    let artists = page.album.artists.join(", ");
    match &page.album.year {
        Some(year) => format!("{artists} · {year}"),
        None => artists,
    }
}
