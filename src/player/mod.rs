//! The player engine: one thread that owns the audio device.
//!
//! Commands come in over a channel. Track data downloads on the tokio
//! runtime and comes back as bytes with a generation number, so a
//! stale download can never interrupt the track the user chose later.
//! The engine polls its own state a few times per second to report
//! the position and the end of a track.

use std::io::Cursor;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::Duration;

use rodio::source::Source;

use crate::core::action::{Action, PlayerEvent};
use crate::core::effect::PlayerCommand;
use crate::stream::ResolverChain;

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
        result: Result<Vec<u8>, String>,
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
        }
    }

    fn run(mut self, receiver: Receiver<PlayerMsg>) {
        loop {
            match receiver.recv_timeout(TICK) {
                Ok(PlayerMsg::Command(command)) => self.apply_command(command),
                Ok(PlayerMsg::Loaded { generation, result }) => self.apply_load(generation, result),
                Err(RecvTimeoutError::Timeout) => self.tick(),
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    fn apply_command(&mut self, command: PlayerCommand) {
        match command {
            PlayerCommand::Load(track) => self.start_download(track.id.0),
            PlayerCommand::Pause => self.with_player(|player| player.pause()),
            PlayerCommand::Resume => self.with_player(|player| player.play()),
            PlayerCommand::Seek(position) => self.seek(position),
            PlayerCommand::SetVolume(volume) => self.set_volume(volume),
            PlayerCommand::Stop => self.stop(),
        }
    }

    /// Downloads happen off this thread. The generation number makes
    /// every download older than the newest Load a no-op on arrival.
    fn start_download(&mut self, video_id: String) {
        self.stop();
        let generation = self.generation;
        let resolvers = self.resolvers.clone();
        let http = self.http.clone();
        let results = self.self_sender.clone();
        self.tokio.spawn(async move {
            let result = fetch_track(&resolvers, &http, &video_id).await;
            let _ = results.send(PlayerMsg::Loaded { generation, result });
        });
    }

    fn apply_load(&mut self, generation: u64, result: Result<Vec<u8>, String>) {
        if generation != self.generation {
            return;
        }
        match result.and_then(|bytes| self.play_bytes(bytes)) {
            Ok(duration) => {
                self.track_loaded = true;
                (self.deliver)(Action::Player(PlayerEvent::TrackStarted { duration }));
            }
            Err(message) => (self.deliver)(Action::Player(PlayerEvent::Failed(message))),
        }
    }

    fn play_bytes(&mut self, bytes: Vec<u8>) -> Result<Option<Duration>, String> {
        let source = rodio::Decoder::new(Cursor::new(bytes))
            .map_err(|error| format!("The audio did not decode: {error}"))?;
        let duration = source.total_duration();
        let volume = self.volume;
        let output = self.output()?;
        output.player.stop();
        output.player.set_volume(volume);
        output.player.append(source);
        output.player.play();
        Ok(duration)
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

    fn stop(&mut self) {
        self.generation += 1;
        self.track_loaded = false;
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

async fn fetch_track(
    resolvers: &ResolverChain,
    http: &reqwest::Client,
    video_id: &str,
) -> Result<Vec<u8>, String> {
    let stream = resolvers.resolve(video_id).await?;
    download(http, &stream.url).await
}

async fn download(http: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let response = http
        .get(url)
        .send()
        .await
        .and_then(reqwest::Response::error_for_status)
        .map_err(|error| format!("The audio download failed: {error}"))?;
    let bytes = response
        .bytes()
        .await
        .map_err(|error| format!("The audio download broke off: {error}"))?;
    Ok(bytes.to_vec())
}
