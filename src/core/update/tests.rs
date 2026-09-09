use super::*;
use crate::core::model::TrackId;

fn track(id: &str) -> Track {
    Track {
        id: TrackId(id.to_string()),
        title: id.to_string(),
        artists: vec![],
        album: None,
        album_id: None,
        duration: None,
        thumbnail_url: None,
        playlist_item_id: None,
    }
}

#[test]
fn retry_then_skip_does_not_seek_the_next_song_to_the_failed_position() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("failed"), track("next")],
            start: 0,
        },
    );
    apply(
        &mut state,
        Action::Player(PlayerEvent::PositionChanged(Duration::from_secs(73))),
    );
    apply(
        &mut state,
        Action::Player(PlayerEvent::Failed("offline".into())),
    );
    apply(&mut state, Action::PlaybackRetryRequested);
    apply(&mut state, Action::NextPressed);
    assert_eq!(state.playback.position, Duration::ZERO);
    assert!(state.playback.resume_position.is_none());
    assert!(state.playback.error.is_none());
}
#[test]
fn repeated_recovery_keeps_position_until_the_stream_starts() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("recovery")],
            start: 0,
        },
    );
    let send = |state: &mut State, event| {
        let generation = state.playback_generation;
        apply(state, Action::ForPlayback { generation, event })
    };
    send(
        &mut state,
        PlayerEvent::PositionChanged(Duration::from_secs(73)),
    );
    send(&mut state, PlayerEvent::Failed("connection lost".into()));
    for _ in 0..3 {
        apply(&mut state, Action::PlayToggled);
        assert_eq!(state.playback.position, Duration::from_secs(73));
        send(&mut state, PlayerEvent::Failed("expired stream".into()));
    }
    apply(&mut state, Action::PlayToggled);
    let effects = send(
        &mut state,
        PlayerEvent::TrackStarted {
            duration: Some(Duration::from_secs(200)),
            channels: 2,
            sample_rate: 44100,
        },
    );
    assert!(
        effects.contains(&Effect::Player(PlayerCommand::Seek(Duration::from_secs(
            73
        ))))
    );
}

fn no_random(_: usize) -> usize {
    0
}

fn apply(state: &mut State, action: Action) -> Vec<Effect> {
    update(state, action, &mut no_random)
}

#[test]
fn rapid_skips_ignore_every_kind_of_late_player_report() {
    let mut state = State::default();
    state.playback.autoplay = false;
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a"), track("b"), track("c")],
            start: 0,
        },
    );
    state.playback.queue.repeat = super::super::queue::RepeatMode::All;
    for step in 0..1000 {
        let stale = state.playback_generation;
        apply(
            &mut state,
            if step % 3 == 0 {
                Action::PreviousPressed
            } else {
                Action::NextPressed
            },
        );
        let expected = state.playback.clone();
        for event in [
            PlayerEvent::TrackStarted {
                duration: Some(Duration::from_secs(999)),
                channels: 6,
                sample_rate: 96000,
            },
            PlayerEvent::PositionChanged(Duration::from_secs(999)),
            PlayerEvent::TrackEnded,
            PlayerEvent::Failed("old error".into()),
        ] {
            let effects = apply(
                &mut state,
                Action::ForPlayback {
                    generation: stale,
                    event,
                },
            );
            assert!(effects.is_empty());
            assert_eq!(state.playback.queue, expected.queue);
            assert_eq!(state.playback.status, expected.status);
            assert_eq!(state.playback.position, expected.position);
            assert!(state.notices.is_empty());
        }
    }
    let stale = state.playback_generation;
    apply(&mut state, Action::SignOutRequested);
    apply(
        &mut state,
        Action::ForPlayback {
            generation: stale,
            event: PlayerEvent::TrackEnded,
        },
    );
    assert!(state.playback.queue.current().is_none());
}
#[test]
fn seek_during_loading_keeps_last_target_and_pause_until_ready() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    apply(&mut state, Action::PlayToggled);
    for second in 0..100 {
        assert!(
            apply(
                &mut state,
                Action::SeekRequested(Duration::from_secs(second))
            )
            .is_empty()
        );
    }
    let generation = state.playback_generation;
    let effects = apply(
        &mut state,
        Action::ForPlayback {
            generation,
            event: PlayerEvent::TrackStarted {
                duration: None,
                channels: 2,
                sample_rate: 48000,
            },
        },
    );
    assert_eq!(state.playback.status, PlayStatus::Paused);
    assert!(
        effects.contains(&Effect::Player(PlayerCommand::Seek(Duration::from_secs(
            99
        ))))
    );
    assert!(state.playback.resume_position.is_none());
}
#[test]
fn pause_during_loading_survives_the_decoder_ready_event() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    apply(&mut state, Action::PlayToggled);
    assert_eq!(state.playback.status, PlayStatus::Paused);
    let effects = apply(
        &mut state,
        Action::Player(PlayerEvent::TrackStarted {
            duration: None,
            channels: 2,
            sample_rate: 44100,
        }),
    );
    assert_eq!(state.playback.status, PlayStatus::Paused);
    assert!(!effects.contains(&Effect::Player(PlayerCommand::Resume)));
    assert_eq!(
        apply(&mut state, Action::PlayToggled),
        vec![Effect::Player(PlayerCommand::Resume)]
    );
    assert_eq!(state.playback.status, PlayStatus::Playing);
}

#[test]
fn a_late_previous_account_result_cannot_change_a_new_account() {
    let mut state = State {
        auth: AuthState::SignedIn,
        session_generation: 4,
        ..State::default()
    };
    apply(&mut state, Action::SignOutRequested);
    apply(
        &mut state,
        Action::StoredAuthFound(super::super::effect::AuthMethod::OAuthToken("test".into())),
    );
    let current = state.session_generation;
    apply(
        &mut state,
        Action::ForSession {
            generation: current,
            action: Box::new(Action::AuthVerified(Ok(()))),
        },
    );
    let liked = state.library.liked.clone();
    let effects = apply(
        &mut state,
        Action::ForSession {
            generation: 4,
            action: Box::new(Action::LikedPageLoaded {
                tracks: vec![track("old")],
                finished: true,
            }),
        },
    );
    assert!(effects.is_empty(), "stale results must not write the cache");
    assert_eq!(state.library.liked, liked);
    assert_eq!(state.auth, AuthState::SignedIn);
}

#[test]
fn repeated_refresh_does_not_start_overlapping_page_streams() {
    let mut state = State::default();
    assert!(!apply(&mut state, Action::LibraryRefreshRequested).is_empty());
    assert!(apply(&mut state, Action::LibraryRefreshRequested).is_empty());
}

#[test]
fn sign_out_stops_playback_and_keeps_the_volume() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    apply(&mut state, Action::VolumeSet(0.3));
    let effects = apply(&mut state, Action::SignOutRequested);
    assert_eq!(state.playback.queue.current(), None);
    assert_eq!(state.playback.status, PlayStatus::Stopped);
    assert_eq!(state.playback.volume, 0.3);
    assert_eq!(
        effects,
        vec![
            Effect::Player(PlayerCommand::Stop),
            Effect::ClearCredentials,
            Effect::ClearLibraryCache,
        ]
    );
}

