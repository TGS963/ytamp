//! A `rodio::Source` that decodes on its own thread.
//!
//! `rodio::Decoder` decodes inside the audio callback. A network
//! stall there is an underrun the listener hears. This module moves
//! the decode to its own thread: a bounded channel carries samples,
//! and a command channel carries seek/stop without waiting for decoding. The
//! `Source` side never blocks.

use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, Sender, SyncSender, TryRecvError, channel, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rodio::source::{SeekError, Source};
use rodio::{ChannelCount, SampleRate};
use symphonia::core::{
    audio::{AudioBufferRef, SampleBuffer, SignalSpec},
    codecs::{Decoder as SymphoniaCodecDecoder, DecoderOptions},
    errors::Error as SymphoniaError,
    formats::{FormatOptions, FormatReader, SeekMode, SeekTo, SeekedTo},
    io::MediaSourceStream,
    meta::MetadataOptions,
    probe::Hint,
};
use symphonia::default::{get_codecs, get_probe};

use crate::stream::AudioBuffer;
use crate::vis::AudioTap;

/// Decoded samples buffered ahead of playback. Chosen as a round
/// number close to one second of audio at common music sample rates,
/// so a short stall never starves the audio callback.
const SAMPLE_CHANNEL_CAPACITY: usize = 48_000;

/// The stream's shape, known once `rodio::Decoder::new` returns.
#[derive(Clone, Copy, Debug)]
pub struct ReadyInfo {
    pub channels: ChannelCount,
    pub sample_rate: SampleRate,
    pub total_duration: Option<Duration>,
}

/// The bytes a decoder reads. Remote audio can grow while decoding;
/// local media stays on disk and can seek immediately.
#[derive(Clone)]
pub enum DecoderInput {
    Remote(AudioBuffer),
    LocalFile(PathBuf),
}

enum DecoderCommand {
    Seek { generation: u64, position: Duration },
    Stop,
}

/// Handles to a decoder thread that has not yet reported `Ready`.
/// Turn these into a playable `StreamingSource` once a `ReadyInfo`
/// arrives, typically carried back by the closure passed to
/// `spawn_decoder`.
pub struct DecoderHandle {
    samples: Receiver<(u64, f32)>,
    commands: Sender<DecoderCommand>,
    progress: Arc<Progress>,
    failure: DecodeFailure,
}

pub type DecodeFailure = Arc<Mutex<Option<String>>>;

impl DecoderHandle {
    /// Builds the playable source from this handle and the stream's
    /// shape. Call once, after the caller's `Ready` report arrives.
    /// The position handle reports from real decoded samples, so a
    /// stall filled with silence does not move the position.
    pub fn into_source(self, ready: ReadyInfo) -> (StreamingSource, PositionHandle) {
        let progress = self.progress;
        let position = PositionHandle {
            progress: progress.clone(),
            samples_per_second: u64::from(ready.channels.get())
                * u64::from(ready.sample_rate.get()),
        };
        let source = StreamingSource {
            generation: 0,
            samples: self.samples,
            commands: self.commands,
            channels: ready.channels,
            sample_rate: ready.sample_rate,
            total_duration: ready.total_duration,
            progress,
            tap: AudioTap::shared(),
            tap_batch: TapBatch::default(),
        };
        (source, position)
    }

    pub fn failure(&self) -> DecodeFailure {
        self.failure.clone()
    }
}

pub fn take_failure(failure: &DecodeFailure) -> Option<String> {
    failure
        .lock()
        .expect("decoder failure mutex poisoned")
        .take()
}

/// Real samples played since the last seek, and where that seek
/// landed. Silence samples never count.
struct Progress {
    real_samples: AtomicU64,
    base_nanos: AtomicU64,
    /// Only end-of-stream for this generation may end the current source.
    ended_generation: AtomicU64,
}

impl Default for Progress {
    fn default() -> Self {
        Self {
            real_samples: AtomicU64::new(0),
            base_nanos: AtomicU64::new(0),
            ended_generation: AtomicU64::new(u64::MAX),
        }
    }
}

pub struct PositionHandle {
    progress: Arc<Progress>,
    samples_per_second: u64,
}

impl PositionHandle {
    pub fn position(&self) -> Duration {
        let base = Duration::from_nanos(self.progress.base_nanos.load(Ordering::Relaxed));
        let samples = self.progress.real_samples.load(Ordering::Relaxed);
        base + played_duration(samples, self.samples_per_second)
    }
}

fn played_duration(samples: u64, samples_per_second: u64) -> Duration {
    if samples_per_second == 0 {
        return Duration::ZERO;
    }
    let seconds = samples / samples_per_second;
    let remainder = samples % samples_per_second;
    Duration::from_secs(seconds)
        + Duration::from_nanos(remainder * 1_000_000_000 / samples_per_second)
}

