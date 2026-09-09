use crate::core::{
    action::Action,
    state::{Loadable, State},
};
use egui::Ui;
pub fn view(ui: &mut Ui, state: &State, out: &mut Vec<Action>) {
    egui::CentralPanel::default().show(ui, |ui| content(ui, state, out));
}
pub(super) fn content(ui: &mut Ui, state: &State, out: &mut Vec<Action>) {
    content_inner(ui, state, out, true);
}
pub(super) fn embedded(ui: &mut Ui, state: &State, out: &mut Vec<Action>) {
    content_inner(ui, state, out, false);
}
fn content_inner(ui: &mut Ui, state: &State, out: &mut Vec<Action>, show_title: bool) {
    let Some(track) = state.playback.queue.current() else {
        ui.label("Play a song to see its lyrics.");
        return;
    };
    if show_title {
        ui.heading(&track.title);
        ui.weak(track.artist_names());
        ui.separator();
    }
    match &state.lyrics.content {
        Loadable::NotAsked | Loadable::Loading | Loadable::Refreshing(_) => {
            ui.spinner();
            ui.label("Loading lyrics…");
        }
        Loadable::Loaded(Some(lyrics)) => {
            let id = egui::Id::new(("lyrics-follow", &track.id.0));
            let (mut follow, previous) = ui.data_mut(|data| {
                data.get_temp::<(bool, Option<usize>)>(id)
                    .unwrap_or((true, None))
            });
            let mut delay = state.lyrics.delays.get(&track.id.0).copied().unwrap_or(0.0);
            let old_delay = delay;
            let mut changed = false;
            if !lyrics.timed_lines.is_empty() {
                ui.horizontal(|ui| {
                        changed |= ui.checkbox(&mut follow, "Follow playback").changed();
                        ui.label("Delay");
                        changed |= ui.add(egui::DragValue::new(&mut delay).speed(0.1).range(-30.0..=30.0).suffix(" s"))
                            .on_hover_text("Positive values show lyrics later. Drag to adjust timing for this song.").changed();
                        if ui.add_enabled(delay != 0.0, egui::Button::new("Reset")).clicked() {
                            delay = 0.0;
                            changed = true;
                        }
                    });
                if delay != old_delay {
                    out.push(Action::LyricsDelaySet {
                        track: track.id.clone(),
                        seconds: delay,
                    });
                }
                ui.weak("Click a line to seek.");
            }
            // Wheel/trackpad scrolling and scrollbar drags release automatic follow.
            let manual_scroll = ui.input(|input| {
                    input.pointer.hover_pos().is_some_and(|pos| ui.available_rect_before_wrap().contains(pos))
                    && (input.raw.events.iter().any(|event| matches!(event, egui::Event::MouseWheel { delta, .. } if delta.y != 0.0)) || input.pointer.is_decidedly_dragging())
                });
            if manual_scroll {
                follow = false;
            }
            let active = lyrics.active_line(state.playback.position, delay);
            egui::ScrollArea::vertical()
                .id_salt(("lyrics", &track.id.0))
                .show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        if lyrics.timed_lines.is_empty() {
                            ui.add(egui::Label::new(&lyrics.text).wrap());
                        } else {
                            for (index, line) in lyrics.timed_lines.iter().enumerate() {
                                let selected = active == Some(index);
                                let text = if line.text.is_empty() {
                                    "♪"
                                } else {
                                    &line.text
                                };
                                let mut rich = egui::RichText::new(text).size(18.0);
                                if selected {
                                    rich = rich.strong().color(ui.visuals().selection.stroke.color);
                                }
                                let response = ui
                                    .add(egui::Label::new(rich).wrap().sense(egui::Sense::click()));
                                if response.clicked() {
                                    out.push(Action::SeekRequested(
                                        std::time::Duration::from_secs_f64(
                                            (line.at.as_secs_f64() + delay).max(0.0),
                                        ),
                                    ));
                                }
                                if selected && follow && (active != previous || changed) {
                                    response.scroll_to_me(Some(egui::Align::Center));
                                }
                                ui.add_space(8.0);
                            }
                        }
                        ui.add_space(16.);
                        if lyrics.source == "Lyrics: LRCLIB" {
                            ui.hyperlink_to(&lyrics.source, "https://lrclib.net");
                        } else {
                            ui.weak(&lyrics.source);
                        }
                    });
                });
            ui.data_mut(|data| data.insert_temp(id, (follow, active)));
        }
        Loadable::Loaded(None) => {
            ui.label("Lyrics aren’t available for this song.");
        }
        Loadable::Failed(error) => {
            ui.label("Couldn’t load lyrics.");
            ui.label(error);
            if ui.button("Retry").clicked() {
                out.push(Action::LyricsReloadRequested);
            }
        }
    }
}
