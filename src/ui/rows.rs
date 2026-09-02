//! Shared list rows: the one way every view draws a clickable track list.
//!
//! `egui::ScrollArea::show_rows` needs one uniform row height. Every row in
//! every list goes through `row_frame`, so the height stays uniform and the
//! scroll math stays correct.

use std::time::Duration;

use crate::core::action::Action;
use crate::core::model::{ArtistRef, Playlist, Track};
use crate::core::state::{Loadable, Page};
use crate::core::update::is_liked;
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};
use crate::thumbnails::sized;

/// What a track row's context menu needs, gathered once per list
/// rather than rebuilt for every row: whether a track is liked, the
/// playlists to offer under "Add to playlist", and the page the row
/// is on, which decides whether "Remove from this playlist" shows.
pub struct RowContext<'a> {
    pub liked: &'a Loadable<Vec<Track>>,
    pub playlists: &'a [Playlist],
    pub page: &'a Page,
}

/// Draws `tracks` as a virtualized, scrollable list that fills the space
/// its caller gives it. Only the rows in view get laid out each frame. A
/// click on a row plays the whole list from that row. The queue button
/// appends one track to the user queue.
///
/// `id_salt` must be unique among the scroll areas on the same page.
/// While `loading_more` is true, a spinner row follows the last track
/// inside the scroll area, so it stays visible on a list that fills
/// the page.
pub fn track_list(
    ui: &mut egui::Ui,
    id_salt: &str,
    tracks: &[Track],
    loading_more: bool,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    track_list_area(ui, id_salt, tracks, loading_more, None, theme, context, out);
}

/// Draws `tracks` the same way as [`track_list`], but caps the visible
/// height so sibling content below it on the page stays reachable.
pub fn track_list_capped(
    ui: &mut egui::Ui,
    id_salt: &str,
    tracks: &[Track],
    max_height: f32,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    track_list_area(
        ui,
        id_salt,
        tracks,
        false,
        Some(max_height),
        theme,
        context,
        out,
    );
}

#[allow(clippy::too_many_arguments)]
fn track_list_area(
    ui: &mut egui::Ui,
    id_salt: &str,
    tracks: &[Track],
    loading_more: bool,
    max_height: Option<f32>,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    let row_height = theme.metric(MetricRole::RowHeight);
    let mut area = egui::ScrollArea::vertical().id_salt(id_salt);
    if let Some(height) = max_height {
        area = area.max_height(height);
    }
    let row_count = tracks.len() + usize::from(loading_more);
    area.show_rows(ui, row_height, row_count, |ui, row_range| {
        for index in row_range {
            match tracks.get(index) {
                Some(track) => out.extend(track_row(ui, track, index, tracks, theme, context)),
                None => loading_more_row(ui, theme),
            }
        }
    });
}