/// Starts a decoder thread over `buffer` and returns at once, before
/// the header decodes.
///
/// The decoder thread opens `rodio::Decoder::new` on a reader over
/// `buffer`, which blocks until the header bytes arrive. It then
/// calls `report` exactly once, from the decoder thread, with the
/// stream's shape on success or a message on failure (a decode error,
/// or the buffer failing before the header arrived).
pub fn spawn_decoder(
    buffer: AudioBuffer,
    report: impl FnOnce(Result<ReadyInfo, String>) + Send + 'static,
) -> DecoderHandle {
    spawn_decoder_input(DecoderInput::Remote(buffer), report)
}

/// Starts a decoder over a local file without reading the file into memory.
pub fn spawn_file_decoder(
    path: PathBuf,
    report: impl FnOnce(Result<ReadyInfo, String>) + Send + 'static,
) -> DecoderHandle {
    spawn_decoder_input(DecoderInput::LocalFile(path), report)
}

fn spawn_decoder_input(
    input: DecoderInput,
    report: impl FnOnce(Result<ReadyInfo, String>) + Send + 'static,
) -> DecoderHandle {
    let (sample_tx, sample_rx) = sync_channel(SAMPLE_CHANNEL_CAPACITY);
    let (command_tx, command_rx) = channel();
    let progress = Arc::new(Progress::default());
    let thread_progress = progress.clone();
    let failure = Arc::new(Mutex::new(None));
    let thread_failure = failure.clone();
    thread::Builder::new()
        .name("decoder".to_string())
        .spawn(move || {
            run_decoder(
                input,
                report,
                sample_tx,
                command_rx,
                thread_progress,
                thread_failure,
            )
        })
        .expect("the decoder thread failed to start");
    DecoderHandle {
        samples: sample_rx,
        commands: command_tx,
        progress,
        failure,
    }
}

type SourceDecoder = Box<dyn Source<Item = f32> + Send>;

/// The decoder thread's whole job: open the stream, report its
/// shape, then decode until the track ends, the buffer fails, or the
/// caller lets go of the source.
fn run_decoder(
    input: DecoderInput,
    report: impl FnOnce(Result<ReadyInfo, String>),
    samples: SyncSender<(u64, f32)>,
    commands: Receiver<DecoderCommand>,
    progress: Arc<Progress>,
    failure: DecodeFailure,
) {
    let decoder = match build_decoder(&input, failure.clone()) {
        Ok(decoder) => decoder,
        Err(error) => {
            report(Err(error));
            return;
        }
    };
    report(Ok(ReadyInfo {
        channels: decoder.channels(),
        sample_rate: decoder.sample_rate(),
        total_duration: decoder.total_duration(),
    }));
    let mut state = DecodeState::new(decoder, input, failure);
    decode_until_done(&mut state, &samples, &commands, &progress);
}

/// A decoder that reads the stream front to back, with no seeking.
/// Symphonia then parses the header up to the first data atom and
/// starts, so playback begins while the download still runs. A
/// seekable decoder would parse every atom to the end of the file
/// first, and needs the byte length, which a filling buffer lacks.
fn build_decoder(input: &DecoderInput, failure: DecodeFailure) -> Result<SourceDecoder, String> {
    match input {
        DecoderInput::Remote(buffer) => build_remote_decoder(buffer),
        DecoderInput::LocalFile(path) => {
            LocalFileDecoder::open(path, failure).map(|decoder| Box::new(decoder) as SourceDecoder)
        }
    }
}

/// Wait for the first byte before probing. Symphonia treats read failures
/// during format detection as an unrecognized format, hiding source errors.
fn build_remote_decoder(buffer: &AudioBuffer) -> Result<SourceDecoder, String> {
    buffer.reader().read_exact(&mut [0u8; 1]).map_err(|error| {
        remote_decode_error(
            buffer,
            format!("The audio stream contained no readable data: {error}"),
        )
    })?;
    rodio::Decoder::builder()
        .with_data(buffer.reader())
        .with_seekable(false)
        .build()
        .map(|decoder| Box::new(decoder) as SourceDecoder)
        .map_err(|error| remote_decode_error(buffer, decode_error(error)))
}

fn remote_decode_error(buffer: &AudioBuffer, fallback: String) -> String {
    match buffer.status() {
        crate::stream::BufferStatus::Failed(message) => message,
        _ => fallback,
    }
}

/// A decoder over a complete buffer, with real seeks. Only valid once
/// the download is complete, because symphonia reads every atom of a
/// seekable stream before it starts.
fn build_seekable_buffer_decoder(buffer: &AudioBuffer, len: u64) -> Result<SourceDecoder, String> {
    rodio::Decoder::builder()
        .with_data(buffer.reader())
        .with_seekable(true)
        .with_byte_len(len)
        .build()
        .map(|decoder| Box::new(decoder) as SourceDecoder)
        .map_err(decode_error)
}

fn build_file_decoder(path: &PathBuf, failure: DecodeFailure) -> Result<SourceDecoder, String> {
    LocalFileDecoder::open(path, failure).map(|decoder| Box::new(decoder) as SourceDecoder)
}

fn decode_error(error: rodio::decoder::DecoderError) -> String {
    format!("The audio did not decode: {error}")
}

