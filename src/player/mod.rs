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

pub mod equalizer;
pub mod source;

use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

use crate::core::action::{Action, PlayerEvent};
use crate::core::effect::PlayerCommand;
use crate::core::model::Track;
use crate::stream::{AudioBuffer, BufferStatus, ResolverChain, disk_cache};
use source::{DecodeFailure, DecoderHandle, DecoderInput, PositionHandle, ReadyInfo};

const TICK: Duration = Duration::from_millis(10);
const POSITION_INTERVAL: Duration = Duration::from_millis(250);

/// The most prefetch downloads that may run at once. Caps the yt-dlp
/// processes a pointer sweep across many rows can start; the
/// queue-next prefetch and the hover prefetch share this budget.
const MAX_PREFETCH_IN_FLIGHT: usize = 2;

pub struct PlayerHandle {
    sender: Sender<PlayerMsg>,
}

impl PlayerHandle {
    pub fn send(&self, generation: u64, command: PlayerCommand) {
        let _ = self.sender.send(PlayerMsg::Command {
            generation,
            command,
        });
    }
}

enum PlayerMsg {
    PreparedReady {
        token: u64,
        video_id: String,
        result: Result<ReadyInfo, String>,
    },
    Command {
        generation: u64,
        command: PlayerCommand,
    },
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
        Self {
            entries: Vec::new(),
        }
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
    report_generation: u64,
    volume: f32,
    equalizer: equalizer::Control,
    paused: bool,
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
    prefetching: std::collections::HashSet<String>,
    /// The decoder handle for the active load, held between
    /// `spawn_decoder` and its `Ready` report.
    pending_source: Option<DecoderHandle>,
    /// The position of the playing track, from real decoded samples.
    position: Option<PositionHandle>,
    active_buffer: Option<AudioBuffer>,
    active_failure: Option<DecodeFailure>,
    next_track: Option<Track>,
    prepare_generation: u64,
    preparing: Option<(DecoderHandle, DecoderInput)>,
    prepared: Option<(String, DecoderHandle, ReadyInfo, DecoderInput)>,
    last_position_report: Instant,
}

struct AudioOutput {
    /// Holds the device open. The player plays into its mixer.
    _device: Option<rodio::MixerDeviceSink>,
    player: rodio::Player,
}

fn remote_buffer(input: &DecoderInput) -> Option<AudioBuffer> {
    match input {
        DecoderInput::Remote(buffer) => Some(buffer.clone()),
        DecoderInput::LocalFile(_) => None,
    }
}

fn input_is_complete_remote(input: &DecoderInput) -> bool {
    matches!(input, DecoderInput::Remote(buffer) if matches!(buffer.status(), BufferStatus::Complete))
}

