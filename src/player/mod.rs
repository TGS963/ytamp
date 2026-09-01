//! The player engine: one thread that owns the audio device.
//!
//! Commands come in over a channel. Track data downloads on the tokio
//! runtime and comes back as bytes with a generation number, so a
//! stale download can never interrupt the track the user chose later.
//! The engine polls its own state a few times per second to report
//! the position and the end of a track.
//!
//! A small cache holds fully downloaded bytes for up to two tracks.
//! A Prefetch command warms it for the track that plays next, at
//! lower priority than the active download. A later Load for the
//! same track then skips the download and plays at once.

use std::io::Cursor;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use bytes::Bytes;
use rodio::source::Source;

use crate::core::action::{Action, PlayerEvent};
use crate::core::effect::PlayerCommand;
use crate::core::model::Track;
use crate::stream::{ResolverChain, disk_cache};

const TICK: Duration = Duration::from_millis(250);

pub struct PlayerHandle {
    sender: Sender<PlayerMsg>,
}

impl PlayerHandle {
    pub fn send(&self, command: PlayerCommand) {
        let _ = self.sender.send(PlayerMsg::Command(command));
    }
}

enum PlayerMsg {
    Command(PlayerCommand),
    Loaded {
        generation: u64,
        video_id: String,
        result: Result<Bytes, String>,
    },
    Prefetched {
        generation: u64,
        video_id: String,
        result: Result<Bytes, String>,
    },
}

/// Starts the engine thread and returns its command handle.
pub fn spawn(
    deliver: impl Fn(Action) + Send + 'static,
    tokio: tokio::runtime::Handle,
    resolvers: Arc<ResolverChain>,
) -> PlayerHandle {
    let (sender, receiver) = channel();
    let engine_sender = sender.clone();
    std::thread::Builder::new()
        .name("player".to_string())
        .spawn(move || {
            Engine::new(Box::new(deliver), tokio, resolvers, engine_sender).run(receiver);
        })
        .expect("the player thread failed to start");
    PlayerHandle { sender }
}

/// Fully downloaded track bytes, keyed by video id. Holds at most
/// `CAPACITY` entries and drops the least recently used one first.
/// `Bytes` makes every get a cheap reference-count bump, never a
/// copy of a whole song.
struct PrefetchCache {
    /// Most recently used entry first.
    entries: Vec<(String, Bytes)>,
}

impl PrefetchCache {
    const CAPACITY: usize = 2;

    fn new() -> Self {
        Self { entries: Vec::new() }
    }

    fn contains(&self, video_id: &str) -> bool {
        self.entries.iter().any(|(id, _)| id == video_id)
    }

    /// The cached bytes for `video_id`, if present. Marks the entry as
    /// most recently used.
    fn get(&mut self, video_id: &str) -> Option<Bytes> {
        let index = self.entries.iter().position(|(id, _)| id == video_id)?;
        let entry = self.entries.remove(index);
        let bytes = entry.1.clone();
        self.entries.insert(0, entry);
        Some(bytes)
    }

    /// Stores `bytes` under `video_id` as the most recently used entry.
    /// Evicts the least recently used entry when the cache is full.
    fn insert(&mut self, video_id: String, bytes: Bytes) {
        self.entries.retain(|(id, _)| id != &video_id);
        self.entries.insert(0, (video_id, bytes));
        self.entries.truncate(Self::CAPACITY);
    }
}

struct Engine {
    deliver: Box<dyn Fn(Action)>,
    tokio: tokio::runtime::Handle,
    resolvers: Arc<ResolverChain>,
    self_sender: Sender<PlayerMsg>,
    http: reqwest::Client,
    output: Option<AudioOutput>,
    generation: u64,
    volume: f32,
    track_loaded: bool,
    /// True between the start of an active download and its result.
    /// A prefetch waits for this to clear before it starts, so it
    /// never competes with the active download for bandwidth.
    active_loading: bool,
    prefetch_cache: PrefetchCache,
    /// Discards a prefetch result superseded by a newer prefetch
    /// request, the same way `generation` discards a stale active
    /// download.
    prefetch_generation: u64,
    prefetch_inflight_id: Option<String>,
    /// A prefetch request that arrived while the active download was
    /// still in flight. Starts once that download finishes.
    pending_prefetch: Option<Track>,
    /// An in-flight prefetch the user skipped onto. Its result plays
    /// as the active track, so the track never downloads twice.
    promoted_load: Option<String>,
}

struct AudioOutput {
    /// Holds the device open. The player plays into its mixer.
    _device: rodio::MixerDeviceSink,
    player: rodio::Player,
}

