//! Runs the app's API calls against the saved credentials and prints
//! each result with its full error chain. It also tries account
//! indexes 0 to 3, so a multi-account session shows which
//! X-Goog-AuthUser value reaches the right library.
//! Usage: cargo run --example auth_probe

use std::error::Error as StdError;

use reqwest::header::{HeaderMap, HeaderValue};
use ytmapi_rs::YtMusicBuilder;
use ytmapi_rs::common::{PlaylistID, YoutubeID};

fn load_cookies() -> String {
    let dirs = directories::ProjectDirs::from("", "", "ytamp").expect("a config directory");
    let dir = dirs.config_dir();
    if let Ok(json) = std::fs::read_to_string(dir.join("auth.json"))
        && let Ok(value) = serde_json::from_str::<serde_json::Value>(&json)
        && let Some(cookies) = value.get("cookies").and_then(|v| v.as_str())
    {
        return cookies.to_string();
    }
    std::fs::read_to_string(dir.join("cookies.txt"))
        .expect("a saved auth.json or cookies.txt")
        .trim()
        .to_string()
}

#[tokio::main]
async fn main() {
    let cookies = load_cookies();
    println!("cookies: {} bytes", cookies.len());

    for authuser in 0..=3 {
        match client_for(authuser, &cookies).await {
            Err(error) => println!("[authuser {authuser}] token build failed: {error}"),
            Ok(yt) => {
                report(
                    authuser,
                    "playlists",
                    &yt.get_library_playlists().await.map(|l| l.len()),
                );
                report(
                    authuser,
                    "liked",
                    &yt.get_playlist_tracks(PlaylistID::from_raw("VLLM"))
                        .await
                        .map(|l| l.len()),
                );
            }
        }
    }
}

async fn client_for(
    authuser: u8,
    cookies: &str,
) -> Result<ytmapi_rs::YtMusic<ytmapi_rs::auth::BrowserToken>, String> {
    let mut headers = HeaderMap::new();
    headers.insert("X-Goog-AuthUser", HeaderValue::from(authuser as u16));
    let http = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|error| error.to_string())?;
    YtMusicBuilder::new_with_client(ytmapi_rs::Client::new_from_reqwest_client(http))
        .with_browser_token_cookie(cookies.to_string())
        .build()
        .await
        .map_err(|error| chain(&error))
}

fn report<T: std::fmt::Debug>(authuser: u8, name: &str, result: &Result<T, ytmapi_rs::Error>) {
    match result {
        Ok(count) => println!("[authuser {authuser}] {name}: {count:?}"),
        Err(error) => println!("[authuser {authuser}] {name} failed: {}", chain(error)),
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
