//! The single application state.
//!
//! Views read this state and return actions. Only the reducer in
//! `update.rs` writes it.

use std::time::Duration;

use super::model::{Playlist, PlaylistId, SearchResults, Track};
use super::queue::Queue;

/// A value that arrives over the network.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Loadable<T> {
    #[default]
    NotAsked,
    Loading,
    Loaded(T),
    Failed(String),
}

impl<T> Loadable<T> {
    pub fn loaded(&self) -> Option<&T> {
        match self {
            Self::Loaded(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum AuthState {
    #[default]
    SignedOut,
    Verifying,
    SignedIn,
    Failed(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub enum Page {
    #[default]
    SignIn,
    Search,
    Library,
    Playlist(PlaylistId),
}

/// The cookie text the user is pasting on the sign-in page.
#[derive(Clone, Debug, Default)]
pub struct SignInState {
    pub draft: String,
}

#[derive(Clone, Debug, Default)]
pub struct SearchState {
    pub input: String,
    pub results: Loadable<SearchResults>,
}

#[derive(Clone, Debug, Default)]
pub struct LibraryState {
    pub playlists: Loadable<Vec<Playlist>>,
    pub liked: Loadable<Vec<Track>>,
    /// The tracks of the playlist page the user has open.
    pub open_playlist: Loadable<Vec<Track>>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PlayStatus {
    #[default]
    Stopped,
    Loading,
    Playing,
    Paused,
}

#[derive(Clone, Debug)]
pub struct PlaybackState {
    pub queue: Queue,
    pub status: PlayStatus,
    pub position: Duration,
    pub track_duration: Option<Duration>,
    pub volume: f32,
}

impl Default for PlaybackState {
    fn default() -> Self {
        Self {
            queue: Queue::default(),
            status: PlayStatus::default(),
            position: Duration::ZERO,
            track_duration: None,
            volume: 1.0,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub auth: AuthState,
    pub sign_in: SignInState,
    pub page: Page,
    pub search: SearchState,
    pub library: LibraryState,
    pub playback: PlaybackState,
    /// User-visible problem reports, newest last.
    pub notices: Vec<String>,
}
