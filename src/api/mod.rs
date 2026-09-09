//! The YouTube Music API layer behind the app's own models.
//!
//! OAuth handles account requests. Anonymous Music search initializes lazily,
//! so its availability never determines whether a saved account can connect.

mod convert;
pub mod discovery;
pub mod history;
mod official;
mod search_parse;

use std::{sync::Arc, time::Duration};

use ytmapi_rs::auth::noauth::NoAuthToken;
use ytmapi_rs::auth::{AuthToken, OAuthToken};
use ytmapi_rs::common::{AlbumID, ArtistChannelID, PlaylistID, VideoID, YoutubeID};
use ytmapi_rs::query::search::{
    AlbumsFilter, ArtistsFilter, FilteredSearch, PlaylistsFilter, SongsFilter,
};
use ytmapi_rs::query::{
    GetAlbumQuery, GetArtistQuery, GetPlaylistDetailsQuery, GetWatchPlaylistQuery, SearchQuery,
};
use ytmapi_rs::{YtMusic, YtMusicBuilder};

use crate::core::effect::AuthMethod;
use crate::core::model::{
    AlbumId, AlbumPage, ArtistId, ArtistPage, Playlist, PlaylistId, SearchResults, Track, TrackId,
};
use crate::core::sign_in::SignInFailure;
use crate::thumbnails::preferred;

/// The most playlist-cover requests this app keeps in flight at once,
/// against the anonymous InnerTube endpoint.
const PLAYLIST_COVER_CONCURRENCY: usize = 6;

struct Session {
    data: official::DataApi,
    // Search is anonymous and lazy: a Music homepage outage must not invalidate
    // an otherwise healthy OAuth account. Failed initialization is retryable.
    innertube: tokio::sync::OnceCell<YtMusic<NoAuthToken>>,
}
#[derive(Clone)]
pub struct Api {
    session: Arc<Session>,
}
impl Api {
    pub async fn sign_in(method: &AuthMethod) -> Result<Api, SignInFailure> {
        let AuthMethod::OAuthToken(json) = method;
        let token: OAuthToken = serde_json::from_str(json).map_err(|_| SignInFailure::Expired)?;
        let api = Api {
            session: Arc::new(Session {
                data: official::DataApi::new(token).map_err(|_| SignInFailure::Connection)?,
                innertube: tokio::sync::OnceCell::new(),
            }),
        };
        tokio::time::timeout(
            std::time::Duration::from_secs(20),
            api.session.data.verify_account(),
        )
        .await
        .map_err(|_| SignInFailure::Connection)?
        .map_err(|e| SignInFailure::classify(&e))?;
        Ok(api)
    }
    pub fn needs_sign_in(&self) -> bool {
        self.session.data.needs_sign_in()
    }
    async fn anonymous(&self) -> Result<&YtMusic<NoAuthToken>, String> {
        self.session
            .innertube
            .get_or_try_init(|| async {
                tokio::time::timeout(
                    std::time::Duration::from_secs(15),
                    YtMusicBuilder::new().build(),
                )
                .await
                .map_err(|_| "Search couldn’t connect. Please try again.".to_owned())?
                .map_err(|_| "Search couldn’t connect. Please try again.".to_owned())
            })
            .await
    }
    pub async fn discovery(
        &self,
        target: &crate::core::discovery::Target,
        continuation: Option<String>,
    ) -> Result<crate::core::discovery::FeedPage, String> {
        self.session.data.discovery(target, continuation).await
    }
    pub async fn history(
        &self,
        continuation: Option<String>,
    ) -> Result<crate::core::listening_history::HistoryPage, String> {
        self.session.data.history(continuation).await
    }
    pub async fn search(&self, query: &str) -> Result<SearchResults, String> {
        filtered_search(self.anonymous().await?, query).await
    }
    pub async fn artist(&self, id: &ArtistId) -> Result<ArtistPage, String> {
        fetch_artist(self.anonymous().await?, id).await
    }
    pub async fn album(&self, id: &AlbumId) -> Result<AlbumPage, String> {
        fetch_album(self.anonymous().await?, id).await
    }
    pub async fn radio(&self, id: &TrackId) -> Result<Vec<Track>, String> {
        fetch_radio(self.anonymous().await?, id).await
    }
    pub async fn track_durations(
        &self,
        ids: &[TrackId],
    ) -> Result<Vec<(TrackId, Duration)>, String> {
        self.session.data.track_durations(ids).await
    }
    pub async fn library_playlists(&self) -> Result<Vec<Playlist>, String> {
        self.session.data.playlists().await
    }
    /// The custom cover art of every playlist in `ids` that has one,
    /// from the anonymous InnerTube playlist page. A private playlist
    /// always fails there, so it is silently missing from the result;
    /// the caller keeps that playlist's existing thumbnail.
    ///
    /// Cost: the first library load after this feature ships runs one
    /// request per playlist (61 for the user who reported this). Every
    /// later load runs one request only for a playlist that still has
    /// no custom cover.
    pub async fn playlist_covers(&self, ids: &[PlaylistId]) -> Vec<(PlaylistId, String)> {
        let mut covers = Vec::new();
        for chunk in ids.chunks(PLAYLIST_COVER_CONCURRENCY) {
            let mut set = tokio::task::JoinSet::new();
            for id in chunk {
                let api = self.clone();
                let id = id.clone();
                set.spawn(async move {
                    let cover = api.playlist_cover(&id).await;
                    (id, cover)
                });
            }
            while let Some(joined) = set.join_next().await {
                if let Ok((id, Some(url))) = joined {
                    covers.push((id, url));
                }
            }
        }
        covers
    }

