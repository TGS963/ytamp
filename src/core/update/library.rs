use super::{
    Action, AlbumId, AlbumPage, ApiRequest, Effect, LibraryCacheWrite, Loadable, Page, Playlist,
    PlaylistId, State, Track, finish_artist_load, set_loadable,
};

pub(super) fn apply(state: &mut State, action: Action) -> Vec<Effect> {
    match action {
        Action::HistoryRequested { more } => state.listening_history.request(more),
        action @ Action::HistoryLoaded { .. } => {
            state.listening_history.apply(action);
            vec![]
        }
        Action::LibraryRefreshRequested => {
            if matches!(
                state.library.playlists,
                Loadable::Loading | Loadable::Refreshing(_)
            ) || matches!(
                state.library.liked,
                Loadable::Loading | Loadable::Refreshing(_)
            ) {
                return vec![];
            }
            state.library.playlists = Loadable::NotAsked;
            state.library.liked = Loadable::NotAsked;
            fetch_missing_library(state)
        }
        Action::PlaylistsLoaded(result) => finish_playlists_load(state, result),
        Action::PlaylistCoversLoaded(covers) => finish_playlist_covers_load(state, covers),
        Action::LikedLoaded(result) => finish_liked_load(state, result),
        Action::PlaylistTracksLoaded(id, result) => finish_playlist_load(state, id, result),
        Action::ArtistLoaded(id, result) => finish_artist_load(state, id, result),
        Action::AlbumLoaded(id, result) => finish_album_load(state, id, result),
        Action::LikedPageLoaded { tracks, finished } => finish_liked_page(state, tracks, finished),
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
        Action::DiscoveryCacheLoaded(page) => {
            let feed = &mut state.discovery.home;
            if !feed.loaded {
                feed.page = page;
                feed.loaded = true;
            }
            vec![]
        }
        _ => unreachable!("action routed to the wrong reducer domain"),
    }
}