impl Engine {
    fn new(
        deliver: Box<dyn Fn(Action)>,
        tokio: tokio::runtime::Handle,
        resolvers: Arc<ResolverChain>,
        self_sender: Sender<PlayerMsg>,
    ) -> Self {
        Self {
            deliver,
            tokio,
            resolvers,
            self_sender,
            http: reqwest::Client::new(),
            output: None,
            generation: 0,
            volume: 1.0,
            track_loaded: false,
            active_loading: false,
            prefetch_cache: PrefetchCache::new(),
            prefetch_generation: 0,
            prefetch_inflight_id: None,
            pending_prefetch: None,
            promoted_load: None,
        }
    }

    fn run(mut self, receiver: Receiver<PlayerMsg>) {
        loop {
            match receiver.recv_timeout(TICK) {
                Ok(PlayerMsg::Command(command)) => self.apply_command(command),
                Ok(PlayerMsg::Loaded {
                    generation,
                    video_id,
                    result,
                }) => self.apply_load(generation, video_id, result),
                Ok(PlayerMsg::Prefetched {
                    generation,
                    video_id,
                    result,
                }) => self.apply_prefetched(generation, video_id, result),
                Err(RecvTimeoutError::Timeout) => self.tick(),
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn apply_command(&mut self, command: PlayerCommand) {
        match command {
            PlayerCommand::Load(track) => self.load(track),
            PlayerCommand::Prefetch(track) => self.prefetch(track),
            PlayerCommand::Pause => self.with_player(|player| player.pause()),
            PlayerCommand::Resume => self.with_player(|player| player.play()),
            PlayerCommand::Seek(position) => self.seek(position),
            PlayerCommand::SetVolume(volume) => self.set_volume(volume),
            PlayerCommand::Stop => self.stop(),
        }
    }

    /// Loads `track` for active playback. Cached bytes from an earlier
    /// prefetch play at once. An in-flight prefetch of the same track
    /// becomes the active load. Otherwise the track downloads now.
    fn load(&mut self, track: Track) {
        if let Some(bytes) = self.prefetch_cache.get(&track.id.0) {
            self.stop();
            self.finish_load(track.id.0, Ok(bytes));
            return;
        }
        if self.prefetch_inflight_id.as_deref() == Some(track.id.0.as_str()) {
            self.adopt_prefetch(track.id.0);
            return;
        }
        self.start_download(track.id.0);
    }

    /// Marks the in-flight prefetch for `video_id` as the active load.
    /// Its result then plays at once, and the same bytes never
    /// download a second time.
    fn adopt_prefetch(&mut self, video_id: String) {
        self.stop();
        self.active_loading = true;
        self.promoted_load = Some(video_id);
    }

    /// Downloads happen off this thread. The generation number makes
    /// every download older than the newest Load a no-op on arrival.
    fn start_download(&mut self, video_id: String) {
        self.stop();
        self.active_loading = true;
        let generation = self.generation;
        let resolvers = self.resolvers.clone();
        let http = self.http.clone();
        let results = self.self_sender.clone();
        self.tokio.spawn(async move {
            let result = disk_cache::fetch_audio(&resolvers, &http, &video_id).await;
            let _ = results.send(PlayerMsg::Loaded {
                generation,
                video_id,
                result,
            });
        });
    }

    fn apply_load(&mut self, generation: u64, video_id: String, result: Result<Bytes, String>) {
        if generation != self.generation {
            return;
        }
        self.finish_load(video_id, result);
    }

    /// Applies a decoded result for `video_id`: plays it on success, or
    /// reports the failure. A success also enters the cache, so a
    /// repeat-one replay of the same track skips its own download.
    /// Either way, a prefetch that waited for this download now starts.
    fn finish_load(&mut self, video_id: String, result: Result<Bytes, String>) {
        self.active_loading = false;
        match result.and_then(|bytes| self.play_and_cache(video_id, bytes)) {
            Ok(duration) => {
                self.track_loaded = true;
                (self.deliver)(Action::Player(PlayerEvent::TrackStarted { duration }));
            }
            Err(message) => (self.deliver)(Action::Player(PlayerEvent::Failed(message))),
        }
        self.start_pending_prefetch();
    }

    /// Bytes that do not decode also leave the disk cache, so a
    /// poisoned entry cannot fail on every later play.
    fn play_and_cache(
        &mut self,
        video_id: String,
        bytes: Bytes,
    ) -> Result<Option<Duration>, String> {
        let source = decode(bytes.clone()).inspect_err(|_| disk_cache::remove(&video_id))?;
        let duration = self.play_source(source)?;
        self.prefetch_cache.insert(video_id, bytes);
        Ok(duration)
    }

    fn play_source(
        &mut self,
        source: rodio::Decoder<Cursor<Bytes>>,
    ) -> Result<Option<Duration>, String> {
        let duration = source.total_duration();
        let volume = self.volume;
        let output = self.output()?;
        output.player.stop();
        output.player.set_volume(volume);
        output.player.append(source);
        output.player.play();
        Ok(duration)
    }

    /// Warms the cache for `track` in the background. A no-op when the
    /// track is already cached or an identical prefetch is already in
    /// flight. Waits for the active download to finish first, so the
    /// active track never competes for bandwidth.
    fn prefetch(&mut self, track: Track) {
        let video_id = track.id.0.clone();
        if self.prefetch_cache.contains(&video_id) {
            return;
        }
        if self.prefetch_inflight_id.as_deref() == Some(video_id.as_str()) {
            return;
        }
        if self.active_loading {
            self.pending_prefetch = Some(track);
            return;
        }
        self.start_prefetch(video_id);
    }

    fn start_prefetch(&mut self, video_id: String) {
        self.prefetch_generation += 1;
        let generation = self.prefetch_generation;
        self.prefetch_inflight_id = Some(video_id.clone());
        let resolvers = self.resolvers.clone();
        let http = self.http.clone();
        let results = self.self_sender.clone();
        self.tokio.spawn(async move {
            let result = disk_cache::fetch_audio(&resolvers, &http, &video_id).await;
            let _ = results.send(PlayerMsg::Prefetched {
                generation,
                video_id,
                result,
            });
        });
    }

    fn start_pending_prefetch(&mut self) {
        if let Some(track) = self.pending_prefetch.take() {
            self.prefetch(track);
        }
    }

    /// Applies a finished background prefetch. A stale result,
    /// superseded by a newer prefetch, is discarded. A prefetch the
    /// user skipped onto plays as the active track. Any other failure
    /// stays silent: the track downloads normally when it plays later.
    fn apply_prefetched(&mut self, generation: u64, video_id: String, result: Result<Bytes, String>) {
        if generation != self.prefetch_generation {
            return;
        }
        self.prefetch_inflight_id = None;
        if self.promoted_load.as_deref() == Some(video_id.as_str()) {
            self.promoted_load = None;
            self.finish_load(video_id, result);
            return;
        }
        match result {
            Ok(bytes) => {
                log::debug!("prefetch ready for {video_id}");
                self.prefetch_cache.insert(video_id, bytes);
            }
            Err(error) => log::debug!("prefetch failed for {video_id}: {error}"),
        }
    }

    fn output(&mut self) -> Result<&AudioOutput, String> {
        if self.output.is_none() {
            let device = rodio::DeviceSinkBuilder::open_default_sink()
                .map_err(|error| format!("No audio device: {error}"))?;
            let player = rodio::Player::connect_new(device.mixer());
            self.output = Some(AudioOutput {
                _device: device,
                player,
            });
        }
        Ok(self.output.as_ref().expect("just created"))
    }

    fn with_player(&mut self, act: impl FnOnce(&rodio::Player)) {
        if let Some(output) = &self.output {
            act(&output.player);
        }
    }

    fn seek(&mut self, position: Duration) {
        self.with_player(|player| {
            if let Err(error) = player.try_seek(position) {
                log::warn!("seek failed: {error}");
            }
        });
    }

    fn set_volume(&mut self, volume: f32) {
        self.volume = volume;
        self.with_player(|player| player.set_volume(volume));
    }

    /// Stops playback and invalidates every in-flight load. A pending
    /// or promoted prefetch belongs to the old queue position, so both
    /// clear here. The reducer prefetches again after the next start.
    fn stop(&mut self) {
        self.generation += 1;
        self.track_loaded = false;
        self.active_loading = false;
        self.pending_prefetch = None;
        self.promoted_load = None;
        self.with_player(|player| player.stop());
    }

    /// The periodic report: the position while a track plays, and the
    /// end signal exactly once when it runs out.
    fn tick(&mut self) {
        if !self.track_loaded {
            return;
        }
        let Some(output) = &self.output else {
            return;
        };
        if output.player.empty() {
            self.track_loaded = false;
            (self.deliver)(Action::Player(PlayerEvent::TrackEnded));
            return;
        }
        if !output.player.is_paused() {
            let position = output.player.get_pos();
            (self.deliver)(Action::Player(PlayerEvent::PositionChanged(position)));
        }
    }
}

fn decode(bytes: Bytes) -> Result<rodio::Decoder<Cursor<Bytes>>, String> {
    rodio::Decoder::new(Cursor::new(bytes))
        .map_err(|error| format!("The audio did not decode: {error}"))
}