fn spawn_prepared_decoder(
    input: DecoderInput,
    report: impl FnOnce(Result<ReadyInfo, String>) + Send + 'static,
) -> DecoderHandle {
    match input {
        DecoderInput::Remote(buffer) => source::spawn_decoder(buffer, report),
        DecoderInput::LocalFile(path) => source::spawn_file_decoder(path, report),
    }
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
            report_generation: 0,
            volume: 1.0,
            equalizer: equalizer::Control::default(),
            paused: false,
            track_loaded: false,
            current_track_id: None,
            load_pending: false,
            prefetch_cache: PrefetchCache::new(),
            pending_prefetch: None,
            prefetch_in_flight: 0,
            prefetching: Default::default(),
            pending_source: None,
            position: None,
            active_buffer: None,
            active_failure: None,
            next_track: None,
            prepare_generation: 0,
            preparing: None,
            prepared: None,
            last_position_report: Instant::now(),
        }
    }

    fn run(mut self, receiver: Receiver<PlayerMsg>) {
        let mut next_tick = Instant::now() + TICK;
        loop {
            match receiver.recv_timeout(next_tick.saturating_duration_since(Instant::now())) {
                Ok(PlayerMsg::PreparedReady {
                    token,
                    video_id,
                    result,
                }) => self.apply_prepared(token, video_id, result),
                Ok(PlayerMsg::Command {
                    generation,
                    command,
                }) => {
                    self.report_generation = generation;
                    self.apply_command(command);
                }
                Ok(PlayerMsg::BufferReady {
                    generation,
                    video_id,
                    buffer,
                }) => self.apply_buffer_ready(generation, video_id, buffer),
                Ok(PlayerMsg::PrefetchBufferReady { video_id, buffer }) => {
                    self.prefetch_in_flight = self.prefetch_in_flight.saturating_sub(1);
                    self.prefetching.remove(&video_id);
                    self.prefetch_cache.insert(video_id, buffer);
                    if let Some(track) = self.next_track.clone() {
                        self.prefetch(track);
                    }
                    self.maybe_prepare();
                }
                Ok(PlayerMsg::SourceReady {
                    generation,
                    video_id,
                    was_complete,
                    result,
                }) => self.apply_source_ready(generation, video_id, was_complete, result),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if Instant::now() >= next_tick {
                self.tick();
                next_tick = Instant::now() + TICK;
            }
        }
    }

    fn report(&self, event: PlayerEvent) {
        (self.deliver)(Action::ForPlayback {
            generation: self.report_generation,
            event,
        });
    }

    fn apply_command(&mut self, command: PlayerCommand) {
        match command {
            PlayerCommand::Load(track) => self.load(track),
            PlayerCommand::Prefetch(track) => self.prefetch(track),
            PlayerCommand::PrepareNext(track) => self.prepare_next(track),
            PlayerCommand::Pause => {
                self.paused = true;
                self.with_player(|player| player.pause());
            }
            PlayerCommand::Resume => {
                self.paused = false;
                self.with_player(|player| player.play());
            }
            PlayerCommand::Seek(position) => self.seek(position),
            PlayerCommand::SetVolume(volume) => self.set_volume(volume),
            PlayerCommand::SetBalance(value) => self.equalizer.set_balance(value),
            PlayerCommand::SetEqualizer(parameters) => self.equalizer.set(parameters),
            PlayerCommand::Stop => self.stop(),
        }
    }

    /// Loads `track` for active playback. A buffer already in the
    /// cache, complete or still filling, streams at once: an earlier
    /// prefetch of the same track is adopted for free.
    fn load(&mut self, track: Track) {
        let key = track.playback_key();
        let prepared = self.prepared.take().filter(|(id, _, _, _)| *id == key);
        self.stop();
        self.current_track_id = Some(key.clone());
        self.load_pending = true;
        if let Some((_, handle, ready, input)) = prepared {
            log::debug!("using prepared decoder for {key}");
            self.active_buffer = remote_buffer(&input);
            self.pending_source = Some(handle);
            self.apply_source_ready(
                self.generation,
                key,
                input_is_complete_remote(&input),
                Ok(ready),
            );
            return;
        }
        match track.local_path() {
            Some(path) => self.start_decoder(key, DecoderInput::LocalFile(path.to_owned())),
            None => match self.prefetch_cache.get(&key) {
                Some(buffer) => self.start_decoder(key, DecoderInput::Remote(buffer)),
                None => self.fetch_for_load(key),
            },
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
            self.start_decoder(video_id, DecoderInput::Remote(buffer));
        }
    }

    /// Starts a decoder thread over `buffer` and remembers its handle
    /// until the thread's `Ready` report arrives.
    fn start_decoder(&mut self, video_id: String, input: DecoderInput) {
        self.active_buffer = remote_buffer(&input);
        let generation = self.generation;
        let results = self.self_sender.clone();
        let report_id = video_id.clone();
        let was_complete = input_is_complete_remote(&input);
        let handle = match input {
            DecoderInput::Remote(buffer) => source::spawn_decoder(buffer, move |result| {
                let _ = results.send(PlayerMsg::SourceReady {
                    generation,
                    video_id: report_id,
                    was_complete,
                    result,
                });
            }),
            DecoderInput::LocalFile(path) => source::spawn_file_decoder(path, move |result| {
                let _ = results.send(PlayerMsg::SourceReady {
                    generation,
                    video_id: report_id,
                    was_complete: false,
                    result,
                });
            }),
        };
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
        let shape = result.as_ref().ok().copied();
        match result.and_then(|ready| self.start_playback(handle, ready)) {
            Ok(duration) => {
                self.track_loaded = true;
                let (channels, sample_rate) = shape
                    .map(|ready| (ready.channels.get(), ready.sample_rate.get()))
                    .unwrap_or_default();
                self.report(PlayerEvent::TrackStarted {
                    duration,
                    channels,
                    sample_rate,
                });
            }
            Err(message) => {
                if was_complete {
                    disk_cache::remove(&video_id);
                }
                self.prefetch_cache.remove(&video_id);
                self.report(PlayerEvent::Failed(message));
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
        let failure = handle.failure();
        let (source, position) = handle.into_source(ready);
        self.position = Some(position);
        let volume = self.volume;
        let paused = self.paused;
        let source = equalizer::Equalized::new(source, self.equalizer.clone());
        let output = self.output()?;
        output.player.stop();
        output.player.set_volume(volume);
        output.player.append(source);
        if paused {
            output.player.pause();
        } else {
            output.player.play();
        }
        self.active_failure = Some(failure);
        Ok(ready.total_duration)
    }

    /// Marks the active load settled, then starts a prefetch request
    /// that arrived while it was still loading.
    fn resolve_load_pending(&mut self) {
        self.load_pending = false;
        if let Some(track) = self.pending_prefetch.take() {
            self.prefetch(track);
        }
        self.maybe_prepare();
    }

    /// Warms the cache for `track` in the background. A no-op when
    /// the track is already cached, is the active track, or would
    /// push the in-flight prefetch count past `MAX_PREFETCH_IN_FLIGHT`.
    /// Waits for the active load to settle first, so the active track
    /// never competes for bandwidth.
    fn prefetch(&mut self, track: Track) {
        let video_id = track.playback_key();
        if self.prefetch_cache.contains(&video_id) || self.prefetching.contains(&video_id) {
            return;
        }
        if self.current_track_id.as_deref() == Some(video_id.as_str()) {
            return;
        }
        if self.load_pending {
            self.pending_prefetch = Some(track);
            return;
        }
        if track.is_local() {
            self.maybe_prepare();
            return;
        }
        if self.prefetch_in_flight >= MAX_PREFETCH_IN_FLIGHT {
            log::debug!(
                "dropping prefetch for {video_id}: {MAX_PREFETCH_IN_FLIGHT} already in flight"
            );
            return;
        }
        self.start_prefetch(video_id);
    }

    fn prepare_next(&mut self, track: Option<Track>) {
        if self.next_track.as_ref().map(Track::playback_key)
            != track.as_ref().map(Track::playback_key)
        {
            self.prepare_generation = self.prepare_generation.wrapping_add(1);
            self.preparing = None;
            self.prepared = None;
        }
        self.next_track = track;
        if let Some(track) = self.next_track.clone() {
            self.prefetch(track);
        }
        self.maybe_prepare();
    }
    fn maybe_prepare(&mut self) {
        if self.load_pending || self.preparing.is_some() || self.prepared.is_some() {
            return;
        }
        let Some(track) = &self.next_track else {
            return;
        };
        let video_id = track.playback_key();
        let input = match track.local_path() {
            Some(path) => DecoderInput::LocalFile(path.to_owned()),
            None => {
                let Some(buffer) = self.prefetch_cache.get(&video_id) else {
                    return;
                };
                DecoderInput::Remote(buffer)
            }
        };
        let token = self.prepare_generation;
        let results = self.self_sender.clone();
        let handle = spawn_prepared_decoder(input.clone(), move |result| {
            let _ = results.send(PlayerMsg::PreparedReady {
                token,
                video_id,
                result,
            });
        });
        self.preparing = Some((handle, input));
    }
    fn apply_prepared(&mut self, token: u64, video_id: String, result: Result<ReadyInfo, String>) {
        if token != self.prepare_generation
            || self.next_track.as_ref().map(Track::playback_key).as_deref() != Some(&video_id)
        {
            return;
        }
        let Some((handle, input)) = self.preparing.take() else {
            return;
        };
        match result {
            Ok(ready) => self.prepared = Some((video_id, handle, ready, input)),
            Err(_) => {
                if matches!(input, DecoderInput::Remote(_)) {
                    self.prefetch_cache.remove(&video_id);
                    disk_cache::remove(&video_id);
                }
            }
        }
    }

    fn start_prefetch(&mut self, video_id: String) {
        self.prefetching.insert(video_id.clone());
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
                _device: Some(device),
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
        self.paused = false;
        self.track_loaded = false;
        self.current_track_id = None;
        self.load_pending = false;
        self.pending_prefetch = None;
        self.pending_source = None;
        self.position = None;
        self.active_buffer = None;
        self.active_failure = None;
        self.prepare_generation = self.prepare_generation.wrapping_add(1);
        self.preparing = None;
        self.prepared = None;
        self.next_track = None;
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
            log::debug!("track output drained; reporting end");
            self.track_loaded = false;
            let event = match self.active_failure.as_ref().and_then(source::take_failure) {
                Some(message) => PlayerEvent::Failed(format!("Playback interrupted: {message}")),
                None => match self.active_buffer.as_ref().map(AudioBuffer::status) {
                    Some(BufferStatus::Failed(message)) => {
                        PlayerEvent::Failed(format!("Playback interrupted: {message}"))
                    }
                    _ => PlayerEvent::TrackEnded,
                },
            };
            self.report(event);
            return;
        }
        if !output.player.is_paused()
            && self.last_position_report.elapsed() >= POSITION_INTERVAL
            && let Some(position) = &self.position
        {
            self.last_position_report = Instant::now();
            self.report(PlayerEvent::PositionChanged(position.position()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::model::MediaSource;
    use std::fs;
    use std::path::PathBuf;
    fn fixture() -> (
        tokio::runtime::Runtime,
        Engine,
        Receiver<PlayerMsg>,
        Receiver<Action>,
        rodio::mixer::MixerSource,
    ) {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let (tx, rx) = channel();
        let (reports, events) = channel();
        let mut engine = Engine::new(
            Box::new(move |event| {
                reports.send(event).unwrap();
            }),
            runtime.handle().clone(),
            Arc::new(ResolverChain::with_default_resolvers()),
            tx,
        );
        let (mixer, source) = rodio::mixer::mixer(2.try_into().unwrap(), 48000.try_into().unwrap());
        engine.output = Some(AudioOutput {
            _device: None,
            player: rodio::Player::connect_new(&mixer),
        });
        (runtime, engine, rx, events, source)
    }
    fn test_track(id: &str) -> Track {
        Track {
            source: Default::default(),
            id: crate::core::model::TrackId(id.into()),
            title: id.into(),
            artists: vec![],
            album: None,
            album_id: None,
            duration: None,
            thumbnail_url: None,
            playlist_item_id: None,
        }
    }
    fn wave() -> AudioBuffer {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36u32 + 96000).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&48000u32.to_le_bytes());
        bytes.extend_from_slice(&96000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&96000u32.to_le_bytes());
        for _ in 0..48000 {
            bytes.extend_from_slice(&4096i16.to_le_bytes());
        }
        let buffer = AudioBuffer::new(None);
        let writer = buffer.writer();
        writer.push(&bytes);
        writer.finish();
        buffer
    }
    fn local_wave() -> PathBuf {
        let path = std::env::temp_dir().join(format!("ytamp-player-{}.wav", std::process::id()));
        let bytes = wave().complete_bytes().expect("complete wave");
        fs::write(&path, bytes).expect("write local wave");
        path
    }

    fn local_track(path: PathBuf) -> Track {
        Track {
            source: MediaSource::LocalFile { path },
            ..test_track("local")
        }
    }

    fn local_fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/local-media")
            .join(name)
    }

    #[test]
    fn local_file_load_bypasses_remote_cache_and_reaches_the_shared_mixer() {
        let (_runtime, mut engine, rx, events, mut mixer) = fixture();
        let path = local_wave();
        engine.load(local_track(path.clone()));
        let PlayerMsg::SourceReady {
            generation,
            video_id,
            was_complete,
            result,
        } = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("local decoder ready")
        else {
            panic!("expected local decoder result");
        };
        assert!(!was_complete);
        assert!(engine.active_buffer.is_none());
        engine.apply_source_ready(generation, video_id, was_complete, result);
        let Action::ForPlayback {
            event:
                PlayerEvent::TrackStarted {
                    duration: Some(duration),
                    ..
                },
            ..
        } = events.try_recv().expect("track started")
        else {
            panic!("expected local track start");
        };
        assert_eq!(duration, Duration::from_secs(1));
        assert!(
            (0..2_000)
                .map(|_| mixer.next().expect("mixer sample"))
                .any(|sample| sample != 0.)
        );
        engine.stop();
        fs::remove_file(path).expect("remove local wave");
    }

    #[test]
    fn every_advertised_local_container_decodes_through_the_player() {
        for name in [
            "tone.wav",
            "tone.mp3",
            "tone.flac",
            "tone.ogg",
            "tone-alac.m4a",
            "tone.aiff",
            "tone-aac.mov",
            "tone-aac.mkv",
            "tone-vorbis.webm",
            "video-first-aac.mp4",
        ] {
            let (_runtime, mut engine, rx, events, mut mixer) = fixture();
            engine.load(local_track(local_fixture(name)));
            let PlayerMsg::SourceReady {
                generation,
                video_id,
                was_complete,
                result,
            } = rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap_or_else(|_| panic!("{name}: local decoder did not report"))
            else {
                panic!("{name}: expected local decoder result");
            };
            assert!(
                !was_complete,
                "{name}: local source used remote cache state"
            );
            engine.apply_source_ready(generation, video_id, was_complete, result);
            assert!(
                matches!(
                    events.try_recv(),
                    Ok(Action::ForPlayback {
                        event: PlayerEvent::TrackStarted { .. },
                        ..
                    })
                ),
                "{name}: player did not start"
            );
            assert!(
                (0..20_000)
                    .map(|_| mixer.next().expect("mixer sample"))
                    .any(|sample| sample != 0.),
                "{name}: player emitted no audio"
            );
            engine.stop();
        }
    }

    #[test]
    fn files_without_supported_audio_never_start_the_player() {
        for name in ["no-audio.mp4", "corrupt.mp3"] {
            let (_runtime, mut engine, rx, events, _mixer) = fixture();
            engine.load(local_track(local_fixture(name)));
            let PlayerMsg::SourceReady {
                generation,
                video_id,
                was_complete,
                result,
            } = rx
                .recv_timeout(Duration::from_secs(5))
                .unwrap_or_else(|_| panic!("{name}: local decoder did not report"))
            else {
                panic!("{name}: expected local decoder result");
            };
            engine.apply_source_ready(generation, video_id, was_complete, result);
            assert!(
                matches!(
                    events.try_recv(),
                    Ok(Action::ForPlayback {
                        event: PlayerEvent::Failed(_),
                        ..
                    })
                ),
                "{name}: player started without supported audio"
            );
            assert!(
                !engine.track_loaded,
                "{name}: player marked the track loaded"
            );
        }
    }

    #[test]
    fn prepared_decoder_handoff_outputs_audio_and_canceled_completion_is_ignored() {
        let (_runtime, mut engine, rx, events, mut mixer) = fixture();
        engine.prefetch_cache.insert("next".into(), wave());
        engine.prepare_next(Some(test_track("next")));
        let PlayerMsg::PreparedReady {
            token,
            video_id,
            result,
        } = rx.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("expected decoder ready");
        };
        engine.apply_prepared(token, video_id, result);
        assert!(engine.prepared.is_some());
        assert!(events.try_recv().is_err());
        std::thread::sleep(Duration::from_millis(5)); // allow producer to fill PCM queue
        let started = Instant::now();
        engine.load(test_track("next"));
        assert!(engine.track_loaded);
        assert!(!engine.load_pending);
        assert!(engine.prepared.is_none());
        assert!(matches!(
            events.try_recv().unwrap(),
            Action::ForPlayback {
                event: PlayerEvent::TrackStarted { .. },
                ..
            }
        ));
        let sample = (0..2000)
            .map(|_| mixer.next().unwrap())
            .find(|v| *v != 0.)
            .expect("prepared PCM reaches mixer");
        assert!((sample - 0.125).abs() < 0.001);
        eprintln!(
            "Prepared decoder -> first mixer sample: {:?}; end polling interval: {:?}",
            started.elapsed(),
            TICK
        );
        engine.stop();
        engine.prefetch_cache.insert("cancel".into(), wave());
        engine.prepare_next(Some(test_track("cancel")));
        let PlayerMsg::PreparedReady {
            token,
            video_id,
            result,
        } = rx.recv_timeout(Duration::from_secs(5)).unwrap()
        else {
            panic!("expected ready");
        };
        engine.prepare_next(None);
        engine.apply_prepared(token, video_id, result);
        assert!(engine.prepared.is_none());
        assert!(!engine.track_loaded);
    }
    #[test]
    fn failed_stream_reports_failure_with_playback_generation_exactly_once() {
        let (_runtime, mut engine, _rx, events, _source) = fixture();
        let buffer = AudioBuffer::new(None);
        buffer.writer().fail("connection lost".into());
        engine.active_buffer = Some(buffer);
        engine.track_loaded = true;
        engine.report_generation = 71;
        engine.tick();
        engine.tick();
        assert!(
            matches!(events.try_recv().unwrap(), Action::ForPlayback { generation: 71, event: PlayerEvent::Failed(message) } if message.contains("connection lost"))
        );
        assert!(events.try_recv().is_err());
    }

    #[test]
    fn a_native_decoder_terminal_error_reports_player_failure() {
        let (_runtime, mut engine, _rx, events, _source) = fixture();
        engine.active_failure = Some(Arc::new(std::sync::Mutex::new(Some(
            "file read failed".into(),
        ))));
        engine.track_loaded = true;
        engine.report_generation = 72;
        engine.tick();
        assert!(
            matches!(events.try_recv().unwrap(), Action::ForPlayback { generation: 72, event: PlayerEvent::Failed(message) } if message.contains("file read failed"))
        );
    }
    #[test]
    fn late_decoder_completion_after_stop_cannot_restart_or_report() {
        let (_runtime, mut engine, _rx, events, _source) = fixture();
        let stale = engine.generation;
        engine.stop();
        engine.apply_source_ready(stale, "old".into(), false, Err("old failure".into()));
        assert!(!engine.track_loaded);
        assert!(events.try_recv().is_err());
    }
}
