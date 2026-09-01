//! Shared list rows: the one way every view draws a clickable track list.
//!
//! `egui::ScrollArea::show_rows` needs one uniform row height. Every row in
//! every list goes through `row_frame`, so the height stays uniform and the
//! scroll math stays correct.

use std::time::Duration;

use crate::core::action::Action;
use crate::core::model::Track;
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

/// Draws `tracks` as a virtualized, scrollable list that fills the space
/// its caller gives it. Only the rows in view get laid out each frame. A
/// click on a row plays the whole list from that row. The queue button
/// appends one track to the user queue.
///
/// `id_salt` must be unique among the scroll areas on the same page.
pub fn track_list(
    ui: &mut egui::Ui,
    id_salt: &str,
    tracks: &[Track],
    theme: &dyn Theme,
    out: &mut Vec<Action>,
) {
    track_list_area(ui, id_salt, tracks, None, theme, out);
}

/// Draws `tracks` the same way as [`track_list`], but caps the visible
/// height so sibling content below it on the page stays reachable.
pub fn track_list_capped(
    ui: &mut egui::Ui,
    id_salt: &str,
    tracks: &[Track],
    max_height: f32,
    theme: &dyn Theme,
    out: &mut Vec<Action>,
) {
    track_list_area(ui, id_salt, tracks, Some(max_height), theme, out);
}

fn track_list_area(
    ui: &mut egui::Ui,
    id_salt: &str,
    tracks: &[Track],
    max_height: Option<f32>,
    theme: &dyn Theme,
    out: &mut Vec<Action>,
) {
    let row_height = theme.metric(MetricRole::RowHeight);
    let mut area = egui::ScrollArea::vertical().id_salt(id_salt);
    if let Some(height) = max_height {
        area = area.max_height(height);
    }
    area.show_rows(ui, row_height, tracks.len(), |ui, row_range| {
        for index in row_range {
            if let Some(action) = track_row(ui, &tracks[index], index, tracks, theme) {
                out.push(action);
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
    let (rect, _response) =
        ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let radius = theme.metric(MetricRole::CornerRadius);
    ui.painter()
        .rect_filled(rect, radius, theme.color(ColorRole::ArtPlaceholder));
    if let Some(url) = thumbnail_url {
        egui::Image::from_uri(url.to_string())
            .corner_radius(radius)
            .show_loading_spinner(false)
            .paint_at(ui, rect);
    }
}

/// Draws one uniform-height row: a hover-highlighted band around centered
/// content, `RowHeight` tall. Every row-shaped widget in the app goes
/// through this, so lists can virtualize on a single row height.
pub fn row_frame(
    ui: &mut egui::Ui,
    theme: &dyn Theme,
    content: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    let height = theme.metric(MetricRole::RowHeight);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), height), egui::Sense::click());
    if response.hovered() {
        let radius = theme.metric(MetricRole::CornerRadius);
        ui.painter()
            .rect_filled(rect, radius, theme.color(ColorRole::RowHover));
    }
    let builder = egui::UiBuilder::new().max_rect(rect.shrink(6.0));
    ui.scope_builder(builder, |ui| {
        ui.horizontal_centered(content);
    });
    response
}

/// A `RowHeight`-tall row with a centered spinner, drawn under a list
/// that is still receiving pages from the network.
pub fn loading_more_row(ui: &mut egui::Ui, theme: &dyn Theme) {
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
) -> Option<Action> {
    let mut action = None;
    let response = row_frame(ui, theme, |ui| {
        action = row_content(ui, track, theme);
    });
    if action.is_none() && response.clicked() {
        action = Some(Action::ContextPlayed {
            tracks: all.to_vec(),
            start: index,
        });
    }
    action
}

fn row_content(ui: &mut egui::Ui, track: &Track, theme: &dyn Theme) -> Option<Action> {
    let mut action = None;
    let art_size = theme.metric(MetricRole::RowArtSize);
    artwork(ui, theme, track.thumbnail_url.as_deref(), art_size);
    ui.label(theme.label(TextRole::Body, &track.title));
    ui.label(theme.secondary_label(TextRole::Caption, track.artists.join(", ")));
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
}