/// Draws one square of album art, `size` pixels on a side, with rounded
/// corners from `MetricRole::CornerRadius`.
///
/// A neutral placeholder square, in `ColorRole::ArtPlaceholder`, always
/// paints first. When `thumbnail_url` is `Some`, the real image paints
/// over the placeholder once egui's loader has it in cache; until then,
/// or when the url is `None`, the placeholder alone shows. The square
/// claims its space either way, so no row ever shifts.
pub fn artwork(ui: &mut egui::Ui, theme: &dyn Theme, thumbnail_url: Option<&str>, size: f32) {
    let (rect, _response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let radius = theme.metric(MetricRole::CornerRadius);
    ui.painter()
        .rect_filled(rect, radius, theme.color(ColorRole::ArtPlaceholder));
    if let Some(url) = thumbnail_url {
        let target_px = (size * ui.ctx().pixels_per_point() * 1.5).ceil() as u32;
        egui::Image::from_uri(sized(url, target_px))
            .corner_radius(radius)
            .show_loading_spinner(false)
            .paint_at(ui, rect);
    }
}

/// Draws one uniform-height row: a hover-highlighted band around centered
/// content, `RowHeight` tall. Every row-shaped widget in the app goes
/// through this, so lists can virtualize on a single row height.
///
/// The content draws in a child that does not allocate in the parent.
/// A `scope` would allocate its rect a second time and move the cursor
/// back, so each row would advance less than `RowHeight` and the
/// `show_rows` math would drift.
pub fn row_frame(
    ui: &mut egui::Ui,
    theme: &dyn Theme,
    content: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let height = theme.metric(MetricRole::RowHeight);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    if response.hovered() {
        let radius = theme.metric(MetricRole::CornerRadius);
        ui.painter()
            .rect_filled(rect, radius, theme.color(ColorRole::RowHover));
    }
    let builder = egui::UiBuilder::new().max_rect(rect.shrink(6.0));
    ui.new_child(builder).horizontal_centered(content);
    response
}

/// A `RowHeight`-tall row with a centered spinner: the last row of a
/// list that still receives pages from the network.
fn loading_more_row(ui: &mut egui::Ui, theme: &dyn Theme) {
    row_frame(ui, theme, |ui| {
        ui.spinner();
    });
}

fn track_row(
    ui: &mut egui::Ui,
    track: &Track,
    index: usize,
    all: &[Track],
    theme: &dyn Theme,
    context: &RowContext,
) -> Vec<Action> {
    let mut button_action = None;
    let response = row_frame(ui, theme, |ui| {
        button_action = row_content(ui, track, theme);
    });
    let mut actions = Vec::new();
    match button_action {
        Some(action) => actions.push(action),
        None if response.clicked() => actions.push(Action::ContextPlayed {
            tracks: all.to_vec(),
            start: index,
        }),
        None => {}
    }
    actions.extend(hover_prefetch_action(ui, &response, track, theme));
    if let Some(action) = row_context_menu(&response, track, context) {
        actions.push(action);
    }
    actions
}

/// The row's right-click menu: like or unlike, add to the queue, add
/// to a playlist, and, on a playlist page, remove from it. Returns at
/// most one action, the last menu item the user clicked.
fn row_context_menu(
    response: &egui::Response,
    track: &Track,
    context: &RowContext,
) -> Option<Action> {
    let mut action = None;
    response.context_menu(|ui| {
        like_menu_item(ui, track, context, &mut action);
        if ui.button("Add to queue").clicked() {
            action = Some(Action::TrackQueued(track.clone()));
            ui.close();
        }
        add_to_playlist_menu(ui, track, context, &mut action);
        remove_from_playlist_menu_item(ui, track, context, &mut action);
    });
    action
}

fn like_menu_item(
    ui: &mut egui::Ui,
    track: &Track,
    context: &RowContext,
    action: &mut Option<Action>,
) {
    let label = if is_liked(context.liked, &track.id) {
        "Unlike"
    } else {
        "Like"
    };
    if ui.button(label).clicked() {
        *action = Some(Action::TrackLikeToggled(track.clone()));
        ui.close();
    }
}

/// The "Add to playlist" submenu: one entry per playlist in the
/// library, plus "New playlist..." to create one and add the track to
/// it once it exists.
fn add_to_playlist_menu(
    ui: &mut egui::Ui,
    track: &Track,
    context: &RowContext,
    action: &mut Option<Action>,
) {
    ui.menu_button("Add to playlist", |ui| {
        for playlist in context.playlists {
            if ui.button(&playlist.title).clicked() {
                *action = Some(Action::TrackAddedToPlaylist {
                    playlist: playlist.id.clone(),
                    track: track.clone(),
                });
                ui.close();
            }
        }
        if ui.button("New playlist...").clicked() {
            *action = Some(Action::CreatePlaylistDialogOpened(Some(track.clone())));
            ui.close();
        }
    });
}

/// "Remove from this playlist", shown only on a playlist page and only
/// for a row that carries the item id a removal needs.
fn remove_from_playlist_menu_item(
    ui: &mut egui::Ui,
    track: &Track,
    context: &RowContext,
    action: &mut Option<Action>,
) {
    let Page::Playlist(playlist_id) = context.page else {
        return;
    };
    let Some(item_id) = &track.playlist_item_id else {
        return;
    };
    if ui.button("Remove from this playlist").clicked() {
        *action = Some(Action::TrackRemovedFromPlaylist {
            playlist: playlist_id.clone(),
            item_id: item_id.clone(),
        });
        ui.close();
    }
}

/// The dwell timer for one track row, in egui's own frame memory
/// instead of app state. The timer is UI-local scratch: it resets on
/// every pointer move, and the reducer must see only the finished
/// decision, not the raw pointer motion, so it stays out of `State`.
///
/// egui repaints only in answer to input, and a resting pointer sends
/// none, so a pending dwell asks for a repaint at the moment its
/// delay ends.
fn hover_prefetch_action(
    ui: &mut egui::Ui,
    response: &egui::Response,
    track: &Track,
    theme: &dyn Theme,
) -> Option<Action> {
    let id = egui::Id::new(("hover_prefetch_dwell", &track.id.0));
    let now = ui.input(|input| input.time);
    let delay = f64::from(theme.metric(MetricRole::HoverPrefetchDelay));
    let start = ui.ctx().data(|data| data.get_temp::<f64>(id));
    match dwell_decision(response.hovered(), start, now, delay) {
        DwellDecision::Start => {
            ui.ctx().data_mut(|data| data.insert_temp(id, now));
            ui.ctx()
                .request_repaint_after(Duration::from_secs_f64(delay));
            None
        }
        DwellDecision::Wait { remaining } => {
            ui.ctx().request_repaint_after(remaining);
            None
        }
        DwellDecision::Emit => {
            ui.ctx().data_mut(|data| data.insert_temp(id, EMITTED));
            Some(Action::TrackHovered(track.clone()))
        }
        DwellDecision::Reset => {
            if start.is_some() {
                ui.ctx().data_mut(|data| data.remove::<f64>(id));
            }
            None
        }
        DwellDecision::Done => None,
    }
}

/// One row's dwell state: whether to start timing, keep waiting, emit
/// the hover action, or clear a stale timer. Pure so the timing rule
/// is testable without egui.
#[derive(Debug, PartialEq)]
enum DwellDecision {
    Start,
    Wait {
        remaining: Duration,
    },
    Emit,
    /// The row emitted already during this hover.
    Done,
    Reset,
}

/// The timer value after an emit. A row emits once per hover: the
/// pointer must leave and return before it emits again.
const EMITTED: f64 = f64::INFINITY;

/// Decides `DwellDecision` from a row's hover state and clock. `start`
/// is the dwell's own recorded start time, `now` and `delay` come
/// from the caller's clock and theme, in seconds.
fn dwell_decision(hovered: bool, start: Option<f64>, now: f64, delay: f64) -> DwellDecision {
    match (hovered, start) {
        (false, _) => DwellDecision::Reset,
        (true, None) => DwellDecision::Start,
        (true, Some(start)) if start == EMITTED => DwellDecision::Done,
        (true, Some(start)) if now - start >= delay => DwellDecision::Emit,
        (true, Some(start)) => DwellDecision::Wait {
            remaining: Duration::from_secs_f64((delay - (now - start)).max(0.0)),
        },
    }
}

fn row_content(ui: &mut egui::Ui, track: &Track, theme: &dyn Theme) -> Option<Action> {
    let mut action = None;
    let art_size = theme.metric(MetricRole::RowArtSize);
    artwork(ui, theme, track.thumbnail_url.as_deref(), art_size);
    ui.label(theme.label(TextRole::Body, &track.title));
    if let Some(artist_action) = artist_labels(ui, track, theme) {
        action = Some(artist_action);
    }
    if let Some(album_action) = album_label(ui, track, theme) {
        action = Some(album_action);
    }
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        if let Some(duration) = track.duration {
            ui.label(theme.secondary_label(TextRole::Caption, format_duration(duration)));
        }
        if ui.small_button("+").on_hover_text("Add to queue").clicked() {
            action = Some(Action::TrackQueued(track.clone()));
        }
    });
    action
}

