//! The YouTube Music API layer behind the app's own models.
//!
//! Two sessions exist. The OAuth session reads account data from the
//! official YouTube Data API (the internal API rejects OAuth since
//! 2025-08) and searches the internal API anonymously. The browser
//! session runs everything on the internal API with cookies, as the
//! fallback. Every method returns the app's model types and a plain
//! error text.

mod convert;
mod official;

use std::sync::Arc;

use ytmapi_rs::auth::noauth::NoAuthToken;
use ytmapi_rs::auth::{AuthToken, BrowserToken, OAuthToken};
use ytmapi_rs::common::{AlbumID, ArtistChannelID, PlaylistID, VideoID, YoutubeID};
use ytmapi_rs::query::search::{
    AlbumsFilter, ArtistsFilter, FilteredSearch, SongsFilter, VideosFilter,
};
use ytmapi_rs::query::{
    GetAlbumQuery, GetArtistQuery, GetLibraryPlaylistsQuery, GetPlaylistTracksQuery,
    GetWatchPlaylistQuery, SearchQuery,
};
use ytmapi_rs::{YtMusic, YtMusicBuilder};

use crate::core::effect::{AuthMethod, Credentials};
use crate::core::model::{
    AlbumId, AlbumPage, ArtistId, ArtistPage, Playlist, PlaylistId, SearchResults, Track, TrackId,
};

enum Session {
    Browser(YtMusic<BrowserToken>),
    OAuth {
        data: official::DataApi,
        /// Anonymous internal-API client: search needs no sign-in.
        innertube: YtMusic<NoAuthToken>,
    },
}

#[derive(Clone)]
pub struct Api {
    session: Arc<Session>,
}

impl Api {
    /// Builds a client from the given method and proves it works with
    /// one authenticated request.
    pub async fn sign_in(method: &AuthMethod) -> Result<Api, String> {
        let session = match method {
            AuthMethod::Browser(credentials) => browser_session(credentials).await?,
            AuthMethod::OAuthToken(json) => oauth_session(json).await?,
        };
        let api = Api {
            session: Arc::new(session),
        };
        api.library_playlists()
            .await
            .map_err(|error| format!("The sign-in check failed: {error}"))?;
        Ok(api)
    }

    pub async fn search(&self, query: &str) -> Result<SearchResults, String> {
        match &*self.session {
            Session::Browser(yt) => filtered_search(yt, query).await,
            Session::OAuth { innertube, .. } => filtered_search(innertube, query).await,
        }
    }

    /// An artist's browse page: name, art, top songs, albums, singles.
    pub async fn artist(&self, id: &ArtistId) -> Result<ArtistPage, String> {
        match &*self.session {
            Session::Browser(yt) => fetch_artist(yt, id).await,
            Session::OAuth { innertube, .. } => fetch_artist(innertube, id).await,
        }
    }

    /// An album's browse page: the album and its track list.
    pub async fn album(&self, id: &AlbumId) -> Result<AlbumPage, String> {
        match &*self.session {
            Session::Browser(yt) => fetch_album(yt, id).await,
            Session::OAuth { innertube, .. } => fetch_album(innertube, id).await,
        }
    }

    /// The "Start radio" list for a track: related songs to play next.
    pub async fn radio(&self, id: &TrackId) -> Result<Vec<Track>, String> {
        match &*self.session {
            Session::Browser(yt) => fetch_radio(yt, id).await,
            Session::OAuth { innertube, .. } => fetch_radio(innertube, id).await,
        }
    }

    pub async fn library_playlists(&self) -> Result<Vec<Playlist>, String> {
        match &*self.session {
            Session::Browser(yt) => {
                let playlists = yt
                    .query::<GetLibraryPlaylistsQuery>(&GetLibraryPlaylistsQuery)
                    .await
                    .map_err(readable)?;
                Ok(playlists
                    .into_iter()
                    .map(convert::library_playlist)
                    .collect())
            }
            Session::OAuth { data, .. } => data.playlists().await,
        }
    }

    /// Streams the liked-songs list to `on_page`. The OAuth session
    /// hands out one page at a time as the network answers; the
    /// browser session has no paged endpoint, so it fetches the whole
    /// list first and delivers it as one finished page.
    pub async fn liked_songs(&self, on_page: impl FnMut(Vec<Track>, bool)) -> Result<(), String> {
        match &*self.session {
            Session::Browser(_) => deliver_whole_list(self.browser_liked_songs().await, on_page),
            Session::OAuth { data, .. } => data.liked_songs(on_page).await,
        }
    }

    /// Streams one playlist's tracks to `on_page`, the same way as
    /// [`Self::liked_songs`].
    pub async fn playlist_tracks(
        &self,
        id: &PlaylistId,
        on_page: impl FnMut(Vec<Track>, bool),
    ) -> Result<(), String> {
        match &*self.session {
            Session::Browser(_) => {
                deliver_whole_list(self.browser_playlist_tracks(&id.0).await, on_page)
            }
            Session::OAuth { data, .. } => data.playlist_tracks(id, on_page).await,
        }
    }

    /// The Liked Music playlist, "LM", per ytmusicapi's get_liked_songs.
    /// A fresh account has an empty one, which parses as a missing
    /// shelf, so that one failure counts as zero songs.
    async fn browser_liked_songs(&self) -> Result<Vec<Track>, String> {
        match self.browser_playlist_tracks("LM").await {
            Ok(tracks) => Ok(tracks),
            Err(error) if error.contains("not found in Api response") => {
                log::info!("liked songs parse failed, treating as empty: {error}");
                Ok(vec![])
            }
            Err(error) => Err(error),
        }
    }

