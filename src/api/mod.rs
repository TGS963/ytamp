//! The YouTube Music API layer: ytmapi-rs behind the app's own models.
//!
//! Every method returns the app's model types and a plain error text,
//! so the reducer and the views never see a ytmapi-rs type.

mod convert;

use ytmapi_rs::YtMusic;
use ytmapi_rs::auth::BrowserToken;
use ytmapi_rs::common::{PlaylistID, YoutubeID};

use crate::core::model::{Playlist, PlaylistId, SearchResults, Track};

#[derive(Clone)]
pub struct Api {
    yt: YtMusic<BrowserToken>,
}

impl Api {
    /// Builds a client from the pasted cookies and proves it works with
    /// one authenticated request.
    pub async fn sign_in(cookies: &str) -> Result<Api, String> {
        let yt = YtMusic::from_cookie(cookies)
            .await
            .map_err(|error| format!("The cookies did not work: {}", error_chain(&error)))?;
        let api = Api { yt };
        api.library_playlists()
            .await
            .map_err(|error| format!("The sign-in check failed: {error}"))?;
        Ok(api)
    }

    pub async fn search(&self, query: &str) -> Result<SearchResults, String> {
        let query: ytmapi_rs::query::SearchQuery<'_, ytmapi_rs::query::search::BasicSearch> =
            query.into();
        let results = self.yt.query(query).await.map_err(readable)?;
        Ok(convert::search_results(results))
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

/// ytmusicapi's rule: the browse endpoint takes "VL" + the playlist id,
/// unless the id already carries the prefix.
fn playlist_browse_id(playlist_id: &str) -> String {
    if playlist_id.starts_with("VL") {
        return playlist_id.to_string();
    }
    format!("VL{playlist_id}")
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
