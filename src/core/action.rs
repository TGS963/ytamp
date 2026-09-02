//! Everything that can happen, as data.
//!
//! Views return actions. The effect runtime and the player send result
//! actions back. The reducer in `update.rs` consumes all of them.

use std::time::Duration;

use super::model::{
    AlbumId, AlbumPage, ArtistId, ArtistPage, Playlist, PlaylistId, SearchResults, Track, TrackId,
};
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
    ArtistOpened(ArtistId),
    /// A click on an artist name with no channel id: searches for
    /// the artist by name instead of opening a page directly.
    ArtistSearchRequested(String),
    AlbumOpened(AlbumId),
    /// Returns to the page Back left, popping `State.history`.
    BackPressed,
    ContextPlayed {
        tracks: Vec<Track>,
        start: usize,
    },
    TrackQueued(Track),
    /// The pointer rested on a track row long enough to warm its
    /// cache. The view emits this once per dwell.
    TrackHovered(Track),
    PlayToggled,
    NextPressed,
    PreviousPressed,
    SeekRequested(Duration),
    VolumeSet(f32),
    ShuffleToggled,
    RepeatCycled,
    AutoplayToggled,
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
    ArtistLoaded(ArtistId, Result<ArtistPage, String>),
    AlbumLoaded(AlbumId, Result<AlbumPage, String>),
    /// The radio fetched for the track that ended the queue, once
    /// autoplay ran out of upcoming tracks.
    RadioLoaded(TrackId, Result<Vec<Track>, String>),
    /// One page of the liked-songs list. `finished` marks the last
    /// page of the stream.
    LikedPageLoaded {
        tracks: Vec<Track>,
        finished: bool,
    },
    /// One page of an open playlist's tracks, the same way as
    /// `LikedPageLoaded`.
    PlaylistTracksPageLoaded {
        id: PlaylistId,
        tracks: Vec<Track>,
        finished: bool,
    },
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
