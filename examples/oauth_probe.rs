//! Runs raw ytmapi queries with the stored OAuth token and prints
//! each result. Usage: cargo run --example oauth_probe

use ytmapi_rs::auth::OAuthToken;
use ytmapi_rs::query::search::{FilteredSearch, SongsFilter};
use ytmapi_rs::query::{GetLibraryPlaylistsQuery, SearchQuery};

#[tokio::main]
async fn main() {
    let json = ytamp::auth::load_oauth_token().expect("a stored oauth token");
    let token: OAuthToken = serde_json::from_str(&json).expect("token parses");
    let yt = ytmapi_rs::YtMusicBuilder::new()
        .with_auth_token(token)
        .build()
        .expect("client");
    let search: SearchQuery<'_, FilteredSearch<SongsFilter>> = "test".into();
    match yt
        .query::<SearchQuery<FilteredSearch<SongsFilter>>>(&search)
        .await
    {
        Ok(songs) => println!("search: {} songs", songs.len()),
        Err(error) => println!("search failed: {error}"),
    }
    match yt
        .query::<GetLibraryPlaylistsQuery>(&GetLibraryPlaylistsQuery)
        .await
    {
        Ok(playlists) => println!("library playlists: {}", playlists.len()),
        Err(error) => println!("library failed: {error}"),
    }
}
