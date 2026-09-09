use super::account::{run_oauth_flow, sign_in};
use super::{Action, Api, ApiRequest, ApiSlot, EffectRuntime, LibraryWrite, PlaylistId};

impl EffectRuntime {
    pub(super) fn run_api_request(&self, request: ApiRequest) {
        let api = self.api.clone();
        let generation = api.read().expect("api lock").generation;
        let deliver = self.scoped_delivery();
        let task = self.tokio.spawn(async move {
            execute_api_request(&api, generation, request, &deliver).await;
        });
        let mut pending = self.pending_api.lock().expect("task lock");
        pending.retain(|task| !task.is_finished());
        pending.push(task.abort_handle());
    }
}

pub(super) async fn execute_api_request(
    slot: &ApiSlot,
    generation: u64,
    request: ApiRequest,
    deliver: &(impl Fn(Action) + Send),
) {
    if slot.read().expect("api lock").generation != generation {
        return;
    }
    match request {
        ApiRequest::VerifyAuth(method) => deliver(sign_in(slot, generation, &method).await),
        ApiRequest::StartOAuth {
            client_id,
            client_secret,
        } => {
            run_oauth_flow(slot, generation, client_id, client_secret, deliver).await;
        }
        other => {
            let signed_in = slot.read().expect("api lock").api.clone();
            match signed_in {
                Some(api) => {
                    execute_signed_in(&api, other, deliver).await;
                    if api.needs_sign_in() {
                        deliver(Action::SessionExpired);
                    }
                }
                None => deliver(request_failure(other, "not signed in".to_string())),
            }
        }
    }
}

/// Runs one request against a signed-in session. `Search` and
/// `FetchPlaylists` deliver a single result action. `FetchLiked` and
/// `FetchPlaylistTracks` stream: one page action per page, the last
/// one carrying `finished: true`, or a failure action on a mid-stream
/// error.
pub(super) async fn execute_signed_in(
    api: &Api,
    request: ApiRequest,
    deliver: &(impl Fn(Action) + Send),
) {
    match request {
        ApiRequest::VerifyAuth(_) | ApiRequest::StartOAuth { .. } => {
            unreachable!("handled before the sign-in check")
        }
        ApiRequest::Search { request_id, query } => {
            deliver(Action::SearchLoaded(request_id, api.search(&query).await))
        }
        ApiRequest::FetchDiscovery {
            request_id,
            target,
            continuation,
        } => {
            let result = api.discovery(&target, continuation).await;
            deliver(Action::DiscoveryLoaded {
                request_id,
                target,
                result,
            });
        }
        ApiRequest::FetchHistory {
            request_id,
            continuation,
        } => {
            deliver(Action::HistoryLoaded {
                request_id,
                result: api.history(continuation).await,
            });
        }
        ApiRequest::FetchPlaylists => {
            deliver(Action::PlaylistsLoaded(api.library_playlists().await))
        }
        ApiRequest::FetchLiked => stream_liked(api, deliver).await,
        ApiRequest::FetchPlaylistTracks(id) => stream_playlist_tracks(api, id, deliver).await,
        ApiRequest::FetchArtist(id) => {
            let result = api.artist(&id).await;
            deliver(Action::ArtistLoaded(id, result));
        }
        ApiRequest::FetchTrackDurations(ids) => {
            deliver(Action::TrackDurationsLoaded(
                api.track_durations(&ids).await,
            ));
        }
        ApiRequest::FetchAlbum(id) => {
            let result = api.album(&id).await;
            deliver(Action::AlbumLoaded(id, result));
        }
        ApiRequest::StartRadio {
            request_id,
            playback_generation,
            seed,
        } => {
            let result = api.radio(&seed.id).await;
            deliver(Action::RadioStarted {
                request_id,
                playback_generation,
                seed,
                result,
            });
        }
        ApiRequest::FetchRadio(id) => {
            let result = api.radio(&id).await;
            deliver(Action::RadioLoaded(id, result));
        }
        ApiRequest::FetchPlaylistCovers(ids) => {
            let covers = api.playlist_covers(&ids).await;
            deliver(Action::PlaylistCoversLoaded(covers));
        }
        ApiRequest::RateTrack { id, liked } => {
            let result = api.rate_track(&id, liked).await;
            deliver(Action::LibraryWriteFinished {
                what: LibraryWrite::Liked,
                result,
            });
        }
        ApiRequest::AddToPlaylist { playlist, track } => {
            let result = api.add_to_playlist(&playlist, &track.id).await;
            deliver(Action::PlaylistItemAdded {
                playlist,
                track,
                result,
            });
        }
        ApiRequest::RemoveFromPlaylist { playlist, item_id } => {
            let result = api.remove_from_playlist(&playlist, &item_id).await;
            deliver(Action::LibraryWriteFinished {
                what: LibraryWrite::Playlist(playlist),
                result,
            });
        }
        ApiRequest::CreatePlaylist(request_id, title) => {
            deliver(Action::PlaylistCreated(
                request_id,
                api.create_playlist(&title).await,
            ));
        }
    }
}

