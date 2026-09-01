//! The player engine: one thread that owns the audio device.
//!
//! Commands come in over a channel. A track's audio arrives as a
//! growing `AudioBuffer`: the download runs on the tokio runtime and
//! keeps filling the buffer after playback starts, so a track plays
//! as soon as its header and a few seconds of audio exist. A decoder
//! thread, one per playing track, turns the buffer into samples; see
//! `source.rs`. A generation number makes every message from an old
//! track a no-op on arrival, so a fast skip never lets a stale track
//! start playing.
//!
//! A small cache holds `AudioBuffer` handles for up to four tracks,
//! complete or still filling: the playing track, the queue's next
//! track, and headroom for a hover prefetch, at about 4 MB a buffer.
//! A Prefetch command warms it for a track that may play soon, at
//! lower priority than the active track. A later Load for the same
//! track then finds its buffer already in the cache and streams from
//! it at once. At most two prefetch downloads run at a time, so a
//! pointer sweep across many rows never starts a flood of yt-dlp
//! processes.

pub mod source;

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use crate::core::action::{Action, PlayerEvent};
use crate::core::effect::PlayerCommand;
use crate::core::model::Track;
use crate::stream::{AudioBuffer, BufferStatus, ResolverChain, disk_cache};
use source::{DecoderHandle, PositionHandle, ReadyInfo};

const TICK: Duration = Duration::from_millis(250);

