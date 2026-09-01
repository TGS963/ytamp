//! The YouTube Music API layer: ytmapi-rs behind the app's own models.
//!
//! Every method returns the app's model types and a plain error text,
//! so the reducer and the views never see a ytmapi-rs type.

mod convert;

use ytmapi_rs::auth::BrowserToken;
use ytmapi_rs::common::{PlaylistID, YoutubeID};
use ytmapi_rs::{YtMusic, YtMusicBuilder};

use crate::core::effect::Credentials;
use crate::core::model::{Playlist, PlaylistId, SearchResults, Track};

#[derive(Clone)]
pub struct Api {
    yt: YtMusic<BrowserToken>,
}

impl Api {
    /// Builds a client from the pasted credentials and proves it works
    /// with one authenticated request. Every request carries the
    /// X-Goog-AuthUser header: without it, a browser session with
    /// several Google accounts answers for account 0, and the library
    /// comes back from the wrong account.
    pub async fn sign_in(credentials: &Credentials) -> Result<Api, String> {
        let client = session_client(credentials)?;
        let yt = YtMusicBuilder::new_with_client(client)
            .with_browser_token_cookie(credentials.cookies.clone())
            .build()
            .await
            .map_err(|error| format!("The cookies did not work: {}", error_chain(&error)))?;
        let api = Api { yt };
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
        let (songs, albums, artists) = tokio::join!(
            self.yt.search_songs(query),
            self.yt.search_albums(query),
            self.yt.search_artists(query),
        );
        Ok(convert::search_results(
            songs.map_err(readable)?,
            section_or_empty("albums", albums),
            section_or_empty("artists", artists),
        ))
    }

    pub async fn library_playlists(&self) -> Result<Vec<Playlist>, String> {
        let playlists = self.yt.get_library_playlists().await.map_err(readable)?;
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
            Err(error) => empty_playlist_or_error(error),
        }
    }

    pub async fn playlist_tracks(&self, id: &PlaylistId) -> Result<Vec<Track>, String> {
        self.raw_playlist_tracks(&id.0).await.map_err(readable)
    }

    async fn raw_playlist_tracks(&self, id: &str) -> Result<Vec<Track>, ytmapi_rs::Error> {
        let browse_id = playlist_browse_id(id);
        let items = self
            .yt
            .get_playlist_tracks(PlaylistID::from_raw(&browse_id))
            .await?;
        Ok(items
            .into_iter()
            .filter_map(convert::playlist_item_to_track)
            .collect())
    }
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
    readable_ref(&error)
}

/// An empty playlist carries no music shelf, which the parser reports
/// as a JSON parsing error. Treat that as zero songs and keep every
/// other failure an error.
fn empty_playlist_or_error(error: ytmapi_rs::Error) -> Result<Vec<Track>, String> {
    let message = readable_ref(&error);
    match error.into_kind() {
        ytmapi_rs::error::ErrorKind::JsonParsing(_) => {
            log::info!("liked songs parse failed, treating as empty: {message}");
            Ok(vec![])
        }
        _ => Err(message),
    }
}

fn readable_ref(error: &ytmapi_rs::Error) -> String {
    format!("YouTube Music request failed: {}", error_chain(error))
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