#[test]
fn home_shuffle_uses_liked_music_and_leaves_empty_library_alone() {
    let mut state = State::default();
    assert!(apply(&mut state, Action::LikedShuffleRequested).is_empty());
    state.library.liked = Loadable::Loaded(vec![track("a"), track("b")]);
    let effects = update(&mut state, Action::LikedShuffleRequested, &mut |n| n - 1);
    assert!(state.playback.queue.shuffle);
    assert_eq!(state.playback.queue.current().unwrap().id.0, "b");
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Player(PlayerCommand::Load(_))))
    );
}
#[test]
fn sign_in_success_opens_home_and_fetches_library() {
    let mut state = State::default();
    let effects = apply(&mut state, Action::AuthVerified(Ok(())));
    assert_eq!(state.auth, AuthState::SignedIn);
    assert_eq!(state.page, Page::Home);
    assert_eq!(
        effects,
        vec![
            Effect::Api(ApiRequest::FetchPlaylists),
            Effect::Api(ApiRequest::FetchLiked),
            Effect::LoadLibraryCache,
            Effect::Api(ApiRequest::FetchDiscovery {
                request_id: 1,
                target: super::super::discovery::Target::home(),
                continuation: None
            }),
        ]
    );
}

#[test]
fn reopening_the_library_does_not_refetch() {
    let mut state = State::default();
    apply(&mut state, Action::AuthVerified(Ok(())));
    let effects = apply(&mut state, Action::NavigatedTo(Page::Library));
    assert_eq!(effects, vec![]);
}

#[test]
fn cache_data_fills_a_loading_slot() {
    let mut state = State::default();
    apply(&mut state, Action::AuthVerified(Ok(())));
    let playlist = Playlist {
        id: PlaylistId("p1".into()),
        title: "Chill".into(),
        track_count: Some(3),
        thumbnail_url: None,
    };
    apply(
        &mut state,
        Action::LibraryCacheLoaded {
            playlists: Some(vec![playlist.clone()]),
            liked: Some(vec![track("a")]),
        },
    );
    assert_eq!(
        state.library.playlists,
        Loadable::Refreshing(vec![playlist])
    );
    assert_eq!(state.library.liked, Loadable::Refreshing(vec![track("a")]));
}

#[test]
fn fresh_network_data_is_never_overwritten_by_a_late_cache_hit() {
    let mut state = State::default();
    apply(&mut state, Action::AuthVerified(Ok(())));
    apply(&mut state, Action::LikedLoaded(Ok(vec![track("fresh")])));

    apply(
        &mut state,
        Action::LibraryCacheLoaded {
            playlists: None,
            liked: Some(vec![track("stale")]),
        },
    );

    assert_eq!(state.library.liked, Loadable::Loaded(vec![track("fresh")]));
}

#[test]
fn fresh_liked_songs_are_saved_to_the_cache() {
    let mut state = State::default();
    apply(&mut state, Action::AuthVerified(Ok(())));
    let effects = apply(&mut state, Action::LikedLoaded(Ok(vec![track("a")])));
    assert_eq!(
        effects,
        vec![Effect::SaveLibraryCache(LibraryCacheWrite::Liked(vec![
            track("a")
        ]))]
    );
}

#[test]
fn opening_a_playlist_reads_the_cache_and_the_network() {
    let mut state = State::default();
    let effects = apply(&mut state, Action::PlaylistOpened(PlaylistId("p1".into())));
    assert_eq!(
        effects,
        vec![
            Effect::Api(ApiRequest::FetchPlaylistTracks(PlaylistId("p1".into()))),
            Effect::LoadPlaylistTracksCache(PlaylistId("p1".into())),
        ]
    );
}

#[test]
fn a_playlist_cache_hit_fills_the_loading_page() {
    let mut state = State::default();
    apply(&mut state, Action::PlaylistOpened(PlaylistId("p1".into())));
    apply(
        &mut state,
        Action::PlaylistTracksCacheLoaded(PlaylistId("p1".into()), vec![track("cached")]),
    );
    assert_eq!(
        state.library.open_playlist,
        Loadable::Refreshing(vec![track("cached")])
    );
}

#[test]
fn a_stale_playlist_cache_hit_is_ignored_after_leaving_the_page() {
    let mut state = State::default();
    apply(&mut state, Action::PlaylistOpened(PlaylistId("p1".into())));
    apply(&mut state, Action::NavigatedTo(Page::Search));
    apply(
        &mut state,
        Action::PlaylistTracksCacheLoaded(PlaylistId("p1".into()), vec![track("cached")]),
    );
    assert_eq!(state.library.open_playlist, Loadable::Loading);
}

fn artist_page(id: &str) -> ArtistPage {
    ArtistPage {
        id: ArtistId(id.to_string()),
        name: id.to_string(),
        thumbnail_url: None,
        top_songs: vec![],
        albums: vec![],
        singles: vec![],
    }
}

fn album(id: &str) -> crate::core::model::Album {
    crate::core::model::Album {
        id: AlbumId(id.to_string()),
        title: id.to_string(),
        artists: vec![],
        year: None,
        thumbnail_url: None,
    }
}

fn album_page(id: &str) -> AlbumPage {
    AlbumPage {
        album: album(id),
        tracks: vec![],
    }
}

#[test]
fn opening_an_artist_pushes_history_and_fetches() {
    let mut state = State {
        page: Page::Search,
        ..State::default()
    };
    let effects = apply(&mut state, Action::ArtistOpened(ArtistId("ar1".into())));
    assert_eq!(state.page, Page::Artist(ArtistId("ar1".into())));
    assert_eq!(state.history, vec![Page::Search]);
    assert_eq!(state.browse.artist, Loadable::Loading);
    assert_eq!(
        effects,
        vec![Effect::Api(ApiRequest::FetchArtist(ArtistId("ar1".into())))]
    );
}

#[test]
fn opening_the_same_artist_twice_is_a_no_op() {
    let mut state = State {
        page: Page::Search,
        ..State::default()
    };
    apply(&mut state, Action::ArtistOpened(ArtistId("ar1".into())));
    let effects = apply(&mut state, Action::ArtistOpened(ArtistId("ar1".into())));
    assert_eq!(effects, vec![]);
    assert_eq!(state.history, vec![Page::Search]);
}

#[test]
fn an_artist_search_request_fills_and_submits_the_search() {
    let mut state = State {
        page: Page::Library,
        ..State::default()
    };
    let effects = apply(
        &mut state,
        Action::ArtistSearchRequested("Radiohead".to_string()),
    );
    assert_eq!(state.search.input, "Radiohead");
    assert_eq!(state.page, Page::Search);
    assert_eq!(state.history, vec![Page::Library]);
    assert_eq!(state.search.results, Loadable::Loading);
    assert_eq!(
        effects,
        vec![Effect::Api(ApiRequest::Search {
            request_id: 1,
            query: "Radiohead".to_string()
        })]
    );
}

#[test]
fn back_pops_and_restores_a_playlist() {
    let mut state = State {
        page: Page::Search,
        ..State::default()
    };
    apply(&mut state, Action::PlaylistOpened(PlaylistId("p1".into())));
    apply(
        &mut state,
        Action::PlaylistTracksLoaded(PlaylistId("p1".into()), Ok(vec![track("a")])),
    );
    apply(&mut state, Action::ArtistOpened(ArtistId("ar1".into())));
    let effects = apply(&mut state, Action::BackPressed);
    assert_eq!(state.page, Page::Playlist(PlaylistId("p1".into())));
    assert_eq!(state.history, vec![Page::Search]);
    assert_eq!(state.library.open_playlist, Loadable::Loading);
    assert_eq!(
        effects,
        vec![
            Effect::Api(ApiRequest::FetchPlaylistTracks(PlaylistId("p1".into()))),
            Effect::LoadPlaylistTracksCache(PlaylistId("p1".into())),
        ]
    );
}

