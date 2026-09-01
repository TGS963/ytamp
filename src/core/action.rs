//! Everything that can happen, as data.
//!
//! Views return actions. The effect runtime and the player send result
//! actions back. The reducer in `update.rs` consumes all of them.

use std::time::Duration;

use super::model::{Playlist, PlaylistId, SearchResults, Track};
use super::state::Page;

#[derive(Clone, Debug)]
pub enum Action {
    // From the user.
    NavigatedTo(Page),
    CookieDraftChanged(String),
    AuthUserDraftChanged(String),
    CookiesSubmitted,
    SignOutRequested,
    OAuthClientIdChanged(String),
    OAuthClientSecretChanged(String),
    OAuthStartRequested,
    /// The device flow produced its verification URL; the user opens
    /// it while the runtime polls for completion.
    OAuthUrlReady(String),
    SearchInputChanged(String),
    SearchSubmitted,
    PlaylistOpened(PlaylistId),
    ContextPlayed {
        tracks: Vec<Track>,
        start: usize,
    },
    TrackQueued(Track),
    PlayToggled,
    NextPressed,
    PreviousPressed,
    SeekRequested(Duration),
    VolumeSet(f32),
    ShuffleToggled,
    RepeatCycled,
    QueuePanelToggled,
    NoticeDismissed(usize),

    // From the shell at startup.
    StoredAuthFound(crate::core::effect::AuthMethod),
    SessionRestored(crate::core::session::SavedSession),

    // From the effect runtime.
    NoticePosted(String),
    AuthVerified(Result<(), String>),
    SearchLoaded(Result<SearchResults, String>),
    PlaylistsLoaded(Result<Vec<crate::core::model::Playlist>, String>),
    LikedLoaded(Result<Vec<Track>, String>),
    PlaylistTracksLoaded(PlaylistId, Result<Vec<Track>, String>),
    /// The library cache read, delivered once whether or not either
    /// file existed. A `None` field is a cache miss for that list.
    LibraryCacheLoaded {
        playlists: Option<Vec<Playlist>>,
        liked: Option<Vec<Track>>,
    },
    /// A playlist's cached track list, delivered only on a cache hit.
    PlaylistTracksCacheLoaded(PlaylistId, Vec<Track>),

    // From the player engine.
    Player(PlayerEvent),
}

#[derive(Clone, Debug)]
pub enum PlayerEvent {
    /// The stream resolved and decoding started. Carries the real duration.
    TrackStarted {
        duration: Option<Duration>,
    },
    PositionChanged(Duration),
    TrackEnded,
    Failed(String),
}