/// A seekable native decoder that selects the same supported audio track as
/// local-media import. Rodio's decoder selects the first non-null stream,
/// which can be a video track in an MP4.
struct LocalFileDecoder {
    decoder: Box<dyn SymphoniaCodecDecoder>,
    format: Box<dyn FormatReader>,
    track_id: u32,
    buffer: SampleBuffer<f32>,
    spec: SignalSpec,
    offset: usize,
    ended: bool,
    time_base: Option<symphonia::core::units::TimeBase>,
    total_duration: Option<Duration>,
    failure: DecodeFailure,
}

impl LocalFileDecoder {
    fn open(path: &PathBuf, failure: DecodeFailure) -> Result<Self, String> {
        let file = File::open(path)
            .map_err(|error| format!("Could not read {}: {error}", path.display()))?;
        let mut hint = Hint::new();
        if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
            hint.with_extension(extension);
        }
        let source = MediaSourceStream::new(Box::new(file), Default::default());
        let mut probed = get_probe()
            .format(
                &hint,
                source,
                &FormatOptions::default(),
                &MetadataOptions::default(),
            )
            .map_err(|error| format!("The audio did not decode: {error}"))?;
        let (track_id, parameters) = selected_track(&*probed.format)?;
        let mut decoder = get_codecs()
            .make(&parameters, &DecoderOptions::default())
            .map_err(|error| format!("The audio did not decode: {error}"))?;
        let (buffer, spec) = next_audio_buffer(&mut *probed.format, &mut *decoder, track_id)?
            .ok_or_else(|| "The audio did not decode any samples".to_string())?;
        let total_duration = parameters
            .time_base
            .zip(parameters.n_frames)
            .map(|(base, frames)| base.calc_time(frames).into())
            .filter(|duration: &Duration| !duration.is_zero());
        Ok(Self {
            decoder,
            format: probed.format,
            track_id,
            buffer,
            spec,
            offset: 0,
            ended: false,
            time_base: parameters.time_base,
            total_duration,
            failure,
        })
    }

    fn refill(&mut self) -> Result<bool, String> {
        match next_audio_buffer(&mut *self.format, &mut *self.decoder, self.track_id) {
            Ok(Some((buffer, spec))) => {
                self.buffer = buffer;
                self.spec = spec;
                self.offset = 0;
                Ok(true)
            }
            Ok(None) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn record_failure(&self, error: String) {
        *self.failure.lock().expect("decoder failure mutex poisoned") = Some(error);
    }

    fn discard_before_target(&mut self, seeked: SeekedTo) {
        let Some(time_base) = self.time_base else {
            return;
        };
        let time = time_base.calc_time(seeked.required_ts.saturating_sub(seeked.actual_ts));
        let samples = ((time.seconds as f64 + time.frac)
            * self.spec.rate as f64
            * self.spec.channels.count() as f64)
            .ceil() as usize;
        for _ in 0..samples {
            if self.next().is_none() {
                break;
            }
        }
    }
}

impl Iterator for LocalFileDecoder {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.ended {
            return None;
        }
        if self.offset >= self.buffer.len() {
            match self.refill() {
                Ok(true) => {}
                Ok(false) => {
                    self.ended = true;
                    return None;
                }
                Err(error) => {
                    self.record_failure(error);
                    self.ended = true;
                    return None;
                }
            }
        }
        let sample = *self.buffer.samples().get(self.offset)?;
        self.offset += 1;
        Some(sample)
    }
}

impl Source for LocalFileDecoder {
    fn current_span_len(&self) -> Option<usize> {
        Some(self.buffer.len().saturating_sub(self.offset))
    }

    fn channels(&self) -> ChannelCount {
        ChannelCount::new(
            self.spec
                .channels
                .count()
                .try_into()
                .expect("channel count fits u16"),
        )
        .expect("audio has channels")
    }

    fn sample_rate(&self) -> SampleRate {
        SampleRate::new(self.spec.rate).expect("audio has a sample rate")
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        let target = self
            .total_duration
            .map_or(position, |end| position.min(end));
        if self.total_duration.is_some_and(|end| target >= end) {
            self.ended = true;
            self.offset = self.buffer.len();
            return Ok(());
        }
        let seeked = self
            .format
            .seek(
                SeekMode::Accurate,
                SeekTo::Time {
                    time: target.into(),
                    track_id: Some(self.track_id),
                },
            )
            .map_err(local_seek_error)?;
        self.decoder.reset();
        self.ended = false;
        self.offset = self.buffer.len();
        match self.refill() {
            Ok(true) => {
                self.discard_before_target(seeked);
                Ok(())
            }
            Ok(false) => {
                self.ended = true;
                Ok(())
            }
            Err(error) => {
                self.record_failure(error.clone());
                Err(SeekError::Other(Arc::new(LocalSeekError(error))))
            }
        }
    }
}

fn selected_track(
    format: &dyn FormatReader,
) -> Result<(u32, symphonia::core::codecs::CodecParameters), String> {
    crate::local_media::select_supported_audio_track(format)
        .map(|track| (track.id, track.codec_params.clone()))
        .ok_or_else(|| "The file has no supported audio track".to_string())
}

