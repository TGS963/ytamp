use super::{RowContext, artwork, menus, row_frame};
use crate::core::action::Action;
use crate::core::model::{ArtistRef, Track};
use crate::theme::{MetricRole, TextRole, Theme};
use std::time::Duration;

pub(super) fn track_row(
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
    if let Some(action) = menus::row_context_menu(&response, track, context) {
        actions.push(action);
    }
    actions
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
pub(crate) enum DwellDecision {
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
pub(crate) const EMITTED: f64 = f64::INFINITY;

/// Decides `DwellDecision` from a row's hover state and clock. `start`
/// is the dwell's own recorded start time, `now` and `delay` come
/// from the caller's clock and theme, in seconds.
pub(crate) fn dwell_decision(
    hovered: bool,
    start: Option<f64>,
    now: f64,
    delay: f64,
) -> DwellDecision {
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
    // Reserve the trailing controls before laying out variable-length text.
    let trailing = 74.0;
    let width = (ui.available_width() - trailing - 8.0).max(0.0);
    ui.allocate_ui_with_layout(
        egui::vec2(width, 24.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
            let title_width = (width * 0.48).max(0.0);
            let (title_rect, _) =
                ui.allocate_exact_size(egui::vec2(title_width, 24.0), egui::Sense::hover());
            let mut title_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(title_rect)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            title_ui
                .add(egui::Label::new(theme.label(TextRole::Body, &track.title)).truncate())
                .on_hover_text(&track.title);
            let artist_width = if track.album_id.is_some() {
                width * 0.25
            } else {
                width * 0.48
            };
            ui.allocate_ui_with_layout(
                egui::vec2(artist_width, 24.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if let Some(a) = artist_labels(ui, track, theme) {
                        action = Some(a);
                    }
                },
            );
            if let Some(a) = album_label(ui, track, theme) {
                action = Some(a);
            }
        },
    );
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
    let response = ui.add(
        egui::Label::new(label)
            .truncate()
            .sense(egui::Sense::click()),
    );
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
    let response = ui.add(
        egui::Label::new(label)
            .truncate()
            .sense(egui::Sense::click()),
    );
    response.clicked().then_some(Action::AlbumOpened(album_id))
}

pub(crate) fn format_duration(duration: Duration) -> String {
    let total = duration.as_secs();
    format!("{}:{:02}", total / 60, total % 60)
}