/// Streams the liked-songs list, delivering one `LikedPageLoaded` per
/// page. A mid-stream error becomes a `LikedLoaded` failure, after the
/// pages already delivered.
pub(super) async fn stream_liked(api: &Api, deliver: &(impl Fn(Action) + Send)) {
    let result = api
        .liked_songs(|tracks, finished| deliver(Action::LikedPageLoaded { tracks, finished }))
        .await;
    if let Err(message) = result {
        deliver(Action::LikedLoaded(Err(message)));
    }
}

/// Streams one playlist's tracks, the same way as `stream_liked`.
pub(super) async fn stream_playlist_tracks(
    api: &Api,
    id: PlaylistId,
    deliver: &(impl Fn(Action) + Send),
) {
    let page_id = id.clone();
    let result = api
        .playlist_tracks(&id, |tracks, finished| {
            deliver(Action::PlaylistTracksPageLoaded {
                id: page_id.clone(),
                tracks,
                finished,
            })
        })
        .await;
    if let Err(message) = result {
        deliver(Action::PlaylistTracksLoaded(id, Err(message)));
    }
}

/// The failure action that matches what a request loads.
pub(super) fn request_failure(request: ApiRequest, message: String) -> Action {
    match request {
        ApiRequest::VerifyAuth(_) | ApiRequest::StartOAuth { .. } => {
            Action::AuthVerified(Err(message.into()))
        }
        ApiRequest::Search { request_id, .. } => Action::SearchLoaded(request_id, Err(message)),
        ApiRequest::FetchDiscovery {
            request_id, target, ..
        } => Action::DiscoveryLoaded {
            request_id,
            target,
            result: Err(message),
        },
        ApiRequest::FetchHistory { request_id, .. } => Action::HistoryLoaded {
            request_id,
            result: Err(message),
        },
        ApiRequest::FetchPlaylists => Action::PlaylistsLoaded(Err(message)),
        ApiRequest::FetchLiked => Action::LikedLoaded(Err(message)),
        ApiRequest::FetchPlaylistTracks(id) => Action::PlaylistTracksLoaded(id, Err(message)),
        ApiRequest::FetchArtist(id) => Action::ArtistLoaded(id, Err(message)),
        ApiRequest::FetchTrackDurations(_) => Action::TrackDurationsLoaded(Err(message)),
        ApiRequest::FetchAlbum(id) => Action::AlbumLoaded(id, Err(message)),
        ApiRequest::StartRadio {
            request_id,
            playback_generation,
            seed,
        } => Action::RadioStarted {
            request_id,
            playback_generation,
            seed,
            result: Err(message),
        },
        ApiRequest::FetchRadio(id) => Action::RadioLoaded(id, Err(message)),
        ApiRequest::FetchPlaylistCovers(_) => Action::PlaylistCoversLoaded(vec![]),
        ApiRequest::RateTrack { .. } => Action::LibraryWriteFinished {
            what: LibraryWrite::Liked,
            result: Err(message),
        },
        ApiRequest::AddToPlaylist { playlist, track } => Action::PlaylistItemAdded {
            playlist,
            track,
            result: Err(message),
        },
        ApiRequest::RemoveFromPlaylist { playlist, .. } => Action::LibraryWriteFinished {
            what: LibraryWrite::Playlist(playlist),
            result: Err(message),
        },
        ApiRequest::CreatePlaylist(request_id, _) => {
            Action::PlaylistCreated(request_id, Err(message))
        }
    }
}
