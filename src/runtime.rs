//! Executes effects and sends result actions back to the app.
//!
//! Network work runs on a tokio runtime. Every task ends by sending one
//! action and asking the UI for a repaint. Player commands are still a
//! stub until the player engine lands.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};

use crate::api::Api;
use crate::auth;
use crate::core::action::{Action, LibraryWrite};
use crate::core::effect::{ApiRequest, Effect, LibraryCacheWrite};
use crate::core::model::PlaylistId;
use crate::library_cache;
use crate::player::PlayerHandle;
use crate::skin::Skin;
use crate::skins_dir;
use crate::stream::ResolverChain;

type ApiSlot = Arc<RwLock<Option<Api>>>;

pub struct EffectRuntime {
    tokio: tokio::runtime::Runtime,
    api: ApiSlot,
    actions: Sender<Action>,
    request_repaint: Arc<dyn Fn() + Send + Sync>,
    player: PlayerHandle,
}

impl EffectRuntime {
    pub fn new(
        actions: Sender<Action>,
        request_repaint: impl Fn() + Send + Sync + 'static,
    ) -> Self {
        let tokio = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("the tokio runtime failed to start");
        let request_repaint: Arc<dyn Fn() + Send + Sync> = Arc::new(request_repaint);
        let resolvers = Arc::new(ResolverChain::with_default_resolvers());
        warm_up_sources(&tokio, resolvers.clone());
        let player = crate::player::spawn(
            delivery(actions.clone(), request_repaint.clone()),
            tokio.handle().clone(),
            resolvers,
        );
        Self {
            tokio,
            api: Arc::new(RwLock::new(None)),
            actions,
            request_repaint,
            player,
        }
    }

    pub fn run(&self, effect: Effect) {
        match effect {
            Effect::Api(request) => self.run_api_request(request),
            Effect::SaveCredentials(credentials) => self.save_credentials(credentials),
            Effect::ClearCredentials => self.clear_credentials(),
            Effect::Player(command) => self.player.send(command),
            Effect::LoadLibraryCache => self.load_library_cache(),
            Effect::LoadPlaylistTracksCache(id) => self.load_playlist_tracks_cache(id),
            Effect::SaveLibraryCache(write) => self.save_library_cache(write),
            Effect::ClearLibraryCache => self.clear_library_cache(),
            Effect::LoadSkin(name) => self.load_skin(name),
            Effect::InstallSkin(path) => self.install_skin(path),
            Effect::RefreshSkinList => self.refresh_skin_list(),
        }
    }

    /// Decodes a skin on the blocking pool and delivers it. `App`
    /// routes the result straight to the shell, never to the reducer.
    fn load_skin(&self, name: Option<String>) {
        let deliver = self.delivery();
        self.tokio.spawn_blocking(move || {
            deliver(Action::SkinLoaded(load_named_skin(name)));
        });
    }

    /// Copies a dropped skin into the skins folder on the blocking
    /// pool and delivers the result.
    fn install_skin(&self, path: PathBuf) {
        let deliver = self.delivery();
        self.tokio.spawn_blocking(move || {
            deliver(Action::SkinInstalled(skins_dir::install(&path)));
        });
    }

    /// Lists the skins folder again on the blocking pool.
    fn refresh_skin_list(&self) {
        let deliver = self.delivery();
        self.tokio
            .spawn_blocking(move || deliver(Action::SkinListRefreshed(skins_dir::list_skins())));
    }

    fn run_api_request(&self, request: ApiRequest) {
        let api = self.api.clone();
        let deliver = self.delivery();
        self.tokio.spawn(async move {
            execute_api_request(&api, request, &deliver).await;
        });
    }

    fn save_credentials(&self, credentials: crate::core::effect::Credentials) {
        let deliver = self.delivery();
        self.tokio.spawn_blocking(move || {
            if let Err(error) = auth::save_credentials(&credentials) {
                deliver(Action::NoticePosted(format!(
                    "Saving the sign-in failed: {error}"
                )));
            }
        });
    }

    fn clear_credentials(&self) {
        *self.api.write().expect("api lock") = None;
        let deliver = self.delivery();
        self.tokio.spawn_blocking(move || {
            if let Err(error) = auth::delete_credentials() {
                deliver(Action::NoticePosted(format!("Signing out failed: {error}")));
            }
        });
    }

