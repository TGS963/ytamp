//! The rustypipe source: pure Rust InnerTube extraction, then a
//! chunked download with the matching user agent.

use rustypipe::client::RustyPipe;
use rustypipe::model::{AudioCodec, AudioStream};

use super::download::{ResolvedStream, download_audio};
use super::{AudioSource, BoxFuture};

pub struct RustyPipeSource {
    client: RustyPipe,
}

impl RustyPipeSource {
    pub fn new() -> Self {
        Self {
            client: RustyPipe::new(),
        }
    }
}

impl AudioSource for RustyPipeSource {
    fn name(&self) -> &'static str {
        "rustypipe"
    }

    fn fetch_audio<'a>(
        &'a self,
        http: &'a reqwest::Client,
        video_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<u8>, String>> {
        Box::pin(async move {
            let player = self
                .client
                .query()
                .player(video_id)
                .await
                .map_err(|error| error.to_string())?;
            let stream = pick_audio_stream(&player.audio_streams)
                .ok_or("the response carries no decodable audio stream")?;
            let resolved = ResolvedStream {
                url: stream.url.clone(),
                user_agent: Some(
                    self.client
                        .query()
                        .user_agent(player.client_type)
                        .to_string(),
                ),
                size: Some(stream.size),
            };
            let result = download_audio(http, &resolved).await;
            if let Err(error) = &result
                && error.forbidden
                && let Some(visitor_data) = &player.visitor_data
            {
                // A 403 marks the session that produced the URL as bad,
                // the way rustypipe-downloader reacts to the same error.
                self.client.query().remove_visitor_data(visitor_data);
            }
            result.map_err(|error| error.message)
        })
    }
}

/// The best stream the player can decode: AAC (M4A) at the highest
/// bitrate. Opus sits in WebM, which the decoder does not read yet.
fn pick_audio_stream(streams: &[AudioStream]) -> Option<&AudioStream> {
    let candidates = streams
        .iter()
        .map(|stream| (stream.codec == AudioCodec::Mp4a, stream.bitrate));
    best_decodable_index(candidates).map(|index| &streams[index])
}

/// The index of the highest-bitrate entry marked decodable.
fn best_decodable_index(streams: impl Iterator<Item = (bool, u32)>) -> Option<usize> {
    streams
        .enumerate()
        .filter(|(_, (decodable, _))| *decodable)
        .max_by_key(|&(_, (_, bitrate))| bitrate)
        .map(|(index, _)| index)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_highest_bitrate_decodable_stream_wins() {
        let streams = [(false, 160_000), (true, 128_000), (true, 48_000)];
        assert_eq!(best_decodable_index(streams.into_iter()), Some(1));
    }

    #[test]
    fn undecodable_only_lists_resolve_to_nothing() {
        assert_eq!(best_decodable_index([(false, 160_000)].into_iter()), None);
    }
}