    async fn browser_playlist_tracks(&self, id: &str) -> Result<Vec<Track>, String> {
        let Session::Browser(yt) = &*self.session else {
            return Err("not a browser session".to_string());
        };
        let browse_id = playlist_browse_id(id);
        let query = GetPlaylistTracksQuery::new(PlaylistID::from_raw(&browse_id));
        let items = yt
            .query::<GetPlaylistTracksQuery>(&query)
            .await
            .map_err(readable)?;
        Ok(items
            .into_iter()
            .filter_map(convert::playlist_item_to_track)
            .collect())
    }
}

/// Adapts a single-shot list result to the paged `on_page` shape, so
/// the browser session's one-call fetch fits the same streaming
/// interface as the OAuth session's paged one.
fn deliver_whole_list(
    result: Result<Vec<Track>, String>,
    mut on_page: impl FnMut(Vec<Track>, bool),
) -> Result<(), String> {
    let tracks = result?;
    on_page(tracks, true);
    Ok(())
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

/// Three filtered queries, the way youtui searches. Basic search adds
/// a top-result card whose parse breaks often; the filtered endpoints
/// skip it. Songs are the core result: a song failure fails the
/// search, a failure of the other two degrades to an empty section.
async fn filtered_search<A: AuthToken>(
    yt: &YtMusic<A>,
    query: &str,
) -> Result<SearchResults, String> {
    let songs: SearchQuery<'_, FilteredSearch<SongsFilter>> = query.into();
    let albums: SearchQuery<'_, FilteredSearch<AlbumsFilter>> = query.into();
    let artists: SearchQuery<'_, FilteredSearch<ArtistsFilter>> = query.into();
    let (songs, albums, artists) = tokio::join!(
        yt.query::<SearchQuery<'_, FilteredSearch<SongsFilter>>>(&songs),
        yt.query::<SearchQuery<'_, FilteredSearch<AlbumsFilter>>>(&albums),
        yt.query::<SearchQuery<'_, FilteredSearch<ArtistsFilter>>>(&artists),
    );
    let song_tracks = match songs {
        Ok(songs) => songs.into_iter().map(convert::song_to_track).collect(),
        Err(error) => video_fallback_search(yt, query, error).await?,
    };
    Ok(convert::search_results_from_tracks(
        song_tracks,
        section_or_empty("albums", albums),
        section_or_empty("artists", artists),
    ))
}

/// One malformed row fails the whole song-search parse in ytmapi-rs.
/// The videos filter has a simpler shape, so it answers instead, with
/// channel names in place of artists.
async fn video_fallback_search<A: AuthToken>(
    yt: &YtMusic<A>,
    query: &str,
    song_error: ytmapi_rs::Error,
) -> Result<Vec<Track>, String> {
    log::warn!(
        "song search parse failed, using videos: {}",
        error_chain(&song_error)
    );
    let videos: SearchQuery<'_, FilteredSearch<VideosFilter>> = query.into();
    let videos = yt
        .query::<SearchQuery<'_, FilteredSearch<VideosFilter>>>(&videos)
        .await
        .map_err(readable)?;
    Ok(videos
        .into_iter()
        .filter_map(convert::video_to_track)
        .collect())
}

async fn browser_session(credentials: &Credentials) -> Result<Session, String> {
    let client = session_client(credentials)?;
    let yt = YtMusicBuilder::new_with_client(client)
        .with_browser_token_cookie(credentials.cookies.clone())
        .build()
        .await
        .map_err(|error| format!("The cookies did not work: {}", error_chain(&error)))?;
    Ok(Session::Browser(yt))
}

async fn oauth_session(token_json: &str) -> Result<Session, String> {
    let token: OAuthToken = serde_json::from_str(token_json)
        .map_err(|error| format!("The stored OAuth token does not parse: {error}"))?;
    let innertube = YtMusicBuilder::new().build().await.map_err(|error| {
        format!(
            "The search session failed to build: {}",
            error_chain(&error)
        )
    })?;
    Ok(Session::OAuth {
        data: official::DataApi::new(token)?,
        innertube,
    })
}

/// An HTTP client that replays the browser's request headers on every
/// call, the way ytmusicapi does. Account selection (X-Goog-AuthUser,
/// and X-Goog-PageId for brand accounts) and consistency checks ride
/// on these headers, and without them YouTube answers as signed out.
fn session_client(credentials: &Credentials) -> Result<ytmapi_rs::Client, String> {
    let mut headers = reqwest::header::HeaderMap::new();
    for (name, value) in &credentials.headers {
        insert_header(&mut headers, name, value);
    }
    if !headers.contains_key("x-goog-authuser") {
        insert_header(&mut headers, "x-goog-authuser", &credentials.authuser);
    }
    let client = reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|error| format!("The HTTP client failed to build: {error}"))?;
    Ok(ytmapi_rs::Client::new_from_reqwest_client(client))
}

/// A header that does not parse gets logged and skipped, so one odd
/// browser header cannot block the sign-in.
fn insert_header(headers: &mut reqwest::header::HeaderMap, name: &str, value: &str) {
    let name = match reqwest::header::HeaderName::try_from(name) {
        Ok(name) => name,
        Err(_) => return log::warn!("skipping header with an invalid name: {name}"),
    };
    match reqwest::header::HeaderValue::from_str(value) {
        Ok(value) => {
            headers.insert(name, value);
        }
        Err(_) => log::warn!("skipping header with an invalid value: {name}"),
    }
}

/// A failed side section of a search becomes an empty list, so a
/// brittle album or artist parse never blocks the songs.
fn section_or_empty<T>(name: &str, result: Result<Vec<T>, ytmapi_rs::Error>) -> Vec<T> {
    result.unwrap_or_else(|error| {
        log::warn!("search {name} section failed: {}", error_chain(&error));
        vec![]
    })
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
