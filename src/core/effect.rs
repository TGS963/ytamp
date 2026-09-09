//! Everything the app does to the outside world, as data.
//!
//! The reducer returns effects. The runtime executes them and sends
//! result actions back. This keeps the reducer honest and testable.

use std::path::PathBuf;
use std::time::Duration;

use super::model::{AlbumId, ArtistId, Playlist, PlaylistId, Track, TrackId};

/// One way to authenticate the YouTube Music session.
#[derive(Clone, Debug, PartialEq)]
pub enum AuthMethod {
    /// A serialized ytmapi-rs OAuth token, from the device flow.
    /// It embeds the client id and secret, so it refreshes itself.
    OAuthToken(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Api(ApiRequest),
    Player(PlayerCommand),
    FetchLyrics(Option<(u64, TrackId)>),
    LoadStoredAuth,
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
    /// Decodes a skin on the blocking pool: the built-in skin for
    /// `None`, or the named skin from the skins folder.
    LoadSkin(Option<String>),
    /// Copies a dropped skin file into the skins folder, on the
    /// blocking pool.
    InstallSkin(PathBuf),
    /// Lists the skins folder again, on the blocking pool.
    RefreshSkinList,
}

/// One fresh network result to persist to the library cache.
#[derive(Clone, Debug, PartialEq)]
pub enum LibraryCacheWrite {
    Discovery(super::discovery::FeedPage),
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
        request_id: u64,
        query: String,
    },
    FetchDiscovery {
        request_id: u64,
        target: super::discovery::Target,
        continuation: Option<String>,
    },
    FetchHistory {
        request_id: u64,
        continuation: Option<String>,
    },
    FetchPlaylists,
    FetchLiked,
    FetchPlaylistTracks(PlaylistId),
    FetchArtist(ArtistId),
    FetchAlbum(AlbumId),
    /// A radio of songs related to a track, for autoplay at the
    /// queue end.
    FetchRadio(TrackId),
    StartRadio {
        request_id: u64,
        playback_generation: u64,
        seed: Track,
    },
    /// The custom cover art of every playlist in the list, for a
    /// playlist whose thumbnail is not one already.
    FetchPlaylistCovers(Vec<PlaylistId>),
    /// Likes or unlikes a track on the server. The reducer has already
    /// applied the change to the liked list.
    RateTrack {
        id: TrackId,
        liked: bool,
    },
    /// Adds a track to a playlist on the server. The reducer has
    /// already raised the playlist's shown track count.
    AddToPlaylist {
        playlist: PlaylistId,
        track: Track,
    },
    /// Removes one row from a playlist on the server, by its item id.
    /// The reducer has already dropped the row and lowered the count.
    RemoveFromPlaylist {
        playlist: PlaylistId,
        item_id: String,
    },
    /// Creates a private playlist with the given title.
    CreatePlaylist(u64, String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerCommand {
    Load(Track),
    /// Downloads `Track` in the background, so a later Load can play it
    /// at once. The engine runs this at a lower priority than an
    /// active download and skips it when the track is already cached.
    Prefetch(Track),
    /// Prepare precisely the next track; None cancels obsolete preparation.
    PrepareNext(Option<Track>),
    Pause,
    Resume,
    Seek(Duration),
    SetVolume(f32),
    SetBalance(f32),
    SetEqualizer(super::equalizer::Parameters),
    Stop,
}