/// The track's artist credits, one clickable label per artist,
/// separated by ", " labels. A click on an artist with an id opens
/// that artist page. A click on a bare name searches for it instead.
pub(crate) fn artist_labels(ui: &mut egui::Ui, track: &Track, theme: &dyn Theme) -> Option<Action> {
    let mut action = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (index, artist) in track.artists.iter().enumerate() {
            if index > 0 {
                ui.label(theme.secondary_label(TextRole::Caption, ","));
            }
            if let Some(clicked) = artist_label(ui, artist, theme) {
                action = Some(clicked);
            }
        }
    });
    action
}

/// One artist's clickable label. The reducer decides between the
/// artist page and a search, so the row only reports the click.
fn artist_label(ui: &mut egui::Ui, artist: &ArtistRef, theme: &dyn Theme) -> Option<Action> {
    let label = theme.secondary_label(TextRole::Caption, &artist.name);
    let response = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
    response
        .clicked()
        .then(|| Action::ArtistLinkOpened(artist.clone()))
}

/// The track's album name, clickable when the track carries an album
/// id. A click opens that album, in place of the row's own play
/// action, the same way the queue button overrides it.
fn album_label(ui: &mut egui::Ui, track: &Track, theme: &dyn Theme) -> Option<Action> {
    let album_id = track.album_id.clone()?;
    let name = track.album.as_deref().unwrap_or("Album");
    let label = theme.secondary_label(TextRole::Caption, name);
    let response = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
    response.clicked().then_some(Action::AlbumOpened(album_id))
}

