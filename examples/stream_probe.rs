//! Drives the real streaming pipeline for one video id and reports
//! the timing of three events: the first downloaded byte, the
//! decoder's Ready report, and the buffer reaching Complete. It then
//! pulls two seconds of audio from the resulting `StreamingSource`
//! and reports how many samples were real versus silence.
//!
//! Usage: cargo run --example stream_probe [video_id]

use std::sync::Arc;
use std::time::{Duration, Instant};

use ytamp::player::source::{ReadyInfo, StreamingSource, spawn_decoder};
use ytamp::stream::{AudioBuffer, BufferStatus, ResolverChain, disk_cache};

/// How often the probe checks the buffer for new bytes.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// How much audio the probe samples once the decoder is ready.
const SAMPLE_WINDOW: Duration = Duration::from_secs(2);

#[tokio::main]
async fn main() {
    env_logger::Builder::new().parse_filters("warn").init();
    let video_id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "dQw4w9WgXcQ".to_string());
    let start = Instant::now();

    let resolvers = Arc::new(ResolverChain::with_default_resolvers());
    let http = reqwest::Client::new();

    println!("fetching audio for {video_id}");
    let buffer = disk_cache::fetch_audio(&resolvers, &http, &video_id).await;

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let handle = spawn_decoder(buffer.clone(), move |result| {
        let _ = ready_tx.send(result);
    });

    let first_byte_at = wait_for_first_byte(&buffer, start).await;
    println!("[{:>6} ms] first byte", first_byte_at.as_millis());

    let ready = match ready_rx.await {
        Ok(Ok(ready)) => ready,
        Ok(Err(message)) => fail(&format!("the decoder did not open: {message}")),
        Err(_) => fail("the decoder thread ended before it reported Ready"),
    };
    println!(
        "[{:>6} ms] Ready: channels={} sample_rate={} total_duration={:?}",
        start.elapsed().as_millis(),
        ready.channels.get(),
        ready.sample_rate.get(),
        ready.total_duration
    );

    match buffer.wait_complete().await {
        Ok(bytes) => println!(
            "[{:>6} ms] Complete: {} bytes",
            start.elapsed().as_millis(),
            bytes.len()
        ),
        Err(message) => fail(&format!("the download failed: {message}")),
    }

    let (mut source, position) = handle.into_source(ready);
    let (real, silence) = sample_playback(&mut source, &position, ready, SAMPLE_WINDOW).await;
    println!(
        "sampled {SAMPLE_WINDOW:?} of audio: {real} real samples, {silence} silence samples, position={:?}",
        position.position()
    );
}

/// Polls `buffer` until it holds at least one byte, or fails before
/// any byte arrives. Returns the elapsed time since `start`.
async fn wait_for_first_byte(buffer: &AudioBuffer, start: Instant) -> Duration {
    loop {
        if buffer.downloaded_len() > 0 {
            return start.elapsed();
        }
        if let BufferStatus::Failed(message) = buffer.status() {
            fail(&format!(
                "the download failed before any byte arrived: {message}"
            ));
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// How often the probe pulls a new batch of samples, to pace
/// playback at roughly real speed. `StreamingSource::next()` never
/// blocks, so a tight loop would race far ahead of the decoder
/// thread and count its own impatience as silence.
const PACE_INTERVAL: Duration = Duration::from_millis(20);

/// Pulls samples from `source`, paced at real speed, until
/// `position` reaches `window` of real audio or the source ends.
/// Returns the real sample count (from `position`, which never
/// counts silence) and the silence count (the remainder of what the
/// probe pulled).
async fn sample_playback(
    source: &mut StreamingSource,
    position: &ytamp::player::source::PositionHandle,
    ready: ReadyInfo,
    window: Duration,
) -> (u64, u64) {
    let samples_per_second = u64::from(ready.channels.get()) * u64::from(ready.sample_rate.get());
    let batch_size = (samples_per_second * PACE_INTERVAL.as_millis() as u64 / 1000).max(1);
    let mut pulled = 0u64;
    while position.position() < window {
        match pull_batch(source, batch_size) {
            PullOutcome::Continue(count) => pulled += count,
            PullOutcome::Ended(count) => {
                pulled += count;
                break;
            }
        }
        tokio::time::sleep(PACE_INTERVAL).await;
    }
    let real = (position.position().as_secs_f64() * samples_per_second as f64).round() as u64;
    let silence = pulled.saturating_sub(real);
    (real, silence)
}

/// What one batch pull found: the source kept going, or ended partway
/// through the batch. Either way, the sample count already pulled.
enum PullOutcome {
    Continue(u64),
    Ended(u64),
}

/// Pulls up to `count` samples from `source`. Stops early when the
/// source ends.
fn pull_batch(source: &mut StreamingSource, count: u64) -> PullOutcome {
    for pulled in 0..count {
        if source.next().is_none() {
            return PullOutcome::Ended(pulled);
        }
    }
    PullOutcome::Continue(count)
}

/// Prints `message` and exits with a non-zero status. The probe has
/// no recoverable failure path past this point, so a normal return
/// would misreport success to the shell.
fn fail(message: &str) -> ! {
    eprintln!("failed: {message}");
    std::process::exit(1);
}
