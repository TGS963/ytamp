//! The player bar: transport controls, the seek slider, and the volume.

use std::time::Duration;

use egui::Ui;

use crate::core::action::Action;
use crate::core::model::Track;
use crate::core::queue::RepeatMode;
use crate::core::state::{PlayStatus, State};
use crate::core::update::is_liked;
use crate::theme::{ColorRole, MetricRole, TextRole, Theme};

use super::panel_frame;
use super::rows::{artist_labels, artwork, format_duration};

pub fn view(ui: &mut Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let frame = panel_frame(ui, theme, ColorRole::PanelBackground)
        .inner_margin(egui::Margin::symmetric(20, 10))
        .stroke(egui::Stroke::new(1., theme.color(ColorRole::Border)));
    let compact = ui.ctx().content_rect().width() < 1000.;
    egui::Panel::bottom("player_bar")
        .exact_size(124.)
        .frame(frame)
        .show(ui, |ui| {
            let available = ui.available_width();
            let (top, _) = ui.allocate_exact_size(egui::vec2(available, 60.), egui::Sense::hover());
            let transport_width = 300.;
            let left_width = if compact {
                available - transport_width - 156. - 24.
            } else {
                (available - transport_width) / 2.
            };
            let metadata =
                egui::Rect::from_min_size(top.min, egui::vec2((left_width - 12.).max(0.), 60.));
            let mut now = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(metadata)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            now.set_clip_rect(ui.clip_rect().intersect(metadata));
            now_playing(&mut now, state, theme, out);
            let controls = egui::Rect::from_min_size(
                top.min + egui::vec2(left_width, 0.),
                egui::vec2(transport_width, 60.),
            );
            let mut transport_ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(controls)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            transport(&mut transport_ui, state, theme, out);
            {
                let extras = egui::Rect::from_min_max(
                    egui::pos2(controls.right() + 12., top.top()),
                    top.max,
                );
                let mut extra_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(extras)
                        .layout(egui::Layout::left_to_right(egui::Align::Center)),
                );
                if compact {
                    compact_extras(&mut extra_ui, state, out);
                } else {
                    volume_slider(&mut extra_ui, state, out);
                }
            }
            ui.add_space(6.);
            ui.horizontal(|ui| seek_slider(ui, state, out));
        });
}

fn now_playing(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let art_size = theme.metric(MetricRole::PlayerArtSize);
    match state.playback.queue.current() {
        Some(track) => now_playing_track(ui, theme, track, art_size, out),
        None => now_playing_empty(ui, theme, art_size),
    }
}

/// The playing track's art, title, and artist credits. The art and
/// the title open the focused Now Playing page;
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

/// The now-playing artwork opens the focused listening page.
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
    now_playing_action(&response, track)
}

/// The now-playing title opens the focused listening page.
fn clickable_title(ui: &mut egui::Ui, theme: &dyn Theme, track: &Track) -> Option<Action> {
    let label = theme.label(TextRole::Body, &track.title);
    let response = ui.add(
        egui::Label::new(label)
            .truncate()
            .sense(egui::Sense::click()),
    );
    now_playing_action(&response, track)
}

/// Open the focused listening page when the artwork or title is clicked.
fn now_playing_action(response: &egui::Response, track: &Track) -> Option<Action> {
    let _ = track;
    response.clicked().then_some(Action::NowPlayingOpened)
}

fn transport(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let shuffle_on = state.playback.queue.shuffle;
    if selectable_icon(ui, "🔀", shuffle_on)
        .on_hover_text("Shuffle")
        .clicked()
    {
        out.push(Action::ShuffleToggled);
    }
    like_button(ui, state, out);
    if ui
        .button("⏮")
        .on_hover_text("Previous (Left arrow)")
        .clicked()
    {
        out.push(Action::PreviousPressed);
    }
    if super::components::play_button(
        ui,
        theme,
        matches!(
            state.playback.status,
            PlayStatus::Playing | PlayStatus::Loading
        ),
        state.playback.queue.current().is_some(),
    )
    .on_hover_text("Play / pause (Space)")
    .clicked()
    {
        out.push(Action::PlayToggled);
    }
    if ui.button("⏭").on_hover_text("Next (Right arrow)").clicked() {
        out.push(Action::NextPressed);
    }
    let repeat = state.playback.queue.repeat;
    if selectable_icon(ui, repeat_icon(repeat), repeat != RepeatMode::Off)
        .on_hover_text("Repeat: off / all / one")
        .clicked()
    {
        out.push(Action::RepeatCycled);
    }
    let autoplay_button = selectable_icon(ui, "📻", state.playback.autoplay)
        .on_hover_text("Autoplay related songs when the queue ends");
    if autoplay_button.clicked() {
        out.push(Action::AutoplayToggled);
    }
}

