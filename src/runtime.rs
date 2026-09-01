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
            Effect::SaveCredentials(credentials) => self.save_credentials(credentials),
            Effect::ClearCredentials => self.clear_credentials(),
            Effect::Player(command) => self.player.send(command),
        }
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
            let action = match signed_in {
                Some(api) => execute_signed_in(&api, other).await,
                None => request_failure(other, "not signed in".to_string()),
            };
            deliver(action);
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

async fn execute_signed_in(api: &Api, request: ApiRequest) -> Action {
    match request {
        ApiRequest::VerifyAuth(_) | ApiRequest::StartOAuth { .. } => {
            unreachable!("handled before the sign-in check")
        }
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
        ApiRequest::VerifyAuth(_) | ApiRequest::StartOAuth { .. } => {
            Action::AuthVerified(Err(message))
        }
        ApiRequest::Search { .. } => Action::SearchLoaded(Err(message)),
        ApiRequest::FetchPlaylists => Action::PlaylistsLoaded(Err(message)),
        ApiRequest::FetchLiked => Action::LikedLoaded(Err(message)),
        ApiRequest::FetchPlaylistTracks(id) => Action::PlaylistTracksLoaded(id, Err(message)),
    }
}
