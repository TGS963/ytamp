//! Everything the app does to the outside world, as data.
//!
//! The reducer returns effects. The runtime executes them and sends
//! result actions back. This keeps the reducer honest and testable.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::model::{PlaylistId, Track};

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
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlayerCommand {
    Load(Track),
    Pause,
    Resume,
    Seek(Duration),
    SetVolume(f32),
    Stop,
}
