//! Exercises the library write path once, end to end: create a
//! playlist, add a track, remove it, delete the playlist. Nothing
//! this probe writes stays in the account; it deletes the playlist
//! itself at the end.
//!
//! Usage: cargo run --example write_probe -- --write
//!
//! Without `--write` this only prints the steps it would run. A
//! failed step stops the run at once; earlier writes are left as
//! they are, for the user to clean up by hand, and the probe never
//! retries a write.

use ytamp::api::Api;
use ytamp::core::model::{PlaylistId, Track, TrackId};

const PROBE_TITLE: &str = "ytamp write probe";
const PROBE_TRACK: &str = "dQw4w9WgXcQ";

#[tokio::main]
async fn main() {
    if !std::env::args().any(|arg| arg == "--write") {
        return print_dry_run();
    }
    let method = ytamp::auth::load_auth_method().expect("a stored sign-in");
    let api = match Api::sign_in(&method).await {
        Ok(api) => api,
        Err(error) => return println!("sign-in failed: {error}"),
    };
    run(&api).await;
}

fn print_dry_run() {
    println!("This probe would, against the stored sign-in:");
    println!("1. create the playlist {PROBE_TITLE:?}");
    println!("2. add track {PROBE_TRACK} to it");
    println!("3. fetch its tracks and print the row with its item id");
    println!("4. remove that item");
    println!("5. fetch again to show the playlist is empty");
    println!("6. delete the playlist");
    println!("Run again with --write to run this for real.");
}

/// Runs every step in order and stops at the first failure. Each
/// step needs the last one's result, so the run reads top to bottom
/// as a plain sequence, not as nested error handling.
async fn run(api: &Api) {
    let Some(playlist) = report("create playlist", api.create_playlist(PROBE_TITLE).await) else {
        return;
    };
    let track_id = TrackId(PROBE_TRACK.to_string());
    let Some(item_id) = report(
        "add track",
        api.add_to_playlist(&playlist.id, &track_id).await,
    ) else {
        return;
    };
    let Some(before) = report("fetch tracks", fetch_tracks(api, &playlist.id).await) else {
        return;
    };
    print_row(&before, &item_id);
    if report(
        "remove item",
        api.remove_from_playlist(&playlist.id, &item_id).await,
    )
    .is_none()
    {
        return;
    }
    let Some(after) = report("fetch tracks again", fetch_tracks(api, &playlist.id).await) else {
        return;
    };
    println!("playlist now holds {} track(s)", after.len());
    report("delete playlist", api.delete_playlist(&playlist.id).await);
}

/// The playlist's full track list, gathered from the paged API into
/// one vector.
async fn fetch_tracks(api: &Api, id: &PlaylistId) -> Result<Vec<Track>, String> {
    let mut tracks = Vec::new();
    api.playlist_tracks(id, |page, _finished| tracks.extend(page))
        .await?;
    Ok(tracks)
}

/// Prints the fetched row whose item id matches `item_id`, or a
/// notice that no row carried it.
fn print_row(tracks: &[Track], item_id: &str) {
    match tracks.iter().find(|track| {
        track
            .playlist_item_id
            .as_deref()
            .is_some_and(|id| id == item_id)
    }) {
        Some(track) => println!(
            "row for item {item_id}: {:?} by {}",
            track.title,
            track.artist_names()
        ),
        None => println!("no fetched row carried item id {item_id}"),
    }
}

/// Prints `label`'s outcome and returns the value on success, so a
/// failed step can end the run with `?`-like brevity through a `let
/// else`.
fn report<T>(label: &str, result: Result<T, String>) -> Option<T> {
    match result {
        Ok(value) => {
            println!("{label}: OK");
            Some(value)
        }
        Err(error) => {
            println!("{label}: FAILED: {error}");
            None
        }
    }
}
