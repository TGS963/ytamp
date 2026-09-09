use crate::core::action::Action;
use std::time::Duration;

use super::*;

#[test]
fn durations_format_as_minutes_and_seconds() {
    assert_eq!(format_duration(Duration::from_secs(65)), "1:05");
    assert_eq!(format_duration(Duration::from_secs(0)), "0:00");
    assert_eq!(format_duration(Duration::from_secs(600)), "10:00");
}

#[test]
fn an_unhovered_row_resets_even_with_no_timer_running() {
    assert_eq!(dwell_decision(false, None, 10.0, 0.4), DwellDecision::Reset);
}

#[test]
fn a_hover_with_no_timer_starts_one() {
    assert_eq!(dwell_decision(true, None, 10.0, 0.4), DwellDecision::Start);
}

#[test]
fn a_hover_short_of_the_delay_waits_for_the_remainder() {
    assert_eq!(
        dwell_decision(true, Some(10.0), 10.1, 0.4),
        DwellDecision::Wait {
            remaining: Duration::from_secs_f64(0.3)
        }
    );
}

#[test]
fn a_hover_at_the_delay_emits() {
    assert_eq!(
        dwell_decision(true, Some(10.0), 10.4, 0.4),
        DwellDecision::Emit
    );
}

#[test]
fn a_row_that_emitted_stays_quiet_until_the_pointer_leaves() {
    assert_eq!(
        dwell_decision(true, Some(EMITTED), 99.0, 0.4),
        DwellDecision::Done
    );
    assert_eq!(
        dwell_decision(false, Some(EMITTED), 99.0, 0.4),
        DwellDecision::Reset
    );
}

#[test]
fn a_hover_past_the_delay_still_emits() {
    assert_eq!(
        dwell_decision(true, Some(10.0), 20.0, 0.4),
        DwellDecision::Emit
    );
}

#[test]
fn the_pointer_leaving_a_timed_row_resets_it() {
    assert_eq!(
        dwell_decision(false, Some(10.0), 10.2, 0.4),
        DwellDecision::Reset
    );
}

mod menu_tests {
    use super::*;
    use crate::core::model::{ArtistRef, Playlist, TrackId};
    use crate::core::state::{Loadable, Page};
    use crate::theme::DefaultTheme;

    fn sample_track(id: &str) -> Track {
        Track {
            id: TrackId(id.to_string()),
            title: format!("Title {id}"),
            artists: vec![ArtistRef::named("Artist")],
            album: Some("Album".into()),
            album_id: None,
            duration: Some(Duration::from_secs(200)),
            thumbnail_url: None,
            playlist_item_id: None,
        }
    }

    fn frame(ctx: &egui::Context, events: Vec<egui::Event>, tracks: &[Track]) -> Vec<Action> {
        let liked = Loadable::Loaded(vec![]);
        let playlists: Vec<Playlist> = vec![];
        let page = Page::Library;
        let context = RowContext {
            liked: &liked,
            playlists: &playlists,
            page: &page,
        };
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1200.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let mut out = Vec::new();
        let mut output = ctx.run_ui(input, |ui| {
            track_list(ui, "test", tracks, false, &DefaultTheme, &context, &mut out);
        });
        output.textures_delta.clear();
        out
    }

    /// The visible layers above the page: a popup adds one.
    fn popup_layers(ctx: &egui::Context) -> usize {
        ctx.memory(|memory| {
            memory
                .areas()
                .visible_layer_ids()
                .iter()
                .filter(|layer| layer.order == egui::Order::Foreground)
                .count()
        })
    }

    fn right_click_at(ctx: &egui::Context, tracks: &[Track], pos: egui::Pos2) -> bool {
        let button = egui::PointerButton::Secondary;
        frame(ctx, vec![egui::Event::PointerMoved(pos)], tracks);
        frame(
            ctx,
            vec![egui::Event::PointerButton {
                pos,
                button,
                pressed: true,
                modifiers: Default::default(),
            }],
            tracks,
        );
        frame(
            ctx,
            vec![egui::Event::PointerButton {
                pos,
                button,
                pressed: false,
                modifiers: Default::default(),
            }],
            tracks,
        );
        let open_after_release = popup_layers(ctx) > 0;
        frame(ctx, vec![], tracks);
        frame(ctx, vec![], tracks);
        let open_two_frames_later = popup_layers(ctx) > 0;
        eprintln!(
            "pos {pos:?}: open after release {open_after_release}, two frames later {open_two_frames_later}"
        );
        open_two_frames_later
    }

