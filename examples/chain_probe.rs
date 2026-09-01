//! Fetches audio bytes through the app's real resolver chain and
//! reports which source produced them.
//! Usage: cargo run --example chain_probe [video_id]

use ytamp::stream::{AudioBuffer, ResolverChain};

#[tokio::main]
async fn main() {
    env_logger::Builder::new().parse_filters("warn").init();
    let video_id = std::env::args().nth(1).unwrap_or("dQw4w9WgXcQ".to_string());
    let chain = ResolverChain::with_default_resolvers();
    let http = reqwest::Client::new();
    let buffer = AudioBuffer::new(None);
    let writer = buffer.writer();
    if let Err(message) = chain.fetch_audio(&http, &video_id, writer).await {
        println!("failed: {message}");
        return;
    }
    match buffer.complete_bytes() {
        Some(bytes) => println!("ok: {} bytes for {video_id}", bytes.len()),
        None => println!("failed: the chain reported success without completing the buffer"),
    }
}