    /// Reads the cached playlist list and liked songs, and delivers
    /// them together even when one or both are a miss. The reducer
    /// decides what a miss means for each slot.
    fn load_library_cache(&self) {
        let deliver = self.delivery();
        self.tokio.spawn_blocking(move || {
            let playlists = library_cache::load_playlists();
            let liked = library_cache::load_liked();
            deliver(Action::LibraryCacheLoaded { playlists, liked });
        });
    }

    /// Reads one playlist's cached track list. Delivers nothing on a
    /// miss, since a loading page already shows a spinner.
    fn load_playlist_tracks_cache(&self, id: PlaylistId) {
        let deliver = self.delivery();
        self.tokio.spawn_blocking(move || {
            if let Some(tracks) = library_cache::load_playlist_tracks(&id) {
                deliver(Action::PlaylistTracksCacheLoaded(id, tracks));
            }
        });
    }

    /// Writes one piece of fresh network data to the library cache.
    /// The reducer has already applied it to the state by the time
    /// this effect runs.
    fn save_library_cache(&self, write: LibraryCacheWrite) {
        self.tokio.spawn_blocking(move || match write {
            LibraryCacheWrite::Playlists(playlists) => library_cache::save_playlists(&playlists),
            LibraryCacheWrite::Liked(tracks) => library_cache::save_liked(&tracks),
            LibraryCacheWrite::PlaylistTracks(id, tracks) => {
                library_cache::save_playlist_tracks(&id, &tracks)
            }
        });
    }

    fn clear_library_cache(&self) {
        self.tokio.spawn_blocking(library_cache::clear);
    }

    /// One closure that sends an action to the app and wakes the UI.
    fn delivery(&self) -> impl Fn(Action) + Send + 'static {
        delivery(self.actions.clone(), self.request_repaint.clone())
    }
}

/// The built-in skin for `None`, or the named skin from the skins
/// folder. A skin that has gone missing since it was listed, or that
/// fails to decode, is reported as an error rather than silently
/// falling back, so the listener learns their skin is gone.
fn load_named_skin(name: Option<String>) -> Result<Arc<Skin>, String> {
    let Some(name) = name else {
        return Ok(Skin::builtin());
    };
    let path = skins_dir::skin_path(&name)
        .ok_or_else(|| format!("the skin \"{name}\" is no longer in the skins folder"))?;
    Skin::load(&path).map(Arc::new).map_err(|error| error.to_string())
}

/// Session setup for the audio sources runs in the background at
/// start, so the first track does not wait for it.
fn warm_up_sources(tokio: &tokio::runtime::Runtime, resolvers: Arc<ResolverChain>) {
    tokio.spawn(async move {
        resolvers.warm_up(&reqwest::Client::new()).await;
    });
}

fn delivery(
    actions: Sender<Action>,
    request_repaint: Arc<dyn Fn() + Send + Sync>,
) -> impl Fn(Action) + Send + 'static {
    move |action| {
        if actions.send(action).is_ok() {
            request_repaint();
        }
    }
}

async fn execute_api_request(
    slot: &ApiSlot,
    request: ApiRequest,
    deliver: &(impl Fn(Action) + Send),
) {
    match request {
        ApiRequest::VerifyAuth(method) => deliver(sign_in(slot, &method).await),
        ApiRequest::StartOAuth {
            client_id,
            client_secret,
        } => {
            run_oauth_flow(slot, client_id, client_secret, deliver).await;
        }
        other => {
            let signed_in = slot.read().expect("api lock").clone();
            match signed_in {
                Some(api) => execute_signed_in(&api, other, deliver).await,
                None => deliver(request_failure(other, "not signed in".to_string())),
            }
        }
    }
}

async fn sign_in(slot: &ApiSlot, method: &crate::core::effect::AuthMethod) -> Action {
    match Api::sign_in(method).await {
        Ok(api) => {
            *slot.write().expect("api lock") = Some(api);
            Action::AuthVerified(Ok(()))
        }
        Err(message) => Action::AuthVerified(Err(message)),
    }
}

/// How long and how often the flow polls Google while the user
/// finishes the sign-in in the browser. Five seconds is the device
/// flow's standard interval.
const OAUTH_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);
const OAUTH_POLL_ATTEMPTS: u32 = 60;

/// The OAuth device flow: get a device code, hand the verification URL
/// to the UI, poll until the user finishes, then store the token and
/// sign in with it.
async fn run_oauth_flow(
    slot: &ApiSlot,
    client_id: String,
    client_secret: String,
    deliver: &(impl Fn(Action) + Send),
) {
    let result = oauth_token_from_device_flow(&client_id, &client_secret, deliver).await;
    let action = match result {
        Ok(token_json) => {
            if let Err(error) = auth::save_oauth_token(&token_json) {
                deliver(Action::NoticePosted(format!(
                    "Saving the sign-in failed: {error}"
                )));
            }
            sign_in(
                slot,
                &crate::core::effect::AuthMethod::OAuthToken(token_json),
            )
            .await
        }
        Err(message) => Action::AuthVerified(Err(message)),
    };
    deliver(action);
}

