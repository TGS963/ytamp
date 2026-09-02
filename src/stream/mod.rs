//! Stream sourcing: a video id in, a growing buffer of playable
//! audio bytes out.
//!
//! YouTube changes its stream protection often, so sourcing WILL
//! break from time to time. The seam here keeps that churn contained:
//! sources implement one trait, and the chain tries each in order
//! until one delivers bytes.
//!
//! The chain today: rustypipe (pure Rust InnerTube extraction), then
//! the yt-dlp subprocess when the binary is installed. yt-dlp
//! downloads the bytes itself, because a URL from `--get-url` binds
//! to yt-dlp's own session and rejects another program's fetch.

pub mod buffer;
pub mod disk_cache;
mod download;
mod rustypipe;
mod visitor_data;
mod ytdlp;

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};

pub use buffer::{AudioBuffer, BufferReader, BufferStatus, BufferWriter};
pub use rustypipe::RustyPipeSource;
pub use ytdlp::YtDlpSource;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One way to turn a video id into audio bytes. A source pushes
/// chunks to `writer` as they arrive and calls `writer.finish` on
/// success.
///
/// A source that fails before its first chunk returns `Err` without
/// touching `writer`, so a resolver chain can hand the same download
/// to the next source. A source that fails after its first chunk
/// calls `writer.fail` before it returns `Err`, since a reader may
/// already be decoding the bytes so far.
pub trait AudioSource: Send + Sync {
    fn name(&self) -> &'static str;
    /// Prepares session state ahead of the first track, for example a
    /// token fetch. The default does nothing.
    fn warm_up<'a>(&'a self, _http: &'a reqwest::Client) -> BoxFuture<'a, ()> {
        Box::pin(async {})
    }
    fn fetch_audio<'a>(
        &'a self,
        http: &'a reqwest::Client,
        video_id: &'a str,
        writer: BufferWriter,
    ) -> BoxFuture<'a, Result<(), String>>;
}

/// How many failures with no chunk delivered trip a source's circuit
/// breaker.
const TRIP_THRESHOLD: u8 = 3;

/// Counts a source's no-chunk-delivered failures across the whole
/// session. Trips once the count reaches `TRIP_THRESHOLD`, and never
/// resets: a source that keeps failing stops wasting the user's time
/// on every later track.
struct CircuitBreaker {
    failures: AtomicU8,
}

impl CircuitBreaker {
    fn new() -> Self {
        Self {
            failures: AtomicU8::new(0),
        }
    }

    /// Records one failure. Returns `true` on the call that reaches
    /// the trip threshold, so the caller logs the trip exactly once.
    fn record_failure(&self) -> bool {
        let previous = self.failures.fetch_add(1, Ordering::SeqCst);
        previous + 1 == TRIP_THRESHOLD
    }

    /// True once the breaker has tripped.
    fn is_open(&self) -> bool {
        self.failures.load(Ordering::SeqCst) >= TRIP_THRESHOLD
    }
}

/// One source with its own circuit breaker.
struct TrackedSource {
    source: Box<dyn AudioSource>,
    breaker: CircuitBreaker,
}

/// Tries each source in order until one delivers audio bytes.
pub struct ResolverChain {
    sources: Vec<TrackedSource>,
}

/// What one source attempt means for the chain.
enum Attempt {
    /// The source finished the buffer. The chain stops here.
    Succeeded,
    /// The source failed after it delivered bytes. The buffer is
    /// already failed, so the chain stops here too.
    EndedTheChain(String),
    /// The source failed before it delivered any bytes. The chain
    /// tries the next source.
    Continue(String),
}

impl ResolverChain {
    /// yt-dlp goes first while rustypipe is broken upstream (its
    /// deobfuscator does not parse the 2025 player script). Each failed
    /// rustypipe attempt cost about 500 ms per track. Swap the order
    /// back when a fixed rustypipe release lands: native extraction is
    /// faster than the subprocess.
    pub fn with_default_resolvers() -> Self {
        Self {
            sources: vec![
                TrackedSource {
                    source: Box::new(YtDlpSource::new()),
                    breaker: CircuitBreaker::new(),
                },
                TrackedSource {
                    source: Box::new(RustyPipeSource::new()),
                    breaker: CircuitBreaker::new(),
                },
            ],
        }
    }

