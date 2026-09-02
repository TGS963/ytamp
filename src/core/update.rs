//! The reducer: the only writer of the application state.
//!
//! `update` applies one action and returns the effects it causes.
//! It touches no clock, no network, and no audio device. Randomness
//! comes in as a parameter, so every path is testable.

use std::time::Duration;

use super::action::{Action, PlayerEvent};
use super::effect::{ApiRequest, Effect, LibraryCacheWrite, PlayerCommand};
use super::model::{
    AlbumId, AlbumPage, ArtistId, ArtistPage, Playlist, PlaylistId, Track, TrackId,
};
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
        Action::AuthUserDraftChanged(draft) => {
            state.sign_in.authuser_draft = draft;
            vec![]
        }
        Action::CookiesSubmitted => submit_cookies(state),
        Action::SignOutRequested => sign_out(state),
        Action::OAuthClientIdChanged(draft) => {
            state.sign_in.client_id_draft = draft;
            vec![]
        }
        Action::OAuthClientSecretChanged(draft) => {
            state.sign_in.client_secret_draft = draft;
            vec![]
        }
        Action::OAuthStartRequested => start_oauth(state),
        Action::OAuthUrlReady(url) => {
            state.sign_in.oauth_url = Some(url);
            vec![]
        }
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
        Action::ArtistOpened(id) => open_artist(state, id),
        Action::AlbumOpened(id) => open_album(state, id),
        Action::BackPressed => go_back(state),
        Action::PlaylistsLoaded(result) => finish_playlists_load(state, result),
        Action::LikedLoaded(result) => finish_liked_load(state, result),
        Action::PlaylistTracksLoaded(id, result) => finish_playlist_load(state, id, result),
        Action::ArtistLoaded(id, result) => finish_artist_load(state, id, result),
        Action::AlbumLoaded(id, result) => finish_album_load(state, id, result),
        Action::LikedPageLoaded { tracks, finished } => {
            finish_liked_page(state, tracks, finished)
        }
        Action::PlaylistTracksPageLoaded {
            id,
            tracks,
            finished,
        } => finish_playlist_tracks_page(state, id, tracks, finished),
        Action::LibraryCacheLoaded { playlists, liked } => {
            apply_library_cache(state, playlists, liked)
        }
        Action::PlaylistTracksCacheLoaded(id, tracks) => {
            apply_playlist_tracks_cache(state, id, tracks)
        }
        Action::ContextPlayed { tracks, start } => play_context(state, tracks, start, random_below),
        Action::TrackQueued(track) => {
            state.playback.queue.queue_track(track);
            prefetch_next(state)
        }
        Action::TrackHovered(track) => hover_prefetch(state, track),
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
            prefetch_next(state)
        }
        Action::RepeatCycled => {
            state.playback.queue.repeat = state.playback.queue.repeat.cycled();
            prefetch_next(state)
        }
        Action::QueuePanelToggled => {
            state.queue_open = !state.queue_open;
            vec![]
        }
        Action::NoticeDismissed(index) => {
            if index < state.notices.len() {
                state.notices.remove(index);
            }
            vec![]
        }
        Action::StoredAuthFound(method) => {
            state.auth = AuthState::Verifying;
            vec![Effect::Api(ApiRequest::VerifyAuth(method))]
        }
        Action::SessionRestored(session) => restore_session(state, session),
        Action::NoticePosted(message) => {
            state.notices.push(message);
            vec![]
        }
        Action::Player(event) => apply_player_event(state, event),
    }
}

/// A sidebar jump to a whole section. Clears `history`: Back must
/// never cross a deliberate jump like this one.
fn navigate(state: &mut State, page: Page) -> Vec<Effect> {
    state.history.clear();
    let effects = match page {
        Page::Library => fetch_missing_library(state),
        _ => vec![],
    };
    state.page = page;
    effects
}

