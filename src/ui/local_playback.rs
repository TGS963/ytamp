//! The signed-out local library: a compact player shell with no account UI.

use crate::{
    core::{action::Action, state::State},
    theme::{ColorRole, TextRole, Theme},
};

pub fn view(ui: &mut egui::Ui, state: &State, theme: &dyn Theme) -> Vec<Action> {
    let mut out = vec![];
    super::keyboard_shortcuts(ui, &mut out);
    super::player_bar::view(ui, state, theme, &mut out);
    sidebar(ui, state, theme, &mut out);
    super::notices(ui, state, theme, &mut out);
    egui::CentralPanel::default()
        .frame(
            egui::Frame::central_panel(ui.style())
                .fill(theme.color(ColorRole::PageBackground))
                .inner_margin(egui::Margin::symmetric(24, 18)),
        )
        .show(ui, |ui| content(ui, state, theme, &mut out));
    out
}

fn sidebar(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    let width = sidebar_width(ui.ctx().content_rect().width());
    let frame = super::panel_frame(ui, theme, ColorRole::PanelBackground).inner_margin(12);
    egui::Panel::left("local-sidebar")
        .exact_size(width)
        .frame(frame)
        .show(ui, |ui| {
            ui.add_space(10.);
            super::brand_header(ui, theme);
            ui.add_space(24.);
            ui.label(
                theme
                    .secondary_label(TextRole::Caption, "LOCAL FILES")
                    .strong(),
            );
            ui.add_space(8.);
            let button_width = ui.available_width();
            if add_files_button(ui, theme, button_width) {
                out.push(Action::AddFilesRequested);
            }
            ui.add_space(14.);
            if nav_button(ui, theme, "Now playing", !state.queue_open) && state.queue_open {
                out.push(Action::QueuePanelToggled);
            }
            if nav_button(ui, theme, "Queue", state.queue_open) && !state.queue_open {
                out.push(Action::QueuePanelToggled);
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                footer_button(
                    ui,
                    theme,
                    "Connect YouTube",
                    Action::YouTubeSignInOpened,
                    out,
                );
                footer_button(ui, theme, "Winamp mode", Action::WinampToggled, out);
                ui.add_space(4.);
                ui.separator();
            });
        });
}

fn sidebar_width(available: f32) -> f32 {
    if available < 900. { 160. } else { 190. }
}

fn nav_button(ui: &mut egui::Ui, theme: &dyn Theme, label: &str, selected: bool) -> bool {
    let color = if selected {
        theme.color(ColorRole::TextPrimary)
    } else {
        theme.color(ColorRole::TextSecondary)
    };
    let button = egui::Button::new(theme.label(TextRole::Body, label).color(color).strong())
        .fill(if selected {
            theme.color(ColorRole::AccentSoft)
        } else {
            egui::Color32::TRANSPARENT
        })
        .stroke(egui::Stroke::NONE)
        .right_text(if selected { "•" } else { "" });
    ui.add_sized([ui.available_width(), 36.], button).clicked()
}

fn add_files_button(ui: &mut egui::Ui, theme: &dyn Theme, width: f32) -> bool {
    ui.add_sized(
        [width, 36.],
        egui::Button::new(
            theme
                .label(TextRole::Body, "Add files")
                .color(theme.color(ColorRole::OnAccent))
                .strong(),
        )
        .fill(theme.color(ColorRole::Accent))
        .stroke(egui::Stroke::NONE),
    )
    .clicked()
}

fn footer_button(
    ui: &mut egui::Ui,
    theme: &dyn Theme,
    label: &str,
    action: Action,
    out: &mut Vec<Action>,
) {
    if ui
        .add_sized(
            [ui.available_width(), 30.],
            egui::Button::new(theme.secondary_label(TextRole::Body, label)).frame(false),
        )
        .clicked()
    {
        out.push(action);
    }
}

fn add_files_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "⌘O  Add files"
    } else {
        "Ctrl+O  Add files"
    }
}

fn content(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    if state.queue_open {
        queue(ui, state, theme, out);
    } else if state.playback.queue.current().is_some() {
        super::now_playing::view(ui, state, theme, out);
    } else {
        empty(ui, theme, out);
    }
}

fn queue(ui: &mut egui::Ui, state: &State, theme: &dyn Theme, out: &mut Vec<Action>) {
    ui.label(theme.label(TextRole::Title, "Queue"));
    ui.add_space(12.);
    if let Some(track) = state.playback.queue.current() {
        ui.label(theme.secondary_label(TextRole::Caption, "NOW PLAYING"));
        ui.add(egui::Label::new(theme.label(TextRole::Heading, &track.title)).truncate());
        ui.label(theme.secondary_label(TextRole::Body, track.artist_names()));
        ui.add_space(16.);
    } else {
        ui.label(theme.secondary_label(TextRole::Body, "Add files to begin a queue."));
        ui.add_space(12.);
    }
    ui.horizontal(|ui| {
        ui.label(theme.secondary_label(TextRole::Caption, "UP NEXT"));
        if ui.small_button("Clear queued tracks").clicked() {
            out.push(Action::QueueCleared);
        }
    });
    super::queue::upcoming_entries(ui, state, theme, out);
}

fn empty(ui: &mut egui::Ui, theme: &dyn Theme, out: &mut Vec<Action>) {
    let available = ui.available_height();
    ui.vertical_centered(|ui| {
        ui.add_space((available * 0.16).clamp(24., 96.));
        music_motif(ui, theme);
        ui.add_space(18.);
        ui.label(theme.label(TextRole::Title, "Bring your music.").strong());
        ui.add_space(8.);
        ui.label(theme.secondary_label(
            TextRole::Body,
            "Drop audio or video files here, or choose them from your computer.",
        ));
        ui.add_space(16.);
        if add_files_button(ui, theme, 132.) {
            out.push(Action::AddFilesRequested);
        }
        ui.add_space(8.);
        ui.label(theme.secondary_label(TextRole::Caption, add_files_hint()));
    });
}