    /// One playlist's custom cover, or `None` when the InnerTube page
    /// fails to load or carries no thumbnails.
    async fn playlist_cover(&self, id: &PlaylistId) -> Option<String> {
        let result = fetch_playlist_cover(self.anonymous().await.ok()?, id).await;
        result.unwrap_or_else(|error| {
            log::debug!("playlist cover fetch failed for {}: {error}", id.0);
            None
        })
    }

    pub async fn liked_songs(&self, on_page: impl FnMut(Vec<Track>, bool)) -> Result<(), String> {
        self.session.data.liked_songs(on_page).await
    }
    pub async fn playlist_tracks(
        &self,
        id: &PlaylistId,
        on_page: impl FnMut(Vec<Track>, bool),
    ) -> Result<(), String> {
        self.session.data.playlist_tracks(id, on_page).await
    }
    pub async fn rate_track(&self, id: &TrackId, liked: bool) -> Result<(), String> {
        self.session.data.rate_video(id, liked).await
    }
    pub async fn add_to_playlist(
        &self,
        playlist: &PlaylistId,
        track: &TrackId,
    ) -> Result<String, String> {
        self.session
            .data
            .insert_playlist_item(playlist, track)
            .await
    }
    pub async fn remove_from_playlist(
        &self,
        _playlist: &PlaylistId,
        item_id: &str,
    ) -> Result<(), String> {
        self.session.data.delete_playlist_item(item_id).await
    }
    pub async fn create_playlist(&self, title: &str) -> Result<Playlist, String> {
        self.session.data.insert_playlist(title).await
    }
    pub async fn delete_playlist(&self, id: &PlaylistId) -> Result<(), String> {
        self.session.data.delete_playlist(id).await
    }
}

async fn fetch_artist<A: AuthToken>(yt: &YtMusic<A>, id: &ArtistId) -> Result<ArtistPage, String> {
    let query = GetArtistQuery::new(ArtistChannelID::from_raw(id.0.as_str()));
    let artist = yt.query(query).await.map_err(readable)?;
    Ok(convert::artist_page(artist, id.clone()))
}

async fn fetch_album<A: AuthToken>(yt: &YtMusic<A>, id: &AlbumId) -> Result<AlbumPage, String> {
    let query = GetAlbumQuery::new(AlbumID::from_raw(id.0.as_str()));
    let album = yt.query(query).await.map_err(readable)?;
    Ok(convert::album_page(album, id.clone()))
}

async fn fetch_radio<A: AuthToken>(yt: &YtMusic<A>, id: &TrackId) -> Result<Vec<Track>, String> {
    let query = GetWatchPlaylistQuery::new_from_video_id(VideoID::from_raw(id.0.as_str()));
    let tracks = yt.query(query).await.map_err(readable)?;
    Ok(tracks.into_iter().map(convert::watch_track).collect())
}

/// One playlist's custom cover, from the anonymous InnerTube playlist
/// page. A private playlist fails to parse there, so its error is a
/// normal outcome, not a bug.
async fn fetch_playlist_cover<A: AuthToken>(
    yt: &YtMusic<A>,
    id: &PlaylistId,
) -> Result<Option<String>, String> {
    let browse_id = playlist_browse_id(&id.0);
    let query = GetPlaylistDetailsQuery::new(PlaylistID::from_raw(&browse_id));
    let details = yt.query(query).await.map_err(readable)?;
    Ok(preferred(&details.thumbnails))
}