    /// Builds a chain from arbitrary sources, each with a fresh
    /// breaker. Test-only: production code always starts from
    /// `with_default_resolvers`.
    /// Runs every source's warm-up once, so the first track does not
    /// pay for session setup.
    pub async fn warm_up(&self, http: &reqwest::Client) {
        for tracked in &self.sources {
            tracked.source.warm_up(http).await;
        }
    }

    #[cfg(test)]
    fn with_sources(sources: Vec<Box<dyn AudioSource>>) -> Self {
        Self {
            sources: sources
                .into_iter()
                .map(|source| TrackedSource {
                    source,
                    breaker: CircuitBreaker::new(),
                })
                .collect(),
        }
    }

    /// Fills `writer` from the first source that delivers bytes. On
    /// total failure, fails `writer` with every source's error joined
    /// into one message.
    pub async fn fetch_audio(
        &self,
        http: &reqwest::Client,
        video_id: &str,
        writer: BufferWriter,
    ) -> Result<(), String> {
        let mut failures = Vec::new();
        let mut skipped: Vec<&TrackedSource> = Vec::new();
        for tracked in self
            .sources
            .iter()
            .filter(|tracked| !tracked.breaker.is_open())
        {
            let outcome = tracked
                .source
                .fetch_audio(http, video_id, writer.share())
                .await;
            match classify(outcome, &writer) {
                Attempt::Succeeded => {
                    record_bypassed_failures(&skipped);
                    return Ok(());
                }
                Attempt::EndedTheChain(message) => return Err(message),
                Attempt::Continue(message) => {
                    failures.push(format!("{}: {message}", tracked.source.name()));
                    skipped.push(tracked);
                }
            }
        }
        let joined = join_failures(&failures);
        writer.fail(joined.clone());
        Err(joined)
    }
}

/// A source failure counts toward its breaker only when a later source
/// delivers the same video. A video no source can deliver is a bad
/// video, not a broken source, and must never disable the last source
/// for the session.
fn record_bypassed_failures(skipped: &[&TrackedSource]) {
    for tracked in skipped {
        if tracked.breaker.record_failure() {
            log::warn!(
                "{} tripped the circuit breaker: later sources kept delivering",
                tracked.source.name()
            );
        }
    }
}

fn classify(outcome: Result<(), String>, writer: &BufferWriter) -> Attempt {
    match outcome {
        Ok(()) => Attempt::Succeeded,
        Err(message) if writer.delivered_any() => Attempt::EndedTheChain(message),
        Err(message) => Attempt::Continue(message),
    }
}

