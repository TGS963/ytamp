use super::{
    Action, ApiRequest, Dialog, Effect, LibraryCacheWrite, LibraryWrite, Loadable, Page, Playlist,
    PlaylistId, State, Track, TrackId,
};

pub(super) fn apply(state: &mut State, action: Action) -> Vec<Effect> {
    match action {
        Action::TrackLikeToggled(track) => toggle_track_like(state, track),
        Action::TrackAddedToPlaylist { playlist, track } => {
            add_track_to_playlist(state, playlist, track)
        }
        Action::TrackRemovedFromPlaylist { playlist, item_id } => {
            remove_track_from_playlist(state, playlist, item_id)
        }
        Action::PlaylistCreateRequested(title) => request_playlist_create(state, title),
        Action::CreatePlaylistDialogOpened(then_add) => {
            if then_add.as_ref().is_some_and(Track::is_local) {
                return vec![];
            }
            state.dialog = Some(Dialog::CreatePlaylist {
                title_draft: String::new(),
                then_add,
            });
            vec![]
        }
        Action::CreatePlaylistDraftChanged(draft) => {
            set_create_playlist_draft(state, draft);
            vec![]
        }
        Action::DialogDismissed => {
            state.dialog = None;
            vec![]
        }
        Action::PlaylistItemAdded {
            playlist,
            track,
            result,
        } => finish_playlist_item_add(state, playlist, track, result),
        Action::PlaylistCreated(request_id, result) => {
            finish_playlist_create(state, request_id, result)
        }
        Action::LibraryWriteFinished { what, result } => finish_library_write(state, what, result),
        _ => unreachable!("action routed to the wrong reducer domain"),
    }
}

/// Whether `id` is a member of the liked-songs list, cached or fresh.
/// Also used by the row and player-bar views, so the like button and
/// the row's context menu agree with the reducer about what "liked"
/// means.
pub fn is_liked(liked: &Loadable<Vec<Track>>, id: &TrackId) -> bool {
    liked
        .loaded()
        .is_some_and(|tracks| tracks.iter().any(|track| &track.id == id))
}

/// `playlists`, with the playlist named `id` having its track count
/// raised or lowered by `delta`. A playlist with no known count stays
/// unknown: there is nothing to adjust. The count never drops below
/// zero.
pub(super) fn with_count_delta(
    playlists: Vec<Playlist>,
    id: &PlaylistId,
    delta: i64,
) -> Vec<Playlist> {
    playlists
        .into_iter()
        .map(
            |playlist| match (&playlist.id == id, playlist.track_count) {
                (true, Some(count)) => Playlist {
                    track_count: Some((count as i64 + delta).max(0) as usize),
                    ..playlist
                },
                _ => playlist,
            },
        )
        .collect()
}

/// `tracks`, with the row whose playlist item id is `item_id` dropped.
pub(super) fn without_item(tracks: Vec<Track>, item_id: &str) -> Vec<Track> {
    tracks
        .into_iter()
        .filter(|track| track.playlist_item_id.as_deref() != Some(item_id))
        .collect()
}

/// Likes or unlikes a track: an optimistic change to the liked list,
/// paired with the effect that tells the server. Liked state is the
/// track's membership in the list, so a like inserts at the front and
/// an unlike removes it.
pub(super) fn toggle_track_like(state: &mut State, track: Track) -> Vec<Effect> {
    if track.is_local() {
        return vec![];
    }
    let liked = is_liked(&state.library.liked, &track.id);
    if let Loadable::Loaded(tracks) | Loadable::Refreshing(tracks) = &mut state.library.liked {
        if liked {
            tracks.retain(|shown| shown.id != track.id);
        } else {
            tracks.insert(0, track.clone());
        }
    }
    vec![Effect::Api(ApiRequest::RateTrack {
        id: track.id,
        liked: !liked,
    })]
}

/// Raises the playlist's shown track count and asks the server to add
/// the track. `PlaylistItemAdded` applies the confirmed row once the
/// server answers.
pub(super) fn add_track_to_playlist(
    state: &mut State,
    playlist: PlaylistId,
    track: Track,
) -> Vec<Effect> {
    if track.is_local() {
        return vec![];
    }
    adjust_playlist_count(state, &playlist, 1);
    vec![Effect::Api(ApiRequest::AddToPlaylist { playlist, track })]
}

/// Applies the server's answer to an add request. Success appends the
/// confirmed row to the open playlist, when that playlist is still the
/// one on screen. Failure reports the problem and reverts the count
/// `add_track_to_playlist` raised.
pub(super) fn finish_playlist_item_add(
    state: &mut State,
    playlist: PlaylistId,
    track: Track,
    result: Result<String, String>,
) -> Vec<Effect> {
    match result {
        Ok(item_id) => {
            if state.page == Page::Playlist(playlist) {
                append_to_open_playlist(state, track, item_id);
            }
        }
        Err(message) => {
            adjust_playlist_count(state, &playlist, -1);
            state.notices.push(message);
        }
    }
    vec![]
}

pub(super) fn append_to_open_playlist(state: &mut State, track: Track, item_id: String) {
    if let Loadable::Loaded(tracks) | Loadable::Refreshing(tracks) =
        &mut state.library.open_playlist
    {
        tracks.push(Track {
            source: Default::default(),
            playlist_item_id: Some(item_id),
            ..track
        });
    }
}