/// The library fetches that have not run yet. Keeps Library cheap to
/// reopen. The first fetch also reads the disk cache, so the last
/// session's library shows at once while the network call is still
/// in flight.
pub(super) fn fetch_missing_library(state: &mut State) -> Vec<Effect> {
    let mut effects = vec![];
    let mut needs_cache = false;
    if matches!(
        state.library.playlists,
        Loadable::NotAsked | Loadable::Failed(_)
    ) {
        state.library.playlists = Loadable::Loading;
        effects.push(Effect::Api(ApiRequest::FetchPlaylists));
        needs_cache = true;
    }
    if matches!(
        state.library.liked,
        Loadable::NotAsked | Loadable::Failed(_)
    ) {
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

/// Resets the playlist's paging state and starts its two loads. Shared
/// by `open_playlist` and by Back restoring a playlist page, so a page
/// still in flight for a playlist the user just left can never leak
/// into the one now shown.
pub(super) fn start_playlist_load(state: &mut State, id: PlaylistId) -> Vec<Effect> {
    state.library.open_playlist = Loadable::Loading;
    state.library.open_playlist_loading_more = false;
    state.library.incoming_playlist.clear();
    vec![
        Effect::Api(ApiRequest::FetchPlaylistTracks(id.clone())),
        Effect::LoadPlaylistTracksCache(id),
    ]
}

/// Applies a loaded or failed album page, the same way as
/// `finish_artist_load`.
pub(super) fn finish_album_load(
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
pub(super) fn finish_playlist_load(
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
pub(super) fn finish_playlist_tracks_page(
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

/// Applies a fresh playlist list, keeping a custom cover a prior load
/// already found even where the fresh list carries the Data API's
/// generic per-video thumbnail instead. Schedules the merged list for
/// the cache, then asks for the custom cover of every playlist that
/// still has none.
pub(super) fn finish_playlists_load(
    state: &mut State,
    result: Result<Vec<Playlist>, String>,
) -> Vec<Effect> {
    let Ok(playlists) = result else {
        apply_load_failure(
            &mut state.library.playlists,
            result.unwrap_err(),
            &mut state.notices,
        );
        return vec![];
    };
    let previous = previous_playlists(&state.library.playlists);
    let merged = merge_covers(playlists, &previous);
    state.library.playlists = Loadable::Loaded(merged.clone());
    let mut effects = vec![Effect::SaveLibraryCache(LibraryCacheWrite::Playlists(
        merged.clone(),
    ))];
    let missing_covers = ids_without_cover(&merged);
    if !missing_covers.is_empty() {
        effects.push(Effect::Api(ApiRequest::FetchPlaylistCovers(missing_covers)));
    }
    effects
}

/// The playlist list a prior load left in `slot`, or an empty list
/// when there was none. Read before a fresh list overwrites `slot`,
/// so a fresh load can carry forward a cover the fresh data lacks.
pub(super) fn previous_playlists(slot: &Loadable<Vec<Playlist>>) -> Vec<Playlist> {
    slot.loaded().cloned().unwrap_or_default()
}

/// `fresh`, with each playlist's cover kept from `previous` where
/// `previous` already held a custom cover and `fresh` does not. A
/// refresh can never downgrade a playlist from its custom cover back
/// to the Data API's generic thumbnail.
pub(super) fn merge_covers(fresh: Vec<Playlist>, previous: &[Playlist]) -> Vec<Playlist> {
    fresh
        .into_iter()
        .map(|playlist| match custom_cover_of(previous, &playlist.id) {
            Some(cover) if !is_custom_cover(playlist.thumbnail_url.as_deref()) => Playlist {
                thumbnail_url: Some(cover),
                ..playlist
            },
            _ => playlist,
        })
        .collect()
}

/// The playlists in `playlists` that carry no custom cover, by id.
pub(super) fn ids_without_cover(playlists: &[Playlist]) -> Vec<PlaylistId> {
    playlists
        .iter()
        .filter(|playlist| !is_custom_cover(playlist.thumbnail_url.as_deref()))
        .map(|playlist| playlist.id.clone())
        .collect()
}

/// `id`'s custom cover in `playlists`, when it has one.
pub(super) fn custom_cover_of(playlists: &[Playlist], id: &PlaylistId) -> Option<String> {
    let playlist = playlists.iter().find(|playlist| &playlist.id == id)?;
    is_custom_cover(playlist.thumbnail_url.as_deref())
        .then(|| playlist.thumbnail_url.clone())
        .flatten()
}

/// A playlist cover url from the anonymous InnerTube playlist page,
/// rather than the Data API's generic per-video thumbnail.
pub(super) fn is_custom_cover(url: Option<&str>) -> bool {
    url.is_some_and(|url| url.contains("googleusercontent.com"))
}

/// Applies a batch of fetched playlist covers to the playlists slot,
/// whether it currently shows a cached list or the fresh network one,
/// and schedules the result for the cache. An empty batch, the shape
/// a failed fetch delivers, changes nothing.
pub(super) fn finish_playlist_covers_load(
    state: &mut State,
    covers: Vec<(PlaylistId, String)>,
) -> Vec<Effect> {
    if covers.is_empty() {
        return vec![];
    }
    match &mut state.library.playlists {
        Loadable::Loaded(playlists) | Loadable::Refreshing(playlists) => {
            *playlists = apply_covers(std::mem::take(playlists), &covers);
            vec![Effect::SaveLibraryCache(LibraryCacheWrite::Playlists(
                playlists.clone(),
            ))]
        }
        _ => vec![],
    }
}

/// `playlists`, with each playlist's thumbnail replaced by its cover
/// in `covers`, when `covers` carries one for that playlist's id.
pub(super) fn apply_covers(
    playlists: Vec<Playlist>,
    covers: &[(PlaylistId, String)],
) -> Vec<Playlist> {
    playlists
        .into_iter()
        .map(
            |playlist| match covers.iter().find(|(id, _)| id == &playlist.id) {
                Some((_, url)) => Playlist {
                    thumbnail_url: Some(url.clone()),
                    ..playlist
                },
                None => playlist,
            },
        )
        .collect()
}

/// Applies a fresh liked-songs list and schedules it for the cache.
pub(super) fn finish_liked_load(
    state: &mut State,
    result: Result<Vec<Track>, String>,
) -> Vec<Effect> {
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
pub(super) fn finish_liked_page(
    state: &mut State,
    tracks: Vec<Track>,
    finished: bool,
) -> Vec<Effect> {
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
pub(super) fn apply_list_page(
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
pub(super) fn save_full_list(
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
pub(super) fn apply_load_failure<T>(
    slot: &mut Loadable<T>,
    message: String,
    notices: &mut Vec<String>,
) {
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
pub(super) fn apply_library_cache(
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
pub(super) fn apply_playlist_tracks_cache(
    state: &mut State,
    id: PlaylistId,
    tracks: Vec<Track>,
) -> Vec<Effect> {
    if state.page == Page::Playlist(id) {
        apply_cached_if_loading(&mut state.library.open_playlist, Some(tracks));
    }
    vec![]
}

/// Puts `cached` into `slot` as a silent refresh, but only while
/// `slot` is still loading. A slot that already holds fresh or
/// failed data keeps it, so a late cache hit can never overwrite a
/// network result.
pub(super) fn apply_cached_if_loading<T>(slot: &mut Loadable<T>, cached: Option<T>) {
    let Some(value) = cached else { return };
    if matches!(slot, Loadable::Loading) {
        *slot = Loadable::Refreshing(value);
    }
}
