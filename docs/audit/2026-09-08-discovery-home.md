# YouTube discovery Home — 2026-09-08

Home now requests the OAuth TV Music feed (`FEtopics_music`) alongside the library.
It preserves YouTube’s section names, order, and card navigation. The first feed
page appears immediately, with one bounded continuation prefetch; Explore more
loads further pages. Refresh and failed requests preserve useful existing content.
Discovery stays in memory and is cleared on sign-out. Session generation, target,
and request IDs reject stale completions; repeated cursors stop pagination.

## Interface

- Compact song shelves, artwork cards for mixes/albums/playlists, circular artwork
  for artist shelves, and left-aligned titles throughout.
- Cards fill the available width, including maximized windows. Horizontal shelves
  have mouse-accessible arrows and retain trackpad scrolling.
- Song clicks play through the existing queue, with the existing right-click menu.
  Mix and collection clicks open a dedicated page with artwork, metadata, Play,
  track menus, and pagination. Back returns to Home; collection navigation remains
  associated with the Home sidebar item.
- Library shelves remain below discovery and accessible directly from the sidebar.
  OAuth remains the only discovery authentication path.

## API boundary

`src/api/discovery.rs` converts content renderers into typed shelves, entries, and
tracks. It reads only a tile’s own selection endpoint, not IDs in menus. Browse
IDs and params are preserved. Radio queues use `next` and select the shelf matching
the requested playlist, excluding unrelated recommendations. Album track tiles,
normal playlist tiles, and playlist video rows have distinct parsing paths.

## Verification and remaining limitation

- `cargo test --offline --all-targets`: passed, including 385 library tests and
  the existing UI interaction audit. New tests cover mixed navigation, radio
  selection, collection metadata, stale responses, sign-out, retries, repeated
  cursors, and bounded prefetch.
- Strict all-target Clippy, formatting, and whitespace checks passed.
- Native fixture preview reviewed at 1710px and 700px widths. Confirmed section
  names, column sizing, circle artwork, vertical scrolling, and clickable shelf
  arrows. The preview executes no network effects or audio playback; image loaders
  can fetch public cover artwork.
- Fresh public TV responses parsed successfully: one album track, 15 playlist
  tracks, and a 20-track radio queue. A mix browse returned one seed track and a
  continuation; its anonymous continuation did not provide the personalized list.
  These public-response checks validate renderer compatibility, not OAuth success.
- **Outstanding:** Google rejected the saved OAuth token with `invalid_grant`
  (expired or revoked) during the production integration probe. The user was asked
  to renew OAuth sign-in. Live personalized collection opening and playback still
  need verification after renewal; no alternative sign-in method was added.

Re-run `cargo run --offline --example discovery_integration_probe` after sign-in
for production feed and collection request checks. `discovery_preview` accepts
local response files for native layout review, or `--check` for parser checks;
private response files are kept outside the repository.

## OAuth verification completed — 2026-09-09

After the user renewed sign-in, the production integration probe successfully
loaded three Home pages (3 / 3 / 4 shelves) and opened album, artist, playlist,
personalized mix (15 tracks), and radio (20 tracks) targets. The optional
`--decode` check also resolved and decoded two seconds from an actual returned
mix track, with the decoder playback position advancing. It uses the real
streaming pipeline without changing the app's queue or playing through speakers.
The earlier OAuth blocker is resolved.