fn music_motif(ui: &mut egui::Ui, theme: &dyn Theme) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(104., 84.), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let file =
        egui::Rect::from_center_size(rect.center() + egui::vec2(-10., 0.), egui::vec2(42., 54.));
    let border = egui::Stroke::new(1.5, theme.color(ColorRole::Border));
    painter.rect_stroke(file, 7., border, egui::StrokeKind::Inside);
    painter.line_segment(
        [
            file.right_top() + egui::vec2(-12., 0.),
            file.right_top() + egui::vec2(0., 12.),
        ],
        border,
    );
    painter.line_segment(
        [
            file.right_top() + egui::vec2(-12., 0.),
            file.right_top() + egui::vec2(-12., 12.),
        ],
        border,
    );
    let accent = theme.color(ColorRole::Accent);
    let note = file.center() + egui::vec2(2., 5.);
    painter.line_segment(
        [note + egui::vec2(7., -16.), note + egui::vec2(7., 8.)],
        egui::Stroke::new(3., accent),
    );
    painter.line_segment(
        [note + egui::vec2(7., -16.), note + egui::vec2(20., -12.)],
        egui::Stroke::new(3., accent),
    );
    painter.circle_filled(note, 7., accent);
    painter.circle_filled(
        rect.center() + egui::vec2(36., 24.),
        3.,
        theme.color(ColorRole::AccentSoft),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        core::state::{AuthState, State},
        theme::DefaultTheme,
    };

    fn text_rect(shape: &egui::Shape, needle: &str) -> Option<egui::Rect> {
        match shape {
            egui::Shape::Text(text) if text.galley.text() == needle => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            egui::Shape::Vec(shapes) => shapes.iter().find_map(|shape| text_rect(shape, needle)),
            _ => None,
        }
    }

    fn local_state() -> State {
        State {
            auth: AuthState::SignedOut,
            local_mode: true,
            ..Default::default()
        }
    }

    fn state_with_track(queue_open: bool) -> State {
        use crate::core::{
            model::{MediaSource, Track, TrackId},
            update::update,
        };
        let mut state = local_state();
        let mut random = |_| 0;
        update(
            &mut state,
            Action::ContextPlayed {
                tracks: vec![Track {
                    source: MediaSource::LocalFile {
                        path: "test-song.wav".into(),
                    },
                    id: TrackId("local:test-song".into()),
                    title: "A local test track".into(),
                    artists: vec![],
                    album: None,
                    album_id: None,
                    duration: None,
                    thumbnail_url: None,
                    playlist_item_id: None,
                }],
                start: 0,
            },
            &mut random,
        );
        state.queue_open = queue_open;
        state
    }

    fn assert_text_in_bounds(output: &egui::FullOutput, screen: egui::Rect, text: &str) {
        let rects: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| text_rect(&shape.shape, text))
            .collect();
        assert!(!rects.is_empty(), "{text:?} must be visible");
        assert!(
            rects.iter().all(|rect| screen.contains_rect(*rect)),
            "{text:?} escapes the viewport: {rects:?}"
        );
    }

    #[test]
    fn local_shell_keeps_empty_now_playing_and_queue_content_in_bounds() {
        let states = [
            (
                local_state(),
                vec![
                    "Bring your music.",
                    "Add files",
                    add_files_hint(),
                    "Winamp mode",
                    "Connect YouTube",
                ],
            ),
            (
                state_with_track(false),
                vec!["A local test track", "Now playing"],
            ),
            (
                state_with_track(true),
                vec!["Queue", "A local test track", "UP NEXT"],
            ),
        ];
        for size in [[700., 480.], [1100., 720.], [1710., 1073.]] {
            let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size.into());
            for (state, labels) in &states {
                let ctx = egui::Context::default();
                for pass in 0..2 {
                    let mut output = ctx.run_ui(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            ..Default::default()
                        },
                        |ui| {
                            super::super::view(ui, state, &DefaultTheme);
                        },
                    );
                    output.textures_delta.clear();
                    if pass == 1 {
                        for label in labels {
                            assert_text_in_bounds(&output, screen, label);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn add_files_sidebar_button_returns_the_import_action() {
        let state = local_state();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700., 480.));
        let frame = |events| {
            let mut actions = vec![];
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| actions = super::view(ui, &state, &DefaultTheme),
            );
            output.textures_delta.clear();
            (output, actions)
        };
        frame(vec![]);
        let (output, _) = frame(vec![]);
        let add = output
            .shapes
            .iter()
            .find_map(|shape| text_rect(&shape.shape, "Add files"))
            .expect("sidebar add files button is visible");
        let pos = add.center();
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
                .any(|action| matches!(action, Action::AddFilesRequested))
        );
    }

    #[test]
    fn queue_sidebar_button_returns_the_navigation_toggle() {
        let state = local_state();
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700., 480.));
        let frame = |events| {
            let mut actions = vec![];
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ui| actions = super::view(ui, &state, &DefaultTheme),
            );
            output.textures_delta.clear();
            (output, actions)
        };
        frame(vec![]);
        let (output, _) = frame(vec![]);
        let queue = output
            .shapes
            .iter()
            .filter_map(|shape| text_rect(&shape.shape, "Queue"))
            .find(|rect| rect.center().x < sidebar_width(screen.width()))
            .expect("sidebar queue button is visible");
        let pos = queue.center();
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
                .any(|action| matches!(action, Action::QueuePanelToggled))
        );
    }
}
