//! Executes effects and sends result actions back to the app.
//!
//! Network work runs on a tokio runtime. Every task ends by sending one
//! action and asking the UI for a repaint. Player commands run on the
//! audio engine thread. Account-scoped completions carry a generation.

use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, RwLock};

use crate::api::Api;
use crate::core::action::{Action, LibraryWrite};
use crate::core::effect::{ApiRequest, Effect, LibraryCacheWrite};
use crate::core::model::PlaylistId;
use crate::player::PlayerHandle;
use crate::stream::ResolverChain;

mod account;
mod cache;
mod local_media;
mod lyrics;
mod requests;
mod skins;

#[derive(Default)]
struct SessionSlot {
    generation: u64,
    api: Option<Api>,
}
type ApiSlot = Arc<RwLock<SessionSlot>>;

pub struct EffectRuntime {
    tokio: tokio::runtime::Runtime,
    api: ApiSlot,
    pending_api: Mutex<Vec<tokio::task::AbortHandle>>,
    actions: Sender<Action>,
    request_repaint: Arc<dyn Fn() + Send + Sync>,
    player: PlayerHandle,
    pending_lyrics: Mutex<Option<tokio::task::AbortHandle>>,
    pending_import: Mutex<Option<Arc<std::sync::atomic::AtomicBool>>>,
    playback_generation: std::sync::atomic::AtomicU64,
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
            api: Arc::new(RwLock::new(SessionSlot::default())),
            pending_api: Mutex::new(Vec::new()),
            actions,
            request_repaint,
            player,
            pending_lyrics: Mutex::new(None),
            pending_import: Mutex::new(None),
            playback_generation: std::sync::atomic::AtomicU64::new(0),
        }
    }

    /// A synchronous barrier: old disk work finishes before changing accounts;
    /// queued old work checks the generation and cannot recreate cleared files.
    pub fn set_session_generation(&self, generation: u64) {
        let mut slot = self.api.write().expect("api lock");
        if slot.generation != generation {
            slot.generation = generation;
            slot.api = None;
            for task in self.pending_api.lock().expect("task lock").drain(..) {
                task.abort();
            }
        }
    }

    fn scoped_delivery(&self) -> impl Fn(Action) + Send + 'static {
        let generation = self.api.read().expect("api lock").generation;
        let deliver = self.delivery();
        move |action| {
            deliver(Action::ForSession {
                generation,
                action: Box::new(action),
            })
        }
    }

    pub fn set_playback_generation(&self, generation: u64) {
        self.playback_generation
            .store(generation, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn run(&self, effect: Effect) {
        match effect {
            Effect::PickLocalFiles {
                generation,
                replace,
            } => self.pick_local_files(generation, replace),
            Effect::ImportLocalFiles { id, paths } => self.import_local_files(id, paths),
            Effect::CancelLocalImport => self.cancel_local_import(),
            Effect::FetchLocalLyrics {
                request_id,
                track,
                path,
            } => self.fetch_local_lyrics(request_id, track, path),
            Effect::FetchLyrics(request) => self.fetch_lyrics(request),
            Effect::Api(request) => self.run_api_request(request),
            Effect::LoadStoredAuth => self.load_stored_auth(),
            Effect::ClearCredentials => self.clear_credentials(),
            Effect::Player(command) => self.player.send(
                self.playback_generation
                    .load(std::sync::atomic::Ordering::Relaxed),
                command,
            ),
            Effect::LoadLibraryCache => self.load_library_cache(),
            Effect::LoadPlaylistTracksCache(id) => self.load_playlist_tracks_cache(id),
            Effect::SaveLibraryCache(write) => self.save_library_cache(write),
            Effect::ClearLibraryCache => self.clear_library_cache(),
            Effect::LoadSkin(name) => self.load_skin(name),
            Effect::InstallSkin(path) => self.install_skin(path),
            Effect::RefreshSkinList => self.refresh_skin_list(),
        }
    }

    /// One closure that sends an action to the app and wakes the UI.
    fn delivery(&self) -> impl Fn(Action) + Send + 'static {
        delivery(self.actions.clone(), self.request_repaint.clone())
    }
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