#[test]
fn back_onto_a_loaded_album_does_not_refetch() {
    let mut state = State {
        page: Page::Search,
        ..State::default()
    };
    apply(&mut state, Action::AlbumOpened(AlbumId("al1".into())));
    apply(
        &mut state,
        Action::AlbumLoaded(AlbumId("al1".into()), Ok(album_page("al1"))),
    );
    apply(&mut state, Action::ArtistOpened(ArtistId("ar1".into())));
    let effects = apply(&mut state, Action::BackPressed);
    assert_eq!(state.page, Page::Album(AlbumId("al1".into())));
    assert_eq!(effects, vec![]);
    assert_eq!(state.browse.album, Loadable::Loaded(album_page("al1")));
}

#[test]
fn back_with_an_empty_history_does_nothing() {
    let mut state = State {
        page: Page::Search,
        ..State::default()
    };
    let effects = apply(&mut state, Action::BackPressed);
    assert_eq!(state.page, Page::Search);
    assert_eq!(effects, vec![]);
}

#[test]
fn a_stale_artist_result_is_ignored() {
    let mut state = State {
        page: Page::Search,
        ..State::default()
    };
    apply(&mut state, Action::ArtistOpened(ArtistId("ar1".into())));
    apply(&mut state, Action::NavigatedTo(Page::Search));
    apply(
        &mut state,
        Action::ArtistLoaded(ArtistId("ar1".into()), Ok(artist_page("ar1"))),
    );
    assert_eq!(state.browse.artist, Loadable::Loading);
}

#[test]
fn a_failed_album_load_sets_failed() {
    let mut state = State {
        page: Page::Search,
        ..State::default()
    };
    apply(&mut state, Action::AlbumOpened(AlbumId("al1".into())));
    apply(
        &mut state,
        Action::AlbumLoaded(AlbumId("al1".into()), Err("no album".into())),
    );
    assert_eq!(state.browse.album, Loadable::Failed("no album".into()));
}

#[test]
fn sidebar_navigation_clears_history() {
    let mut state = State {
        page: Page::Search,
        ..State::default()
    };
    apply(&mut state, Action::ArtistOpened(ArtistId("ar1".into())));
    assert_eq!(state.history, vec![Page::Search]);
    apply(&mut state, Action::NavigatedTo(Page::Library));
    assert_eq!(state.history, vec![]);
}

#[test]
fn sign_out_resets_browse_and_history() {
    let mut state = State {
        page: Page::Search,
        ..State::default()
    };
    apply(&mut state, Action::ArtistOpened(ArtistId("ar1".into())));
    apply(&mut state, Action::SignOutRequested);
    assert_eq!(state.page, Page::SignIn);
    assert_eq!(state.history, vec![]);
    assert_eq!(state.browse.artist, Loadable::NotAsked);
}

#[test]
fn playing_a_context_loads_the_start_track() {
    let mut state = State::default();
    let effects = apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a"), track("b")],
            start: 1,
        },
    );
    assert_eq!(state.playback.status, PlayStatus::Loading);
    assert_eq!(
        effects,
        vec![Effect::Player(PlayerCommand::Load(track("b")))]
    );
}

#[test]
fn track_end_at_the_queue_end_stops_the_player_with_autoplay_off() {
    let mut state = State::default();
    state.playback.autoplay = false;
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    apply(
        &mut state,
        Action::Player(PlayerEvent::TrackStarted {
            duration: None,
            channels: 2,
            sample_rate: 44_100,
        }),
    );
    let effects = apply(&mut state, Action::Player(PlayerEvent::TrackEnded));
    assert_eq!(state.playback.status, PlayStatus::Stopped);
    assert_eq!(effects, vec![Effect::Player(PlayerCommand::Stop)]);
}

#[test]
fn track_end_at_the_queue_end_fetches_a_radio_with_autoplay_on() {
    let mut state = State::default();
    assert!(state.playback.autoplay);
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    apply(
        &mut state,
        Action::Player(PlayerEvent::TrackStarted {
            duration: None,
            channels: 2,
            sample_rate: 44_100,
        }),
    );
    let effects = apply(&mut state, Action::Player(PlayerEvent::TrackEnded));
    assert_eq!(state.playback.status, PlayStatus::Loading);
    assert_eq!(
        effects,
        vec![Effect::Api(ApiRequest::FetchRadio(TrackId("a".into())))]
    );
}

#[test]
fn a_radio_result_appends_dedups_and_loads_the_first_new_track() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    apply(
        &mut state,
        Action::Player(PlayerEvent::TrackStarted {
            duration: None,
            channels: 2,
            sample_rate: 44_100,
        }),
    );
    apply(&mut state, Action::Player(PlayerEvent::TrackEnded));
    let effects = apply(
        &mut state,
        Action::RadioLoaded(
            TrackId("a".into()),
            Ok(vec![track("a"), track("b"), track("c")]),
        ),
    );
    assert_eq!(
        effects,
        vec![Effect::Player(PlayerCommand::Load(track("b")))]
    );
    assert_eq!(state.playback.radio_request, None);
    assert!(state.playback.queue.contains(&TrackId("c".into())));
}

#[test]
fn an_empty_radio_result_stops_playback() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    apply(
        &mut state,
        Action::Player(PlayerEvent::TrackStarted {
            duration: None,
            channels: 2,
            sample_rate: 44_100,
        }),
    );
    apply(&mut state, Action::Player(PlayerEvent::TrackEnded));
    let effects = apply(
        &mut state,
        Action::RadioLoaded(TrackId("a".into()), Ok(vec![track("a")])),
    );
    assert_eq!(state.playback.status, PlayStatus::Stopped);
    assert_eq!(effects, vec![Effect::Player(PlayerCommand::Stop)]);
}

#[test]
fn a_failed_radio_stops_playback_with_a_notice() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    apply(
        &mut state,
        Action::Player(PlayerEvent::TrackStarted {
            duration: None,
            channels: 2,
            sample_rate: 44_100,
        }),
    );
    apply(&mut state, Action::Player(PlayerEvent::TrackEnded));
    let effects = apply(
        &mut state,
        Action::RadioLoaded(TrackId("a".into()), Err("offline".into())),
    );
    assert_eq!(state.playback.status, PlayStatus::Stopped);
    assert_eq!(effects, vec![Effect::Player(PlayerCommand::Stop)]);
    assert_eq!(state.notices, vec!["offline".to_string()]);
}

#[test]
fn a_radio_result_for_a_different_request_is_ignored() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    apply(
        &mut state,
        Action::Player(PlayerEvent::TrackStarted {
            duration: None,
            channels: 2,
            sample_rate: 44_100,
        }),
    );
    apply(&mut state, Action::Player(PlayerEvent::TrackEnded));
    let effects = apply(
        &mut state,
        Action::RadioLoaded(TrackId("stale".into()), Ok(vec![track("b")])),
    );
    assert_eq!(effects, vec![]);
    assert_eq!(state.playback.status, PlayStatus::Loading);
    assert_eq!(state.playback.radio_request, Some(TrackId("a".into())));
}

#[test]
fn a_radio_result_after_a_new_context_is_ignored() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    apply(
        &mut state,
        Action::Player(PlayerEvent::TrackStarted {
            duration: None,
            channels: 2,
            sample_rate: 44_100,
        }),
    );
    apply(&mut state, Action::Player(PlayerEvent::TrackEnded));
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("x"), track("y")],
            start: 0,
        },
    );
    let effects = apply(
        &mut state,
        Action::RadioLoaded(TrackId("a".into()), Ok(vec![track("b")])),
    );
    assert_eq!(effects, vec![]);
}

