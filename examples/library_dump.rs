//! Dumps the raw library-playlists JSON to a local file for diagnosis.
//! Usage: cargo run --example library_dump [authuser]

use reqwest::header::{HeaderMap, HeaderValue};
use ytmapi_rs::YtMusicBuilder;

fn load(field: &str) -> String {
    let dirs = directories::ProjectDirs::from("", "", "ytamp").expect("a config directory");
    let json = std::fs::read_to_string(dirs.config_dir().join("auth.json")).expect("auth.json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("valid json");
    value
        .get(field)
        .and_then(|v| v.as_str())
        .expect("field")
        .to_string()
}

#[tokio::main]
async fn main() {
    let authuser = std::env::args().nth(1).unwrap_or_else(|| load("authuser"));
    let mut headers = HeaderMap::new();
    headers.insert(
        "X-Goog-AuthUser",
        HeaderValue::from_str(&authuser).expect("index"),
    );
    let http = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .expect("client");
    let yt = YtMusicBuilder::new_with_client(ytmapi_rs::Client::new_from_reqwest_client(http))
        .with_browser_token_cookie(load("cookies"))
        .build()
        .await
        .expect("sign-in");
    let json = yt
        .json_query(ytmapi_rs::query::GetLibraryPlaylistsQuery)
        .await
        .expect("query")
        .into_inner();
    let path = std::env::temp_dir().join("ytamp-library-playlists.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&json).expect("serialize"),
    )
    .expect("write");
    println!("authuser {authuser}, wrote {}", path.display());
}
