//! The YouTube Music API layer: ytmapi-rs behind the app's own models.
//!
//! Every method returns the app's model types and a plain error text,
//! so the reducer and the views never see a ytmapi-rs type. The
//! session authenticates with the OAuth device flow (preferred) or
//! with browser cookies (the fallback).

mod convert;

use std::sync::Arc;

use tokio::sync::RwLock;
use ytmapi_rs::auth::{BrowserToken, OAuthToken};
use ytmapi_rs::common::{PlaylistID, YoutubeID};
use ytmapi_rs::query::search::{AlbumsFilter, ArtistsFilter, FilteredSearch, SongsFilter};
use ytmapi_rs::query::{GetLibraryPlaylistsQuery, GetPlaylistTracksQuery, Query, SearchQuery};
use ytmapi_rs::{YtMusic, YtMusicBuilder};

use crate::core::effect::{AuthMethod, Credentials};
use crate::core::model::{Playlist, PlaylistId, SearchResults, Track};

enum Session {
    Browser(YtMusic<BrowserToken>),
    /// The lock exists for token refresh, which needs exclusive
    /// access, the same pattern youtui uses.
    OAuth(RwLock<YtMusic<OAuthToken>>),
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

    /// Three filtered queries, the way youtui searches. Basic search
    /// adds a top-result card whose parse breaks often; the filtered
    /// endpoints skip it. Songs are the core result: a song failure
    /// fails the search, a failure of the other two degrades to an
    /// empty section and a log line.
    pub async fn search(&self, query: &str) -> Result<SearchResults, String> {
        let songs: SearchQuery<'_, FilteredSearch<SongsFilter>> = query.into();
        let albums: SearchQuery<'_, FilteredSearch<AlbumsFilter>> = query.into();
        let artists: SearchQuery<'_, FilteredSearch<ArtistsFilter>> = query.into();
        let (songs, albums, artists) =
            tokio::join!(self.run(songs), self.run(albums), self.run(artists));
        Ok(convert::search_results(
            songs?,
            section_or_empty("albums", albums),
            section_or_empty("artists", artists),
        ))
    }

    pub async fn library_playlists(&self) -> Result<Vec<Playlist>, String> {
        let playlists = self.run(GetLibraryPlaylistsQuery).await?;
        Ok(playlists
            .into_iter()
            .map(convert::library_playlist)
            .collect())
    }

    /// The Liked Music playlist, "LM", per ytmusicapi's get_liked_songs.
    /// A fresh account has an empty one, which parses as a missing
    /// shelf, so that one failure counts as zero songs.
    pub async fn liked_songs(&self) -> Result<Vec<Track>, String> {
        match self.raw_playlist_tracks("LM").await {
            Ok(tracks) => Ok(tracks),
            Err(error) if error.contains("not found in Api response") => {
                log::info!("liked songs parse failed, treating as empty: {error}");
                Ok(vec![])
            }
            Err(error) => Err(error),
        }
    }

    pub async fn playlist_tracks(&self, id: &PlaylistId) -> Result<Vec<Track>, String> {
        self.raw_playlist_tracks(&id.0).await
    }

    async fn raw_playlist_tracks(&self, id: &str) -> Result<Vec<Track>, String> {
        let browse_id = playlist_browse_id(id);
        let query = GetPlaylistTracksQuery::new(PlaylistID::from_raw(&browse_id));
        let items = self.run(query).await?;
        Ok(items
            .into_iter()
            .filter_map(convert::playlist_item_to_track)
            .collect())
    }

    /// Runs one query against the session. An expired OAuth token
    /// refreshes once and the query retries, per the youtui pattern.
    async fn run<Q, O>(&self, query: Q) -> Result<O, String>
    where
        Q: Query<BrowserToken, Output = O> + Query<OAuthToken, Output = O>,
    {
        match &*self.session {
            Session::Browser(yt) => yt.query::<Q>(&query).await.map_err(readable),
            Session::OAuth(lock) => {
                let result = lock.read().await.query::<Q>(&query).await;
                let error = match result {
                    Ok(output) => return Ok(output),
                    Err(error) => error,
                };
                let message = format!("YouTube Music request failed: {}", error_chain(&error));
                match error.into_kind() {
                    ytmapi_rs::error::ErrorKind::OAuthTokenExpired { .. } => {
                        refresh_token(lock).await?;
                        lock.read().await.query::<Q>(&query).await.map_err(readable)
                    }
                    _ => Err(message),
                }
            }
        }
    }
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
    let yt = YtMusicBuilder::new()
        .with_auth_token(token)
        .build()
        .map_err(|error| format!("The OAuth session failed to build: {}", error_chain(&error)))?;
    Ok(Session::OAuth(RwLock::new(yt)))
}

/// Refreshes the OAuth token under the write lock and persists it, so
/// the next start skips the expired token.
async fn refresh_token(lock: &RwLock<YtMusic<OAuthToken>>) -> Result<(), String> {
    let mut yt = lock.write().await;
    let token = yt
        .refresh_token()
        .await
        .map_err(|error| format!("The OAuth refresh failed: {}", error_chain(&error)))?;
    let json = serde_json::to_string(&token)
        .map_err(|error| format!("The refreshed token does not serialize: {error}"))?;
    if let Err(error) = crate::auth::save_oauth_token(&json) {
        log::warn!("could not persist the refreshed OAuth token: {error}");
    }
    Ok(())
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
fn section_or_empty<T>(name: &str, result: Result<Vec<T>, String>) -> Vec<T> {
    result.unwrap_or_else(|error| {
        log::warn!("search {name} section failed: {error}");
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