#[test]
fn next_pressed_at_the_queue_end_also_fetches_a_radio() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    let effects = apply(&mut state, Action::NextPressed);
    assert_eq!(
        effects,
        vec![Effect::Api(ApiRequest::FetchRadio(TrackId("a".into())))]
    );
}

#[test]
fn autoplay_toggled_flips_the_flag() {
    let mut state = State::default();
    assert!(state.playback.autoplay);
    apply(&mut state, Action::AutoplayToggled);
    assert!(!state.playback.autoplay);
}

#[test]
fn the_autoplay_setting_survives_a_session_round_trip() {
    let mut state = State::default();
    apply(&mut state, Action::AutoplayToggled);
    let saved = crate::core::session::SavedSession::capture(&state);
    let mut restored = State::default();
    apply(&mut restored, Action::SessionRestored(Box::new(saved)));
    assert!(!restored.playback.autoplay);
}

#[test]
fn an_artist_link_with_an_id_opens_the_page_and_keeps_the_name() {
    let mut state = State {
        page: Page::Library,
        ..State::default()
    };
    let artist = ArtistRef {
        name: "Nitrogen".into(),
        id: Some(ArtistId("UC1".into())),
    };
    let effects = apply(&mut state, Action::ArtistLinkOpened(artist));
    assert_eq!(state.page, Page::Artist(ArtistId("UC1".into())));
    assert_eq!(state.browse.artist_fallback, Some("Nitrogen".to_string()));
    assert_eq!(
        effects,
        vec![Effect::Api(ApiRequest::FetchArtist(ArtistId("UC1".into())))]
    );
}

#[test]
fn an_artist_link_without_an_id_searches_the_name() {
    let mut state = State {
        page: Page::Library,
        ..State::default()
    };
    let artist = ArtistRef::named("Nitrogen");
    let effects = apply(&mut state, Action::ArtistLinkOpened(artist));
    assert_eq!(state.page, Page::Search);
    assert_eq!(state.search.input, "Nitrogen");
    assert_eq!(state.history, vec![Page::Library]);
    assert_eq!(effects.len(), 1);
}

#[test]
fn a_failed_artist_page_from_a_link_falls_back_to_search() {
    let mut state = State {
        page: Page::Library,
        ..State::default()
    };
    let id = ArtistId("UC1".into());
    apply(
        &mut state,
        Action::ArtistLinkOpened(ArtistRef {
            name: "Nitrogen".into(),
            id: Some(id.clone()),
        }),
    );
    let effects = apply(
        &mut state,
        Action::ArtistLoaded(id, Err("no header".into())),
    );
    assert_eq!(state.page, Page::Search);
    assert_eq!(state.search.input, "Nitrogen");
    assert_eq!(state.history, vec![Page::Library]);
    assert_eq!(state.browse.artist_fallback, None);
    assert_eq!(state.notices.len(), 1);
    assert_eq!(
        effects,
        vec![Effect::Api(ApiRequest::Search {
            request_id: 1,
            query: "Nitrogen".to_string()
        })]
    );
}

#[test]
fn a_failed_artist_page_without_a_link_name_shows_the_failure() {
    let mut state = State {
        page: Page::Search,
        ..State::default()
    };
    let id = ArtistId("UC1".into());
    apply(&mut state, Action::ArtistOpened(id.clone()));
    apply(
        &mut state,
        Action::ArtistLoaded(id, Err("no header".into())),
    );
    assert_eq!(state.browse.artist, Loadable::Failed("no header".into()));
}

#[test]
fn decide_next_step_loads_when_a_next_track_exists() {
    let step = decide_next_step(Some(track("b")), true, Some(&track("a")));
    assert!(matches!(step, NextStep::Load(t) if t.id.0 == "b"));
}

#[test]
fn decide_next_step_fetches_radio_at_the_end_with_autoplay_on() {
    let step = decide_next_step(None, true, Some(&track("a")));
    assert!(matches!(step, NextStep::FetchRadio(id) if id.0 == "a"));
}

#[test]
fn decide_next_step_stops_at_the_end_with_autoplay_off() {
    let step = decide_next_step(None, false, Some(&track("a")));
    assert!(matches!(step, NextStep::Stop));
}

#[test]
fn decide_next_step_stops_with_no_current_track() {
    let step = decide_next_step(None, true, None);
    assert!(matches!(step, NextStep::Stop));
}

#[test]
fn previous_restarts_after_the_threshold() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a"), track("b")],
            start: 1,
        },
    );
    state.playback.position = Duration::from_secs(10);
    let effects = apply(&mut state, Action::PreviousPressed);
    assert_eq!(
        effects,
        vec![Effect::Player(PlayerCommand::Seek(Duration::ZERO))]
    );
}

#[test]
fn stale_playlist_results_are_ignored() {
    let mut state = State::default();
    apply(&mut state, Action::PlaylistOpened(PlaylistId("p1".into())));
    apply(&mut state, Action::NavigatedTo(Page::Search));
    apply(
        &mut state,
        Action::PlaylistTracksLoaded(PlaylistId("p1".into()), Ok(vec![track("a")])),
    );
    assert_eq!(state.library.open_playlist, Loadable::Loading);
}

#[test]
fn a_restored_session_resumes_at_the_old_position() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    state.playback.position = Duration::from_secs(30);
    let saved = crate::core::session::SavedSession::capture(&state);

    let mut restored = State::default();
    apply(&mut restored, Action::SessionRestored(Box::new(saved)));
    assert_eq!(restored.playback.status, PlayStatus::Stopped);
    assert_eq!(restored.playback.position, Duration::from_secs(30));

    let effects = apply(&mut restored, Action::PlayToggled);
    assert_eq!(
        effects,
        vec![Effect::Player(PlayerCommand::Load(track("a")))]
    );
    let effects = apply(
        &mut restored,
        Action::Player(PlayerEvent::TrackStarted {
            duration: None,
            channels: 2,
            sample_rate: 44_100,
        }),
    );
    assert_eq!(
        effects,
        vec![
            Effect::Player(PlayerCommand::PrepareNext(None)),
            Effect::Player(PlayerCommand::Seek(Duration::from_secs(30)))
        ]
    );
    assert_eq!(restored.playback.position, Duration::from_secs(30));
}

#[test]
fn volume_is_clamped() {
    let mut state = State::default();
    apply(&mut state, Action::VolumeSet(1.7));
    assert_eq!(state.playback.volume, 1.0);
}

#[test]
fn a_track_start_prefetches_the_next_track() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a"), track("b")],
            start: 0,
        },
    );
    let effects = apply(
        &mut state,
        Action::Player(PlayerEvent::TrackStarted {
            duration: None,
            channels: 2,
            sample_rate: 44_100,
        }),
    );
    assert_eq!(
        effects,
        vec![Effect::Player(PlayerCommand::PrepareNext(Some(track("b"))))]
    );
}

#[test]
fn queuing_a_track_prefetches_it_ahead_of_the_context() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a"), track("b")],
            start: 0,
        },
    );
    let effects = apply(&mut state, Action::TrackQueued(track("q")));
    assert_eq!(
        effects,
        vec![Effect::Player(PlayerCommand::PrepareNext(Some(track("q"))))]
    );
}

#[test]
fn toggling_shuffle_prefetches_the_new_next_track() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a"), track("b"), track("c")],
            start: 0,
        },
    );
    let effects = apply(&mut state, Action::ShuffleToggled);
    assert_eq!(effects.len(), 1);
    assert!(matches!(
        effects[0],
        Effect::Player(PlayerCommand::PrepareNext(Some(_)))
    ));
}

