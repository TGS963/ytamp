//! Runs the artist, album, and radio API methods with the app's
//! stored sign-in and prints what the library returns. Usage:
//! `cargo run --example browse_probe [artist_id] [album_id] [video_id]`.
//!
//! The default artist id `UCoC47do520os_4DBMEFGg4A` fails to parse
//! with ytmapi-rs 0.3.3 ("Key /header/musicImmersiveHeaderRenderer
//! not found"), so the defaults here are ids checked to work: The
//! Beatles and their "Yellow Submarine" album.

use ytamp::core::model::{AlbumId, ArtistId, TrackId};

const DEFAULT_ARTIST_ID: &str = "UC2XdaAVUannpujzv32jcouQ";
const DEFAULT_ALBUM_ID: &str = "MPREb_nn3MWHNM0jS";
const DEFAULT_VIDEO_ID: &str = "dQw4w9WgXcQ";

#[tokio::main]
async fn main() {
    let mut args = std::env::args().skip(1);
    let artist_id = args.next().unwrap_or_else(|| DEFAULT_ARTIST_ID.to_string());
    let album_id = args.next().unwrap_or_else(|| DEFAULT_ALBUM_ID.to_string());
    let video_id = args.next().unwrap_or_else(|| DEFAULT_VIDEO_ID.to_string());

    let method = ytamp::auth::load_auth_method().expect("a stored sign-in");
    let api = match ytamp::api::Api::sign_in(&method).await {
        Ok(api) => api,
        Err(error) => return println!("sign-in failed: {error}"),
    };

    print_artist(&api, &artist_id).await;
    print_album(&api, &album_id).await;
    print_radio(&api, &video_id).await;
}

async fn print_artist(api: &ytamp::api::Api, artist_id: &str) {
    match api.artist(&ArtistId(artist_id.to_string())).await {
        Ok(page) => println!(
            "artist: {} (top songs {}, albums {}, singles {})",
            page.name,
            page.top_songs.len(),
            page.albums.len(),
            page.singles.len()
        ),
        Err(error) => println!("artist failed: {error}"),
    }
}

async fn print_album(api: &ytamp::api::Api, album_id: &str) {
    match api.album(&AlbumId(album_id.to_string())).await {
        Ok(page) => {
            println!("album: {} ({} tracks)", page.album.title, page.tracks.len());
            for track in page.tracks.iter().take(5) {
                println!("- {}", track.title);
            }
        }
        Err(error) => println!("album failed: {error}"),
    }
}

async fn print_radio(api: &ytamp::api::Api, video_id: &str) {
    match api.radio(&TrackId(video_id.to_string())).await {
        Ok(tracks) => {
            println!("radio: {} tracks", tracks.len());
            for track in tracks.iter().take(5) {
                println!("- {} ({:?})", track.title, track.artists);
            }
        }
        Err(error) => println!("radio failed: {error}"),
    }
}
