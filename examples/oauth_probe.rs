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
    match api.liked_songs().await {
        Ok(tracks) => println!("liked: {} tracks", tracks.len()),
        Err(error) => println!("liked failed: {error}"),
    }
}
