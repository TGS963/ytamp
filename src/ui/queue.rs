//! The queue side panel: the current track and everything after it.

use egui::Ui;

use crate::core::action::Action;
use crate::core::model::Track;
use crate::core::state::State;
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::panel_frame;
use super::queue_edit::Editor;

pub fn view(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let frame = panel_frame(ui, theme, ColorRole::PanelBackground);
    egui::Panel::right("queue")
        .exact_size(theme.metric(MetricRole::SidebarWidth) * 1.4)
        .frame(frame)
        .show(ui, |ui| {
            ui.add_space(theme.metric(MetricRole::PagePadding));
            ui.label(theme.label(TextRole::Heading, "Queue"));
            ui.add_space(theme.metric(MetricRole::GapSmall));
            now_playing_entry(ui, state, theme);
            if ui.button("Clear queued tracks").clicked() {
                out.push(Action::QueueCleared);
            }
            upcoming_entries(ui, state, theme, out);
        });
}

fn now_playing_entry(ui: &mut Ui, state: &State, theme: &dyn Theme) {
    let Some(track) = state.playback.queue.current() else {
        ui.label(theme.secondary_label(TextRole::Body, "The queue is empty."));
        return;
    };
    ui.label(theme.label(TextRole::Body, format!("▶ {}", track.title)));
}

pub(super) fn upcoming_entries(
    ui: &mut Ui,
    state: &State,
    theme: &dyn Theme,
    out: &mut Vec<Action>,
) {
    let upcoming: Vec<&Track> = state.playback.queue.upcoming().collect();
    let mut editor = Editor::load(ui.ctx(), &state.playback.queue);
    let row_height = theme.metric(MetricRole::RowHeight);
    ui.label(theme.secondary_label(
        TextRole::Body,
        "Select · double-click to play · drag to move",
    ));
    egui::ScrollArea::vertical()
        .id_salt("queue_upcoming")
        .show_rows(ui, row_height, upcoming.len(), |ui, range| {
            for index in range {
                ui.push_id(index, |ui| {
                    let response =
                        queue_row(ui, upcoming[index], theme, editor.selected.contains(&index));
                    editor.row(ui, &response, index, out);
                    response.context_menu(|ui| editor.menu(ui, index, out));
                });
            }
        });
    editor.keyboard(ui, upcoming.len(), out);
    editor.store(ui.ctx());
}

/// The queue keeps its own selection and drag behavior while sharing track artwork.
fn queue_row(ui: &mut Ui, track: &Track, theme: &dyn Theme, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), theme.metric(MetricRole::RowHeight)),
        egui::Sense::click_and_drag(),
    );
    if selected || response.hovered() {
        let color = if selected {
            ColorRole::AccentSoft
        } else {
            ColorRole::RowHover
        };
        ui.painter().rect_filled(
            rect,
            theme.metric(MetricRole::CornerRadius),
            theme.color(color),
        );
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(6.)));
    child.set_clip_rect(child.clip_rect().intersect(rect));
    child.horizontal_centered(|ui| queue_row_content(ui, track, theme));
    response
}
fn queue_row_content(ui: &mut Ui, track: &Track, theme: &dyn Theme) {
    super::rows::artwork(
        ui,
        theme,
        track.thumbnail_url.as_deref(),
        theme.metric(MetricRole::RowArtSize),
    );
    let width = (ui.available_width() - 44.).max(0.);
    ui.allocate_ui_with_layout(
        egui::vec2(width, 34.),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.spacing_mut().item_spacing.y = 1.;
            ui.add(
                egui::Label::new(theme.label(TextRole::Body, &track.title))
                    .truncate()
                    .sense(egui::Sense::empty()),
            );
            let artist = track.artist_names();
            let subtitle = if artist.is_empty() && track.is_local() {
                "Local file"
            } else {
                &artist
            };
            ui.add(
                egui::Label::new(theme.secondary_label(TextRole::Caption, subtitle))
                    .truncate()
                    .sense(egui::Sense::empty()),
            );
        },
    );
    if let Some(duration) = track.duration {
        ui.label(theme.secondary_label(TextRole::Caption, super::rows::format_duration(duration)));
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::model::{ArtistRef, TrackId},
        theme::DefaultTheme,
    };
    use egui::{Pos2, Rect, vec2};
    use std::time::Duration;

    fn track() -> Track {
        Track {
            source: Default::default(),
            id: TrackId("queue-row".into()),
            title: "A deliberately long queue title that must not cover the duration".into(),
            artists: vec![ArtistRef::named("A long artist name")],
            album: None,
            album_id: None,
            duration: Some(Duration::from_secs(245)),
            thumbnail_url: None,
            playlist_item_id: None,
        }
    }

    fn text_rect(shape: &egui::Shape, text: &str) -> Option<Rect> {
        match shape {
            egui::Shape::Text(value) if value.galley.text() == text => {
                Some(Rect::from_min_size(value.pos, value.galley.size()))
            }
            egui::Shape::Vec(values) => values.iter().find_map(|value| text_rect(value, text)),
            _ => None,
        }
    }

    #[test]
    fn queue_row_clips_long_text_and_keeps_duration_visible_at_small_and_large_widths() {
        for width in [260., 600.] {
            let ctx = egui::Context::default();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(width, 90.))),
                    ..Default::default()
                },
                |ui| {
                    ui.set_width(width);
                    queue_row(ui, &track(), &DefaultTheme, false);
                },
            );
            output.textures_delta.clear();
            let duration = output
                .shapes
                .iter()
                .find_map(|shape| text_rect(&shape.shape, "4:05"))
                .unwrap();
            assert!(duration.right() <= width, "duration overflows at {width}");
            assert!(
                output
                    .shapes
                    .iter()
                    .find_map(|shape| text_rect(&shape.shape, &track().title))
                    .is_some()
            );
        }
    }

    #[test]
    fn row_click_reaches_the_editor_response_over_every_visual_child() {
        let ctx = egui::Context::default();
        let frame = |events| {
            let mut clicked = false;
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(260., 90.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    super::super::apply_page_style(ui, &DefaultTheme);
                    ui.set_width(260.);
                    clicked = queue_row(ui, &track(), &DefaultTheme, false).clicked();
                },
            );
            output.textures_delta.clear();
            clicked
        };
        frame(vec![]);
        frame(vec![]);
        for point in [
            Pos2::new(15., 22.),
            Pos2::new(90., 14.),
            Pos2::new(90., 37.),
            Pos2::new(242., 22.),
        ] {
            frame(vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
            assert!(frame(vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ]));
        }
    }
}