/// A heart toggle for the current track, filled while it is liked.
/// Draws nothing while nothing plays.
fn like_button(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    let Some(track) = state.playback.queue.current() else {
        return;
    };
    let liked = is_liked(&state.library.liked, &track.id);
    let icon = if liked { "♥" } else { "♡" };
    let button =
        selectable_icon(ui, icon, liked).on_hover_text(if liked { "Unlike" } else { "Like" });
    if button.clicked() {
        out.push(Action::TrackLikeToggled(track.clone()));
    }
}

fn selectable_icon(ui: &mut egui::Ui, icon: &str, on: bool) -> egui::Response {
    ui.add(egui::Button::new(icon).selected(on))
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
    let mut position = if duration.is_zero() {
        0.
    } else {
        state.playback.position.as_secs_f64().min(total)
    };
    ui.label(format_duration(state.playback.position));
    let total_label = if duration.is_zero() {
        "–:––".into()
    } else {
        format_duration(duration)
    };
    let label_width = ui
        .painter()
        .layout_no_wrap(
            total_label.clone(),
            egui::TextStyle::Body.resolve(ui.style()),
            ui.visuals().text_color(),
        )
        .size()
        .x;
    ui.spacing_mut().slider_width =
        (ui.available_width() - label_width - ui.spacing().item_spacing.x).max(40.0);
    let response =
        super::components::slider(ui, &mut position, 0.0..=total, false, !duration.is_zero())
            .on_hover_text("Seek");
    if response.drag_stopped() || (response.changed() && !response.dragged()) {
        out.push(Action::SeekRequested(Duration::from_secs_f64(position)));
    }
    ui.label(total_label);
}

fn compact_extras(ui: &mut Ui, state: &State, out: &mut Vec<Action>) {
    let mut volume = state.playback.volume;
    ui.spacing_mut().slider_width = (ui.available_width() - 44.).max(48.);
    if super::components::slider(ui, &mut volume, 0.0..=1.0, false, true)
        .on_hover_text(format!("Volume: {:.0}%", volume * 100.))
        .changed()
    {
        out.push(Action::VolumeSet(volume));
    }
    ui.menu_button("•••", |ui| {
        if ui.button("Lyrics").clicked() {
            out.push(Action::LyricsToggled);
            ui.close();
        }
        ui.menu_button("Equalizer", |ui| {
            egui::ScrollArea::vertical()
                .max_height(300.)
                .show(ui, |ui| super::equalizer::controls(ui, state, out));
        });
        if ui.button("Queue").clicked() {
            out.push(Action::QueuePanelToggled);
            ui.close();
        }
        if ui.button("Winamp skin").clicked() {
            out.push(Action::WinampToggled);
            ui.close();
        }
        if ui.button("Now playing").clicked() {
            out.push(Action::NowPlayingOpened);
            ui.close();
        }
    })
    .response
    .on_hover_text("More player controls");
}

