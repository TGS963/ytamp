# Artist pages, album pages, and autoplay

## Goal

The user opens an artist or an album from search, from a track's
album, or from another page, and plays from there. When the queue
ends, the player continues with a radio of related songs, unless the
user turns autoplay off.

## Facts from ytmapi-rs 0.3.3 (checked in the vendored source)

- `GetArtistQuery::new(channel_id)` -> `GetArtist { name, thumbnails,
  top_releases: { songs: Option<GetArtistSongs { results:
  Vec<ArtistSong>, browse_id }>, albums: Option<GetArtistAlbums>,
  singles: Option<GetArtistAlbums>, .. }, radio_id, .. }`.
  `ArtistSong { video_id, title, artists: Vec<ParsedSongArtist>,
  album: ParsedSongAlbum, .. }` has no duration and no thumbnails.
- `GetAlbumQuery::new(album_id)` -> `GetAlbum { title, thumbnails,
  artists, year, tracks: Vec<AlbumSong { video_id, track_no,
  duration, title, .. }>, audio_playlist_id, .. }`. Album songs carry
  no thumbnails: use the album's.
- `GetWatchPlaylistQuery::new_from_video_id(id)` -> a list of
  `WatchPlaylistTrack { title, author, duration, thumbnails,
  video_id }`. This is the "Start radio" list for a song.
- All three work on the anonymous `YtMusic<NoAuthToken>` session the
  OAuth path already uses for search. The Browser session uses the
  same queries.
- Rule from the reference repos: read `ytmapi-rs` parse types before
  a conversion, never guess a JSON shape.

## Design

### Phase A: API layer (`src/api/`, `src/core/model.rs`)

- Models: `ArtistPage { id: ArtistId, name, thumbnail_url, top_songs:
  Vec<Track>, albums: Vec<Album>, singles: Vec<Album> }`, `AlbumPage
  { album: Album, tracks: Vec<Track> }`. `Album` exists. Add
  `Track.album_id: Option<AlbumId>` so a track row can open its album
  later. Bump `library_cache::CACHE_VERSION`, because `Track`
  changes shape.
- `Api::artist(&ArtistId) -> Result<ArtistPage, String>`,
  `Api::album(&AlbumId) -> Result<AlbumPage, String>`,
  `Api::radio(&TrackId) -> Result<Vec<Track>, String>`. Pure
  conversions in `src/api/convert.rs` with unit tests on constructed
  ytmapi-rs values where the types allow construction, otherwise on
  the pure helpers.
- `examples/browse_probe.rs`: takes an artist channel id, an album id,
  and a video id, prints the three results. Run it once against the
  real API.

### Phase B: pages and navigation (`src/core/`, `src/ui/`)

- `Page::Artist(ArtistId)`, `Page::Album(AlbumId)`. `State.browse {
  artist: Loadable<ArtistPage>, album: Loadable<AlbumPage> }`.
- Actions: `ArtistOpened(ArtistId)`, `AlbumOpened(AlbumId)`,
  `ArtistLoaded(ArtistId, Result)`, `AlbumLoaded(AlbumId, Result)`,
  `BackPressed`. A result for a page the user left is ignored, the
  same guard as playlists.
- Navigation history: `State.history: Vec<Page>`. Opening a page
  pushes the current page. `BackPressed` pops. A Back button shows in
  the page header when the history is not empty. The sidebar items
  clear the history.
- Views: `src/ui/artist.rs` (header with art and name, top songs as a
  track list, albums and singles as rows that open the album),
  `src/ui/album.rs` (header with art, title, artists, year, tracks as
  a track list, a Play button that plays the whole album). Search
  results: album rows open the album, artist rows open the artist.
  All rows through `row_frame`, all sizes and colors from the theme.
- Effects: `ApiRequest::FetchArtist`, `FetchAlbum`. The runtime maps
  them like the other requests.

### Phase C: autoplay (`src/core/`, `src/ui/player_bar.rs`)

- `PlaybackState.autoplay: bool`, default true, saved in the session.
- On `TrackEnded` with no next track and autoplay on: status
  `Loading`, emit `ApiRequest::FetchRadio(current track id)`.
  `RadioLoaded(Result<Vec<Track>>)`: drop tracks already in the
  queue, `Queue::extend_context(tracks)`, then advance and load. On
  an empty or failed radio: `Stopped`, a notice on failure.
- A player bar toggle for autoplay, a theme-role icon like the
  shuffle and repeat toggles.
- Reducer tests: autoplay on at the end fetches a radio; autoplay off
  stops; radio tracks append and play; duplicates drop; a radio
  result after the user started another context is ignored (guard on
  the requesting track id).

## Phases and reviews

| Phase | Scope | Files |
|-------|-------|-------|
| A | Models, API methods, conversions, probe | `src/api/*`, `src/core/model.rs`, `src/library_cache.rs`, `examples/` |
| B | Pages, navigation, history, views | `src/core/*`, `src/ui/*`, `src/runtime.rs` |
| C | Autoplay | `src/core/*`, `src/ui/player_bar.rs`, `src/core/session.rs` |

Each phase gets an adversarial review before the next starts.
