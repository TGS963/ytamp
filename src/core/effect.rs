//! Everything the app does to the outside world, as data.
//!
//! The reducer returns effects. The runtime executes them and sends
//! result actions back. This keeps the reducer honest and testable.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::model::{AlbumId, ArtistId, Playlist, PlaylistId, Track, TrackId};

/// What a YouTube Music session needs: the Cookie header, the
/// X-Goog-AuthUser index that picks the account inside the session,
/// and the other request headers the browser sent. ytmusicapi keeps
/// and replays all copied headers, because account selection (brand
/// accounts included) and consistency checks ride on them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    pub cookies: String,
    #[serde(default)]
    pub authuser: String,
    /// Lowercased header names with their values, from the cURL paste.
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

/// One way to authenticate the YouTube Music session.
#[derive(Clone, Debug, PartialEq)]
pub enum AuthMethod {
    Browser(Credentials),
    /// A serialized ytmapi-rs OAuth token, from the device flow.
    /// It embeds the client id and secret, so it refreshes itself.
    OAuthToken(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Api(ApiRequest),
    Player(PlayerCommand),
    SaveCredentials(Credentials),
    ClearCredentials,
    /// Reads the cached playlist list and liked songs from disk.
    LoadLibraryCache,
    /// Reads the cached track list for one playlist from disk.
    LoadPlaylistTracksCache(PlaylistId),
    /// Writes fresh network data to the library cache, after the
    /// reducer has already applied it to the state.
    SaveLibraryCache(LibraryCacheWrite),
    /// Deletes the whole library cache. Sign-out pairs this with
    /// `ClearCredentials`, so no stale library survives for the next
    /// account.
    ClearLibraryCache,
}

/// One fresh network result to persist to the library cache.
#[derive(Clone, Debug, PartialEq)]
pub enum LibraryCacheWrite {
    Playlists(Vec<Playlist>),
    Liked(Vec<Track>),
    PlaylistTracks(PlaylistId, Vec<Track>),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ApiRequest {
    VerifyAuth(AuthMethod),
    /// Runs the whole OAuth device flow: gets a device code, reports
    /// the verification URL back, and polls until the user finishes.
    StartOAuth {
        client_id: String,
        client_secret: String,
    },
    Search {
        query: String,
    },
    FetchPlaylists,
    FetchLiked,
    FetchPlaylistTracks(PlaylistId),
    FetchArtist(ArtistId),
    FetchAlbum(AlbumId),
    /// A radio of songs related to a track, for autoplay at the
    /// queue end.
    FetchRadio(TrackId),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerCommand {
    Load(Track),
    /// Downloads `Track` in the background, so a later Load can play it
    /// at once. The engine runs this at a lower priority than an
    /// active download and skips it when the track is already cached.
    Prefetch(Track),
    Pause,
    Resume,
    Seek(Duration),
    SetVolume(f32),
    Stop,
}
