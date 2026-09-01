//! Stream sourcing: a video id in, playable audio bytes out.
//!
//! YouTube changes its stream protection often, so sourcing WILL
//! break from time to time. The seam here keeps that churn contained:
//! sources implement one trait, and the chain tries each in order
//! until one produces bytes.
//!
//! The chain today: rustypipe (pure Rust InnerTube extraction), then
//! the yt-dlp subprocess when the binary is installed. yt-dlp
//! downloads the bytes itself, because a URL from `--get-url` binds
//! to yt-dlp's own session and rejects another program's fetch.

mod download;
mod rustypipe;
mod ytdlp;

use std::future::Future;
use std::pin::Pin;

pub use rustypipe::RustyPipeSource;
pub use ytdlp::YtDlpSource;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One way to turn a video id into audio bytes the decoder reads
/// (AAC in M4A preferred).
pub trait AudioSource: Send + Sync {
    fn name(&self) -> &'static str;
    fn fetch_audio<'a>(
        &'a self,
        http: &'a reqwest::Client,
        video_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<u8>, String>>;
}

/// Tries each source in order until one produces audio bytes.
pub struct ResolverChain {
    sources: Vec<Box<dyn AudioSource>>,
}

impl ResolverChain {
    pub fn with_default_resolvers() -> Self {
        Self {
            sources: vec![
                Box::new(RustyPipeSource::new()),
                Box::new(YtDlpSource::new()),
            ],
        }
    }

    /// The error text names every source that failed and why.
    pub async fn fetch_audio(
        &self,
        http: &reqwest::Client,
        video_id: &str,
    ) -> Result<Vec<u8>, String> {
        let mut failures = Vec::new();
        for source in &self.sources {
            match source.fetch_audio(http, video_id).await {
                Ok(bytes) => return Ok(bytes),
                Err(message) => {
                    log::warn!("{}: {message}", source.name());
                    failures.push(format!("{}: {message}", source.name()));
                }
            }
        }
        Err(format!(
            "No source produced audio. {}",
            failures.join(" / ")
        ))
    }
}
