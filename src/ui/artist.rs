//! An artist's browse page: header, top songs, then albums and singles.

use crate::core::action::Action;
use crate::core::model::{Album, ArtistPage};
use crate::core::state::{Loadable, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::rows::{self, RowContext};

/// How many top-song rows show before the songs panel scrolls
/// internally. Caps the panel so the album and single rows below it
/// stay reachable.
const TOP_SONGS_VISIBLE_ROWS: f32 = 8.0;

pub fn view(
    ui: &mut egui::Ui,
    state: &State,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    match &state.browse.artist {
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
    page: &ArtistPage,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    egui::ScrollArea::vertical()
        .id_salt("artist_page")
        .show(ui, |ui| {
            header(ui, page, theme);
            ui.add_space(theme.metric(MetricRole::GapLarge));
            top_songs_section(ui, page, theme, context, out);
            albums_section(ui, "Albums", &page.albums, theme, out);
            albums_section(ui, "Singles", &page.singles, theme, out);
        });
}

fn header(ui: &mut egui::Ui, page: &ArtistPage, theme: &dyn Theme) {
    let art_size = theme.metric(MetricRole::PlayerArtSize);
    ui.horizontal(|ui| {
        rows::artwork(ui, theme, page.thumbnail_url.as_deref(), art_size);
        super::components::page_title(ui, theme, &page.name);
    });
}

fn top_songs_section(
    ui: &mut egui::Ui,
    page: &ArtistPage,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    if page.top_songs.is_empty() {
        return;
    }
    super::components::heading(ui, theme, "Top songs");
    let max_height = theme.metric(MetricRole::RowHeight) * TOP_SONGS_VISIBLE_ROWS;
    rows::track_list_capped(
        ui,
        "artist_top_songs",
        &page.top_songs,
        max_height,
        theme,
        context,
        out,
    );
    ui.add_space(theme.metric(MetricRole::GapLarge));
}

/// One heading, "Albums" or "Singles", with its rows below. Draws
/// nothing when `albums` is empty, so an artist with no singles shows
/// no empty heading.
fn albums_section(
    ui: &mut egui::Ui,
    heading: &str,
    albums: &[Album],
    theme: &dyn Theme,
    out: &mut Vec<Action>,
) {
    if albums.is_empty() {
        return;
    }
    ui.label(theme.label(TextRole::Heading, heading));
    for album in albums {
        album_row(ui, album, theme, out);
    }
    ui.add_space(theme.metric(MetricRole::GapLarge));
}

fn album_row(ui: &mut egui::Ui, album: &Album, theme: &dyn Theme, out: &mut Vec<Action>) {
    let response = rows::row_frame(ui, theme, |ui| {
        let art_size = theme.metric(MetricRole::RowArtSize);
        rows::artwork(ui, theme, album.thumbnail_url.as_deref(), art_size);
        ui.label(theme.label(TextRole::Body, &album.title));
        if let Some(year) = &album.year {
            ui.label(theme.secondary_label(TextRole::Caption, year));
        }
    });
    if response.clicked() {
        out.push(Action::AlbumOpened(album.id.clone()));
    }
}