/// The library fetches that have not run yet. Keeps Library cheap to
/// reopen. The first fetch also reads the disk cache, so the last
/// session's library shows at once while the network call is still
/// in flight.
fn fetch_missing_library(state: &mut State) -> Vec<Effect> {
    let mut effects = vec![];
    let mut needs_cache = false;
    if state.library.playlists == Loadable::NotAsked {
        state.library.playlists = Loadable::Loading;
        effects.push(Effect::Api(ApiRequest::FetchPlaylists));
        needs_cache = true;
    }
    if state.library.liked == Loadable::NotAsked {
        state.library.liked = Loadable::Loading;
        state.library.liked_loading_more = false;
        state.library.incoming_liked.clear();
        effects.push(Effect::Api(ApiRequest::FetchLiked));
        needs_cache = true;
    }
    if needs_cache {
        effects.push(Effect::LoadLibraryCache);
    }
    effects
}

fn submit_cookies(state: &mut State) -> Vec<Effect> {
    if state.sign_in.draft.trim().is_empty() {
        return vec![];
    }
    let parsed = match super::cookie_paste::credentials_from_paste(&state.sign_in.draft) {
        Ok(parsed) => parsed,
        Err(problem) => {
            state.auth = AuthState::Failed(problem);
            return vec![];
        }
    };
    let credentials = crate::core::effect::Credentials {
        cookies: parsed.cookies,
        authuser: normalized_authuser(&state.sign_in.authuser_draft),
        headers: parsed.headers,
    };
    state.auth = AuthState::Verifying;
    vec![
        Effect::SaveCredentials(credentials.clone()),
        Effect::Api(ApiRequest::VerifyAuth(
            crate::core::effect::AuthMethod::Browser(credentials),
        )),
    ]
}

/// The account index for the X-Goog-AuthUser header: "0" when the
/// field stays empty.
fn normalized_authuser(draft: &str) -> String {
    let trimmed = draft.trim();
    if trimmed.is_empty() {
        return "0".to_string();
    }
    trimmed.to_string()
}

fn start_oauth(state: &mut State) -> Vec<Effect> {
    let client_id = state.sign_in.client_id_draft.trim().to_string();
    let client_secret = state.sign_in.client_secret_draft.trim().to_string();
    if client_id.is_empty() || client_secret.is_empty() {
        state.auth =
            AuthState::Failed("Enter both the OAuth client id and the client secret.".to_string());
        return vec![];
    }
    state.auth = AuthState::Verifying;
    state.sign_in.oauth_url = None;
    vec![Effect::Api(ApiRequest::StartOAuth {
        client_id,
        client_secret,
    })]
}

