//! Executes effects and sends result actions back to the app.
//!
//! Network work runs on a tokio runtime. Every task ends by sending one
//! action and asking the UI for a repaint. Player commands are still a
//! stub until the player engine lands.

use std::sync::mpsc::Sender;
use std::sync::{Arc, RwLock};

use crate::api::Api;
use crate::auth;
use crate::core::action::Action;
use crate::core::effect::{ApiRequest, Effect};
use crate::player::PlayerHandle;
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
        let player = crate::player::spawn(
            delivery(actions.clone(), request_repaint.clone()),
            tokio.handle().clone(),
            Arc::new(ResolverChain::with_default_resolvers()),
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
            Effect::SaveCookies(cookies) => self.save_cookies(cookies),
            Effect::Player(command) => self.player.send(command),
        }
    }

    fn run_api_request(&self, request: ApiRequest) {
        let api = self.api.clone();
        let deliver = self.delivery();
        self.tokio.spawn(async move {
            deliver(execute_api_request(&api, request).await);
        });
    }

    fn save_cookies(&self, cookies: String) {
        let deliver = self.delivery();
        self.tokio.spawn_blocking(move || {
            if let Err(error) = auth::save_cookies(&cookies) {
                deliver(Action::NoticePosted(format!(
                    "Saving the sign-in failed: {error}"
                )));
            }
        });
    }

    /// One closure that sends an action to the app and wakes the UI.
    fn delivery(&self) -> impl Fn(Action) + Send + 'static {
        delivery(self.actions.clone(), self.request_repaint.clone())
    }
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

async fn execute_api_request(slot: &ApiSlot, request: ApiRequest) -> Action {
    if let ApiRequest::VerifyAuth { cookies } = request {
        return sign_in(slot, &cookies).await;
    }
    let signed_in = slot.read().expect("api lock").clone();
    match signed_in {
        Some(api) => execute_signed_in(&api, request).await,
        None => request_failure(request, "not signed in".to_string()),
    }
}

async fn sign_in(slot: &ApiSlot, cookies: &str) -> Action {
    match Api::sign_in(cookies).await {
        Ok(api) => {
            *slot.write().expect("api lock") = Some(api);
            Action::AuthVerified(Ok(()))
        }
        Err(message) => Action::AuthVerified(Err(message)),
    }
}

async fn execute_signed_in(api: &Api, request: ApiRequest) -> Action {
    match request {
        ApiRequest::VerifyAuth { .. } => unreachable!("handled before the sign-in check"),
        ApiRequest::Search { query } => Action::SearchLoaded(api.search(&query).await),
        ApiRequest::FetchPlaylists => Action::PlaylistsLoaded(api.library_playlists().await),
        ApiRequest::FetchLiked => Action::LikedLoaded(api.liked_songs().await),
        ApiRequest::FetchPlaylistTracks(id) => {
            let tracks = api.playlist_tracks(&id).await;
            Action::PlaylistTracksLoaded(id, tracks)
        }
    }
}

/// The failure action that matches what a request loads.
fn request_failure(request: ApiRequest, message: String) -> Action {
    match request {
        ApiRequest::VerifyAuth { .. } => Action::AuthVerified(Err(message)),
        ApiRequest::Search { .. } => Action::SearchLoaded(Err(message)),
        ApiRequest::FetchPlaylists => Action::PlaylistsLoaded(Err(message)),
        ApiRequest::FetchLiked => Action::LikedLoaded(Err(message)),
        ApiRequest::FetchPlaylistTracks(id) => Action::PlaylistTracksLoaded(id, Err(message)),
    }
}