#[test]
fn cycling_repeat_to_one_prefetches_the_current_track() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a"), track("b")],
            start: 0,
        },
    );
    apply(&mut state, Action::RepeatCycled); // Off -> All
    let effects = apply(&mut state, Action::RepeatCycled); // All -> One
    assert_eq!(
        effects,
        vec![Effect::Player(PlayerCommand::PrepareNext(Some(track("a"))))]
    );
}

#[test]
fn a_failed_refresh_keeps_the_cached_library_and_posts_a_notice() {
    let mut state = State::default();
    apply(&mut state, Action::AuthVerified(Ok(())));
    apply(
        &mut state,
        Action::LibraryCacheLoaded {
            playlists: None,
            liked: Some(vec![track("cached")]),
        },
    );
    let effects = apply(&mut state, Action::LikedLoaded(Err("offline".into())));
    assert_eq!(state.library.liked, Loadable::Loaded(vec![track("cached")]));
    assert_eq!(state.notices, vec!["offline".to_string()]);
    assert!(effects.is_empty());
}

#[test]
fn a_failed_load_without_cached_data_shows_the_failure() {
    let mut state = State::default();
    apply(&mut state, Action::AuthVerified(Ok(())));
    apply(&mut state, Action::LikedLoaded(Err("offline".into())));
    assert_eq!(state.library.liked, Loadable::Failed("offline".into()));
    assert_eq!(state.notices, Vec::<String>::new());
}

#[test]
fn a_track_start_at_the_queue_end_cancels_preparation() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    let effects = apply(
        &mut state,
        Action::Player(PlayerEvent::TrackStarted {
            duration: None,
            channels: 2,
            sample_rate: 44_100,
        }),
    );
    assert_eq!(
        effects,
        vec![Effect::Player(PlayerCommand::PrepareNext(None))]
    );
}

#[test]
fn a_liked_page_on_a_loading_slot_shows_at_once_and_marks_loading_more() {
    let mut state = State::default();
    apply(&mut state, Action::AuthVerified(Ok(())));
    apply(
        &mut state,
        Action::LikedPageLoaded {
            tracks: vec![track("a")],
            finished: false,
        },
    );
    assert_eq!(state.library.liked, Loadable::Loaded(vec![track("a")]));
    assert!(state.library.liked_loading_more);
}

#[test]
fn a_later_liked_page_appends_to_the_visible_list() {
    let mut state = State::default();
    apply(&mut state, Action::AuthVerified(Ok(())));
    apply(
        &mut state,
        Action::LikedPageLoaded {
            tracks: vec![track("a")],
            finished: false,
        },
    );
    apply(
        &mut state,
        Action::LikedPageLoaded {
            tracks: vec![track("b")],
            finished: true,
        },
    );
    assert_eq!(
        state.library.liked,
        Loadable::Loaded(vec![track("a"), track("b")])
    );
    assert!(!state.library.liked_loading_more);
}

#[test]
fn liked_pages_on_a_refreshing_slot_buffer_and_swap_in_together() {
    let mut state = State::default();
    apply(&mut state, Action::AuthVerified(Ok(())));
    apply(
        &mut state,
        Action::LibraryCacheLoaded {
            playlists: None,
            liked: Some(vec![track("cached")]),
        },
    );
    apply(
        &mut state,
        Action::LikedPageLoaded {
            tracks: vec![track("a")],
            finished: false,
        },
    );
    // The cached list stays on screen while pages buffer off screen.
    assert_eq!(
        state.library.liked,
        Loadable::Refreshing(vec![track("cached")])
    );
    assert_eq!(state.library.incoming_liked, vec![track("a")]);

    apply(
        &mut state,
        Action::LikedPageLoaded {
            tracks: vec![track("b")],
            finished: true,
        },
    );
    assert_eq!(
        state.library.liked,
        Loadable::Loaded(vec![track("a"), track("b")])
    );
    assert_eq!(state.library.incoming_liked, Vec::<Track>::new());
}

#[test]
fn a_finished_liked_stream_writes_the_full_list_to_the_cache() {
    let mut state = State::default();
    apply(&mut state, Action::AuthVerified(Ok(())));
    apply(
        &mut state,
        Action::LikedPageLoaded {
            tracks: vec![track("a")],
            finished: false,
        },
    );
    let effects = apply(
        &mut state,
        Action::LikedPageLoaded {
            tracks: vec![track("b")],
            finished: true,
        },
    );
    assert_eq!(
        effects,
        vec![Effect::SaveLibraryCache(LibraryCacheWrite::Liked(vec![
            track("a"),
            track("b")
        ]))]
    );
}

#[test]
fn a_partial_liked_page_never_writes_the_cache() {
    let mut state = State::default();
    apply(&mut state, Action::AuthVerified(Ok(())));
    let effects = apply(
        &mut state,
        Action::LikedPageLoaded {
            tracks: vec![track("a")],
            finished: false,
        },
    );
    assert_eq!(effects, vec![]);
}

#[test]
fn a_stale_playlist_page_is_ignored_after_leaving_the_playlist() {
    let mut state = State::default();
    apply(&mut state, Action::PlaylistOpened(PlaylistId("p1".into())));
    apply(&mut state, Action::NavigatedTo(Page::Search));
    let effects = apply(
        &mut state,
        Action::PlaylistTracksPageLoaded {
            id: PlaylistId("p1".into()),
            tracks: vec![track("a")],
            finished: true,
        },
    );
    assert_eq!(state.library.open_playlist, Loadable::Loading);
    assert_eq!(effects, vec![]);
}

#[test]
fn hovering_a_track_prefetches_it() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    let effects = apply(&mut state, Action::TrackHovered(track("b")));
    assert_eq!(
        effects,
        vec![Effect::Player(PlayerCommand::Prefetch(track("b")))]
    );
}

#[test]
fn hovering_the_current_track_prefetches_nothing() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    let effects = apply(&mut state, Action::TrackHovered(track("a")));
    assert_eq!(effects, vec![]);
}

#[test]
fn hovering_the_same_track_twice_prefetches_only_once() {
    let mut state = State::default();
    apply(&mut state, Action::TrackHovered(track("b")));
    let effects = apply(&mut state, Action::TrackHovered(track("b")));
    assert_eq!(effects, vec![]);
}

#[test]
fn the_dwell_decision_is_pure() {
    let a = TrackId("a".into());
    let b = TrackId("b".into());
    let c = TrackId("c".into());
    assert!(should_hover_prefetch(None, None, &b));
    assert!(!should_hover_prefetch(Some(&a), None, &a));
    assert!(!should_hover_prefetch(None, Some(&b), &b));
    assert!(should_hover_prefetch(Some(&a), Some(&b), &c));
}

#[test]
fn a_mid_stream_failure_keeps_the_pages_already_shown() {
    let mut state = State::default();
    apply(&mut state, Action::AuthVerified(Ok(())));
    apply(
        &mut state,
        Action::LikedPageLoaded {
            tracks: vec![track("a")],
            finished: false,
        },
    );
    let effects = apply(&mut state, Action::LikedLoaded(Err("offline".into())));
    assert_eq!(state.library.liked, Loadable::Loaded(vec![track("a")]));
    assert!(!state.library.liked_loading_more);
    assert_eq!(state.notices, vec!["offline".to_string()]);
    assert_eq!(effects, vec![]);
}

#[test]
fn winamp_toggled_flips_open() {
    let mut state = State::default();
    assert!(!state.winamp.open);
    apply(&mut state, Action::WinampToggled);
    assert!(state.winamp.open);
    apply(&mut state, Action::WinampToggled);
    assert!(!state.winamp.open);
}

