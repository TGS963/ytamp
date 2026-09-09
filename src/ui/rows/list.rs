use super::{RowContext, cells};
use crate::core::action::Action;
use crate::core::model::Track;
use crate::theme::{ColorRole, MetricRole, Theme};
use crate::thumbnails::sized;

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
                Some(track) => {
                    ui.push_id((id_salt, index, &track.id), |ui| {
                        out.extend(cells::track_row(ui, track, index, tracks, theme, context));
                    });
                }
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
pub(super) fn loading_more_row(ui: &mut egui::Ui, theme: &dyn Theme) {
    row_frame(ui, theme, |ui| {
        ui.spinner();
    });
}
