//! Everything the app does to the outside world, as data.
//!
//! The reducer returns effects. The runtime executes them and sends
//! result actions back. This keeps the reducer honest and testable.

use std::time::Duration;

use super::model::{PlaylistId, Track};

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Api(ApiRequest),
    Player(PlayerCommand),
    SaveCookies(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum ApiRequest {
    VerifyAuth { cookies: String },
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
