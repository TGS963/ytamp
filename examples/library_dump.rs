//! Dumps the raw library-playlists JSON to a local file for diagnosis.
//! Usage: cargo run --example library_dump

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
    let yt = ytmapi_rs::YtMusic::from_cookie(load_cookies())
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
    println!("wrote {}", path.display());
}
