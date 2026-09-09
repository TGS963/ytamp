# Home and default-interface redesign — 2026-09-08

Home now opens after sign-in and uses the existing library and playback session.
Continue listening resumes the current queue at its saved position. Playlist
cards open their track lists. Cards under From your likes use the shared row
context menu; Shuffle your likes starts a random saved track and enables shuffle.
Home search submits to Search. A final card shows the next track, or On repeat
when the current recording will repeat. Missing data has loading, retry, and
empty-library states.

The default interface uses darker layered surfaces, a distinct branded sidebar,
clear active navigation, artwork shelves, more readable rows, and a larger
play/pause control. Home fills the available width; shelves add columns as the window grows,
including when maximized, and adapt when the queue opens. Compact windows hide sidebar
playlist shortcuts and group secondary actions under Player & account. All song
titles stay left-aligned. Winamp retains its bitmap interface.

This is a library-based Home page, not a YouTube recommendation feed. The likes
shelf is labeled accurately because the OAuth liked list can include ordinary
videos as well as music. Watch history is not used as a music recommendation
source.

Verification:

- `cargo test --offline --all-targets`: passed, including 378 library tests and
  the interaction audit example.
- New checks exercise Home resume without replacing the queue, Home search
  submission, shuffled likes, and sign-in loading Home's library data.
- Strict all-target Clippy, formatting, and whitespace checks passed.
- Native checks at 1710px, 1100px, and 700px widths; minimum-height sidebar
  overlap found and fixed. Scrolling reaches the lower shelves; queue opening
  reduces Home to a narrower card layout.
- Audit updated to target the foreground playlist submenu rather than a sidebar
  shortcut with the same title, and to verify single-click selection followed
  by double-click playback in the queue.
- The native app is left on Home, paused. Private library screenshots were used
  locally for inspection and are not included in the repository.