/// Drops the row from the open playlist, lowers the playlist's shown
/// count, and asks the server to remove it.
pub(super) fn remove_track_from_playlist(
    state: &mut State,
    playlist: PlaylistId,
    item_id: String,
) -> Vec<Effect> {
    if let Loadable::Loaded(tracks) | Loadable::Refreshing(tracks) =
        &mut state.library.open_playlist
    {
        *tracks = without_item(std::mem::take(tracks), &item_id);
    }
    adjust_playlist_count(state, &playlist, -1);
    vec![Effect::Api(ApiRequest::RemoveFromPlaylist {
        playlist,
        item_id,
    })]
}

/// Applies `with_count_delta` to the playlists slot, when it holds
/// data. A slot that is still loading, or has none, has nothing to
/// adjust.
pub(super) fn adjust_playlist_count(state: &mut State, id: &PlaylistId, delta: i64) {
    if let Loadable::Loaded(playlists) | Loadable::Refreshing(playlists) =
        &mut state.library.playlists
    {
        *playlists = with_count_delta(std::mem::take(playlists), id, delta);
    }
}

/// The create-playlist dialog's Create button. An empty title is
/// ignored, so an accidental Enter on an untouched field does
/// nothing. Otherwise the dialog closes and the track it remembered,
/// if any, waits in `pending_playlist_tracks` for `finish_playlist_create`.
pub(super) fn request_playlist_create(state: &mut State, title: String) -> Vec<Effect> {
    let title = title.trim().to_string();
    if title.is_empty() {
        return vec![];
    }
    state.playlist_request_id += 1;
    let id = state.playlist_request_id;
    state
        .pending_playlist_tracks
        .insert(id, dialog_then_add(state.dialog.take()));
    vec![Effect::Api(ApiRequest::CreatePlaylist(id, title))]
}

pub(super) fn dialog_then_add(dialog: Option<Dialog>) -> Option<Track> {
    match dialog {
        Some(Dialog::CreatePlaylist { then_add, .. }) => then_add,
        None => None,
    }
}

pub(super) fn set_create_playlist_draft(state: &mut State, draft: String) {
    if let Some(Dialog::CreatePlaylist { title_draft, .. }) = &mut state.dialog {
        *title_draft = draft;
    }
}

/// Applies the server's answer to a create request: inserts the new
/// playlist at the front of the library list, or reports the failure.
/// A remembered track from `pending_playlist_tracks` chains straight
/// into an add, the same way a row's "Add to playlist" click would.
pub(super) fn finish_playlist_create(
    state: &mut State,
    request_id: u64,
    result: Result<Playlist, String>,
) -> Vec<Effect> {
    let Some(then_add) = state.pending_playlist_tracks.remove(&request_id) else {
        return vec![];
    };
    match result {
        Ok(playlist) => apply_playlist_created(state, playlist, then_add),
        Err(message) => {
            state.notices.push(message);
            vec![]
        }
    }
}

pub(super) fn apply_playlist_created(
    state: &mut State,
    playlist: Playlist,
    then_add: Option<Track>,
) -> Vec<Effect> {
    insert_playlist(state, playlist.clone());
    let mut effects = vec![Effect::SaveLibraryCache(LibraryCacheWrite::Playlists(
        playlists_snapshot(state),
    ))];
    if let Some(track) = then_add {
        effects.extend(add_track_to_playlist(state, playlist.id, track));
    }
    effects
}

/// Inserts `playlist` at the front of the playlists slot. A slot with
/// no data yet starts fresh with just this playlist in it.
pub(super) fn insert_playlist(state: &mut State, playlist: Playlist) {
    match &mut state.library.playlists {
        Loadable::Loaded(playlists) | Loadable::Refreshing(playlists) => {
            playlists.insert(0, playlist);
        }
        slot => *slot = Loadable::Loaded(vec![playlist]),
    }
}

pub(super) fn playlists_snapshot(state: &State) -> Vec<Playlist> {
    state
        .library
        .playlists
        .loaded()
        .cloned()
        .unwrap_or_default()
}

/// A failed rate or playlist-item write reports the problem and
/// refetches the affected list, so the optimistic change reverts to
/// the truth. A success needs no further action.
pub(super) fn finish_library_write(
    state: &mut State,
    what: LibraryWrite,
    result: Result<(), String>,
) -> Vec<Effect> {
    let Err(message) = result else {
        return vec![];
    };
    state.notices.push(message);
    match what {
        LibraryWrite::Liked => {
            begin_refresh(
                &mut state.library.liked,
                &mut state.library.liked_loading_more,
                &mut state.library.incoming_liked,
            );
            vec![Effect::Api(ApiRequest::FetchLiked)]
        }
        LibraryWrite::Playlist(id) => {
            begin_refresh(
                &mut state.library.open_playlist,
                &mut state.library.open_playlist_loading_more,
                &mut state.library.incoming_playlist,
            );
            vec![
                Effect::Api(ApiRequest::FetchPlaylistTracks(id)),
                Effect::Api(ApiRequest::FetchPlaylists),
            ]
        }
    }
}

/// Prepares a loaded list for a streamed refetch. The pages of a
/// refetch buffer behind a `Refreshing` slot and replace the list at
/// the end. On a `Loaded` slot they would append, and the list would
/// double.
pub(super) fn begin_refresh(
    slot: &mut Loadable<Vec<Track>>,
    loading_more: &mut bool,
    incoming: &mut Vec<Track>,
) {
    *slot = match std::mem::take(slot) {
        Loadable::Loaded(tracks) | Loadable::Refreshing(tracks) => Loadable::Refreshing(tracks),
        other => other,
    };
    *loading_more = false;
    incoming.clear();
}
