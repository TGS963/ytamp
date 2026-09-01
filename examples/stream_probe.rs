//! Resolves one video and downloads its audio the way the player does:
//! the client's user agent, and googlevideo chunks through the range
//! URL parameter. Prints the byte count.
//! Usage: cargo run --example stream_probe [video_id]

use rustypipe::client::RustyPipe;
use rustypipe::model::AudioCodec;

#[tokio::main]
async fn main() {
    let video_id = std::env::args().nth(1).unwrap_or("dQw4w9WgXcQ".to_string());
    let rp = RustyPipe::new();
    let player = rp.query().player(&video_id).await.expect("player response");
    let stream = player
        .audio_streams
        .iter()
        .filter(|stream| stream.codec == AudioCodec::Mp4a)
        .max_by_key(|stream| stream.bitrate)
        .expect("an AAC stream");
    let user_agent = rp.query().user_agent(player.client_type).to_string();
    println!(
        "client: {:?}, itag {}, {} bytes",
        player.client_type, stream.itag, stream.size
    );

    let http = reqwest::Client::new();
    let mut bytes: u64 = 0;
    let mut offset: u64 = 0;
    while offset < stream.size {
        let end = (offset + 9_000_000 - 1).min(stream.size - 1);
        let url = format!("{}&range={offset}-{end}", stream.url);
        let chunk = http
            .get(&url)
            .header(reqwest::header::USER_AGENT, &user_agent)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .expect("chunk request")
            .bytes()
            .await
            .expect("chunk body");
        assert!(!chunk.is_empty(), "empty chunk");
        offset += chunk.len() as u64;
        bytes += chunk.len() as u64;
    }
    println!("downloaded {bytes} bytes ok");
}