fn next_audio_buffer(
    format: &mut dyn FormatReader,
    decoder: &mut dyn SymphoniaCodecDecoder,
    track_id: u32,
) -> Result<Option<(SampleBuffer<f32>, SignalSpec)>, String> {
    loop {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(error))
                if error.kind() == std::io::ErrorKind::UnexpectedEof =>
            {
                return Ok(None);
            }
            Err(error) => return Err(format!("The audio did not decode: {error}")),
        };
        if packet.track_id() != track_id {
            continue;
        }
        match decoder.decode(&packet) {
            Ok(decoded) if decoded.frames() > 0 => return copied_samples(decoded).map(Some),
            Ok(_) | Err(SymphoniaError::DecodeError(_)) => continue,
            Err(error) => return Err(format!("The audio did not decode: {error}")),
        }
    }
}

fn copied_samples(decoded: AudioBufferRef<'_>) -> Result<(SampleBuffer<f32>, SignalSpec), String> {
    let spec = *decoded.spec();
    let mut buffer = SampleBuffer::new(decoded.capacity() as u64, spec);
    buffer.copy_interleaved_ref(decoded);
    (!buffer.is_empty())
        .then_some((buffer, spec))
        .ok_or_else(|| "The audio did not decode any samples".to_string())
}

fn local_seek_error(error: SymphoniaError) -> SeekError {
    SeekError::Other(Arc::new(LocalSeekError(error.to_string())))
}

#[derive(Debug)]
struct LocalSeekError(String);

impl std::fmt::Display for LocalSeekError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for LocalSeekError {}

/// The decoder plus where it stands, in samples since the track
/// start, so a seek can compute how far to skip.
struct DecodeState {
    generation: u64,
    decoder: SourceDecoder,
    input: DecoderInput,
    failure: DecodeFailure,
    position_samples: u64,
    samples_per_second: u64,
}

impl DecodeState {
    fn new(decoder: SourceDecoder, input: DecoderInput, failure: DecodeFailure) -> Self {
        let samples_per_second =
            u64::from(decoder.channels().get()) * u64::from(decoder.sample_rate().get());
        Self {
            generation: 0,
            decoder,
            input,
            failure,
            position_samples: 0,
            samples_per_second,
        }
    }

    fn next_sample(&mut self) -> Option<f32> {
        let sample = self.decoder.next()?;
        self.position_samples += 1;
        Some(sample)
    }

    /// Three seek strategies. A complete buffer gets a real seek on a
    /// fresh seekable decoder. A filling buffer skips forward through
    /// decoded samples, or restarts from the front and then skips.
    fn seek(&mut self, target: Duration) -> Result<(), String> {
        let target_samples = samples_at(target, self.samples_per_second);
        match self.input.clone() {
            DecoderInput::LocalFile(path) => self.seek_in_file(&path, target, target_samples),
            DecoderInput::Remote(buffer) => self.seek_in_remote(&buffer, target, target_samples),
        }
    }

    fn seek_in_remote(
        &mut self,
        buffer: &AudioBuffer,
        target: Duration,
        target_samples: u64,
    ) -> Result<(), String> {
        match buffer.complete_bytes().map(|bytes| bytes.len() as u64) {
            Some(len) => self.seek_in_complete_buffer(buffer, target, target_samples, len),
            None if target_samples >= self.position_samples => {
                self.skip_to(target_samples);
                Ok(())
            }
            None => self.restart_and_skip_to(buffer, target_samples),
        }
    }

    fn seek_in_complete_buffer(
        &mut self,
        buffer: &AudioBuffer,
        target: Duration,
        target_samples: u64,
        len: u64,
    ) -> Result<(), String> {
        let mut decoder = build_seekable_buffer_decoder(buffer, len)?;
        decoder
            .try_seek(target)
            .map_err(|error| format!("The seek failed: {error}"))?;
        self.decoder = decoder;
        self.position_samples = target_samples;
        Ok(())
    }

    fn seek_in_file(
        &mut self,
        path: &PathBuf,
        target: Duration,
        target_samples: u64,
    ) -> Result<(), String> {
        let mut decoder = build_file_decoder(path, self.failure.clone())?;
        decoder
            .try_seek(target)
            .map_err(|error| format!("The seek failed: {error}"))?;
        self.decoder = decoder;
        self.position_samples = target_samples;
        Ok(())
    }

    fn restart_and_skip_to(
        &mut self,
        buffer: &AudioBuffer,
        target_samples: u64,
    ) -> Result<(), String> {
        self.decoder = build_decoder(&DecoderInput::Remote(buffer.clone()), self.failure.clone())?;
        self.position_samples = 0;
        self.skip_to(target_samples);
        Ok(())
    }

    fn skip_to(&mut self, target_samples: u64) {
        while self.position_samples < target_samples && self.next_sample().is_some() {}
    }
}