async fn oauth_token_from_device_flow(
    client_id: &str,
    client_secret: &str,
    deliver: &(impl Fn(Action) + Send),
) -> Result<String, String> {
    let client = ytmapi_rs::Client::new()
        .map_err(|error| format!("The HTTP client failed to build: {error}"))?;
    let (code, url) = ytmapi_rs::generate_oauth_code_and_url(&client, client_id)
        .await
        .map_err(|error| format!("The OAuth start failed: {error}. Check the client id."))?;
    deliver(Action::OAuthUrlReady(url));
    let mut last_error = String::new();
    for _ in 0..OAUTH_POLL_ATTEMPTS {
        tokio::time::sleep(OAUTH_POLL_INTERVAL).await;
        match ytmapi_rs::generate_oauth_token(&client, code.clone(), client_id, client_secret).await
        {
            Ok(token) => {
                return serde_json::to_string(&token)
                    .map_err(|error| format!("The token does not serialize: {error}"));
            }
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(format!("The sign-in did not finish in time: {last_error}"))
}

/// Runs one request against a signed-in session. `Search` and
/// `FetchPlaylists` deliver a single result action. `FetchLiked` and
/// `FetchPlaylistTracks` stream: one page action per page, the last
/// one carrying `finished: true`, or a failure action on a mid-stream
/// error.
async fn execute_signed_in(api: &Api, request: ApiRequest, deliver: &(impl Fn(Action) + Send)) {
    match request {
        ApiRequest::VerifyAuth(_) | ApiRequest::StartOAuth { .. } => {
            unreachable!("handled before the sign-in check")
        }
        ApiRequest::Search { query } => deliver(Action::SearchLoaded(api.search(&query).await)),
        ApiRequest::FetchPlaylists => {
            deliver(Action::PlaylistsLoaded(api.library_playlists().await))
        }
        ApiRequest::FetchLiked => stream_liked(api, deliver).await,
        ApiRequest::FetchPlaylistTracks(id) => stream_playlist_tracks(api, id, deliver).await,
        ApiRequest::FetchArtist(id) => {
            let result = api.artist(&id).await;
            deliver(Action::ArtistLoaded(id, result));
        }
        ApiRequest::FetchAlbum(id) => {
            let result = api.album(&id).await;
            deliver(Action::AlbumLoaded(id, result));
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
        ApiRequest::CreatePlaylist(title) => {
            deliver(Action::PlaylistCreated(api.create_playlist(&title).await));
        }
    }
}

/// Streams the liked-songs list, delivering one `LikedPageLoaded` per
/// page. A mid-stream error becomes a `LikedLoaded` failure, after the
/// pages already delivered.
async fn stream_liked(api: &Api, deliver: &(impl Fn(Action) + Send)) {
    let result = api
        .liked_songs(|tracks, finished| deliver(Action::LikedPageLoaded { tracks, finished }))
        .await;
    if let Err(message) = result {
        deliver(Action::LikedLoaded(Err(message)));
    }
}

/// Streams one playlist's tracks, the same way as `stream_liked`.
async fn stream_playlist_tracks(api: &Api, id: PlaylistId, deliver: &(impl Fn(Action) + Send)) {
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
fn request_failure(request: ApiRequest, message: String) -> Action {
    match request {
        ApiRequest::VerifyAuth(_) | ApiRequest::StartOAuth { .. } => {
            Action::AuthVerified(Err(message))
        }
        ApiRequest::Search { .. } => Action::SearchLoaded(Err(message)),
        ApiRequest::FetchPlaylists => Action::PlaylistsLoaded(Err(message)),
        ApiRequest::FetchLiked => Action::LikedLoaded(Err(message)),
        ApiRequest::FetchPlaylistTracks(id) => Action::PlaylistTracksLoaded(id, Err(message)),
        ApiRequest::FetchArtist(id) => Action::ArtistLoaded(id, Err(message)),
        ApiRequest::FetchAlbum(id) => Action::AlbumLoaded(id, Err(message)),
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
        ApiRequest::CreatePlaylist(_) => Action::PlaylistCreated(Err(message)),
    }
}
