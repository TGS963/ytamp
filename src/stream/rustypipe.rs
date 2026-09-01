//! The rustypipe resolver: pure Rust InnerTube extraction.

use rustypipe::client::RustyPipe;
use rustypipe::model::{AudioCodec, AudioStream};

use super::{BoxFuture, ResolvedStream, StreamResolver};

pub struct RustyPipeResolver {
    client: RustyPipe,
}

impl RustyPipeResolver {
    pub fn new() -> Self {
        Self {
            client: RustyPipe::new(),
        }
    }
}

impl StreamResolver for RustyPipeResolver {
    fn name(&self) -> &'static str {
        "rustypipe"
    }

    fn resolve<'a>(&'a self, video_id: &'a str) -> BoxFuture<'a, Result<ResolvedStream, String>> {
        Box::pin(async move {
            let player = self
                .client
                .query()
                .player(video_id)
                .await
                .map_err(|error| error.to_string())?;
            let stream = pick_audio_stream(&player.audio_streams)
                .ok_or("the response carries no decodable audio stream")?;
            let user_agent = self
                .client
                .query()
                .user_agent(player.client_type)
                .to_string();
            Ok(ResolvedStream {
                url: stream.url.clone(),
                mime: stream.mime.clone(),
                user_agent: Some(user_agent),
                size: Some(stream.size),
            })
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
