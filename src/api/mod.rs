//! The YouTube Music API layer: ytmapi-rs behind the app's own models.
//!
//! Every method returns the app's model types and a plain error text,
//! so the reducer and the views never see a ytmapi-rs type.

mod convert;

use ytmapi_rs::YtMusic;
use ytmapi_rs::auth::BrowserToken;
use ytmapi_rs::common::{PlaylistID, YoutubeID};

use crate::core::model::{Playlist, PlaylistId, SearchResults, Track};

/// The playlist id YouTube Music gives every account's liked songs.
const LIKED_SONGS_PLAYLIST: &str = "LM";

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

    pub async fn liked_songs(&self) -> Result<Vec<Track>, String> {
        self.playlist_tracks(&PlaylistId(LIKED_SONGS_PLAYLIST.to_string()))
            .await
    }

    pub async fn playlist_tracks(&self, id: &PlaylistId) -> Result<Vec<Track>, String> {
        let items = self
            .yt
            .get_playlist_tracks(PlaylistID::from_raw(&id.0))
            .await
            .map_err(readable)?;
        Ok(items
            .into_iter()
            .filter_map(convert::playlist_item_to_track)
            .collect())
    }
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