pub fn format_duration(duration: Duration) -> String {
    let total = duration.as_secs();
    format!("{}:{:02}", total / 60, total % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_format_as_minutes_and_seconds() {
        assert_eq!(format_duration(Duration::from_secs(65)), "1:05");
        assert_eq!(format_duration(Duration::from_secs(0)), "0:00");
        assert_eq!(format_duration(Duration::from_secs(600)), "10:00");
    }

    #[test]
    fn an_unhovered_row_resets_even_with_no_timer_running() {
        assert_eq!(dwell_decision(false, None, 10.0, 0.4), DwellDecision::Reset);
    }

    #[test]
    fn a_hover_with_no_timer_starts_one() {
        assert_eq!(dwell_decision(true, None, 10.0, 0.4), DwellDecision::Start);
    }

    #[test]
    fn a_hover_short_of_the_delay_waits_for_the_remainder() {
        assert_eq!(
            dwell_decision(true, Some(10.0), 10.1, 0.4),
            DwellDecision::Wait {
                remaining: Duration::from_secs_f64(0.3)
            }
        );
    }

    #[test]
    fn a_hover_at_the_delay_emits() {
        assert_eq!(
            dwell_decision(true, Some(10.0), 10.4, 0.4),
            DwellDecision::Emit
        );
    }

    #[test]
    fn a_row_that_emitted_stays_quiet_until_the_pointer_leaves() {
        assert_eq!(
            dwell_decision(true, Some(EMITTED), 99.0, 0.4),
            DwellDecision::Done
        );
        assert_eq!(
            dwell_decision(false, Some(EMITTED), 99.0, 0.4),
            DwellDecision::Reset
        );
    }

    #[test]
    fn a_hover_past_the_delay_still_emits() {
        assert_eq!(
            dwell_decision(true, Some(10.0), 20.0, 0.4),
            DwellDecision::Emit
        );
    }

    #[test]
    fn the_pointer_leaving_a_timed_row_resets_it() {
        assert_eq!(
            dwell_decision(false, Some(10.0), 10.2, 0.4),
            DwellDecision::Reset
        );
    }
}