/// A single message that names every source failure, for the final
/// `writer.fail` call when the whole chain runs out of sources.
fn join_failures(failures: &[String]) -> String {
    if failures.is_empty() {
        return "No source produced audio. Every source is tripped by earlier failures."
            .to_string();
    }
    format!("No source produced audio. {}", failures.join(" / "))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fake source for chain tests: fails before it delivers any
    /// bytes when `push_first` is `false`, otherwise pushes one chunk
    /// then fails.
    struct FakeSource {
        label: &'static str,
        push_first: bool,
        succeeds: bool,
    }

    impl AudioSource for FakeSource {
        fn name(&self) -> &'static str {
            self.label
        }

        fn fetch_audio<'a>(
            &'a self,
            _http: &'a reqwest::Client,
            _video_id: &'a str,
            writer: BufferWriter,
        ) -> BoxFuture<'a, Result<(), String>> {
            Box::pin(async move {
                if self.push_first {
                    writer.push(b"chunk");
                }
                if self.succeeds {
                    writer.finish();
                    return Ok(());
                }
                let message = format!("{} failed", self.label);
                if self.push_first {
                    writer.fail(message.clone());
                }
                Err(message)
            })
        }
    }

    fn http_client() -> reqwest::Client {
        reqwest::Client::new()
    }

    #[tokio::test]
    async fn the_chain_falls_to_the_next_source_before_the_first_chunk() {
        let chain = ResolverChain::with_sources(vec![
            Box::new(FakeSource {
                label: "first",
                push_first: false,
                succeeds: false,
            }),
            Box::new(FakeSource {
                label: "second",
                push_first: true,
                succeeds: true,
            }),
        ]);
        let buffer = AudioBuffer::new(None);
        let writer = buffer.writer();
        let http = http_client();
        chain
            .fetch_audio(&http, "vid", writer)
            .await
            .expect("the second source completes the buffer");
        assert_eq!(buffer.status(), BufferStatus::Complete);
    }

    #[tokio::test]
    async fn the_chain_stops_after_a_source_delivers_then_fails() {
        let chain = ResolverChain::with_sources(vec![
            Box::new(FakeSource {
                label: "first",
                push_first: true,
                succeeds: false,
            }),
            Box::new(FakeSource {
                label: "second",
                push_first: true,
                succeeds: true,
            }),
        ]);
        let buffer = AudioBuffer::new(None);
        let writer = buffer.writer();
        let http = http_client();
        let error = chain
            .fetch_audio(&http, "vid", writer)
            .await
            .expect_err("the first source's late failure ends the chain");
        assert_eq!(error, "first failed");
        assert_eq!(
            buffer.status(),
            BufferStatus::Failed("first failed".to_string())
        );
    }

    fn failing_source(label: &'static str) -> Box<FakeSource> {
        Box::new(FakeSource {
            label,
            push_first: false,
            succeeds: false,
        })
    }

    fn working_source(label: &'static str) -> Box<FakeSource> {
        Box::new(FakeSource {
            label,
            push_first: true,
            succeeds: true,
        })
    }

    async fn run_chain(chain: &ResolverChain) {
        let buffer = AudioBuffer::new(None);
        let _ = chain
            .fetch_audio(&http_client(), "video", buffer.writer())
            .await;
    }

    #[tokio::test]
    async fn a_source_trips_only_when_a_later_source_delivers() {
        let chain =
            ResolverChain::with_sources(vec![failing_source("broken"), working_source("working")]);
        for _ in 0..3 {
            run_chain(&chain).await;
        }
        assert!(chain.sources[0].breaker.is_open());
        assert!(!chain.sources[1].breaker.is_open());
    }

    #[tokio::test]
    async fn a_video_no_source_delivers_never_trips_a_breaker() {
        let chain =
            ResolverChain::with_sources(vec![failing_source("first"), failing_source("last")]);
        for _ in 0..5 {
            run_chain(&chain).await;
        }
        assert!(!chain.sources[0].breaker.is_open());
        assert!(!chain.sources[1].breaker.is_open());
    }

    #[test]
    fn a_breaker_stays_closed_under_the_threshold() {
        let breaker = CircuitBreaker::new();
        assert!(!breaker.record_failure());
        assert!(!breaker.record_failure());
        assert!(!breaker.is_open());
    }

    #[test]
    fn a_breaker_trips_on_the_third_failure() {
        let breaker = CircuitBreaker::new();
        assert!(!breaker.record_failure());
        assert!(!breaker.record_failure());
        assert!(breaker.record_failure());
        assert!(breaker.is_open());
    }

    #[test]
    fn a_tripped_breaker_reports_no_further_trips() {
        let breaker = CircuitBreaker::new();
        for _ in 0..3 {
            breaker.record_failure();
        }
        assert!(!breaker.record_failure());
        assert!(breaker.is_open());
    }

    #[test]
    fn join_failures_names_every_source_when_some_ran() {
        let failures = vec![
            "rustypipe: boom".to_string(),
            "yt-dlp: also boom".to_string(),
        ];
        let message = join_failures(&failures);
        assert!(message.contains("rustypipe: boom"));
        assert!(message.contains("yt-dlp: also boom"));
    }

    #[test]
    fn join_failures_reports_an_all_tripped_chain() {
        assert!(join_failures(&[]).contains("tripped"));
    }
}
