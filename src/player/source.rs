//! A `rodio::Source` that decodes on its own thread.
//!
//! `rodio::Decoder` decodes inside the audio callback. A network
//! stall there is an underrun the listener hears. This module moves
//! the decode to its own thread, behind two bounded channels: samples
//! flow one way, seek and stop commands flow the other way. The
//! `Source` side never blocks.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, sync_channel};
use std::thread;
use std::time::Duration;

use rodio::source::{SeekError, Source};
use rodio::{ChannelCount, SampleRate};

use crate::stream::AudioBuffer;

/// Decoded samples buffered ahead of playback. Chosen as a round
/// number close to one second of audio at common music sample rates,
/// so a short stall never starves the audio callback.
const SAMPLE_CHANNEL_CAPACITY: usize = 48_000;

/// Pending seek and stop commands. Small: the decoder thread checks
/// this channel between every sample, so a command never waits long.
const COMMAND_CHANNEL_CAPACITY: usize = 4;

/// The stream's shape, known once `rodio::Decoder::new` returns.
#[derive(Clone, Copy, Debug)]
pub struct ReadyInfo {
    pub channels: ChannelCount,
    pub sample_rate: SampleRate,
    pub total_duration: Option<Duration>,
}

enum DecoderCommand {
    Seek(Duration),
    Stop,
}

/// Handles to a decoder thread that has not yet reported `Ready`.
/// Turn these into a playable `StreamingSource` once a `ReadyInfo`
/// arrives, typically carried back by the closure passed to
/// `spawn_decoder`.
pub struct DecoderHandle {
    samples: Receiver<f32>,
    commands: SyncSender<DecoderCommand>,
}

impl DecoderHandle {
    /// Builds the playable source from this handle and the stream's
    /// shape. Call once, after the caller's `Ready` report arrives.
    /// The position handle reports from real decoded samples, so a
    /// stall filled with silence does not move the position.
    pub fn into_source(self, ready: ReadyInfo) -> (StreamingSource, PositionHandle) {
        let progress = Arc::new(Progress::default());
        let position = PositionHandle {
            progress: progress.clone(),
            samples_per_second: u64::from(ready.channels.get())
                * u64::from(ready.sample_rate.get()),
        };
        let source = StreamingSource {
            samples: self.samples,
            commands: self.commands,
            channels: ready.channels,
            sample_rate: ready.sample_rate,
            total_duration: ready.total_duration,
            progress,
        };
        (source, position)
    }
}

/// Real samples played since the last seek, and where that seek
/// landed. Silence samples never count.
#[derive(Default)]
struct Progress {
    real_samples: AtomicU64,
    base_nanos: AtomicU64,
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
    let (sample_tx, sample_rx) = sync_channel(SAMPLE_CHANNEL_CAPACITY);
    let (command_tx, command_rx) = sync_channel(COMMAND_CHANNEL_CAPACITY);
    thread::Builder::new()
        .name("decoder".to_string())
        .spawn(move || run_decoder(buffer, report, sample_tx, command_rx))
        .expect("the decoder thread failed to start");
    DecoderHandle {
        samples: sample_rx,
        commands: command_tx,
    }
}

type BufferDecoder = rodio::Decoder<crate::stream::BufferReader>;

/// The decoder thread's whole job: open the stream, report its
/// shape, then decode until the track ends, the buffer fails, or the
/// caller lets go of the source.
fn run_decoder(
    buffer: AudioBuffer,
    report: impl FnOnce(Result<ReadyInfo, String>),
    samples: SyncSender<f32>,
    commands: Receiver<DecoderCommand>,
) {
    let decoder = match build_streaming_decoder(&buffer) {
        Ok(decoder) => decoder,
        Err(error) => {
            report(Err(format!("The audio did not decode: {error}")));
            return;
        }
    };
    report(Ok(ReadyInfo {
        channels: decoder.channels(),
        sample_rate: decoder.sample_rate(),
        total_duration: decoder.total_duration(),
    }));
    let mut state = DecodeState::new(decoder, buffer);
    decode_until_done(&mut state, &samples, &commands);
}

/// A decoder that reads the stream front to back, with no seeking.
/// Symphonia then parses the header up to the first data atom and
/// starts, so playback begins while the download still runs. A
/// seekable decoder would parse every atom to the end of the file
/// first, and needs the byte length, which a filling buffer lacks.
fn build_streaming_decoder(
    buffer: &AudioBuffer,
) -> Result<BufferDecoder, rodio::decoder::DecoderError> {
    rodio::Decoder::builder()
        .with_data(buffer.reader())
        .with_seekable(false)
        .build()
}

/// A decoder over a complete buffer, with real seeks. Only valid once
/// the download is complete, because symphonia reads every atom of a
/// seekable stream before it starts.
fn build_seekable_decoder(
    buffer: &AudioBuffer,
    len: u64,
) -> Result<BufferDecoder, rodio::decoder::DecoderError> {
    rodio::Decoder::builder()
        .with_data(buffer.reader())
        .with_seekable(true)
        .with_byte_len(len)
        .build()
}

/// The decoder plus where it stands, in samples since the track
/// start, so a seek can compute how far to skip.
struct DecodeState {
    decoder: BufferDecoder,
    buffer: AudioBuffer,
    position_samples: u64,
    samples_per_second: u64,
}

