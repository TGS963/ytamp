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
            render_loaded(ui, state, out, track, lyrics);
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

fn render_loaded(
    ui: &mut Ui,
    state: &State,
    out: &mut Vec<Action>,
    track: &crate::core::model::Track,
    lyrics: &crate::core::lyrics::Lyrics,
) {
    let id = egui::Id::new(("lyrics-follow", &track.id.0));
    let (mut follow, previous) = ui.data_mut(|data| {
        data.get_temp::<(bool, Option<usize>)>(id)
            .unwrap_or((true, None))
    });
    let mut delay = state.lyrics.delays.get(&track.id.0).copied().unwrap_or(0.0);
    let changed = render_controls(ui, out, track, lyrics, &mut follow, &mut delay);
    if manual_scroll(ui) {
        follow = false;
    }
    let active = lyrics.active_line(state.playback.position, delay);
    egui::ScrollArea::vertical()
        .id_salt(("lyrics", &track.id.0))
        .show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                render_lines(
                    ui,
                    out,
                    lyrics,
                    LinePresentation {
                        active,
                        delay,
                        scroll_active: follow && (active != previous || changed),
                    },
                );
                ui.add_space(16.);
                render_source(ui, lyrics);
            });
        });
    ui.data_mut(|data| data.insert_temp(id, (follow, active)));
}

fn render_controls(
    ui: &mut Ui,
    out: &mut Vec<Action>,
    track: &crate::core::model::Track,
    lyrics: &crate::core::lyrics::Lyrics,
    follow: &mut bool,
    delay: &mut f64,
) -> bool {
    if lyrics.timed_lines.is_empty() {
        return false;
    }
    let old_delay = *delay;
    let mut changed = false;
    ui.horizontal(|ui| {
        changed |= ui.checkbox(follow, "Follow playback").changed();
        ui.label("Delay");
        changed |= ui
            .add(
                egui::DragValue::new(delay)
                    .speed(0.1)
                    .range(-30.0..=30.0)
                    .suffix(" s"),
            )
            .on_hover_text(
                "Positive values show lyrics later. Drag to adjust timing for this song.",
            )
            .changed();
        if ui
            .add_enabled(*delay != 0.0, egui::Button::new("Reset"))
            .clicked()
        {
            *delay = 0.0;
            changed = true;
        }
    });
    if *delay != old_delay {
        out.push(Action::LyricsDelaySet {
            track: track.id.clone(),
            seconds: *delay,
        });
    }
    ui.weak("Click a line to seek.");
    changed
}

fn manual_scroll(ui: &Ui) -> bool {
    ui.input(|input| {
        input
            .pointer
            .hover_pos()
            .is_some_and(|pos| ui.available_rect_before_wrap().contains(pos))
            && (input.raw.events.iter().any(
                |event| matches!(event, egui::Event::MouseWheel { delta, .. } if delta.y != 0.0),
            ) || input.pointer.is_decidedly_dragging())
    })
}

#[derive(Clone, Copy)]
struct LinePresentation {
    active: Option<usize>,
    delay: f64,
    scroll_active: bool,
}

fn render_lines(
    ui: &mut Ui,
    out: &mut Vec<Action>,
    lyrics: &crate::core::lyrics::Lyrics,
    presentation: LinePresentation,
) {
    if lyrics.timed_lines.is_empty() {
        ui.add(egui::Label::new(&lyrics.text).wrap());
        return;
    }
    for (index, line) in lyrics.timed_lines.iter().enumerate() {
        render_line(ui, out, line, index, presentation);
    }
}

fn render_line(
    ui: &mut Ui,
    out: &mut Vec<Action>,
    line: &crate::core::lyrics::TimedLine,
    index: usize,
    presentation: LinePresentation,
) {
    let selected = presentation.active == Some(index);
    let text = if line.text.is_empty() {
        "♪"
    } else {
        &line.text
    };
    let mut rich = egui::RichText::new(text).size(18.0);
    if selected {
        rich = rich.strong().color(ui.visuals().selection.stroke.color);
    }
    let response = ui.add(egui::Label::new(rich).wrap().sense(egui::Sense::click()));
    if response.clicked() {
        out.push(Action::SeekRequested(std::time::Duration::from_secs_f64(
            (line.at.as_secs_f64() + presentation.delay).max(0.0),
        )));
    }
    if selected && presentation.scroll_active {
        response.scroll_to_me(Some(egui::Align::Center));
    }
    ui.add_space(8.0);
}

fn render_source(ui: &mut Ui, lyrics: &crate::core::lyrics::Lyrics) {
    if lyrics.source == "Lyrics: LRCLIB" {
        ui.hyperlink_to(&lyrics.source, "https://lrclib.net");
    } else {
        ui.weak(&lyrics.source);
    }
}
