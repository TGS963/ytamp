# Discovery, cached Home, and Now Playing

User authorized items 3–5, then requested further slider and responsive-player corrections.

## Delivered

- Track context menus include Play next, Add to queue, and Start radio. Radio
  leaves existing playback intact until a successful response; failures preserve
  it, and request identity/playback generation reject late results. Loading has
  a cancel control. Loaded collections can be queued next or appended in order.
- Artwork shelves hide their horizontal scrollbar; arrows and horizontal scrolling remain. A paint regression test reproduces the stray bar from an off-screen shelf and verifies its removal.
- Every discovery shelf has See all. Song shelves use the shared full-width
  track list; other shelves use an artwork grid. Page-specific widget identities
  keep Home and collection scroll state separate during navigation.
- Home reads its previous discovery snapshot with the library cache and refreshes
  in the background. A late cache result cannot overwrite fresh network data.
  Writes use the existing atomic, session-generation-guarded cache path, and
  sign-out clears the cache. Artwork uses reserved card/row rectangles.
- Now Playing is available in navigation and from the bottom artwork/title.
  Larger windows show artwork beside Lyrics, Queue, and Equalizer tabs; narrow
  windows use a compact header. Embedded lyrics share timing, line seeking,
  follow behavior, and saved delay with the separate lyrics window. Loading
  cancels safely on navigation and retries when the view returns.
- Play/pause uses centered vector shapes. Sliders retain egui input and keyboard
  behavior, with solid round thumbs and fills ending at the thumb center.
  EQ/balance fill from zero and retain editable numeric values.
- The seek row spans the player width at 700, 1100, and 1710px. On narrow windows,
  volume remains beside transport, with auxiliary actions in More. No utility
  row appears below the seek bar. Decoded duration survives session restoration.

## Validation

Final library suite: 390 passing tests. Strict all-target Clippy, formatting, and whitespace checks pass.

- Regression tests cover visible seek/volume trails, exact fill endpoint and
  solid-thumb geometry, keyboard adjustment, responsive width/overflow, queue
  insertion after manual reordering, stale radio rejection, cached/fresh races,
  lyrics navigation during loading, and old-session compatibility.
- Native macOS checks: minimum and normal player layouts, More menu, Now Playing
  lyrics/EQ, and See all navigation. Final play/seek/volume controls inspected
  with a paused track at 2:14 of 3:21.
- Live OAuth integration probe: three Home pages, 20 radio tracks, 13 artist
  shelves, 7 playlist tracks, 6 album tracks, and 15 mix tracks. No account writes.
- Discovery cache was present after live use and loaded on restart.

See all exposes all entries supplied for that shelf; it does not invent an
unsupported YouTube shelf-pagination endpoint. Classic skin bitmap controls are
unchanged by the default-interface paint/layout changes.