#[test]
fn opening_the_winamp_window_loads_the_worn_skin_and_lists_the_folder() {
    let mut state = State::default();
    state.winamp.skin = Some("Zaxon".to_string());
    let effects = apply(&mut state, Action::WinampToggled);
    assert_eq!(
        effects,
        vec![
            Effect::LoadSkin(Some("Zaxon".to_string())),
            Effect::RefreshSkinList
        ]
    );
    let effects = apply(&mut state, Action::WinampToggled);
    assert_eq!(effects, vec![]);
}

#[test]
fn choosing_a_skin_records_it_and_loads_it() {
    let mut state = State::default();
    let effects = apply(&mut state, Action::SkinChosen(Some("Base".to_string())));
    assert_eq!(state.winamp.skin, Some("Base".to_string()));
    assert_eq!(effects, vec![Effect::LoadSkin(Some("Base".to_string()))]);

    let effects = apply(&mut state, Action::SkinChosen(None));
    assert_eq!(state.winamp.skin, None);
    assert_eq!(effects, vec![Effect::LoadSkin(None)]);
}

#[test]
fn a_dropped_skin_file_is_installed() {
    let mut state = State::default();
    let path = std::path::PathBuf::from("/tmp/Zaxon.wsz");
    let effects = apply(&mut state, Action::SkinFileDropped(path.clone()));
    assert_eq!(effects, vec![Effect::InstallSkin(path)]);
}

#[test]
fn an_installed_skin_is_worn_and_a_failure_is_a_notice() {
    let mut state = State::default();
    let effects = apply(&mut state, Action::SkinInstalled(Ok("Zaxon".to_string())));
    assert_eq!(state.winamp.skin, Some("Zaxon".to_string()));
    assert_eq!(effects, vec![Effect::LoadSkin(Some("Zaxon".to_string()))]);

    let effects = apply(&mut state, Action::SkinInstalled(Err("bad file".into())));
    assert_eq!(state.notices, vec!["bad file".to_string()]);
    assert_eq!(effects, vec![]);
}

#[test]
fn the_skin_list_refresh_replaces_the_available_skins() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::SkinListRefreshed(vec!["Base".to_string(), "Zaxon".to_string()]),
    );
    assert_eq!(
        state.winamp.available_skins,
        vec!["Base".to_string(), "Zaxon".to_string()]
    );
}

#[test]
fn winamp_scale_set_clamps_to_the_allowed_range() {
    let mut state = State::default();
    apply(&mut state, Action::WinampScaleSet(3));
    assert_eq!(state.winamp.scale, 3);
    apply(&mut state, Action::WinampScaleSet(0));
    assert_eq!(state.winamp.scale, 1);
    apply(&mut state, Action::WinampScaleSet(9));
    assert_eq!(state.winamp.scale, 4);
}

#[test]
fn winamp_on_top_toggled_flips_on_top() {
    let mut state = State::default();
    assert!(!state.winamp.on_top);
    apply(&mut state, Action::WinampOnTopToggled);
    assert!(state.winamp.on_top);
    apply(&mut state, Action::WinampOnTopToggled);
    assert!(!state.winamp.on_top);
}

#[test]
fn track_started_carries_the_stream_shape() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::Player(PlayerEvent::TrackStarted {
            duration: Some(Duration::from_secs(180)),
            channels: 2,
            sample_rate: 44_100,
        }),
    );
    assert_eq!(state.playback.channels, 2);
    assert_eq!(state.playback.sample_rate, 44_100);
}

#[test]
fn queue_jumped_loads_the_track_at_that_upcoming_index() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a"), track("b"), track("c")],
            start: 0,
        },
    );
    let effects = apply(&mut state, Action::QueueJumped(1));
    assert_eq!(
        effects,
        vec![Effect::Player(PlayerCommand::Load(track("c")))]
    );
    assert_eq!(state.playback.status, PlayStatus::Loading);
}

#[test]
fn queue_jumped_past_the_end_preserves_playback() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a")],
            start: 0,
        },
    );
    let before = state.playback.clone();
    let effects = apply(&mut state, Action::QueueJumped(5));
    assert!(effects.is_empty());
    assert_eq!(state.playback.status, before.status);
}

#[test]
fn queue_cleared_drops_only_the_explicitly_queued_tracks() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::ContextPlayed {
            tracks: vec![track("a"), track("b")],
            start: 0,
        },
    );
    apply(&mut state, Action::TrackQueued(track("q")));
    apply(&mut state, Action::QueueCleared);
    let upcoming: Vec<&str> = state
        .playback
        .queue
        .upcoming()
        .map(|track| track.id.0.as_str())
        .collect();
    assert_eq!(upcoming, vec!["b"]);
}

fn playlist(id: &str, thumbnail_url: Option<&str>) -> Playlist {
    Playlist {
        id: PlaylistId(id.to_string()),
        title: id.to_string(),
        track_count: None,
        thumbnail_url: thumbnail_url.map(str::to_string),
    }
}

const CUSTOM_COVER: &str = "https://yt3.googleusercontent.com/abc=s1200";
const GENERIC_COVER: &str = "https://i.ytimg.com/vi/abc/hqdefault.jpg";

#[test]
fn merge_covers_keeps_a_prior_custom_cover_a_fresh_load_lacks() {
    let fresh = vec![playlist("p1", Some(GENERIC_COVER))];
    let previous = vec![playlist("p1", Some(CUSTOM_COVER))];
    let merged = merge_covers(fresh, &previous);
    assert_eq!(merged[0].thumbnail_url.as_deref(), Some(CUSTOM_COVER));
}

#[test]
fn merge_covers_leaves_a_fresh_custom_cover_alone() {
    let fresh = vec![playlist("p1", Some(CUSTOM_COVER))];
    let previous = vec![playlist("p1", Some(GENERIC_COVER))];
    let merged = merge_covers(fresh, &previous);
    assert_eq!(merged[0].thumbnail_url.as_deref(), Some(CUSTOM_COVER));
}

#[test]
fn merge_covers_leaves_a_playlist_with_no_prior_entry_alone() {
    let fresh = vec![playlist("p1", Some(GENERIC_COVER))];
    let merged = merge_covers(fresh, &[]);
    assert_eq!(merged[0].thumbnail_url.as_deref(), Some(GENERIC_COVER));
}

#[test]
fn ids_without_cover_names_only_the_generic_ones() {
    let playlists = vec![
        playlist("p1", Some(CUSTOM_COVER)),
        playlist("p2", Some(GENERIC_COVER)),
        playlist("p3", None),
    ];
    assert_eq!(
        ids_without_cover(&playlists),
        vec![PlaylistId("p2".into()), PlaylistId("p3".into())]
    );
}

#[test]
fn a_fresh_playlists_load_asks_for_covers_it_still_lacks() {
    let mut state = State::default();
    let effects = apply(
        &mut state,
        Action::PlaylistsLoaded(Ok(vec![
            playlist("p1", Some(CUSTOM_COVER)),
            playlist("p2", Some(GENERIC_COVER)),
        ])),
    );
    assert_eq!(
        effects,
        vec![
            Effect::SaveLibraryCache(LibraryCacheWrite::Playlists(vec![
                playlist("p1", Some(CUSTOM_COVER)),
                playlist("p2", Some(GENERIC_COVER)),
            ])),
            Effect::Api(ApiRequest::FetchPlaylistCovers(vec![PlaylistId(
                "p2".into()
            )])),
        ]
    );
}