    fn page_frame(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        state: &crate::core::state::State,
    ) {
        let clock = egui::Id::new("test_clock");
        let time = ctx.data(|data| data.get_temp::<f64>(clock)).unwrap_or(0.0) + 0.016;
        ctx.data_mut(|data| data.insert_temp(clock, time));
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1370.0, 923.0),
            )),
            time: Some(time),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(input, |ui| {
            let _ = crate::ui::view(ui, state, &DefaultTheme);
        });
        output.textures_delta.clear();
    }

    fn page_right_click_at(
        ctx: &egui::Context,
        state: &crate::core::state::State,
        pos: egui::Pos2,
    ) -> bool {
        let button = egui::PointerButton::Secondary;
        page_frame(ctx, vec![egui::Event::PointerMoved(pos)], state);
        page_frame(
            ctx,
            vec![egui::Event::PointerButton {
                pos,
                button,
                pressed: true,
                modifiers: Default::default(),
            }],
            state,
        );
        page_frame(
            ctx,
            vec![egui::Event::PointerButton {
                pos,
                button,
                pressed: false,
                modifiers: Default::default(),
            }],
            state,
        );
        let after_release = popup_layers(ctx) > 0;
        page_frame(ctx, vec![], state);
        page_frame(ctx, vec![], state);
        let later = popup_layers(ctx) > 0;
        eprintln!("page pos {pos:?}: open after release {after_release}, two frames later {later}");
        later
    }

    #[test]
    fn a_same_frame_press_and_release_opens_the_menu_too() {
        use crate::core::model::PlaylistId;
        use crate::core::state::{AuthState, State};
        let tracks: Vec<Track> = (0..8).map(|i| sample_track(&i.to_string())).collect();
        let mut state = State {
            auth: AuthState::SignedIn,
            page: Page::Playlist(PlaylistId("p".into())),
            ..State::default()
        };
        state.library.open_playlist = Loadable::Loaded(tracks);
        state.library.liked = Loadable::Loaded(vec![]);
        state.library.playlists = Loadable::Loaded(vec![]);
        let ctx = egui::Context::default();
        page_frame(&ctx, vec![], &state);
        page_frame(&ctx, vec![], &state);
        let pos = egui::pos2(848.0, 371.0);
        let button = egui::PointerButton::Secondary;
        page_frame(
            &ctx,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: true,
                    modifiers: Default::default(),
                },
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: false,
                    modifiers: Default::default(),
                },
            ],
            &state,
        );
        let after = popup_layers(&ctx) > 0;
        page_frame(
            &ctx,
            vec![egui::Event::PointerMoved(egui::pos2(850.0, 373.0))],
            &state,
        );
        let after_jitter = popup_layers(&ctx) > 0;
        page_frame(
            &ctx,
            vec![egui::Event::PointerMoved(egui::pos2(740.0, 371.0))],
            &state,
        );
        let mut after_left_move = popup_layers(&ctx) > 0;
        for i in 0..70 {
            page_frame(&ctx, vec![], &state);
            if after_left_move && popup_layers(&ctx) == 0 {
                eprintln!("closed {i} frames after the left move");
                after_left_move = false;
            }
        }
        page_frame(
            &ctx,
            vec![egui::Event::PointerMoved(egui::pos2(880.0, 400.0))],
            &state,
        );
        let after_move_into_menu = popup_layers(&ctx) > 0;
        page_frame(&ctx, vec![], &state);
        let later = popup_layers(&ctx) > 0;
        eprintln!(
            "same frame: after {after}, jitter {after_jitter}, left move {after_left_move}, into menu {after_move_into_menu}, later {later}"
        );
        assert!(later);
    }

    #[test]
    fn a_right_click_on_a_playlist_page_row_opens_the_menu() {
        use crate::core::model::PlaylistId;
        use crate::core::state::{AuthState, State};
        let tracks: Vec<Track> = (0..8).map(|i| sample_track(&i.to_string())).collect();
        let mut state = State {
            auth: AuthState::SignedIn,
            page: Page::Playlist(PlaylistId("p".into())),
            ..State::default()
        };
        state.library.open_playlist = Loadable::Loaded(tracks);
        state.library.liked = Loadable::Loaded(vec![]);
        state.library.playlists = Loadable::Loaded(vec![]);
        let ctx = egui::Context::default();
        page_frame(&ctx, vec![], &state);
        page_frame(&ctx, vec![], &state);
        let middle = page_right_click_at(&ctx, &state, egui::pos2(848.0, 371.0));
        let ctx2 = egui::Context::default();
        page_frame(&ctx2, vec![], &state);
        page_frame(&ctx2, vec![], &state);
        let right = page_right_click_at(&ctx2, &state, egui::pos2(1228.0, 365.0));
        assert!(right, "the far right must open the menu");
        assert!(middle, "the middle must open the menu");
    }

    #[test]
    fn a_right_click_anywhere_on_a_row_opens_and_keeps_the_menu() {
        let tracks: Vec<Track> = (0..5).map(|i| sample_track(&i.to_string())).collect();
        let ctx = egui::Context::default();
        frame(&ctx, vec![], &tracks);
        frame(&ctx, vec![], &tracks);
        let row_y = 8.0 + 36.0 * 2.5;
        let middle = right_click_at(&ctx, &tracks, egui::pos2(600.0, row_y));
        let ctx2 = egui::Context::default();
        frame(&ctx2, vec![], &tracks);
        frame(&ctx2, vec![], &tracks);
        let far_right = right_click_at(&ctx2, &tracks, egui::pos2(1150.0, row_y));
        assert!(far_right, "the far right must open the menu");
        assert!(middle, "the middle must open the menu");
    }
}