/// The most prefetch downloads that may run at once. Caps the yt-dlp
/// processes a pointer sweep across many rows can start; the
/// queue-next prefetch and the hover prefetch share this budget.
const MAX_PREFETCH_IN_FLIGHT: usize = 2;

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
    /// The buffer for an active load, from the cache or a fresh
    /// fetch. Carries the load's generation, so a buffer for a track
    /// the user has since left plays nothing.
    BufferReady {
        generation: u64,
        video_id: String,
        buffer: AudioBuffer,
    },
    /// The buffer for a background prefetch. Always applied: a
    /// prefetch only ever adds a cache entry, so it is harmless even
    /// for a track the user has since left.
    PrefetchBufferReady {
        video_id: String,
        buffer: AudioBuffer,
    },
    /// The decoder thread's one report for an active load: the
    /// stream's shape, or the reason it never opened. Carries the
    /// load's generation, and whether its buffer was already
    /// complete, so a decode failure on cached bytes can evict them.
    SourceReady {
        generation: u64,
        video_id: String,
        was_complete: bool,
        result: Result<ReadyInfo, String>,
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

/// Audio buffers for up to two tracks, complete or still filling.
/// Cheap to hold and to clone: an `AudioBuffer` is a handle onto
/// shared bytes, never a copy of a whole song.
struct PrefetchCache {
    /// Most recently used entry first.
    entries: Vec<(String, AudioBuffer)>,
}

impl PrefetchCache {
    const CAPACITY: usize = 4;

    fn new() -> Self {
        Self { entries: Vec::new() }
    }

    fn contains(&self, video_id: &str) -> bool {
        self.entries.iter().any(|(id, _)| id == video_id)
    }

    /// The cached buffer for `video_id`, if present. Marks the entry
    /// as most recently used.
    fn get(&mut self, video_id: &str) -> Option<AudioBuffer> {
        let index = self.entries.iter().position(|(id, _)| id == video_id)?;
        let entry = self.entries.remove(index);
        if matches!(entry.1.status(), BufferStatus::Failed(_)) {
            return None;
        }
        let buffer = entry.1.clone();
        self.entries.insert(0, entry);
        Some(buffer)
    }

    /// Drops the entry for `video_id`, so a buffer that failed to
    /// decode never serves a retry.
    fn remove(&mut self, video_id: &str) {
        self.entries.retain(|(id, _)| id != video_id);
    }

    /// Stores `buffer` under `video_id` as the most recently used
    /// entry. Evicts the least recently used entry when the cache is
    /// full.
    fn insert(&mut self, video_id: String, buffer: AudioBuffer) {
        self.entries.retain(|(id, _)| id != &video_id);
        self.entries.insert(0, (video_id, buffer));
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
    /// The video id this engine is loading or playing. Cleared on
    /// `stop`. Lets a `Prefetch` request recognize the active track
    /// and skip it.
    current_track_id: Option<String>,
    /// True from `load` until the active track's decoder reports
    /// `Ready` or fails. A `Prefetch` request that arrives during
    /// this window waits in `pending_prefetch`, so it never competes
    /// with the active track for bandwidth.
    load_pending: bool,
    prefetch_cache: PrefetchCache,
    /// A prefetch request that arrived while the active track was
    /// still loading. Starts once that load settles.
    pending_prefetch: Option<Track>,
    /// How many prefetch downloads have started but not yet delivered
    /// `PrefetchBufferReady`. A new prefetch request drops when this
    /// reaches `MAX_PREFETCH_IN_FLIGHT`.
    prefetch_in_flight: usize,
    /// The decoder handle for the active load, held between
    /// `spawn_decoder` and its `Ready` report.
    pending_source: Option<DecoderHandle>,
    /// The position of the playing track, from real decoded samples.
    position: Option<PositionHandle>,
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
            current_track_id: None,
            load_pending: false,
            prefetch_cache: PrefetchCache::new(),
            pending_prefetch: None,
            prefetch_in_flight: 0,
            pending_source: None,
            position: None,
        }
    }

    fn run(mut self, receiver: Receiver<PlayerMsg>) {
        loop {
            match receiver.recv_timeout(TICK) {
                Ok(PlayerMsg::Command(command)) => self.apply_command(command),
                Ok(PlayerMsg::BufferReady {
                    generation,
                    video_id,
                    buffer,
                }) => self.apply_buffer_ready(generation, video_id, buffer),
                Ok(PlayerMsg::PrefetchBufferReady { video_id, buffer }) => {
                    self.prefetch_in_flight = self.prefetch_in_flight.saturating_sub(1);
                    self.prefetch_cache.insert(video_id, buffer);
                }
                Ok(PlayerMsg::SourceReady {
                    generation,
                    video_id,
                    was_complete,
                    result,
                }) => self.apply_source_ready(generation, video_id, was_complete, result),
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

    /// Loads `track` for active playback. A buffer already in the
    /// cache, complete or still filling, streams at once: an earlier
    /// prefetch of the same track is adopted for free.
    fn load(&mut self, track: Track) {
        self.stop();
        let video_id = track.id.0;
        self.current_track_id = Some(video_id.clone());
        self.load_pending = true;
        match self.prefetch_cache.get(&video_id) {
            Some(buffer) => self.start_decoder(video_id, buffer),
            None => self.fetch_for_load(video_id),
        }
    }

    /// Fetches `video_id`'s buffer off the engine thread, since
    /// `disk_cache::fetch_audio` is async, then reports it back as a
    /// message so `apply_buffer_ready` can act on it on this thread.
    fn fetch_for_load(&mut self, video_id: String) {
        let generation = self.generation;
        let resolvers = self.resolvers.clone();
        let http = self.http.clone();
        let results = self.self_sender.clone();
        self.tokio.spawn(async move {
            let buffer = disk_cache::fetch_audio(&resolvers, &http, &video_id).await;
            let _ = results.send(PlayerMsg::BufferReady {
                generation,
                video_id,
                buffer,
            });
        });
    }

    /// Caches the buffer a load fetched, then starts its decoder when
    /// the load is still current. A stale load's buffer still enters
    /// the cache: a later Load or Prefetch for the same track can use
    /// it.
    fn apply_buffer_ready(&mut self, generation: u64, video_id: String, buffer: AudioBuffer) {
        self.prefetch_cache.insert(video_id.clone(), buffer.clone());
        if generation == self.generation {
            self.start_decoder(video_id, buffer);
        }
    }

    /// Starts a decoder thread over `buffer` and remembers its handle
    /// until the thread's `Ready` report arrives.
    fn start_decoder(&mut self, video_id: String, buffer: AudioBuffer) {
        let generation = self.generation;
        let was_complete = matches!(buffer.status(), BufferStatus::Complete);
        let results = self.self_sender.clone();
        let report_id = video_id.clone();
        let handle = source::spawn_decoder(buffer, move |result| {
            let _ = results.send(PlayerMsg::SourceReady {
                generation,
                video_id: report_id,
                was_complete,
                result,
            });
        });
        self.pending_source = Some(handle);
    }

    /// Applies a decoder thread's report for the active load. A
    /// report from an old generation is dropped without touching
    /// state that may already belong to a newer load: `stop` already
    /// bumped the generation for every message still in flight.
    fn apply_source_ready(
        &mut self,
        generation: u64,
        video_id: String,
        was_complete: bool,
        result: Result<ReadyInfo, String>,
    ) {
        if generation != self.generation {
            return;
        }
        let Some(handle) = self.pending_source.take() else {
            return;
        };
        match result.and_then(|ready| self.start_playback(handle, ready)) {
            Ok(duration) => {
                self.track_loaded = true;
                (self.deliver)(Action::Player(PlayerEvent::TrackStarted { duration }));
            }
            Err(message) => {
                if was_complete {
                    disk_cache::remove(&video_id);
                }
                self.prefetch_cache.remove(&video_id);
                (self.deliver)(Action::Player(PlayerEvent::Failed(message)));
            }
        }
        self.resolve_load_pending();
    }

    /// Appends the decoded source to the audio device and starts
    /// playback.
    fn start_playback(
        &mut self,
        handle: DecoderHandle,
        ready: ReadyInfo,
    ) -> Result<Option<Duration>, String> {
        let (source, position) = handle.into_source(ready);
        self.position = Some(position);
        let volume = self.volume;
        let output = self.output()?;
        output.player.stop();
        output.player.set_volume(volume);
        output.player.append(source);
        output.player.play();
        Ok(ready.total_duration)
    }

    /// Marks the active load settled, then starts a prefetch request
    /// that arrived while it was still loading.
    fn resolve_load_pending(&mut self) {
        self.load_pending = false;
        if let Some(track) = self.pending_prefetch.take() {
            self.prefetch(track);
        }
    }

    /// Warms the cache for `track` in the background. A no-op when
    /// the track is already cached, is the active track, or would
    /// push the in-flight prefetch count past `MAX_PREFETCH_IN_FLIGHT`.
    /// Waits for the active load to settle first, so the active track
    /// never competes for bandwidth.
    fn prefetch(&mut self, track: Track) {
        let video_id = track.id.0.clone();
        if self.prefetch_cache.contains(&video_id) {
            return;
        }
        if self.current_track_id.as_deref() == Some(video_id.as_str()) {
            return;
        }
        if self.load_pending {
            self.pending_prefetch = Some(track);
            return;
        }
        if self.prefetch_in_flight >= MAX_PREFETCH_IN_FLIGHT {
            log::debug!("dropping prefetch for {video_id}: {MAX_PREFETCH_IN_FLIGHT} already in flight");
            return;
        }
        self.start_prefetch(video_id);
    }

    fn start_prefetch(&mut self, video_id: String) {
        self.prefetch_in_flight += 1;
        let resolvers = self.resolvers.clone();
        let http = self.http.clone();
        let results = self.self_sender.clone();
        self.tokio.spawn(async move {
            let buffer = disk_cache::fetch_audio(&resolvers, &http, &video_id).await;
            let _ = results.send(PlayerMsg::PrefetchBufferReady { video_id, buffer });
        });
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
    /// prefetch and the active load's decoder handle both belong to
    /// the old queue position, so both clear here. The reducer
    /// prefetches again after the next start.
    fn stop(&mut self) {
        self.generation += 1;
        self.track_loaded = false;
        self.current_track_id = None;
        self.load_pending = false;
        self.pending_prefetch = None;
        self.pending_source = None;
        self.position = None;
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
        if !output.player.is_paused()
            && let Some(position) = &self.position
        {
            (self.deliver)(Action::Player(PlayerEvent::PositionChanged(position.position())));
        }
    }
}