#[test]
fn a_refresh_keeps_a_custom_cover_the_fresh_load_no_longer_carries() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::PlaylistsLoaded(Ok(vec![playlist("p1", Some(CUSTOM_COVER))])),
    );
    apply(
        &mut state,
        Action::PlaylistsLoaded(Ok(vec![playlist("p1", Some(GENERIC_COVER))])),
    );
    let Loadable::Loaded(playlists) = &state.library.playlists else {
        panic!("expected the playlists slot to be loaded");
    };
    assert_eq!(playlists[0].thumbnail_url.as_deref(), Some(CUSTOM_COVER));
}

#[test]
fn a_covers_result_updates_the_slot_and_saves_the_cache() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::PlaylistsLoaded(Ok(vec![playlist("p1", Some(GENERIC_COVER))])),
    );
    let effects = apply(
        &mut state,
        Action::PlaylistCoversLoaded(vec![(PlaylistId("p1".into()), CUSTOM_COVER.into())]),
    );
    let Loadable::Loaded(playlists) = &state.library.playlists else {
        panic!("expected the playlists slot to be loaded");
    };
    assert_eq!(playlists[0].thumbnail_url.as_deref(), Some(CUSTOM_COVER));
    assert_eq!(
        effects,
        vec![Effect::SaveLibraryCache(LibraryCacheWrite::Playlists(
            vec![playlist("p1", Some(CUSTOM_COVER))]
        ))]
    );
}

#[test]
fn an_empty_covers_result_changes_nothing() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::PlaylistsLoaded(Ok(vec![playlist("p1", Some(GENERIC_COVER))])),
    );
    let effects = apply(&mut state, Action::PlaylistCoversLoaded(vec![]));
    assert_eq!(effects, vec![]);
}

fn counted_playlist(id: &str, track_count: Option<usize>) -> Playlist {
    Playlist {
        id: PlaylistId(id.to_string()),
        title: id.to_string(),
        track_count,
        thumbnail_url: None,
    }
}

fn track_in_playlist(id: &str, item_id: &str) -> Track {
    Track {
        playlist_item_id: Some(item_id.to_string()),
        ..track(id)
    }
}

// -- is_liked, with_count_delta, without_item --

#[test]
fn is_liked_reads_membership_from_a_loaded_or_refreshing_slot() {
    let id = TrackId("a".into());
    assert!(!is_liked(&Loadable::NotAsked, &id));
    assert!(!is_liked(&Loadable::Loaded(vec![track("b")]), &id));
    assert!(is_liked(&Loadable::Loaded(vec![track("a")]), &id));
    assert!(is_liked(&Loadable::Refreshing(vec![track("a")]), &id));
}

#[test]
fn with_count_delta_raises_and_lowers_a_known_count_only() {
    let playlists = vec![
        counted_playlist("p1", Some(3)),
        counted_playlist("p2", None),
    ];
    let raised = with_count_delta(playlists.clone(), &PlaylistId("p1".into()), 1);
    assert_eq!(raised[0].track_count, Some(4));
    let lowered = with_count_delta(playlists.clone(), &PlaylistId("p1".into()), -1);
    assert_eq!(lowered[0].track_count, Some(2));
    let unknown = with_count_delta(playlists, &PlaylistId("p2".into()), 1);
    assert_eq!(unknown[1].track_count, None);
}

#[test]
fn with_count_delta_never_drops_below_zero() {
    let playlists = vec![counted_playlist("p1", Some(0))];
    let result = with_count_delta(playlists, &PlaylistId("p1".into()), -1);
    assert_eq!(result[0].track_count, Some(0));
}

#[test]
fn without_item_drops_only_the_matching_row() {
    let tracks = vec![track_in_playlist("a", "i1"), track_in_playlist("b", "i2")];
    let result = without_item(tracks, "i1");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].id, TrackId("b".into()));
}

#[test]
fn a_failed_like_refetches_without_doubling_the_liked_list() {
    let mut state = State::default();
    state.library.liked = Loadable::Loaded(vec![track("a"), track("b")]);
    let effects = apply(
        &mut state,
        Action::LibraryWriteFinished {
            what: LibraryWrite::Liked,
            result: Err("quota".into()),
        },
    );
    assert_eq!(effects, vec![Effect::Api(ApiRequest::FetchLiked)]);
    apply(
        &mut state,
        Action::LikedPageLoaded {
            tracks: vec![track("b")],
            finished: true,
        },
    );
    assert_eq!(state.library.liked, Loadable::Loaded(vec![track("b")]));
    assert_eq!(state.notices.len(), 1);
}

// -- TrackLikeToggled --

#[test]
fn liking_an_unliked_track_inserts_it_and_rates_it() {
    let mut state = State::default();
    state.library.liked = Loadable::Loaded(vec![track("existing")]);
    let effects = apply(&mut state, Action::TrackLikeToggled(track("new")));
    let Loadable::Loaded(liked) = &state.library.liked else {
        panic!("expected the liked slot to stay loaded");
    };
    assert_eq!(liked[0].id, TrackId("new".into()));
    assert_eq!(
        effects,
        vec![Effect::Api(ApiRequest::RateTrack {
            id: TrackId("new".into()),
            liked: true
        })]
    );
}

#[test]
fn unliking_a_liked_track_removes_it_and_rates_it() {
    let mut state = State::default();
    state.library.liked = Loadable::Loaded(vec![track("a"), track("b")]);
    let effects = apply(&mut state, Action::TrackLikeToggled(track("a")));
    let Loadable::Loaded(liked) = &state.library.liked else {
        panic!("expected the liked slot to stay loaded");
    };
    assert_eq!(liked.len(), 1);
    assert_eq!(liked[0].id, TrackId("b".into()));
    assert_eq!(
        effects,
        vec![Effect::Api(ApiRequest::RateTrack {
            id: TrackId("a".into()),
            liked: false
        })]
    );
}

// -- TrackAddedToPlaylist / PlaylistItemAdded --

#[test]
fn adding_a_track_raises_the_count_and_requests_the_write() {
    let mut state = State::default();
    state.library.playlists = Loadable::Loaded(vec![counted_playlist("p1", Some(2))]);
    let effects = apply(
        &mut state,
        Action::TrackAddedToPlaylist {
            playlist: PlaylistId("p1".into()),
            track: track("a"),
        },
    );
    let Loadable::Loaded(playlists) = &state.library.playlists else {
        panic!("expected the playlists slot to stay loaded");
    };
    assert_eq!(playlists[0].track_count, Some(3));
    assert_eq!(
        effects,
        vec![Effect::Api(ApiRequest::AddToPlaylist {
            playlist: PlaylistId("p1".into()),
            track: track("a"),
        })]
    );
}

#[test]
fn a_confirmed_add_appends_the_row_to_the_open_playlist() {
    let mut state = State {
        page: Page::Playlist(PlaylistId("p1".into())),
        ..State::default()
    };
    state.library.open_playlist = Loadable::Loaded(vec![]);
    apply(
        &mut state,
        Action::PlaylistItemAdded {
            playlist: PlaylistId("p1".into()),
            track: track("a"),
            result: Ok("item1".into()),
        },
    );
    let Loadable::Loaded(tracks) = &state.library.open_playlist else {
        panic!("expected the open playlist to stay loaded");
    };
    assert_eq!(tracks[0].playlist_item_id, Some("item1".into()));
}

#[test]
fn a_confirmed_add_is_ignored_once_the_user_left_the_playlist() {
    let mut state = State {
        page: Page::Library,
        ..State::default()
    };
    state.library.open_playlist = Loadable::Loaded(vec![]);
    apply(
        &mut state,
        Action::PlaylistItemAdded {
            playlist: PlaylistId("p1".into()),
            track: track("a"),
            result: Ok("item1".into()),
        },
    );
    let Loadable::Loaded(tracks) = &state.library.open_playlist else {
        panic!("expected the open playlist to stay loaded");
    };
    assert!(tracks.is_empty());
}

