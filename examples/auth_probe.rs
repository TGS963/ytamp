//! Runs every API call the app makes against the saved cookie file
//! and prints each result with its full error chain.
//! Usage: cargo run --example auth_probe

use std::error::Error as StdError;
use std::fs;

use directories::ProjectDirs;
use ytmapi_rs::common::{PlaylistID, YoutubeID};

#[tokio::main]
async fn main() {
    let dirs = ProjectDirs::from("", "", "ytamp").expect("a config directory");
    let path = dirs.config_dir().join("cookies.txt");
    let cookies = fs::read_to_string(&path).expect("the saved cookie file");
    println!(
        "cookie file: {} ({} bytes)",
        path.display(),
        cookies.trim().len()
    );

    let yt = match ytmapi_rs::YtMusic::from_cookie(cookies.trim()).await {
        Ok(yt) => yt,
        Err(error) => {
            println!("token build failed: {}", chain(&error));
            return;
        }
    };

    report(
        "library playlists",
        &yt.get_library_playlists().await.map(|list| list.len()),
    );
    report(
        "liked songs (VLLM)",
        &yt.get_playlist_tracks(PlaylistID::from_raw("VLLM"))
            .await
            .map(|list| list.len()),
    );
    let query: ytmapi_rs::query::SearchQuery<'_, ytmapi_rs::query::search::BasicSearch> =
        "test".into();
    report(
        "search",
        &yt.query(query).await.map(|results| results.songs.len()),
    );
}

fn report<T>(name: &str, result: &Result<T, ytmapi_rs::Error>) {
    match result {
        Ok(_) => println!("[{name}] ok"),
        Err(error) => println!("[{name}] failed: {}", chain(error)),
    }
}

/// The error and every cause under it, joined for one line.
fn chain(error: &dyn StdError) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        parts.push(cause.to_string());
        source = cause.source();
    }
    parts.join(" <- ")
}