/// The sample index at `position`, counting every channel.
fn samples_at(position: Duration, samples_per_second: u64) -> u64 {
    position.as_nanos() as u64 / 1_000_000_000 * samples_per_second
        + (position.as_nanos() as u64 % 1_000_000_000) * samples_per_second / 1_000_000_000
}

/// Applies a pending seek, decodes one sample, and sends it, in a
/// loop. Ends on a `Stop` command, on the sample channel losing its
/// receiver, or when the decoder itself runs out of samples.
fn decode_until_done(
    state: &mut DecodeState,
    samples: &SyncSender<(u64, f32)>,
    commands: &Receiver<DecoderCommand>,
    progress: &Progress,
) {
    loop {
        match commands.try_recv() {
            Ok(DecoderCommand::Seek {
                generation,
                position,
            }) => {
                state.generation = generation;
                apply_seek(state, position);
                continue;
            }
            Ok(DecoderCommand::Stop) => return,
            Err(TryRecvError::Disconnected) => return,
            Err(TryRecvError::Empty) => {}
        }
        let Some(sample) = state.next_sample() else {
            if !wait_for_seek_after_end(state, commands, progress) {
                return;
            }
            continue;
        };
        if samples.send((state.generation, sample)).is_err() {
            return;
        }
    }
}

fn apply_seek(state: &mut DecodeState, position: Duration) {
    if let Err(error) = state.seek(position) {
        log::warn!("seek failed: {error}");
    }
}

/// The samples ran out. The thread stays alive for a seek, because
/// a seek back near the end must still work. Returns true when a seek
/// arrived and decoding continues, false when the source let go.
fn wait_for_seek_after_end(
    state: &mut DecodeState,
    commands: &Receiver<DecoderCommand>,
    progress: &Progress,
) -> bool {
    progress
        .ended_generation
        .store(state.generation, Ordering::Release);
    match commands.recv() {
        Ok(DecoderCommand::Seek {
            generation,
            position,
        }) => {
            state.generation = generation;
            apply_seek(state, position);
            true
        }
        Ok(DecoderCommand::Stop) | Err(_) => false,
    }
}

/// A `rodio::Source` fed by a decoder thread. Never blocks in
/// `next()`: a stall plays silence, so the position stays honest
/// while it waits for more bytes.
pub struct StreamingSource {
    generation: u64,
    samples: Receiver<(u64, f32)>,
    commands: Sender<DecoderCommand>,
    channels: ChannelCount,
    sample_rate: SampleRate,
    total_duration: Option<Duration>,
    progress: Arc<Progress>,
    /// Where every real sample this source plays also goes, for the
    /// Winamp skin's visualiser. See `crate::vis::AudioTap`.
    tap: Arc<AudioTap>,
    /// Mono frames not yet handed to the tap. `next` runs in the audio
    /// callback, so the tap lock is taken once per batch, not once
    /// per sample.
    tap_batch: TapBatch,
}

/// Downmixes interleaved samples to mono frames and collects them
/// into a small batch. Pure state, no lock.
#[derive(Default)]
struct TapBatch {
    frame_sum: f32,
    frame_len: u32,
    pending: Vec<f32>,
}

const TAP_BATCH_FRAMES: usize = 64;

impl TapBatch {
    /// Adds one interleaved sample. Returns the finished batch when it
    /// reaches `TAP_BATCH_FRAMES` mono frames.
    fn push(&mut self, sample: f32, channels: u32) -> Option<&[f32]> {
        self.frame_sum += sample;
        self.frame_len += 1;
        if self.frame_len < channels {
            return None;
        }
        self.pending.push(self.frame_sum / channels as f32);
        self.frame_sum = 0.0;
        self.frame_len = 0;
        (self.pending.len() >= TAP_BATCH_FRAMES).then_some(self.pending.as_slice())
    }

    fn clear(&mut self) {
        self.pending.clear();
    }
}

impl Iterator for StreamingSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        // Bound cleanup per callback. Late samples from a prior seek are
        // never played or counted toward the new position.
        for _ in 0..64 {
            // Observe completion before checking the channel: completion is
            // published after the final send, so that last sample is visible.
            let ended = self.progress.ended_generation.load(Ordering::Acquire) == self.generation;
            let received = match self.samples.try_recv() {
                Ok((generation, _)) if generation != self.generation => continue,
                Ok((_, sample)) => Ok(sample),
                Err(error) => Err(error),
            };
            if let Ok(sample) = received {
                self.progress.real_samples.fetch_add(1, Ordering::Relaxed);
                self.tap_sample(sample);
            }
            return decide_next_sample(received, ended);
        }
        Some(0.)
    }
}

impl StreamingSource {
    fn tap_sample(&mut self, sample: f32) {
        let channels = u32::from(self.channels.get()).max(1);
        if let Some(batch) = self.tap_batch.push(sample, channels) {
            self.tap.push_mono(batch);
            self.tap_batch.clear();
        }
    }
}

