//! The player bar: transport controls, the seek slider, and the volume.

use std::time::Duration;

use egui::Ui;

use crate::core::action::Action;
use crate::core::model::Track;
use crate::core::queue::RepeatMode;
use crate::core::state::{PlayStatus, State};
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::panel_frame;
use super::rows::{artist_labels, artwork, format_duration};

pub fn view(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let frame = panel_frame(ui, theme, ColorRole::PanelBackground);
    egui::Panel::bottom("player_bar")
        .exact_size(theme.metric(MetricRole::PlayerBarHeight))
        .frame(frame)
        .show(ui, |ui| {
            ui.add_space(theme.metric(MetricRole::GapSmall));
            ui.horizontal(|ui| {
                now_playing(ui, state, theme, out);
                transport(ui, state, out);
                seek_slider(ui, state, out);
                volume_slider(ui, state, out);
            });
        });
}

fn now_playing(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    ui.set_min_width(220.0);
    let art_size = theme.metric(MetricRole::PlayerArtSize);
    match state.playback.queue.current() {
        Some(track) => now_playing_track(ui, theme, track, art_size, out),
        None => now_playing_empty(ui, theme, art_size),
    }
}

/// The playing track's art, title, and artist credits. The art and
/// the title each open the album when the track carries an album id;
/// an artist label opens that artist, or searches by name.
fn now_playing_track(
    ui: &mut egui::Ui,
    theme: &dyn Theme,
    track: &Track,
    art_size: f32,
    out: &mut Vec<Action>,
) {
    ui.horizontal(|ui| {
        if let Some(action) = clickable_art(ui, theme, track, art_size) {
            out.push(action);
        }
        ui.vertical(|ui| {
            if let Some(action) = clickable_title(ui, theme, track) {
                out.push(action);
            }
            if let Some(action) = artist_labels(ui, track, theme) {
                out.push(action);
            }
        });
    });
}

fn now_playing_empty(ui: &mut egui::Ui, theme: &dyn Theme, art_size: f32) {
    ui.horizontal(|ui| {
        artwork(ui, theme, None, art_size);
        ui.label(theme.secondary_label(TextRole::Body, "Nothing playing"));
    });
}

/// The now-playing artwork, clickable to open the album.
fn clickable_art(ui: &mut egui::Ui, theme: &dyn Theme, track: &Track, size: f32) -> Option<Action> {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let radius = theme.metric(MetricRole::CornerRadius);
    ui.painter()
        .rect_filled(rect, radius, theme.color(ColorRole::ArtPlaceholder));
    if let Some(url) = track.thumbnail_url.as_deref() {
        egui::Image::from_uri(url.to_string())
            .corner_radius(radius)
            .show_loading_spinner(false)
            .paint_at(ui, rect);
    }
    album_open_action(&response, track)
}

/// The now-playing title, clickable to open the album.
fn clickable_title(ui: &mut egui::Ui, theme: &dyn Theme, track: &Track) -> Option<Action> {
    let label = theme.label(TextRole::Body, &track.title);
    let response = ui.add(egui::Label::new(label).sense(egui::Sense::click()));
    album_open_action(&response, track)
}

/// `AlbumOpened` when the track carries an album id and the response
/// was clicked, `None` otherwise.
fn album_open_action(response: &egui::Response, track: &Track) -> Option<Action> {
    let album_id = track.album_id.clone()?;
    response.clicked().then_some(Action::AlbumOpened(album_id))
}

fn transport(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    let shuffle_on = state.playback.queue.shuffle;
    if selectable_icon(ui, "🔀", shuffle_on).clicked() {
        out.push(Action::ShuffleToggled);
    }
    if ui.button("⏮").clicked() {
        out.push(Action::PreviousPressed);
    }
    if ui.button(play_pause_icon(state.playback.status)).clicked() {
        out.push(Action::PlayToggled);
    }
    if ui.button("⏭").clicked() {
        out.push(Action::NextPressed);
    }
    let repeat = state.playback.queue.repeat;
    if selectable_icon(ui, repeat_icon(repeat), repeat != RepeatMode::Off).clicked() {
        out.push(Action::RepeatCycled);
    }
    let autoplay_button = selectable_icon(ui, "📻", state.playback.autoplay)
        .on_hover_text("Autoplay related songs when the queue ends");
    if autoplay_button.clicked() {
        out.push(Action::AutoplayToggled);
    }
}

fn selectable_icon(ui: &mut egui::Ui, icon: &str, on: bool) -> egui::Response {
    ui.add(egui::Button::new(icon).selected(on))
}

fn play_pause_icon(status: PlayStatus) -> &'static str {
    match status {
        PlayStatus::Playing | PlayStatus::Loading => "⏸",
        PlayStatus::Paused | PlayStatus::Stopped => "▶",
    }
}

fn repeat_icon(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::One => "🔂",
        _ => "🔁",
    }
}

fn seek_slider(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    let duration = state.playback.track_duration.unwrap_or(Duration::ZERO);
    let total = duration.as_secs_f64().max(1.0);
    let mut position = state.playback.position.as_secs_f64().min(total);
    ui.label(format_duration(state.playback.position));
    let slider = egui::Slider::new(&mut position, 0.0..=total).show_value(false);
    let response = ui.add_sized([ui.available_width() - 160.0, 20.0], slider);
    if response.drag_stopped() || (response.changed() && !response.dragged()) {
        out.push(Action::SeekRequested(Duration::from_secs_f64(position)));
    }
    ui.label(format_duration(duration));
}

fn volume_slider(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    let mut volume = state.playback.volume;
    let slider = egui::Slider::new(&mut volume, 0.0..=1.0).show_value(false);
    if ui.add_sized([100.0, 20.0], slider).changed() {
        out.push(Action::VolumeSet(volume));
    }
    let queue_button = selectable_icon(ui, "☰", state.queue_open).on_hover_text("Queue (Q)");
    if queue_button.clicked() {
        out.push(Action::QueuePanelToggled);
    }
    let winamp_hint = if cfg!(target_os = "macos") {
        "Winamp skin (Cmd+Shift+M)"
    } else {
        "Winamp skin (Ctrl+M)"
    };
    let winamp_button =
        selectable_icon(ui, "🎵", state.winamp.open).on_hover_text(winamp_hint);
    if winamp_button.clicked() {
        out.push(Action::WinampToggled);
    }
}