#[test]
fn a_failed_add_reverts_the_count_and_posts_a_notice() {
    let mut state = State::default();
    state.library.playlists = Loadable::Loaded(vec![counted_playlist("p1", Some(3))]);
    apply(
        &mut state,
        Action::PlaylistItemAdded {
            playlist: PlaylistId("p1".into()),
            track: track("a"),
            result: Err("nope".into()),
        },
    );
    let Loadable::Loaded(playlists) = &state.library.playlists else {
        panic!("expected the playlists slot to stay loaded");
    };
    assert_eq!(playlists[0].track_count, Some(2));
    assert_eq!(state.notices, vec!["nope".to_string()]);
}

// -- TrackRemovedFromPlaylist --

#[test]
fn removing_a_track_drops_the_row_lowers_the_count_and_requests_the_write() {
    let mut state = State::default();
    state.library.playlists = Loadable::Loaded(vec![counted_playlist("p1", Some(2))]);
    state.library.open_playlist = Loadable::Loaded(vec![
        track_in_playlist("a", "i1"),
        track_in_playlist("b", "i2"),
    ]);
    let effects = apply(
        &mut state,
        Action::TrackRemovedFromPlaylist {
            playlist: PlaylistId("p1".into()),
            item_id: "i1".into(),
        },
    );
    let Loadable::Loaded(tracks) = &state.library.open_playlist else {
        panic!("expected the open playlist to stay loaded");
    };
    assert_eq!(tracks.len(), 1);
    let Loadable::Loaded(playlists) = &state.library.playlists else {
        panic!("expected the playlists slot to stay loaded");
    };
    assert_eq!(playlists[0].track_count, Some(1));
    assert_eq!(
        effects,
        vec![Effect::Api(ApiRequest::RemoveFromPlaylist {
            playlist: PlaylistId("p1".into()),
            item_id: "i1".into(),
        })]
    );
}

// -- PlaylistCreateRequested / PlaylistCreated --

#[test]
fn an_empty_playlist_title_is_ignored() {
    let mut state = State {
        dialog: Some(Dialog::CreatePlaylist {
            title_draft: "  ".into(),
            then_add: None,
        }),
        ..State::default()
    };
    let effects = apply(&mut state, Action::PlaylistCreateRequested("  ".into()));
    assert_eq!(effects, vec![]);
    assert!(state.dialog.is_some());
}

#[test]
fn creating_a_playlist_closes_the_dialog_and_requests_it() {
    let mut state = State {
        dialog: Some(Dialog::CreatePlaylist {
            title_draft: "Chill".into(),
            then_add: None,
        }),
        ..State::default()
    };
    let effects = apply(&mut state, Action::PlaylistCreateRequested("Chill".into()));
    assert_eq!(state.dialog, None);
    assert_eq!(
        effects,
        vec![Effect::Api(ApiRequest::CreatePlaylist(1, "Chill".into()))]
    );
}

#[test]
fn a_created_playlist_is_inserted_at_the_front_and_cached() {
    let mut state = State::default();
    state.library.playlists = Loadable::Loaded(vec![counted_playlist("old", Some(1))]);
    apply(&mut state, Action::PlaylistCreateRequested("New".into()));
    let effects = apply(
        &mut state,
        Action::PlaylistCreated(1, Ok(counted_playlist("new", Some(0)))),
    );
    let Loadable::Loaded(playlists) = &state.library.playlists else {
        panic!("expected the playlists slot to stay loaded");
    };
    assert_eq!(playlists[0].id, PlaylistId("new".into()));
    assert_eq!(
        effects,
        vec![Effect::SaveLibraryCache(LibraryCacheWrite::Playlists(
            vec![
                counted_playlist("new", Some(0)),
                counted_playlist("old", Some(1)),
            ]
        ))]
    );
}

#[test]
fn creating_a_playlist_with_a_remembered_track_chains_the_add() {
    let mut state = State {
        dialog: Some(Dialog::CreatePlaylist {
            title_draft: "Chill".into(),
            then_add: Some(track("a")),
        }),
        ..State::default()
    };
    apply(&mut state, Action::PlaylistCreateRequested("Chill".into()));
    let effects = apply(
        &mut state,
        Action::PlaylistCreated(1, Ok(counted_playlist("new", Some(0)))),
    );
    assert!(
        effects
            .iter()
            .any(|effect| matches!(effect, Effect::Api(ApiRequest::AddToPlaylist { .. })))
    );
}

#[test]
fn a_failed_create_posts_a_notice() {
    let mut state = State::default();
    apply(&mut state, Action::PlaylistCreateRequested("New".into()));
    let effects = apply(&mut state, Action::PlaylistCreated(1, Err("nope".into())));
    assert_eq!(effects, vec![]);
    assert_eq!(state.notices, vec!["nope".to_string()]);
}

// -- CreatePlaylistDialogOpened / CreatePlaylistDraftChanged / DialogDismissed --

#[test]
fn opening_the_dialog_remembers_the_track_that_asked_for_it() {
    let mut state = State::default();
    apply(
        &mut state,
        Action::CreatePlaylistDialogOpened(Some(track("a"))),
    );
    assert_eq!(
        state.dialog,
        Some(Dialog::CreatePlaylist {
            title_draft: String::new(),
            then_add: Some(track("a")),
        })
    );
}

#[test]
fn typing_in_the_dialog_updates_its_draft() {
    let mut state = State::default();
    apply(&mut state, Action::CreatePlaylistDialogOpened(None));
    apply(
        &mut state,
        Action::CreatePlaylistDraftChanged("Chill".into()),
    );
    assert_eq!(
        state.dialog,
        Some(Dialog::CreatePlaylist {
            title_draft: "Chill".into(),
            then_add: None,
        })
    );
}

#[test]
fn dismissing_the_dialog_closes_it() {
    let mut state = State::default();
    apply(&mut state, Action::CreatePlaylistDialogOpened(None));
    apply(&mut state, Action::DialogDismissed);
    assert_eq!(state.dialog, None);
}

// -- LibraryWriteFinished --

#[test]
fn a_successful_write_does_nothing() {
    let mut state = State::default();
    let effects = apply(
        &mut state,
        Action::LibraryWriteFinished {
            what: LibraryWrite::Liked,
            result: Ok(()),
        },
    );
    assert_eq!(effects, vec![]);
}

#[test]
fn a_failed_liked_write_posts_a_notice_and_refetches_liked() {
    let mut state = State::default();
    let effects = apply(
        &mut state,
        Action::LibraryWriteFinished {
            what: LibraryWrite::Liked,
            result: Err("nope".into()),
        },
    );
    assert_eq!(state.notices, vec!["nope".to_string()]);
    assert_eq!(effects, vec![Effect::Api(ApiRequest::FetchLiked)]);
}

#[test]
fn a_failed_playlist_write_refetches_its_tracks_and_the_playlists() {
    let mut state = State::default();
    let effects = apply(
        &mut state,
        Action::LibraryWriteFinished {
            what: LibraryWrite::Playlist(PlaylistId("p1".into())),
            result: Err("nope".into()),
        },
    );
    assert_eq!(state.notices, vec!["nope".to_string()]);
    assert_eq!(
        effects,
        vec![
            Effect::Api(ApiRequest::FetchPlaylistTracks(PlaylistId("p1".into()))),
            Effect::Api(ApiRequest::FetchPlaylists),
        ]
    );
}
