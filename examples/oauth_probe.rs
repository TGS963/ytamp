//! Runs the app's OAuth sign-in path with the stored token and prints
//! what the library returns. Usage: cargo run --example oauth_probe

#[tokio::main]
async fn main() {
    let method = ytamp::auth::load_auth_method().expect("a stored sign-in");
    let api = match ytamp::api::Api::sign_in(&method).await {
        Ok(api) => api,
        Err(error) => return println!("sign-in failed: {error}"),
    };
    match api.library_playlists().await {
        Ok(playlists) => {
            println!("playlists: {}", playlists.len());
            for playlist in playlists.iter().take(3) {
                println!("- {} ({:?} tracks)", playlist.title, playlist.track_count);
            }
        }
        Err(error) => println!("library failed: {error}"),
    }
    match api.search("test").await {
        Ok(results) => println!("search: {} songs", results.songs.len()),
        Err(error) => println!("search failed: {error}"),
    }
    let mut page_count = 0;
    let mut total = 0;
    let mut first_tracks = Vec::new();
    let result = api
        .liked_songs(|tracks, finished| {
            page_count += 1;
            total += tracks.len();
            println!(
                "liked page {page_count}: {} tracks (total {total}, finished {finished})",
                tracks.len()
            );
            if first_tracks.is_empty() {
                first_tracks = tracks.into_iter().take(3).collect();
            }
        })
        .await;
    match result {
        Ok(()) => println!("liked: {total} tracks across {page_count} pages"),
        Err(error) => println!("liked failed after {page_count} pages: {error}"),
    }
    probe_artist_ids(&api, &first_tracks).await;
}

/// Prints the first liked tracks' artist ids, then resolves the first
/// present id through `Api::artist`, to check a channel id from the
/// liked list opens as an artist page.
async fn probe_artist_ids(api: &ytamp::api::Api, first_tracks: &[ytamp::core::model::Track]) {
    for track in first_tracks {
        let ids: Vec<String> = track
            .artists
            .iter()
            .map(|artist| format!("{:?}", artist.id))
            .collect();
        println!(
            "track {:?} artists {:?} ids {ids:?}",
            track.title,
            track.artist_names()
        );
    }
    let Some(id) = first_tracks
        .iter()
        .flat_map(|track| track.artists.iter())
        .find_map(|artist| artist.id.clone())
    else {
        println!("no liked track carried an artist id");
        return;
    };
    match api.artist(&id).await {
        Ok(page) => println!("artist {id:?} resolved to {:?}", page.name),
        Err(error) => println!("artist {id:?} failed to resolve: {error}"),
    }
}
