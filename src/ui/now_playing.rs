//! A focused listening page; detailed controls stay out of the transport bar.
use crate::{
    core::{action::Action, state::State},
    theme::{TextRole, Theme},
};
pub fn view(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    if ui.available_height() < 400. {
        super::components::heading(ui, theme, "Now playing");
        ui.add_space(10.);
    } else {
        super::components::page_title(ui, theme, "Now playing");
        ui.add_space(20.);
    }
    let Some(track) = state.playback.queue.current() else {
        ui.weak("Choose a song to start listening.");
        return;
    };
    let wide = ui.available_width() >= 760.;
    let art_size = if wide {
        (ui.available_width() * 0.38).min(420.)
    } else {
        160.
    };
    if wide {
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(art_size, ui.available_height()),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    super::rows::artwork(ui, theme, track.thumbnail_url.as_deref(), art_size);
                    ui.add_space(16.);
                    ui.add(
                        egui::Label::new(theme.label(TextRole::Title, &track.title).strong())
                            .wrap(),
                    );
                    ui.label(theme.secondary_label(TextRole::Body, track.artist_names()));
                },
            );
            ui.add_space(24.);
            ui.vertical(|ui| detail(ui, state, theme, out));
        });
    } else {
        ui.horizontal(|ui| {
            super::rows::artwork(ui, theme, track.thumbnail_url.as_deref(), 64.);
            ui.vertical(|ui| {
                ui.add(egui::Label::new(theme.label(TextRole::Heading, &track.title)).truncate());
                ui.weak(track.artist_names());
            });
        });
        ui.add_space(12.);
        detail(ui, state, theme, out);
    }
}
fn detail(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let id = egui::Id::new("now-playing-tab");
    let mut tab = ui.data(|d| d.get_temp::<u8>(id)).unwrap_or(0);
    ui.horizontal(|ui| {
        for (value, label) in [(0, "Lyrics"), (1, "Queue"), (2, "Equalizer")] {
            ui.selectable_value(&mut tab, value, label);
        }
    });
    ui.data_mut(|d| d.insert_temp(id, tab));
    ui.add_space(16.);
    match tab {
        0 => super::lyrics::embedded(ui, state, out),
        1 => super::queue::upcoming_entries(ui, state, theme, out),
        _ => {
            egui::ScrollArea::vertical()
                .id_salt("now-playing-eq")
                .show(ui, |ui| super::equalizer::controls(ui, state, out));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn listening_page_fits_small_and_large_windows() {
        use crate::core::{
            model::{Track, TrackId},
            state::{AuthState, Page},
            update::update,
        };
        let mut state = State {
            auth: AuthState::SignedIn,
            ..Default::default()
        };
        update(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![Track {
                    id: TrackId("test".into()),
                    title: "A song with a long title to check the listening view layout".into(),
                    artists: vec![],
                    album: None,
                    album_id: None,
                    duration: None,
                    thumbnail_url: None,
                    playlist_item_id: None,
                }],
                start: 0,
            },
            &mut |_| 0,
        );
        state.page = Page::NowPlaying;
        for size in [[700., 480.], [1100., 720.], [1710., 1073.]] {
            let ctx = egui::Context::default();
            for _ in 0..2 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
                        ..Default::default()
                    },
                    |ui| {
                        super::super::view(ui, &state, &crate::theme::DefaultTheme);
                    },
                );
                output.textures_delta.clear();
                for needle in ["Lyrics", "Queue", "Equalizer"] {
                    assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == needle && t.pos.x + t.galley.size().x <= size[0] && t.pos.y + t.galley.size().y <= size[1])), "{needle} is offscreen at {size:?}");
                }
            }
        }
    }
}