/// Three filtered queries, the way youtui searches. Basic search adds
/// a top-result card whose parse breaks often; the filtered endpoints
/// skip it.
///
/// Each shelf's raw JSON is read row by row (`search_parse`), so one
/// odd row (a video mixed into the songs shelf, an audiobook mixed
/// into the albums shelf) never fails the whole shelf the way
/// ytmapi-rs's typed parse does. A songs transport error fails the
/// search; a transport error on albums or artists degrades to an
/// empty section. The typed parse still runs, only as a fallback, for
/// the rare case where the tolerant parser finds zero songs in a
/// shelf that plainly carries rows.
async fn filtered_search<A: AuthToken>(
    yt: &YtMusic<A>,
    query: &str,
) -> Result<SearchResults, String> {
    let songs: SearchQuery<'_, FilteredSearch<SongsFilter>> = query.into();
    let albums: SearchQuery<'_, FilteredSearch<AlbumsFilter>> = query.into();
    let artists: SearchQuery<'_, FilteredSearch<ArtistsFilter>> = query.into();
    let playlists: SearchQuery<'_, FilteredSearch<PlaylistsFilter>> = query.into();
    let (songs, albums, artists, playlists) = tokio::join!(
        yt.json_query::<SearchQuery<'_, FilteredSearch<SongsFilter>>>(&songs),
        yt.json_query::<SearchQuery<'_, FilteredSearch<AlbumsFilter>>>(&albums),
        yt.json_query::<SearchQuery<'_, FilteredSearch<ArtistsFilter>>>(&artists),
        yt.json_query::<SearchQuery<'_, FilteredSearch<PlaylistsFilter>>>(&playlists),
    );
    let songs_json = songs.map_err(readable)?.into_inner();
    let song_tracks = songs_or_typed_fallback(yt, query, &songs_json).await?;
    Ok(SearchResults {
        songs: song_tracks,
        albums: json_section_or_empty("albums", albums, search_parse::parse_albums),
        artists: json_section_or_empty("artists", artists, search_parse::parse_artists),
        playlists: json_section_or_empty("playlists", playlists, search_parse::parse_playlists),
    })
}

/// The tolerant song parse, or the typed ytmapi-rs parse when the
/// tolerant one finds no songs in a shelf that plainly carries rows.
/// A shelf with no rows at all is a real empty result, not a parse
/// failure, so it does not trigger the fallback.
async fn songs_or_typed_fallback<A: AuthToken>(
    yt: &YtMusic<A>,
    query: &str,
    songs_json: &serde_json::Value,
) -> Result<Vec<Track>, String> {
    let tracks = search_parse::parse_songs(songs_json);
    if !tracks.is_empty() || !search_parse::shelf_has_rows(songs_json) {
        return Ok(tracks);
    }
    log::warn!(
        "the tolerant song parse found no songs in a non-empty shelf for {query:?}, \
         falling back to the typed parse"
    );
    typed_song_fallback(yt, query).await
}

async fn typed_song_fallback<A: AuthToken>(
    yt: &YtMusic<A>,
    query: &str,
) -> Result<Vec<Track>, String> {
    let songs: SearchQuery<'_, FilteredSearch<SongsFilter>> = query.into();
    let songs = yt
        .query::<SearchQuery<'_, FilteredSearch<SongsFilter>>>(&songs)
        .await
        .map_err(readable)?;
    Ok(songs.into_iter().map(convert::song_to_track).collect())
}

/// A failed side section of a search becomes an empty list, so a
/// transport error on albums or artists never blocks the songs. A
/// successful response is read with the tolerant row-by-row parser.
fn json_section_or_empty<T>(
    name: &str,
    result: Result<ytmapi_rs::json::Json, ytmapi_rs::Error>,
    parse: impl Fn(&serde_json::Value) -> Vec<T>,
) -> Vec<T> {
    match result {
        Ok(json) => parse(&json.into_inner()),
        Err(error) => {
            log::warn!("search {name} section failed: {}", error_chain(&error));
            vec![]
        }
    }
}

/// ytmusicapi's rule: the browse endpoint takes "VL" + the playlist id,
/// unless the id already carries the prefix.
fn playlist_browse_id(playlist_id: &str) -> String {
    if playlist_id.starts_with("VL") {
        return playlist_id.to_string();
    }
    format!("VL{playlist_id}")
}

fn readable(error: ytmapi_rs::Error) -> String {
    format!("YouTube Music request failed: {}", error_chain(&error))
}

/// The error and every cause under it, on one line. A bare reqwest
/// message like "error sending request" hides the real reason (DNS,
/// TLS, a dropped connection); the chain names it.
fn error_chain(error: &dyn std::error::Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();
    while let Some(cause) = source {
        parts.push(cause.to_string());
        source = cause.source();
    }
    parts.join(" <- ")
}

#[cfg(test)]
mod tests {
    use super::playlist_browse_id;

    #[test]
    fn browse_ids_get_the_vl_prefix_once() {
        assert_eq!(playlist_browse_id("LM"), "VLLM");
        assert_eq!(playlist_browse_id("PL123"), "VLPL123");
        assert_eq!(playlist_browse_id("VLPL123"), "VLPL123");
    }
}

pub mod lyrics;