impl DecodeState {
    fn new(decoder: BufferDecoder, buffer: AudioBuffer) -> Self {
        let samples_per_second =
            u64::from(decoder.channels().get()) * u64::from(decoder.sample_rate().get());
        Self {
            decoder,
            buffer,
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
        match self.buffer.complete_bytes().map(|bytes| bytes.len() as u64) {
            Some(len) => self.seek_in_complete_buffer(target, target_samples, len),
            None if target_samples >= self.position_samples => {
                self.skip_to(target_samples);
                Ok(())
            }
            None => self.restart_and_skip_to(target_samples),
        }
    }

    fn seek_in_complete_buffer(
        &mut self,
        target: Duration,
        target_samples: u64,
        len: u64,
    ) -> Result<(), String> {
        let mut decoder = build_seekable_decoder(&self.buffer, len)
            .map_err(|error| format!("The audio did not decode: {error}"))?;
        decoder
            .try_seek(target)
            .map_err(|error| format!("The seek failed: {error}"))?;
        self.decoder = decoder;
        self.position_samples = target_samples;
        Ok(())
    }

    fn restart_and_skip_to(&mut self, target_samples: u64) -> Result<(), String> {
        self.decoder = build_streaming_decoder(&self.buffer)
            .map_err(|error| format!("The audio did not decode: {error}"))?;
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
    samples: &SyncSender<f32>,
    commands: &Receiver<DecoderCommand>,
) {
    loop {
        match commands.try_recv() {
            Ok(DecoderCommand::Seek(position)) => {
                if let Err(error) = state.seek(position) {
                    log::warn!("seek failed: {error}");
                }
                continue;
            }
            Ok(DecoderCommand::Stop) => return,
            Err(TryRecvError::Disconnected) => return,
            Err(TryRecvError::Empty) => {}
        }
        let Some(sample) = state.next_sample() else {
            return;
        };
        if samples.send(sample).is_err() {
            return;
        }
    }
}

/// A `rodio::Source` fed by a decoder thread. Never blocks in
/// `next()`: a stall plays silence, so the position stays honest
/// while it waits for more bytes.
pub struct StreamingSource {
    samples: Receiver<f32>,
    commands: SyncSender<DecoderCommand>,
    channels: ChannelCount,
    sample_rate: SampleRate,
    total_duration: Option<Duration>,
    progress: Arc<Progress>,
}

impl Iterator for StreamingSource {
    type Item = f32;

    fn next(&mut self) -> Option<f32> {
        let received = self.samples.try_recv();
        if received.is_ok() {
            self.progress.real_samples.fetch_add(1, Ordering::Relaxed);
        }
        decide_next_sample(received)
    }
}

/// What `next()` returns, given the decoder thread's channel state. A
/// pure decision, tested without a real decoder thread: a ready
/// sample passes through, an empty channel plays silence (the decoder
/// is alive but has not decoded far enough yet), and a disconnected
/// channel ends the track.
fn decide_next_sample(received: Result<f32, TryRecvError>) -> Option<f32> {
    match received {
        Ok(sample) => Some(sample),
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

    /// Sends the seek to the decoder thread, then drops every sample
    /// already queued for the old position. A seek beyond the
    /// downloaded bytes blocks inside the decoder thread until the
    /// bytes arrive; that thread is not this one, so playback keeps
    /// its callback responsive.
    fn try_seek(&mut self, position: Duration) -> Result<(), SeekError> {
        let _ = self.commands.send(DecoderCommand::Seek(position));
        drain(&self.samples);
        self.progress
            .base_nanos
            .store(position.as_nanos() as u64, Ordering::Relaxed);
        self.progress.real_samples.store(0, Ordering::Relaxed);
        Ok(())
    }
}

fn drain(samples: &Receiver<f32>) {
    while samples.try_recv().is_ok() {}
}

impl Drop for StreamingSource {
    /// Asks the decoder thread to stop. A best-effort nudge: the
    /// thread may already have ended, or may be blocked on a network
    /// read and only notice this later.
    fn drop(&mut self) {
        let _ = self.commands.try_send(DecoderCommand::Stop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::AudioBuffer;
    use std::sync::mpsc::channel;
    use std::thread;
    use std::time::Duration;

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
    fn decide_next_sample_passes_through_a_ready_sample() {
        assert_eq!(decide_next_sample(Ok(0.5)), Some(0.5));
    }

    #[test]
    fn decide_next_sample_plays_silence_while_the_decoder_is_alive() {
        assert_eq!(decide_next_sample(Err(TryRecvError::Empty)), Some(0.0));
    }

    #[test]
    fn decide_next_sample_ends_the_track_when_the_decoder_is_gone() {
        assert_eq!(decide_next_sample(Err(TryRecvError::Disconnected)), None);
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
    fn a_seek_on_a_complete_buffer_lands_at_the_target() {
        let sample_count = 8_000;
        let buffer = AudioBuffer::from_complete(wav_bytes(8_000, sample_count).into());
        let (ready, handle) = wait_for_ready(buffer);
        let info = ready.expect("a well-formed WAV decodes");
        let (mut source, position) = handle.into_source(info);
        source.try_seek(Duration::from_millis(500)).expect("seek is accepted");

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut real = 0u64;
        while std::time::Instant::now() < deadline {
            match source.next() {
                Some(sample) if sample != 0.0 => real += 1,
                Some(_) => thread::sleep(Duration::from_millis(1)),
                None => break,
            }
        }
        assert!((3_900..=4_100).contains(&real), "played {real} samples after the seek");
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

        let (source, _position) = handle.into_source(info);
        let mut decoded = 0;
        loop {
            match source.samples.recv_timeout(Duration::from_secs(5)) {
                Ok(_) => decoded += 1,
                Err(_) => break,
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