/// Back to the sign-in page with a fresh state. Playback stops and
/// the queue empties: nothing of the session stays audible. Only the
/// volume setting survives.
fn sign_out(state: &mut State) -> Vec<Effect> {
    let volume = state.playback.volume;
    *state = State::default();
    state.playback.volume = volume;
    vec![
        Effect::Player(PlayerCommand::Stop),
        Effect::ClearCredentials,
        Effect::ClearLibraryCache,
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

/// Opens a playlist page and starts its two loads. A playlist already
/// open stays open, with no repeat fetch and no history entry.
fn open_playlist(state: &mut State, id: PlaylistId) -> Vec<Effect> {
    let target = Page::Playlist(id.clone());
    if state.page == target {
        return vec![];
    }
    push_history(state);
    state.page = target;
    start_playlist_load(state, id)
}

/// Resets the playlist's paging state and starts its two loads. Shared
/// by `open_playlist` and by Back restoring a playlist page, so a page
/// still in flight for a playlist the user just left can never leak
/// into the one now shown.
fn start_playlist_load(state: &mut State, id: PlaylistId) -> Vec<Effect> {
    state.library.open_playlist = Loadable::Loading;
    state.library.open_playlist_loading_more = false;
    state.library.incoming_playlist.clear();
    vec![
        Effect::Api(ApiRequest::FetchPlaylistTracks(id.clone())),
        Effect::LoadPlaylistTracksCache(id),
    ]
}

/// Opens an artist page and starts its load. An artist page already
/// open stays open, with no repeat fetch and no history entry.
fn open_artist(state: &mut State, id: ArtistId) -> Vec<Effect> {
    let target = Page::Artist(id.clone());
    if state.page == target {
        return vec![];
    }
    push_history(state);
    state.page = target;
    state.browse.artist = Loadable::Loading;
    vec![Effect::Api(ApiRequest::FetchArtist(id))]
}

/// Opens an album page and starts its load, the same way as
/// `open_artist`.
fn open_album(state: &mut State, id: AlbumId) -> Vec<Effect> {
    let target = Page::Album(id.clone());
    if state.page == target {
        return vec![];
    }
    push_history(state);
    state.page = target;
    state.browse.album = Loadable::Loading;
    vec![Effect::Api(ApiRequest::FetchAlbum(id))]
}

/// Records the page the user is leaving, so Back can return to it.
fn push_history(state: &mut State) {
    state.history.push(state.page.clone());
}

/// Returns to the page Back left. Does nothing with an empty history.
/// A playlist page always re-fetches its tracks, the simplest correct
/// rule since the track list carries no id of its own to check. An
/// artist or album page re-fetches only when its slot does not
/// already hold that same page.
fn go_back(state: &mut State) -> Vec<Effect> {
    let Some(previous) = state.history.pop() else {
        return vec![];
    };
    state.page = previous.clone();
    match previous {
        Page::Playlist(id) => start_playlist_load(state, id),
        Page::Artist(id) => reopen_artist(state, id),
        Page::Album(id) => reopen_album(state, id),
        Page::SignIn | Page::Search | Page::Library => vec![],
    }
}

fn reopen_artist(state: &mut State, id: ArtistId) -> Vec<Effect> {
    if slot_holds(&state.browse.artist, |page| page.id == id) {
        return vec![];
    }
    state.browse.artist = Loadable::Loading;
    vec![Effect::Api(ApiRequest::FetchArtist(id))]
}

fn reopen_album(state: &mut State, id: AlbumId) -> Vec<Effect> {
    if slot_holds(&state.browse.album, |page| page.album.id == id) {
        return vec![];
    }
    state.browse.album = Loadable::Loading;
    vec![Effect::Api(ApiRequest::FetchAlbum(id))]
}

/// Whether `slot` already holds a value that `holds` accepts. Used to
/// decide whether Back needs a fresh fetch for the page it restores.
fn slot_holds<T>(slot: &Loadable<T>, holds: impl Fn(&T) -> bool) -> bool {
    match slot {
        Loadable::Loaded(value) | Loadable::Refreshing(value) => holds(value),
        _ => false,
    }
}

/// Applies a loaded or failed artist page. Ignores a result for an
/// artist page the user has already left, the same guard as
/// `finish_playlist_load`.
fn finish_artist_load(
    state: &mut State,
    id: ArtistId,
    result: Result<ArtistPage, String>,
) -> Vec<Effect> {
    if state.page != Page::Artist(id) {
        return vec![];
    }
    set_loadable(&mut state.browse.artist, result);
    vec![]
}

/// Applies a loaded or failed album page, the same way as
/// `finish_artist_load`.
fn finish_album_load(
    state: &mut State,
    id: AlbumId,
    result: Result<AlbumPage, String>,
) -> Vec<Effect> {
    if state.page != Page::Album(id) {
        return vec![];
    }
    set_loadable(&mut state.browse.album, result);
    vec![]
}

/// Applies a fresh track list and schedules it for the cache.
/// Ignores a result for a playlist the user has already left.
fn finish_playlist_load(
    state: &mut State,
    id: PlaylistId,
    result: Result<Vec<Track>, String>,
) -> Vec<Effect> {
    if state.page != Page::Playlist(id.clone()) {
        return vec![];
    }
    state.library.open_playlist_loading_more = false;
    let Ok(tracks) = result else {
        apply_load_failure(
            &mut state.library.open_playlist,
            result.unwrap_err(),
            &mut state.notices,
        );
        return vec![];
    };
    state.library.open_playlist = Loadable::Loaded(tracks.clone());
    vec![Effect::SaveLibraryCache(LibraryCacheWrite::PlaylistTracks(
        id, tracks,
    ))]
}

/// Applies one page of the open playlist's track list. Ignores a page
/// for a playlist the user has already left, the same way as
/// `finish_playlist_load`.
fn finish_playlist_tracks_page(
    state: &mut State,
    id: PlaylistId,
    tracks: Vec<Track>,
    finished: bool,
) -> Vec<Effect> {
    if state.page != Page::Playlist(id.clone()) {
        return vec![];
    }
    let full = apply_list_page(
        &mut state.library.open_playlist,
        &mut state.library.open_playlist_loading_more,
        &mut state.library.incoming_playlist,
        tracks,
        finished,
    );
    save_full_list(full, |all| LibraryCacheWrite::PlaylistTracks(id, all))
}

/// Applies a fresh playlist list and schedules it for the cache.
fn finish_playlists_load(state: &mut State, result: Result<Vec<Playlist>, String>) -> Vec<Effect> {
    let Ok(playlists) = result else {
        apply_load_failure(
            &mut state.library.playlists,
            result.unwrap_err(),
            &mut state.notices,
        );
        return vec![];
    };
    state.library.playlists = Loadable::Loaded(playlists.clone());
    vec![Effect::SaveLibraryCache(LibraryCacheWrite::Playlists(
        playlists,
    ))]
}

/// Applies a fresh liked-songs list and schedules it for the cache.
fn finish_liked_load(state: &mut State, result: Result<Vec<Track>, String>) -> Vec<Effect> {
    state.library.liked_loading_more = false;
    let Ok(tracks) = result else {
        apply_load_failure(
            &mut state.library.liked,
            result.unwrap_err(),
            &mut state.notices,
        );
        return vec![];
    };
    state.library.liked = Loadable::Loaded(tracks.clone());
    vec![Effect::SaveLibraryCache(LibraryCacheWrite::Liked(tracks))]
}

/// Applies one page of the liked-songs list.
fn finish_liked_page(state: &mut State, tracks: Vec<Track>, finished: bool) -> Vec<Effect> {
    let full = apply_list_page(
        &mut state.library.liked,
        &mut state.library.liked_loading_more,
        &mut state.library.incoming_liked,
        tracks,
        finished,
    );
    save_full_list(full, LibraryCacheWrite::Liked)
}

/// Applies one page of a streaming track list to `slot`. A page while
/// `slot` shows no cached value starts or grows the visible list at
/// once, with `loading_more` as the spinner flag. A page while `slot`
/// shows a cached refresh buffers into `incoming` instead, so the
/// cached list stays on screen until the stream finishes. Returns the
/// full list once `finished` is true, so the caller can cache it; a
/// partial list is never returned, and so never reaches the cache.
fn apply_list_page(
    slot: &mut Loadable<Vec<Track>>,
    loading_more: &mut bool,
    incoming: &mut Vec<Track>,
    tracks: Vec<Track>,
    finished: bool,
) -> Option<Vec<Track>> {
    if matches!(slot, Loadable::Refreshing(_)) {
        incoming.extend(tracks);
        if !finished {
            return None;
        }
        let all = std::mem::take(incoming);
        *slot = Loadable::Loaded(all.clone());
        return Some(all);
    }
    if !matches!(slot, Loadable::Loaded(_)) {
        *slot = Loadable::Loaded(Vec::new());
    }
    let Loadable::Loaded(existing) = slot else {
        unreachable!("the branch above just normalized the slot to Loaded")
    };
    existing.extend(tracks);
    *loading_more = !finished;
    finished.then(|| existing.clone())
}

/// The cache-write effect for a finished stream's full list, or no
/// effect while the list is still partial.
fn save_full_list(
    full: Option<Vec<Track>>,
    write: impl FnOnce(Vec<Track>) -> LibraryCacheWrite,
) -> Vec<Effect> {
    match full {
        Some(tracks) => vec![Effect::SaveLibraryCache(write(tracks))],
        None => vec![],
    }
}

/// Applies a network failure to a library slot. A slot that already
/// shows data, cached or from pages the stream already delivered,
/// keeps it on screen: the failure becomes a notice instead of an
/// error page. An empty slot shows the failure itself.
fn apply_load_failure<T>(slot: &mut Loadable<T>, message: String, notices: &mut Vec<String>) {
    match std::mem::take(slot) {
        Loadable::Refreshing(shown) | Loadable::Loaded(shown) => {
            *slot = Loadable::Loaded(shown);
            notices.push(message);
        }
        _ => *slot = Loadable::Failed(message),
    }
}

/// Applies cached library data. A slot accepts the cache only while
/// it is still loading, so fresh network data that already landed,
/// or a load that already failed, never loses to a late cache hit.
fn apply_library_cache(
    state: &mut State,
    playlists: Option<Vec<Playlist>>,
    liked: Option<Vec<Track>>,
) -> Vec<Effect> {
    apply_cached_if_loading(&mut state.library.playlists, playlists);
    apply_cached_if_loading(&mut state.library.liked, liked);
    vec![]
}

/// Applies a playlist's cached track list, honoring the same
/// still-loading rule as `apply_library_cache`. Ignores a cache hit
/// for a playlist the user has already left.
fn apply_playlist_tracks_cache(state: &mut State, id: PlaylistId, tracks: Vec<Track>) -> Vec<Effect> {
    if state.page == Page::Playlist(id) {
        apply_cached_if_loading(&mut state.library.open_playlist, Some(tracks));
    }
    vec![]
}

/// Puts `cached` into `slot` as a silent refresh, but only while
/// `slot` is still loading. A slot that already holds fresh or
/// failed data keeps it, so a late cache hit can never overwrite a
/// network result.
fn apply_cached_if_loading<T>(slot: &mut Loadable<T>, cached: Option<T>) {
    let Some(value) = cached else { return };
    if matches!(slot, Loadable::Loading) {
        *slot = Loadable::Refreshing(value);
    }
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
        PlayStatus::Stopped => restart_current(state),
        PlayStatus::Loading => vec![],
    }
}

/// Play on a stopped player reloads the current track and continues
/// where it stopped. This is how a restored session resumes.
fn restart_current(state: &mut State) -> Vec<Effect> {
    let Some(track) = state.playback.queue.current().cloned() else {
        return vec![];
    };
    let position = state.playback.position;
    state.playback.resume_position = (!position.is_zero()).then_some(position);
    load_track(state, Some(track))
}

fn restore_session(state: &mut State, session: crate::core::session::SavedSession) -> Vec<Effect> {
    state.playback.position = session.position();
    state.playback.queue = session.queue;
    state.playback.volume = session.volume.clamp(0.0, 1.0);
    state.playback.status = PlayStatus::Stopped;
    state.playback.track_duration = state
        .playback
        .queue
        .current()
        .and_then(|track| track.duration);
    vec![Effect::Player(PlayerCommand::SetVolume(
        state.playback.volume,
    ))]
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
            state.playback.track_duration = duration.or(state.playback.track_duration);
            let mut effects = prefetch_next(state);
            if let Some(position) = state.playback.resume_position.take() {
                state.playback.position = position;
                effects.push(Effect::Player(PlayerCommand::Seek(position)));
            }
            effects
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

/// The effect that warms the cache for the track after the current
/// one, or no effect when the queue has nothing more to play.
fn prefetch_next(state: &State) -> Vec<Effect> {
    match state.playback.queue.peek_next() {
        Some(track) => vec![Effect::Player(PlayerCommand::Prefetch(track))],
        None => vec![],
    }
}

/// Warms the cache for a track the pointer has rested on, unless
/// `should_hover_prefetch` finds a reason to skip it. Remembers the
/// id, so a pointer that leaves and returns to the same row does not
/// start a second download.
fn hover_prefetch(state: &mut State, track: Track) -> Vec<Effect> {
    let current = state.playback.queue.current().map(|track| &track.id);
    let last = state.playback.last_hover_prefetch.as_ref();
    if !should_hover_prefetch(current, last, &track.id) {
        return vec![];
    }
    state.playback.last_hover_prefetch = Some(track.id.clone());
    vec![Effect::Player(PlayerCommand::Prefetch(track))]
}

/// Whether a hover on `hovered` should start a prefetch: not the
/// track already playing, and not a repeat of the last hover
/// prefetch.
fn should_hover_prefetch(
    current: Option<&TrackId>,
    last_hover_prefetch: Option<&TrackId>,
    hovered: &TrackId,
) -> bool {
    current != Some(hovered) && last_hover_prefetch != Some(hovered)
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
            album_id: None,
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
        const FULL: &str = "SID=a; SAPISID=b; __Secure-3PAPISID=c; __Secure-3PSID=d";
        let mut state = State::default();
        state.sign_in.draft = format!("  {FULL}  ");
        state.sign_in.authuser_draft = " 2 ".into();
        let effects = apply(&mut state, Action::CookiesSubmitted);
        let credentials = crate::core::effect::Credentials {
            cookies: FULL.into(),
            authuser: "2".into(),
            headers: vec![],
        };
        assert_eq!(state.auth, AuthState::Verifying);
        assert_eq!(
            effects,
            vec![
                Effect::SaveCredentials(credentials.clone()),
                Effect::Api(ApiRequest::VerifyAuth(
                    crate::core::effect::AuthMethod::Browser(credentials)
                )),
            ]
        );
    }

    #[test]
    fn an_empty_authuser_field_means_account_zero() {
        assert_eq!(normalized_authuser("  "), "0");
        assert_eq!(normalized_authuser(" 1 "), "1");
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
    fn a_paste_without_session_cookies_fails_at_once() {
        let mut state = State::default();
        state.sign_in.draft = "SAPISID=b; YSC=x".into();
        let effects = apply(&mut state, Action::CookiesSubmitted);
        assert_eq!(effects, vec![]);
        let AuthState::Failed(message) = &state.auth else {
            panic!("expected a failure, got {:?}", state.auth);
        };
        assert!(message.contains("SID"));
        assert!(message.contains("__Secure-3PSID"));
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
                Effect::LoadLibraryCache,
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
        let mut state = State::default();
        state.page = Page::Search;
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
        let mut state = State::default();
        state.page = Page::Search;
        apply(&mut state, Action::ArtistOpened(ArtistId("ar1".into())));
        let effects = apply(&mut state, Action::ArtistOpened(ArtistId("ar1".into())));
        assert_eq!(effects, vec![]);
        assert_eq!(state.history, vec![Page::Search]);
    }

    #[test]
    fn back_pops_and_restores_a_playlist() {
        let mut state = State::default();
        state.page = Page::Search;
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
        let mut state = State::default();
        state.page = Page::Search;
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
        let mut state = State::default();
        state.page = Page::Search;
        let effects = apply(&mut state, Action::BackPressed);
        assert_eq!(state.page, Page::Search);
        assert_eq!(effects, vec![]);
    }

    #[test]
    fn a_stale_artist_result_is_ignored() {
        let mut state = State::default();
        state.page = Page::Search;
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
        let mut state = State::default();
        state.page = Page::Search;
        apply(&mut state, Action::AlbumOpened(AlbumId("al1".into())));
        apply(
            &mut state,
            Action::AlbumLoaded(AlbumId("al1".into()), Err("no album".into())),
        );
        assert_eq!(state.browse.album, Loadable::Failed("no album".into()));
    }

    #[test]
    fn sidebar_navigation_clears_history() {
        let mut state = State::default();
        state.page = Page::Search;
        apply(&mut state, Action::ArtistOpened(ArtistId("ar1".into())));
        assert_eq!(state.history, vec![Page::Search]);
        apply(&mut state, Action::NavigatedTo(Page::Library));
        assert_eq!(state.history, vec![]);
    }

    #[test]
    fn sign_out_resets_browse_and_history() {
        let mut state = State::default();
        state.page = Page::Search;
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
        apply(&mut restored, Action::SessionRestored(saved));
        assert_eq!(restored.playback.status, PlayStatus::Stopped);
        assert_eq!(restored.playback.position, Duration::from_secs(30));

        let effects = apply(&mut restored, Action::PlayToggled);
        assert_eq!(
            effects,
            vec![Effect::Player(PlayerCommand::Load(track("a")))]
        );
        let effects = apply(
            &mut restored,
            Action::Player(PlayerEvent::TrackStarted { duration: None }),
        );
        assert_eq!(
            effects,
            vec![Effect::Player(PlayerCommand::Seek(Duration::from_secs(30)))]
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
            Action::Player(PlayerEvent::TrackStarted { duration: None }),
        );
        assert_eq!(
            effects,
            vec![Effect::Player(PlayerCommand::Prefetch(track("b")))]
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
            vec![Effect::Player(PlayerCommand::Prefetch(track("q")))]
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
            Effect::Player(PlayerCommand::Prefetch(_))
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
            vec![Effect::Player(PlayerCommand::Prefetch(track("a")))]
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
        assert_eq!(effects, vec![]);
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
    fn a_track_start_at_the_queue_end_prefetches_nothing() {
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
            Action::Player(PlayerEvent::TrackStarted { duration: None }),
        );
        assert_eq!(effects, vec![]);
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
}
