//! Compares playlist covers from the official Data API with the covers
//! the anonymous InnerTube playlist page returns. A playlist with a
//! custom cover shows the difference.
//! Usage: cargo run --example cover_probe [count]

use ytmapi_rs::YtMusicBuilder;
use ytmapi_rs::common::{PlaylistID, YoutubeID};
use ytmapi_rs::query::GetPlaylistDetailsQuery;

#[tokio::main]
async fn main() {
    let count: usize = std::env::args()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(8);
    let method = ytamp::auth::load_auth_method().expect("a stored sign-in");
    let api = ytamp::api::Api::sign_in(&method)
        .await
        .expect("the sign-in works");
    let playlists = api.library_playlists().await.expect("playlists load");
    let yt = YtMusicBuilder::new()
        .build()
        .await
        .expect("anonymous client");
    for playlist in playlists.iter().take(count) {
        println!("{} ({})", playlist.title, playlist.id.0);
        println!("  data api : {:?}", playlist.thumbnail_url);
        let browse = format!("VL{}", playlist.id.0);
        let query = GetPlaylistDetailsQuery::new(PlaylistID::from_raw(&browse));
        match yt.query::<GetPlaylistDetailsQuery>(&query).await {
            Ok(details) => {
                let best = details
                    .thumbnails
                    .iter()
                    .max_by_key(|t| t.width * t.height)
                    .map(|t| t.url.clone());
                println!("  innertube: {best:?} (privacy {:?})", details.privacy);
            }
            Err(error) => println!("  innertube: error {error}"),
        }
    }
}