fn volume_slider(ui: &mut egui::Ui, state: &State, out: &mut Vec<Action>) {
    let mut volume = state.playback.volume;
    let font = egui::TextStyle::Button.resolve(ui.style());
    let buttons_width: f32 = ["Lyrics", "EQ", "Queue", "Skin"]
        .iter()
        .map(|text| {
            ui.painter()
                .layout_no_wrap((*text).into(), font.clone(), ui.visuals().text_color())
                .size()
                .x
                + 2. * ui.spacing().button_padding.x
        })
        .sum();
    ui.spacing_mut().slider_width =
        (ui.available_width() - buttons_width - 4. * ui.spacing().item_spacing.x).max(48.);
    if super::components::slider(ui, &mut volume, 0.0..=1.0, false, true)
        .on_hover_text(format!("Volume: {:.0}%", state.playback.volume * 100.))
        .changed()
    {
        out.push(Action::VolumeSet(volume));
    }
    if ui.button("Lyrics").clicked() {
        out.push(Action::LyricsToggled);
    }
    let response = ui.button("EQ").on_hover_text("Equalizer");
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            egui::ScrollArea::vertical()
                .max_height(350.)
                .show(ui, |ui| super::equalizer::controls(ui, state, out));
        });
    let queue_button = selectable_icon(ui, "Queue", state.queue_open).on_hover_text("Queue (Q)");
    if queue_button.clicked() {
        out.push(Action::QueuePanelToggled);
    }
    let winamp_hint = if cfg!(target_os = "macos") {
        "Winamp skin (Cmd+Shift+M)"
    } else {
        "Winamp skin (Ctrl+M)"
    };
    let winamp_button = selectable_icon(ui, "Skin", state.winamp.open).on_hover_text(winamp_hint);
    if winamp_button.clicked() {
        out.push(Action::WinampToggled);
    }
}

#[cfg(test)]
mod polish_tests {
    use super::*;
    #[test]
    fn bottom_controls_use_available_width_without_overflow() {
        for width in [700., 1100., 1710.] {
            let ctx = egui::Context::default();
            let mut state = State::default();
            state.playback.track_duration = Some(Duration::from_secs(200));
            state.playback.position = Duration::from_secs(100);
            for pass in 0..2 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 720.),
                        )),
                        ..Default::default()
                    },
                    |ui| {
                        super::super::apply_page_style(ui, &crate::theme::DefaultTheme);
                        view(ui, &state, &crate::theme::DefaultTheme, &mut vec![]);
                    },
                );
                output.textures_delta.clear();
                if pass == 0 {
                    continue;
                }
                let rail_width = output
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Rect(r) if r.rect.height() == 4. => Some(r.rect.width()),
                        _ => None,
                    })
                    .fold(0., f32::max);
                assert!(
                    rail_width > width - 170.,
                    "seek rail leaves unused width at {width}: {rail_width}"
                );
                let skin = output
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        egui::Shape::Text(t)
                            if t.galley.text()
                                == if width < 1000. { "•••" } else { "Skin" } =>
                        {
                            Some(t.pos.x + t.galley.size().x)
                        }
                        _ => None,
                    })
                    .unwrap();
                assert!(skin <= width - 18., "controls overflow at {width}: {skin}");
                assert!(
                    skin >= width - 45.,
                    "controls leave unused width at {width}: {skin}"
                );
            }
        }
    }
    #[test]
    fn seek_and_volume_paint_a_visible_progress_trail() {
        let ctx = egui::Context::default();
        let mut warm = ctx.run_ui(egui::RawInput::default(), |ui| {
            super::super::apply_page_style(ui, &crate::theme::DefaultTheme)
        });
        warm.textures_delta.clear();
        let mut state = State::default();
        state.playback.track_duration = Some(Duration::from_secs(200));
        state.playback.position = Duration::from_secs(100);
        state.playback.volume = 0.5;
        for seek in [true, false] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(700., 480.),
                    )),
                    ..Default::default()
                },
                |ui| {
                    super::super::apply_page_style(ui, &crate::theme::DefaultTheme);
                    ui.horizontal(|ui| {
                        if seek {
                            seek_slider(ui, &state, &mut vec![]);
                        } else {
                            volume_slider(ui, &state, &mut vec![]);
                        }
                    });
                },
            );
            output.textures_delta.clear();
            let accent = crate::theme::DefaultTheme.color(ColorRole::Accent);
            assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == accent && r.rect.width() > 15. && r.rect.height() <= 8.)), "{} has no visible progress trail", if seek {"seek"} else {"volume"});
        }
    }
}
