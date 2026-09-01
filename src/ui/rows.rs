//! Shared list rows: the one way every view draws a clickable track list.

use std::time::Duration;

use crate::core::action::Action;
use crate::core::model::Track;
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

/// Draws `tracks` as rows. A click on a row plays the whole list from
/// that row. The queue button appends one track to the user queue.
pub fn track_list(ui: &mut egui::Ui, tracks: &[Track], theme: &dyn Theme, out: &mut Vec<Action>) {
    for (index, track) in tracks.iter().enumerate() {
        if let Some(action) = track_row(ui, track, index, tracks, theme) {
            out.push(action);
        }
    }
}

fn track_row(
    ui: &mut egui::Ui,
    track: &Track,
    index: usize,
    all: &[Track],
    theme: &dyn Theme,
) -> Option<Action> {
    let height = theme.metric(MetricRole::RowHeight);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), height),
        egui::Sense::click(),
    );
    if response.hovered() {
        ui.painter()
            .rect_filled(rect, 4.0, theme.color(ColorRole::RowHover));
    }
    let mut action = None;
    let builder = egui::UiBuilder::new().max_rect(rect.shrink(6.0));
    ui.scope_builder(builder, |ui| {
        ui.horizontal_centered(|ui| {
            action = row_content(ui, track, theme);
        });
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
