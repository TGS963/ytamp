# Library actions

## Goal

The user likes and unlikes a track, adds a track to a playlist,
removes a track from a playlist, and creates a playlist, from any
track row and from the player bar. The library updates at once and
the network call follows.

## Facts

- The stored OAuth token has the scope
  `https://www.googleapis.com/auth/youtube`, so writes are allowed.
- Official Data API v3 writes, 50 quota units each of the 10,000 per
  day: `videos.rate?id=..&rating=like|none`, `playlistItems.insert`
  with `{snippet: {playlistId, resourceId: {kind: "youtube#video",
  videoId}}}`, `playlistItems.delete?id=<playlistItemId>`,
  `playlists.insert` with `{snippet: {title}, status: {privacyStatus:
  "private"}}`, `playlists.delete?id=..`.
- A removal needs the playlist item id. `parse_playlist_item` drops
  it today. `Track` gains `playlist_item_id: Option<String>`, set only
  for tracks that came from a playlist page.
- The liked state of a track is its membership in the loaded liked
  list, which is the `LL` playlist. No extra request.
- The cookie session has ytmapi-rs queries for the same actions:
  `RateSongQuery`, `AddPlaylistItemsQuery`, `RemovePlaylistItemsQuery`,
  `CreatePlaylistQuery`, `DeletePlaylistQuery`. Implement them too,
  through the same `Api` methods.

## Design

### Phase A: API layer

- `Api::rate_track(&TrackId, liked: bool)`, `Api::add_to_playlist(&PlaylistId,
  &TrackId) -> Result<String>` (the new item id),
  `Api::remove_from_playlist(item_id: &str)`,
  `Api::create_playlist(title) -> Result<Playlist>`,
  `Api::delete_playlist(&PlaylistId)`. OAuth session through
  `official::DataApi` with the existing bearer and refresh path, plus
  POST and DELETE helpers next to `get_json`. Browser session through
  ytmapi-rs.
- `Track.playlist_item_id: Option<String>` with `#[serde(default)]`,
  filled in `parse_playlist_item` from `/id`. Bump `CACHE_VERSION`.
- `examples/write_probe.rs`, opt-in with `--write`: creates a playlist
  "ytamp write probe", adds one track, lists it, removes the item,
  deletes the playlist, and prints each step. Run once.

### Phase B: core and UI

- Actions: `TrackLiked(Track)`, `TrackUnliked(TrackId)`,
  `TrackAddedToPlaylist { playlist, track }`,
  `TrackRemovedFromPlaylist { playlist, item_id, track_id }`,
  `PlaylistCreateRequested(title)`, `PlaylistCreated(Result<Playlist>)`,
  `LibraryWriteFinished(Result<(), String>)` for the fire-and-forget
  writes, `PlaylistItemAdded { playlist, track, item_id }`.
- Optimistic reducer: a like inserts the track at the front of the
  liked list and a `Rate` effect follows. An unlike removes it. A
  removal drops the row from the open playlist and lowers the count.
  An add raises the count, and when the playlist page is open, appends
  the row with the item id once `PlaylistItemAdded` arrives. A failed
  write posts a notice and refetches the affected list.
- UI: right-click on a track row opens a context menu with Like or
  Unlike, Add to queue, Add to playlist (a submenu with the user's
  playlists and "New playlist..."), and Remove from this playlist on
  a playlist page. The player bar gets a heart toggle for the current
  track. A small modal with a text field creates a playlist.
- The Winamp playlist window's REM menu gets "Remove from playlist"
  when the queue context is a playlist page: deferred, note only.

## Phases

| Phase | Scope | Files |
|-------|-------|-------|
| A | API methods, item ids, write probe | `src/api/*`, `src/core/model.rs`, `src/library_cache.rs`, `examples/` |
| B | Actions, optimistic reducer, context menu, heart, create dialog | `src/core/*`, `src/runtime.rs`, `src/ui/*` |

Each phase gets an adversarial review before the next starts.
