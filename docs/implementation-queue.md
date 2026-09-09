# Implementation queue

User-authorized sequence (2026-09-08). Complete and verify each before starting the next.

- [x] Queue editing: shared ordering, multi-selection, Delete, drag/drop, context menus; native checks in both interfaces.
- [x] Playback stress testing: rapid skip/seek, shuffle/repeat, failed streams, restart; fix issues found.
- [x] Track transitions: prepare the next decoded stream, measure transitions and handle queue changes safely.
- [x] Lyrics: loading, unavailable/error states, track-change cancellation, usable display.
- [x] macOS menu-bar player: transport actions, current track, show/hide, quit, native verification.

Completed. All five items are implemented and verified on macOS.

Queue validation: 356 library tests, strict all-target Clippy, native main/Winamp drag and context menus, native range selection and Delete. All six legacy skins pass drag/Delete checks at 1×, 2×, 4×. Queue test snapshot restored.

Playback validation: 1,000 skip cycles reject stale started/position/end/failure events; 100 loading seeks retain the last target; paused state and restart seeking covered. Decoder callbacks after Stop cannot restart audio. A failed buffer reports one scoped failure rather than normal completion.

Transition validation: 361 library tests and strict Clippy pass. Prepared decoder -> first offline mixer sample: 156 µs in one local run. End polling is 10 ms, position updates remain 250 ms, and command traffic cannot starve the end check. Native playback advanced from A Sky Full of Stars to Yellow using the prepared decoder. This is reduced transition latency, not a claim of sample-accurate gapless playback.

Lyrics validation: 363 library tests and strict Clippy pass. Live Yellow request returned 991 characters and attribution. Native window opened from Winamp, closed, and reopened from the main player. Lyrics text is fetched on demand, never bundled or written into session storage.

Menu-bar validation: native Play/Pause, Previous/Next, Lyrics, Hide/Show, and Quit pass in both normal and Winamp modes. Next and Pause also pass with every window hidden. Incoming events now run in eframe’s background logic callback, fixing hidden/minimized playback progression. Windows/Linux tray menus remain a separate platform task.

Final validation: 364 library tests, strict all-target Clippy, all 18 legacy-skin combinations, formatting and whitespace checks pass. The pre-test queue and playback position were restored; the app is left paused. See [full verification notes](audit/2026-09-08-roadmap.md).

## Follow-up sequence

- [x] Lyrics: saved per-song delays, reset, manual scrolling releases follow.
- [x] Playback recovery: Retry preserving position; dropout/expired-stream checks.
- [x] Skin browser: installed thumbnails, previews, incompatible archive errors.
- [x] YouTube account history via OAuth TV client (`FEhistory`); pagination and native UI verified. Music-specific endpoint fails, but TV history succeeds.
- [x] Home and default-interface redesign (subsequently authorized): session resume, playlist/liked shelves, search, shuffle, responsive navigation and player controls.

Follow-up validation: 376 library tests and strict all-target Clippy passed.
Native History loaded and appended older entries; live production OAuth fetch
returned three pages of 15 entries each. Gallery thumbnails and enlarged previews
rendered installed skins. Manual lyric scrolling released Follow playback.
Saved delay/reset and old-session compatibility are tested. Local HTTP fixtures
exercise 403 stream rejection and truncated connections before successful retry;
reducer tests preserve 1:13 through repeated failures and prevent it leaking into
the next song on skip. The library-based Home page was subsequently authorized and implemented. See
[OAuth investigation](audit/2026-09-08-oauth-history.md).

## YouTube discovery Home

- [x] OAuth discovery feed, responsive shelves, collection navigation, refresh/retry, and pagination.
- [x] Parser/state tests and native fixture layout checks.
- [x] Renewed OAuth verified (2026-09-09): live personalized collections and two seconds of mix audio through the production resolver/decoder.

See [discovery verification](audit/2026-09-08-discovery-home.md).

## Sign-in recovery and interface consistency — 2026-09-09

- [x] OAuth-only sign-in, connection retry, distinct expired/configuration states, and stale-attempt protection.
- [x] Shared default-interface buttons and headings, stable section actions, responsive spacing, and visible keyboard focus.
- [x] Item 3: Play next, Add to queue, Start radio, See all, and page-specific scroll state.
- [x] Item 4: cached Home discovery, quiet refresh, fixed artwork allocations.
- [x] Item 5: responsive Now Playing with artwork, synced lyrics, queue, and EQ tabs.
- [x] Playback polish: centered vector play/pause icon, solid circular slider thumbs, signed EQ/balance trails, and a full-width bottom seek row at every supported window size.

See [items 3–5 verification](audit/2026-09-09-discovery-now-playing.md).

See [verification notes](audit/2026-09-09-recovery-consistency.md).