/// What `next()` returns, given the decoder thread's channel state. A
/// pure decision, tested without a real decoder thread: a ready
/// sample passes through, an empty channel plays silence (the decoder
/// is alive but has not decoded far enough yet), and a disconnected
/// channel ends the track.
/// What `next` returns. A ready sample passes through. An empty
/// channel plays silence while the decoder still works, and ends the
/// track once the decoder reported the end of the samples.
fn decide_next_sample(received: Result<f32, TryRecvError>, ended: bool) -> Option<f32> {
    match received {
        Ok(sample) => Some(sample),
        Err(TryRecvError::Empty) if ended => None,
        Err(TryRecvError::Empty) => Some(0.0),
        Err(TryRecvError::Disconnected) => None,
    }
}

impl Source for StreamingSource {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> ChannelCount {
        self.channels
    }

    fn sample_rate(&self) -> SampleRate {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        self.total_duration
    }

    /// Drops queued samples before sending the seek to the decoder
    /// thread. Generation tags reject late samples from the old position. A seek beyond the
    /// downloaded bytes blocks inside the decoder thread until the
    /// bytes arrive; that thread is not this one, so playback keeps
    /// its callback responsive.
    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        // Drain before publishing the seek: after publication the channel
        // may already contain valid samples for the new position.
        drain(&self.samples);
        self.generation += 1;
        let _ = self.commands.send(DecoderCommand::Seek {
            generation: self.generation,
            position,
        });
        self.tap_batch = TapBatch::default();
        self.progress
            .base_nanos
            .store(position.as_nanos() as u64, Ordering::Relaxed);
        self.progress.real_samples.store(0, Ordering::Relaxed);
        Ok(())
    }
}

fn drain(samples: &Receiver<(u64, f32)>) {
    while samples.try_recv().is_ok() {}
}

