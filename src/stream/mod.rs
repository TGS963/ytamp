//! Stream resolution: a video id in, a playable audio URL out.
//!
//! YouTube changes its stream protection often, so resolution WILL
//! break from time to time. The seam here keeps that churn contained:
//! resolvers implement one trait, and the chain tries each in order
//! until one produces a stream.
//!
//! The chain today: rustypipe first, the yt-dlp subprocess as the
//! fallback when it is installed.

mod rustypipe;
mod ytdlp;

use std::future::Future;
use std::pin::Pin;

pub use rustypipe::RustyPipeResolver;
pub use ytdlp::YtDlpResolver;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A playable audio stream. The player decodes AAC in M4A, so
/// resolvers must prefer that container.
#[derive(Clone, Debug)]
pub struct ResolvedStream {
    pub url: String,
    pub mime: String,
}

pub trait StreamResolver: Send + Sync {
    fn name(&self) -> &'static str;
    fn resolve<'a>(&'a self, video_id: &'a str) -> BoxFuture<'a, Result<ResolvedStream, String>>;
}

/// Tries each resolver in order and returns the first stream.
pub struct ResolverChain {
    resolvers: Vec<Box<dyn StreamResolver>>,
}

impl ResolverChain {
    pub fn with_default_resolvers() -> Self {
        Self {
            resolvers: vec![
                Box::new(RustyPipeResolver::new()),
                Box::new(YtDlpResolver::new()),
            ],
        }
    }

    /// The error text names every resolver that failed and why.
    pub async fn resolve(&self, video_id: &str) -> Result<ResolvedStream, String> {
        let mut failures = Vec::new();
        for resolver in &self.resolvers {
            match resolver.resolve(video_id).await {
                Ok(stream) => return Ok(stream),
                Err(message) => failures.push(format!("{}: {message}", resolver.name())),
            }
        }
        Err(format!(
            "No resolver produced a stream. {}",
            failures.join(" / ")
        ))
    }
}
