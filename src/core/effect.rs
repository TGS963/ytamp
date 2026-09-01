//! Everything the app does to the outside world, as data.
//!
//! The reducer returns effects. The runtime executes them and sends
//! result actions back. This keeps the reducer honest and testable.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::model::{PlaylistId, Track};

/// What a YouTube Music session needs: the Cookie header, and the
/// X-Goog-AuthUser index that picks the account inside the session.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Credentials {
    pub cookies: String,
    pub authuser: String,
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
    VerifyAuth(Credentials),
    Search { query: String },
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
