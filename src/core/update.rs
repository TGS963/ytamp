//! The reducer: the only writer of the application state.
//!
//! `update` applies one action and returns the effects it causes.
//! It touches no clock, no network, and no audio device. Randomness
//! comes in as a parameter, so every path is testable.

use std::time::Duration;

use super::action::{Action, PlayerEvent};
use super::effect::{ApiRequest, Effect, PlayerCommand};
use super::model::{PlaylistId, Track};
use super::queue::RandomBelow;
use super::state::{AuthState, Loadable, Page, PlayStatus, State};

/// Below this position, Previous moves to the previous track.
/// At or above it, Previous restarts the current track.
const RESTART_THRESHOLD: Duration = Duration::from_secs(3);

pub fn update(state: &mut State, action: Action, random_below: RandomBelow) -> Vec<Effect> {
    match action {
        Action::NavigatedTo(page) => navigate(state, page),
        Action::CookieDraftChanged(draft) => {
            state.sign_in.draft = draft;
            vec![]
        }
        Action::CookiesSubmitted => submit_cookies(state),
        Action::AuthVerified(result) => finish_sign_in(state, result),
        Action::SearchInputChanged(input) => {
            state.search.input = input;
            vec![]
        }
        Action::SearchSubmitted => submit_search(state),
        Action::SearchLoaded(result) => {
            set_loadable(&mut state.search.results, result);
            vec![]
        }
        Action::PlaylistOpened(id) => open_playlist(state, id),
        Action::PlaylistsLoaded(result) => {
            set_loadable(&mut state.library.playlists, result);
            vec![]
        }
        Action::LikedLoaded(result) => {
            set_loadable(&mut state.library.liked, result);
            vec![]
        }
        Action::PlaylistTracksLoaded(id, result) => {
            finish_playlist_load(state, id, result);
            vec![]
        }
        Action::ContextPlayed { tracks, start } => play_context(state, tracks, start, random_below),
        Action::TrackQueued(track) => {
            state.playback.queue.queue_track(track);
            vec![]
        }
        Action::PlayToggled => toggle_play(state),
        Action::NextPressed => load_or_stop(state, |state| state.playback.queue.next()),
        Action::PreviousPressed => go_previous(state),
        Action::SeekRequested(position) => {
            state.playback.position = position;
            vec![Effect::Player(PlayerCommand::Seek(position))]
        }
        Action::VolumeSet(volume) => set_volume(state, volume),
        Action::ShuffleToggled => {
            let on = !state.playback.queue.shuffle;
            state.playback.queue.set_shuffle(on, random_below);
            vec![]
        }
        Action::RepeatCycled => {
            state.playback.queue.repeat = state.playback.queue.repeat.cycled();
            vec![]
        }
        Action::NoticeDismissed(index) => {
            if index < state.notices.len() {
                state.notices.remove(index);
            }
            vec![]
        }
        Action::Player(event) => apply_player_event(state, event),
    }
}

fn navigate(state: &mut State, page: Page) -> Vec<Effect> {
    let effects = match page {
        Page::Library => fetch_missing_library(state),
        _ => vec![],
    };
    state.page = page;
    effects
}

/// The library fetches that have not run yet. Keeps Library cheap to reopen.
fn fetch_missing_library(state: &mut State) -> Vec<Effect> {
    let mut effects = vec![];
    if state.library.playlists == Loadable::NotAsked {
        state.library.playlists = Loadable::Loading;
        effects.push(Effect::Api(ApiRequest::FetchPlaylists));
    }
    if state.library.liked == Loadable::NotAsked {
        state.library.liked = Loadable::Loading;
        effects.push(Effect::Api(ApiRequest::FetchLiked));
    }
    effects
}

fn submit_cookies(state: &mut State) -> Vec<Effect> {
    let cookies = state.sign_in.draft.trim().to_string();
    if cookies.is_empty() {
        return vec![];
    }
    state.auth = AuthState::Verifying;
    vec![
        Effect::SaveCookies(cookies.clone()),
        Effect::Api(ApiRequest::VerifyAuth { cookies }),
    ]
}

fn finish_sign_in(state: &mut State, result: Result<(), String>) -> Vec<Effect> {
    match result {
        Ok(()) => {
            state.auth = AuthState::SignedIn;
            navigate(state, Page::Library)
        }
        Err(message) => {
            state.auth = AuthState::Failed(message);
            vec![]
        }
    }
}

fn submit_search(state: &mut State) -> Vec<Effect> {
    let query = state.search.input.trim().to_string();
    if query.is_empty() {
        return vec![];
    }
    state.search.results = Loadable::Loading;
    vec![Effect::Api(ApiRequest::Search { query })]
}

fn open_playlist(state: &mut State, id: PlaylistId) -> Vec<Effect> {
    state.library.open_playlist = Loadable::Loading;
    state.page = Page::Playlist(id.clone());
    vec![Effect::Api(ApiRequest::FetchPlaylistTracks(id))]
}

/// Ignores a result for a playlist the user has already left.
fn finish_playlist_load(state: &mut State, id: PlaylistId, result: Result<Vec<Track>, String>) {
    if state.page != Page::Playlist(id) {
        return;
    }
    set_loadable(&mut state.library.open_playlist, result);
}