impl Drop for StreamingSource {
    /// Asks the decoder thread to stop. A best-effort nudge: the
    /// thread may already have ended, or may be blocked on a network
    /// read and only notice this later.
    fn drop(&mut self) {
        let _ = self.commands.send(DecoderCommand::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::AudioBuffer;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::mpsc::channel;
    use std::thread;
    use std::time::Duration;

    fn wave_file() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ytamp-decoder-{}-{}.wav",
            std::process::id(),
            std::thread::current()
                .name()
                .unwrap_or("test")
                .replace(':', "-")
        ));
        fs::write(&path, wave_bytes()).expect("write wave fixture");
        path
    }

    fn wave_bytes() -> Vec<u8> {
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
        for _ in 0..48_000 {
            bytes.extend_from_slice(&4096i16.to_le_bytes());
        }
        bytes
    }

    fn varying_wave_file() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "ytamp-varying-decoder-{}-{}.wav",
            std::process::id(),
            std::thread::current()
                .name()
                .unwrap_or("test")
                .replace(':', "-")
        ));
        let samples: Vec<i16> = (0..48_000).map(|sample| (sample - 24_000) as i16).collect();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36u32 + (samples.len() * 2) as u32).to_le_bytes());
        bytes.extend_from_slice(b"WAVEfmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&48_000u32.to_le_bytes());
        bytes.extend_from_slice(&96_000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&((samples.len() * 2) as u32).to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        fs::write(&path, bytes).expect("write varying wave fixture");
        path
    }

    #[test]
    fn a_local_wave_file_decodes_and_seeks_without_a_byte_buffer() {
        let path = wave_file();
        let failure = Arc::new(Mutex::new(None));
        let mut decoder = build_file_decoder(&path, failure).expect("decode local wave");
        assert!(decoder.next().is_some());
        decoder
            .try_seek(Duration::from_millis(500))
            .expect("seek local wave");
        assert!(decoder.next().is_some());
        fs::remove_file(path).expect("remove wave fixture");
    }

    #[test]
    fn a_local_seek_discards_to_the_requested_sample_and_accepts_end_of_file() {
        let path = varying_wave_file();
        let failure = Arc::new(Mutex::new(None));
        let mut decoder = build_file_decoder(&path, failure).expect("decode varying wave");
        decoder
            .try_seek(Duration::from_millis(750))
            .expect("seek to the middle");
        let expected = 12_000f32 / 32_768f32;
        assert!((decoder.next().expect("sample after seek") - expected).abs() < 0.0001);
        decoder
            .try_seek(Duration::from_secs(1))
            .expect("seek to end");
        assert!(decoder.next().is_none());
        fs::remove_file(path).expect("remove varying wave fixture");
    }

    #[test]
    fn streaming_player_seek_delivers_the_requested_local_sample() {
        let path = varying_wave_file();
        let (ready_tx, ready_rx) = channel();
        let handle = spawn_decoder_input(DecoderInput::LocalFile(path.clone()), move |ready| {
            ready_tx.send(ready).expect("report ready");
        });
        let ready = ready_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("local decoder ready")
            .expect("local decoder opens");
        let (mut player, _position) = handle.into_source(ready);
        player
            .try_seek(Duration::from_millis(750))
            .expect("seek local player");
        let expected = 12_000f32 / 32_768f32;
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let mut reached = false;
        while std::time::Instant::now() < deadline {
            match player.next() {
                Some(sample) if (sample - expected).abs() < 0.0001 => {
                    reached = true;
                    break;
                }
                Some(0.) => thread::sleep(Duration::from_millis(1)),
                Some(_) => {}
                None => panic!("the local player ended before the requested sample"),
            }
        }
        assert!(
            reached,
            "the player did not deliver the requested local sample"
        );
        fs::remove_file(path).expect("remove varying wave fixture");
    }

    #[test]
    fn samples_at_inverts_played_duration() {
        assert_eq!(samples_at(Duration::from_secs(2), 88_200), 176_400);
        assert_eq!(samples_at(Duration::from_millis(500), 96_000), 48_000);
        assert_eq!(samples_at(Duration::ZERO, 96_000), 0);
    }

    #[test]
    fn played_duration_counts_samples_of_every_channel() {
        assert_eq!(played_duration(96_000, 96_000), Duration::from_secs(1));
        assert_eq!(played_duration(48_000, 96_000), Duration::from_millis(500));
        assert_eq!(played_duration(5, 0), Duration::ZERO);
    }

    #[test]
    fn a_tap_batch_downmixes_and_fills_after_sixty_four_frames() {
        let mut batch = TapBatch::default();
        for frame in 0..TAP_BATCH_FRAMES - 1 {
            assert!(batch.push(1.0, 2).is_none(), "frame {frame} half");
            assert!(batch.push(0.0, 2).is_none(), "frame {frame} full");
        }
        assert!(batch.push(1.0, 2).is_none());
        let full = batch
            .push(0.0, 2)
            .expect("the batch fills on the last frame");
        assert_eq!(full.len(), TAP_BATCH_FRAMES);
        assert!(full.iter().all(|mono| (*mono - 0.5).abs() < 1e-6));
    }

    #[test]
    fn decide_next_sample_passes_through_a_ready_sample() {
        assert_eq!(decide_next_sample(Ok(0.5), false), Some(0.5));
    }

    #[test]
    fn decide_next_sample_plays_silence_while_the_decoder_is_alive() {
        assert_eq!(
            decide_next_sample(Err(TryRecvError::Empty), false),
            Some(0.0)
        );
    }

    #[test]
    fn decide_next_sample_ends_the_track_after_the_last_sample() {
        assert_eq!(decide_next_sample(Err(TryRecvError::Empty), true), None);
    }

    #[test]
    fn decide_next_sample_ends_the_track_when_the_decoder_is_gone() {
        assert_eq!(
            decide_next_sample(Err(TryRecvError::Disconnected), false),
            None
        );
    }

    /// A minimal RIFF/WAVE header for 16-bit PCM, plus `sample_count`
    /// samples of silence, all in one channel.
    fn wav_bytes(sample_rate: u32, sample_count: u32) -> Vec<u8> {
        let data_len = sample_count * 2;
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes()); // PCM
        bytes.extend_from_slice(&1u16.to_le_bytes()); // one channel
        bytes.extend_from_slice(&sample_rate.to_le_bytes());
        bytes.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
        bytes.extend_from_slice(&2u16.to_le_bytes()); // block align
        bytes.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for i in 0..sample_count {
            // Never zero, so a decoded sample is never mistaken for
            // the silence `next()` plays during a stall.
            let value = 1_000 + (i % 100) as i16;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    /// Waits for `spawn_decoder`'s report, off the calling thread's
    /// hot path, so the test can assert on it directly.
    fn wait_for_ready(buffer: AudioBuffer) -> (Result<ReadyInfo, String>, DecoderHandle) {
        let (report_tx, report_rx) = channel();
        let handle = spawn_decoder(buffer, move |result| {
            let _ = report_tx.send(result);
        });
        let ready = report_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("the decoder reports Ready or an error");
        (ready, handle)
    }

    #[test]
    fn source_failure_before_first_byte_reaches_the_decoder_report() {
        let buffer = AudioBuffer::new(None);
        let cause = "No source produced audio. yt-dlp: executable missing / rustypipe: HTTP 403";
        buffer.writer().fail(cause.into());
        let (ready, _handle) = wait_for_ready(buffer);
        assert_eq!(ready.unwrap_err(), cause);
    }

    #[test]
    fn source_failure_during_header_probe_keeps_the_download_error() {
        let buffer = AudioBuffer::new(None);
        let writer = buffer.writer();
        writer.push(b"R");
        writer.fail("The audio connection ended during the header".into());
        let (ready, _handle) = wait_for_ready(buffer);
        assert_eq!(
            ready.unwrap_err(),
            "The audio connection ended during the header"
        );
    }

    #[test]
    fn empty_completed_stream_reports_missing_audio_data() {
        let buffer = AudioBuffer::from_complete(bytes::Bytes::new());
        let (ready, _handle) = wait_for_ready(buffer);
        assert!(ready.unwrap_err().contains("no readable data"));
    }

    #[test]
    fn a_seek_rejects_late_old_samples_and_old_end_of_stream() {
        let (sample_tx, sample_rx) = sync_channel(8);
        let (command_tx, command_rx) = channel();
        let progress = Arc::new(Progress::default());
        let handle = DecoderHandle {
            samples: sample_rx,
            commands: command_tx,
            progress: progress.clone(),
            failure: Arc::new(Mutex::new(None)),
        };
        let (mut source, position) = handle.into_source(ReadyInfo {
            channels: 1.try_into().unwrap(),
            sample_rate: 8000.try_into().unwrap(),
            total_duration: Some(Duration::from_secs(1)),
        });
        source.try_seek(Duration::from_millis(500)).unwrap();
        assert!(matches!(
            command_rx.try_recv(),
            Ok(DecoderCommand::Seek { generation: 1, .. })
        ));
        progress.ended_generation.store(0, Ordering::Release);
        sample_tx.send((0, 0.9)).unwrap();
        assert_eq!(source.next(), Some(0.));
        sample_tx.send((1, 0.2)).unwrap();
        sample_tx.send((1, 0.3)).unwrap();
        progress.ended_generation.store(1, Ordering::Release);
        assert_eq!(source.next(), Some(0.2));
        assert_eq!(source.next(), Some(0.3));
        assert_eq!(source.next(), None);
        assert_eq!(position.position(), Duration::from_micros(500250));
    }

    #[test]
    fn a_seek_on_a_complete_buffer_lands_at_the_target() {
        let sample_count = 8_000;
        let buffer = AudioBuffer::from_complete(wav_bytes(8_000, sample_count).into());
        let (ready, handle) = wait_for_ready(buffer);
        let info = ready.expect("a well-formed WAV decodes");
        let (mut source, position) = handle.into_source(info);
        source
            .try_seek(Duration::from_millis(500))
            .expect("seek is accepted");

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut real = 0u64;
        while std::time::Instant::now() < deadline {
            match source.next() {
                Some(sample) if sample != 0.0 => real += 1,
                Some(_) => thread::sleep(Duration::from_millis(1)),
                None => break,
            }
        }
        assert_eq!(
            real, 4_000,
            "every sample after the seek must play exactly once"
        );
        assert!(position.position() >= Duration::from_millis(900));
    }

    #[test]
    fn a_complete_wav_buffer_reports_ready_then_yields_its_samples_then_ends() {
        let sample_count = 300;
        let buffer = AudioBuffer::from_complete(wav_bytes(8_000, sample_count).into());
        let (ready, handle) = wait_for_ready(buffer);
        let info = ready.expect("a well-formed WAV decodes");
        assert_eq!(info.channels.get(), 1);
        assert_eq!(info.sample_rate.get(), 8_000);

        let (mut source, _position) = handle.into_source(info);
        let mut decoded = 0;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match source.next() {
                Some(sample) if sample != 0.0 => decoded += 1,
                Some(_) => thread::sleep(Duration::from_millis(1)),
                None => break,
            }
        }
        assert_eq!(decoded, sample_count as usize);
    }

    /// Symphonia's WAV reader packs PCM into packets of up to this
    /// many frames. `Decoder::new` decodes the first packet before it
    /// returns, so a test buffer needs at least this many samples
    /// pushed before `Ready` arrives.
    const FIRST_PACKET_FRAMES: u32 = 1_152;

    #[test]
    fn a_stalled_buffer_yields_silence_then_resumes_once_the_writer_finishes() {
        let sample_count = FIRST_PACKET_FRAMES * 3;
        let full = wav_bytes(8_000, sample_count);
        let header_len = full.len() - sample_count as usize * 2;
        let first_packet_end = header_len + FIRST_PACKET_FRAMES as usize * 2;

        let buffer = AudioBuffer::new(Some(full.len() as u64));
        let writer = buffer.writer();
        writer.push(&full[..first_packet_end]);

        let (ready, handle) = wait_for_ready(buffer);
        let info = ready.expect("one full packet is enough to decode");
        let (mut source, _position) = handle.into_source(info);

        // The decoder already buffered the first packet inside
        // `Decoder::new`. `next()` may still report silence a few
        // times while that packet crosses the sample channel, so
        // poll until every one of its samples arrives.
        let mut decoded = 0;
        while decoded < FIRST_PACKET_FRAMES {
            match source.next() {
                Some(0.0) => continue,
                Some(_) => decoded += 1,
                None => panic!("the track ended before the first packet finished"),
            }
        }

        // No more sample data has arrived: the decoder thread is now
        // blocked reading the second packet, with nothing left of the
        // first to send. `next()` plays silence, reliably, since the
        // decoder cannot unblock until the writer pushes more.
        for _ in 0..5 {
            assert_eq!(source.next(), Some(0.0));
        }

        let rest = full[first_packet_end..].to_vec();
        let pusher = thread::spawn(move || {
            writer.push(&rest);
            writer.finish();
        });
        pusher.join().expect("writer thread panicked");

        let mut decoded = FIRST_PACKET_FRAMES;
        for _ in 0..1_000_000 {
            match source.next() {
                Some(0.0) => continue, // still catching up, poll again
                Some(_) => decoded += 1,
                None => break,
            }
        }
        assert_eq!(decoded, sample_count);
    }
}
