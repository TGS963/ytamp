//! Home combines the current listening session, YouTube discovery, and the library.
use super::rows::{self, RowContext};
use crate::{
    core::{
        action::Action,
        state::{Loadable, Page, PlayStatus, State},
    },
    theme::{ColorRole, TextRole, Theme},
};
use egui::{Sense, Ui, vec2};

pub fn view(
    ui: &mut Ui,
    state: &State,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    egui::ScrollArea::vertical().id_salt("home").show(ui, |ui| {
        ui.add_space(8.0);
        ui.label(theme.secondary_label(TextRole::Caption, "YOUR MUSIC"));
        super::components::page_title(ui, theme, "Home");
        ui.label(theme.secondary_label(
            TextRole::Body,
            "Pick up where you left off. Find your next favorite.",
        ));
        ui.add_space(20.0);
        search(ui, state, out);
        ui.add_space(24.0);
        continue_listening(ui, state, theme, context, out);
        ui.add_space(28.0);
        super::discovery::feed(ui, state, theme, context, out);
        section(ui, "Your playlists", "View library →", theme, out);
        match &state.library.playlists {
            Loadable::Loaded(playlists) | Loadable::Refreshing(playlists)
                if !playlists.is_empty() =>
            {
                let columns = columns(ui.available_width());
                let width = (ui.available_width() - (columns - 1) as f32 * 16.0) / columns as f32;
                egui::Grid::new(("home-playlists", columns, width.round() as u32))
                    .spacing([16., 20.])
                    .show(ui, |ui| {
                        for (index, playlist) in playlists.iter().take(columns).enumerate() {
                            ui.push_id(&playlist.id, |ui| {
                                let response = card(
                                    ui,
                                    width,
                                    &playlist.title,
                                    &playlist
                                        .track_count
                                        .map(|n| format!("{n} tracks"))
                                        .unwrap_or_else(|| "Playlist".into()),
                                    playlist.thumbnail_url.as_deref(),
                                    theme,
                                );
                                if response.clicked() {
                                    out.push(Action::PlaylistOpened(playlist.id.clone()));
                                }
                            });
                            if (index + 1) % columns == 0 {
                                ui.end_row();
                            }
                        }
                    });
            }
            Loadable::Failed(_) => {
                ui.label("Your playlists couldn’t load.");
                retry(ui, out);
            }
            Loadable::Loaded(_) | Loadable::Refreshing(_) => {
                ui.label(theme.secondary_label(
                    TextRole::Body,
                    "A place for every mood. Create your first playlist.",
                ));
                if ui.button("New playlist").clicked() {
                    out.push(Action::CreatePlaylistDialogOpened(None));
                }
            }
            _ => {
                ui.spinner();
                ui.weak("Loading your playlists…");
            }
        }
        ui.add_space(28.0);
        section(ui, "From your likes", "View all →", theme, out);
        if state
            .library
            .liked
            .loaded()
            .is_some_and(|tracks| !tracks.is_empty())
        {
            if ui.button("Shuffle your likes").clicked() {
                out.push(Action::LikedShuffleRequested);
            }
            ui.add_space(10.);
        }
        match &state.library.liked {
            Loadable::Loaded(tracks) | Loadable::Refreshing(tracks) if !tracks.is_empty() => {
                let columns = columns(ui.available_width());
                let width = (ui.available_width() - (columns - 1) as f32 * 16.0) / columns as f32;
                egui::Grid::new(("home-liked", columns, width.round() as u32))
                    .spacing([16., 20.])
                    .show(ui, |ui| {
                        for (index, track) in tracks.iter().take(columns).enumerate() {
                            ui.push_id(&track.id, |ui| {
                                let response = card(
                                    ui,
                                    width,
                                    &track.title,
                                    &track.artist_names(),
                                    track.thumbnail_url.as_deref(),
                                    theme,
                                )
                                .on_hover_text("Play · right-click for more");
                                if response.clicked() {
                                    out.push(Action::ContextPlayed {
                                        tracks: tracks.clone(),
                                        start: index,
                                    });
                                }
                                if let Some(action) =
                                    rows::row_context_menu(&response, track, context)
                                {
                                    out.push(action);
                                }
                            });
                        }
                    });
            }
            Loadable::Failed(_) => {
                ui.label("Your liked songs couldn’t load.");
                retry(ui, out);
            }
            Loadable::Loaded(_) | Loadable::Refreshing(_) => {
                ui.label(theme.secondary_label(
                    TextRole::Body,
                    "Like songs as you listen. They’ll be waiting here.",
                ));
            }
            _ => {
                ui.spinner();
                ui.weak("Loading your liked songs…");
            }
        }
        ui.add_space(28.0);
        if let Some(next) = state.playback.queue.peek_next() {
            surface(theme).show(ui, |ui| {
                ui.set_width((ui.available_width() - 4.0).max(0.0));
                ui.horizontal(|ui| {
                    rows::artwork(ui, theme, next.thumbnail_url.as_deref(), 48.0);
                    ui.vertical(|ui| {
                        ui.label(
                            theme.secondary_label(
                                TextRole::Caption,
                                if state
                                    .playback
                                    .queue
                                    .current()
                                    .is_some_and(|track| track.id == next.id)
                                {
                                    "ON REPEAT"
                                } else {
                                    "UP NEXT"
                                },
                            ),
                        );
                        ui.add(
                            egui::Label::new(theme.label(TextRole::Body, &next.title)).truncate(),
                        );
                        ui.label(theme.secondary_label(TextRole::Caption, next.artist_names()));
                    });
                });
                if ui.button("Open queue →").clicked() && !state.queue_open {
                    out.push(Action::QueuePanelToggled);
                }
            });
        }
        ui.add_space(24.0);
    });
}
fn columns(width: f32) -> usize {
    ((width + 16.) / 190.).floor().max(1.) as usize
}
fn surface(theme: &dyn Theme) -> egui::Frame {
    egui::Frame::new()
        .fill(theme.color(ColorRole::Surface))
        .corner_radius(12)
        .inner_margin(18)
        .stroke(egui::Stroke::new(1., theme.color(ColorRole::Border)))
}
// Hover-only frames must stay borderless: egui includes the border in layout size.
fn section(ui: &mut Ui, title: &str, link: &str, theme: &dyn Theme, out: &mut Vec<Action>) {
    if super::components::section(ui, theme, title, link, true).clicked() {
        out.push(Action::NavigatedTo(Page::Library));
    }
}
fn search(ui: &mut Ui, state: &State, out: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let mut input = state.search.input.clone();
        let response = ui.add_sized(
            [(ui.available_width() - 86.).max(100.), 36.],
            egui::TextEdit::singleline(&mut input)
                .hint_text("Search songs, artists, albums…")
                .margin(vec2(12., 9.)),
        );
        if response.changed() {
            out.push(Action::SearchInputChanged(input.clone()));
        }
        let submit = ui
            .add_sized([76., 36.], egui::Button::new("Search"))
            .clicked()
            || (response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        if submit && !input.trim().is_empty() {
            out.push(Action::NavigatedTo(Page::Search));
            out.push(Action::SearchSubmitted);
        }
    });
}
fn continue_listening(
    ui: &mut Ui,
    state: &State,
    theme: &dyn Theme,
    context: &RowContext,
    out: &mut Vec<Action>,
) {
    let Some(track) = state.playback.queue.current() else {
        surface(theme).show(ui, |ui| {
            ui.set_width((ui.available_width() - 4.).max(0.));
            ui.label(theme.label(TextRole::Title, "What’s on repeat today?"));
            ui.label(theme.secondary_label(
                TextRole::Body,
                "Choose a playlist below, or search for something you love.",
            ));
        });
        return;
    };
    let playing = matches!(
        state.playback.status,
        PlayStatus::Playing | PlayStatus::Loading
    );
    let width = ui.available_width();
    surface(theme)
        .fill(theme.color(ColorRole::AccentSoft))
        .show(ui, |ui| {
            ui.set_width((width - 38.).max(100.));
            ui.horizontal(|ui| {
                let art_size = if width > 540. { 136. } else { 96. };
                if width >= 400. {
                    rows::artwork(ui, theme, track.thumbnail_url.as_deref(), art_size);
                    ui.add_space(12.);
                }
                ui.vertical(|ui| {
                    ui.label(theme.secondary_label(
                        TextRole::Caption,
                        if playing {
                            "NOW PLAYING"
                        } else {
                            "CONTINUE LISTENING"
                        },
                    ));
                    ui.add(
                        egui::Label::new(theme.label(TextRole::Title, &track.title).strong())
                            .truncate(),
                    )
                    .on_hover_text(&track.title);
                    ui.add(
                        egui::Label::new(
                            theme.secondary_label(TextRole::Body, track.artist_names()),
                        )
                        .truncate(),
                    );
                    ui.add_space(12.);
                    let label = if playing {
                        "Pause".to_owned()
                    } else if state.playback.position.is_zero() {
                        "Play".into()
                    } else {
                        format!(
                            "Resume at {}",
                            rows::format_duration(state.playback.position)
                        )
                    };
                    let response =
                        ui.add(super::components::primary(theme, label).min_size(vec2(120., 36.)));
                    if response.clicked() {
                        out.push(Action::PlayToggled);
                    }
                    if let Some(action) = rows::row_context_menu(&response, track, context) {
                        out.push(action);
                    }
                });
            });
        });
}
pub(super) fn card(
    ui: &mut Ui,
    width: f32,
    title: &str,
    subtitle: &str,
    art: Option<&str>,
    theme: &dyn Theme,
) -> egui::Response {
    let height = width + 60.;
    let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    if response.hovered() {
        ui.painter()
            .rect_filled(rect.expand(6.), 10, theme.color(ColorRole::RowHover));
    }
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child.set_clip_rect(ui.clip_rect().intersect(rect));
    rows::artwork(&mut child, theme, art, width);
    child.add_space(6.);
    child.add(egui::Label::new(theme.label(TextRole::Body, title).strong()).truncate());
    child.add(egui::Label::new(theme.secondary_label(TextRole::Caption, subtitle)).truncate());
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(title)
}
fn retry(ui: &mut Ui, out: &mut Vec<Action>) {
    if ui.button("Retry").clicked() {
        out.push(Action::LibraryRefreshRequested);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::{Track, TrackId};
    use crate::theme::DefaultTheme;
    fn text_rect(shape: &egui::Shape, needle: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == needle => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text_rect(shape, needle)),
            _ => None,
        }
    }
    #[test]
    fn section_links_do_not_move_content_when_hovered() {
        for link in ["View library →", "View all →"] {
            let ctx = egui::Context::default();
            let frame = |events| {
                let mut actions = vec![];
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(700., 400.),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        super::super::apply_page_style(ui, &DefaultTheme);
                        section(ui, "Your playlists", link, &DefaultTheme, &mut actions);
                        ui.label("Content below the header");
                    },
                );
                output.textures_delta.clear();
                let rect = |needle| {
                    output
                        .shapes
                        .iter()
                        .find_map(|shape| text_rect(&shape.shape, needle))
                        .unwrap()
                };
                (rect(link), rect("Content below the header"), actions)
            };
            frame(vec![]);
            let (button, baseline, _) = frame(vec![]);
            for _ in 0..3 {
                let (_, hovered, _) = frame(vec![egui::Event::PointerMoved(button.center())]);
                assert_eq!(hovered, baseline, "{link} moves content on hover");
                let (_, away, _) = frame(vec![egui::Event::PointerGone]);
                assert_eq!(away, baseline, "{link} moves content after hover");
            }
        }
    }
    #[test]
    fn resume_uses_existing_session_and_home_renders_at_minimum_width() {
        let mut state = State {
            auth: crate::core::state::AuthState::SignedIn,
            page: Page::Home,
            ..Default::default()
        };
        crate::core::update::update(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![Track {
                    id: TrackId("example".into()),
                    title: "A long song title that must stay within its own panel".into(),
                    artists: vec![],
                    album: None,
                    album_id: None,
                    duration: Some(std::time::Duration::from_secs(200)),
                    thumbnail_url: None,
                    playlist_item_id: None,
                }],
                start: 0,
            },
            &mut |_| 0,
        );
        state.playback.status = PlayStatus::Stopped;
        state.playback.position = std::time::Duration::from_secs(73);
        state.search.input = "Coldplay".into();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(700., 600.));
        let frame = |events| {
            let mut actions = vec![];
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| {
                    actions = super::super::view(ui, &state, &DefaultTheme);
                },
            );
            output.textures_delta.clear();
            (output, actions)
        };
        frame(vec![]);
        let (output, _) = frame(vec![]);
        let resume = output
            .shapes
            .iter()
            .find_map(|shape| text_rect(&shape.shape, "Resume at 1:13"))
            .expect("visible resume button");
        assert!(screen.contains_rect(resume));
        let search = output
            .shapes
            .iter()
            .rev()
            .find_map(|shape| text_rect(&shape.shape, "Search"))
            .expect("search button");
        let pos = search.center();
        frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        let (_, actions) = frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, Action::SearchSubmitted))
        );

        let pos = resume.center();
        frame(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            },
        ]);
        let (_, actions) = frame(vec![egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Default::default(),
        }]);
        assert!(
            actions
                .iter()
                .any(|action| matches!(action, Action::PlayToggled))
        );
        assert!(
            !actions
                .iter()
                .any(|action| matches!(action, Action::ContextPlayed { .. }))
        );
    }
}