fn play_context(
    state: &mut State,
    tracks: Vec<Track>,
    start: usize,
    random_below: RandomBelow,
) -> Vec<Effect> {
    let started = state
        .playback
        .queue
        .play_context(tracks, start, random_below);
    load_track(state, started)
}

fn toggle_play(state: &mut State) -> Vec<Effect> {
    match state.playback.status {
        PlayStatus::Playing => {
            state.playback.status = PlayStatus::Paused;
            vec![Effect::Player(PlayerCommand::Pause)]
        }
        PlayStatus::Paused => {
            state.playback.status = PlayStatus::Playing;
            vec![Effect::Player(PlayerCommand::Resume)]
        }
        PlayStatus::Stopped | PlayStatus::Loading => vec![],
    }
}

fn go_previous(state: &mut State) -> Vec<Effect> {
    if state.playback.position >= RESTART_THRESHOLD {
        state.playback.position = Duration::ZERO;
        return vec![Effect::Player(PlayerCommand::Seek(Duration::ZERO))];
    }
    load_or_stop(state, |state| state.playback.queue.previous())
}

fn set_volume(state: &mut State, volume: f32) -> Vec<Effect> {
    let volume = volume.clamp(0.0, 1.0);
    state.playback.volume = volume;
    vec![Effect::Player(PlayerCommand::SetVolume(volume))]
}

fn apply_player_event(state: &mut State, event: PlayerEvent) -> Vec<Effect> {
    match event {
        PlayerEvent::TrackStarted { duration } => {
            state.playback.status = PlayStatus::Playing;
            state.playback.track_duration = duration;
            vec![]
        }
        PlayerEvent::PositionChanged(position) => {
            state.playback.position = position;
            vec![]
        }
        PlayerEvent::TrackEnded => load_or_stop(state, |state| state.playback.queue.on_track_end()),
        PlayerEvent::Failed(message) => {
            state.playback.status = PlayStatus::Stopped;
            state.notices.push(message);
            vec![]
        }
    }
}

/// Advances the queue with `step` and loads the result, or stops playback
/// when the queue is exhausted.
fn load_or_stop(state: &mut State, step: impl FnOnce(&mut State) -> Option<Track>) -> Vec<Effect> {
    let next = step(state);
    if next.is_none() && state.playback.status != PlayStatus::Stopped {
        state.playback.status = PlayStatus::Stopped;
        return vec![Effect::Player(PlayerCommand::Stop)];
    }
    load_track(state, next)
}

fn load_track(state: &mut State, track: Option<Track>) -> Vec<Effect> {
    let Some(track) = track else {
        return vec![];
    };
    state.playback.status = PlayStatus::Loading;
    state.playback.position = Duration::ZERO;
    state.playback.track_duration = track.duration;
    vec![Effect::Player(PlayerCommand::Load(track))]
}

fn set_loadable<T>(slot: &mut Loadable<T>, result: Result<T, String>) {
    *slot = match result {
        Ok(value) => Loadable::Loaded(value),
        Err(message) => Loadable::Failed(message),
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::TrackId;

    fn track(id: &str) -> Track {
        Track {
            id: TrackId(id.to_string()),
            title: id.to_string(),
            artists: vec![],
            album: None,
            duration: None,
            thumbnail_url: None,
        }
    }

    fn no_random(_: usize) -> usize {
        0
    }

    fn apply(state: &mut State, action: Action) -> Vec<Effect> {
        update(state, action, &mut no_random)
    }

    #[test]
    fn cookie_submission_saves_and_verifies() {
        let mut state = State::default();
        state.sign_in.draft = "  c=1  ".into();
        let effects = apply(&mut state, Action::CookiesSubmitted);
        assert_eq!(state.auth, AuthState::Verifying);
        assert_eq!(
            effects,
            vec![
                Effect::SaveCookies("c=1".into()),
                Effect::Api(ApiRequest::VerifyAuth {
                    cookies: "c=1".into()
                }),
            ]
        );
    }

    #[test]
    fn empty_cookie_submission_does_nothing() {
        let mut state = State::default();
        state.sign_in.draft = "  ".into();
        assert_eq!(apply(&mut state, Action::CookiesSubmitted), vec![]);
        assert_eq!(state.auth, AuthState::SignedOut);
    }

    #[test]
    fn sign_in_success_opens_the_library_and_fetches_it() {
        let mut state = State::default();
        let effects = apply(&mut state, Action::AuthVerified(Ok(())));
        assert_eq!(state.auth, AuthState::SignedIn);
        assert_eq!(state.page, Page::Library);
        assert_eq!(
            effects,
            vec![
                Effect::Api(ApiRequest::FetchPlaylists),
                Effect::Api(ApiRequest::FetchLiked),
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
    fn track_end_at_the_queue_end_stops_the_player() {
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
            Action::Player(PlayerEvent::TrackStarted { duration: None }),
        );
        let effects = apply(&mut state, Action::Player(PlayerEvent::TrackEnded));
        assert_eq!(state.playback.status, PlayStatus::Stopped);
        assert_eq!(effects, vec![Effect::Player(PlayerCommand::Stop)]);
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
    fn volume_is_clamped() {
        let mut state = State::default();
        apply(&mut state, Action::VolumeSet(1.7));
        assert_eq!(state.playback.volume, 1.0);
    }
}
