//! Dumps the raw JSON of the three filtered searches (songs, albums,
//! artists) for a query, so a broken row can get inspected offline.
//! Usage: cargo run --example search_dump ["query"]

use serde_json::Value;
use ytmapi_rs::YtMusicBuilder;
use ytmapi_rs::auth::noauth::NoAuthToken;
use ytmapi_rs::query::SearchQuery;
use ytmapi_rs::query::search::{AlbumsFilter, ArtistsFilter, FilteredSearch, SongsFilter};

/// Below this size, the whole response gets written. At or above it,
/// only the section list survives, since that is all the parser
/// reads.
const MAX_WHOLE_FILE_BYTES: usize = 300_000;

#[tokio::main]
async fn main() {
    let query = std::env::args().nth(1).unwrap_or_else(|| "Coldplay".into());
    let yt = YtMusicBuilder::new()
        .build()
        .await
        .expect("anonymous client");

    let songs: SearchQuery<'_, FilteredSearch<SongsFilter>> = query.as_str().into();
    let albums: SearchQuery<'_, FilteredSearch<AlbumsFilter>> = query.as_str().into();
    let artists: SearchQuery<'_, FilteredSearch<ArtistsFilter>> = query.as_str().into();

    let json = run_query(&yt, songs).await;
    write_fixture(&json, &query, "songs");
    let json = run_query(&yt, albums).await;
    write_fixture(&json, &query, "albums");
    let json = run_query(&yt, artists).await;
    write_fixture(&json, &query, "artists");
}

async fn run_query<Q: ytmapi_rs::query::Query<NoAuthToken>>(
    yt: &ytmapi_rs::YtMusic<NoAuthToken>,
    query: Q,
) -> Value {
    yt.json_query(query)
        .await
        .expect("search query failed")
        .into_inner()
}

fn write_fixture(json: &Value, query: &str, filter: &str) {
    let slug = query.to_lowercase().replace(' ', "_");
    let path = format!("tests/fixtures/search_{filter}_{slug}.json");
    let text = fixture_text(json);
    if text.len() >= MAX_WHOLE_FILE_BYTES {
        eprintln!("warning: {path} is still {} bytes", text.len());
    }
    std::fs::write(&path, &text).expect("write fixture");
    println!("wrote {path} ({} bytes)", text.len());
}

/// The compact JSON text for a fixture: the whole response when it
/// fits the size budget, else the section list alone, since that is
/// all the parser reads.
fn fixture_text(json: &Value) -> String {
    let whole = serde_json::to_string(json).expect("serialize");
    if whole.len() < MAX_WHOLE_FILE_BYTES {
        return whole;
    }
    match section_list(json) {
        Some(sections) => serde_json::to_string(sections).expect("serialize"),
        None => whole,
    }
}

fn section_list(json: &Value) -> Option<&Value> {
    json.pointer(
        "/contents/tabbedSearchResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents",
    )
}
