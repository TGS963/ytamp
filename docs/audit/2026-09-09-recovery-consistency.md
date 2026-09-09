# OAuth recovery and default UI consistency

Authorized scope: items 1 and 2 only. Further discovery controls await user input.

## Changes

OAuth account verification no longer depends on anonymous Music homepage/search
initialization. Search initializes lazily and can retry after a failed connection.
Account verification is bounded; connection failures preserve saved credentials
and provide Try again. Expired authorization and OAuth configuration failures
have distinct guidance. Session expiry preserves navigation and the queue.
Retry/cancel use session generations to reject late responses. The browser-cookie
sign-in path has been removed; explicit sign-out still cleans old credential files.

The default interface shares primary, secondary, and quiet button styles and
page/section headings. Section actions reserve their width and align right;
long headings truncate with tooltips. Hover and focus do not change control
geometry. Keyboard focus uses a contrasting inset outline. The sign-in card
scrolls at small sizes, and existing playback controls remain accessible during
account recovery when a queue exists.

## Verification

- All-target offline tests pass: 380 library tests, including recovery transitions,
  stale attempts, queue/position preservation, retry clicks at three window sizes,
  and section hover/focus/disabled geometry and Enter activation at four widths.
- Strict all-target Clippy and formatting pass.
- Live production OAuth probe passed: three discovery pages plus radio, artist,
  mix, playlist, and album collections after the API refactor.
- Native macOS app launched with saved OAuth and loaded personalized Home.
  Inspected minimum (700px), normal (1100px), and maximized (1710px) layouts.
- Native recovery preview uses simulated state and the production UI, without
  reading or modifying real credentials. Its retry screen fits at minimum size. Native Tab displayed the focus outline;
  Enter activated Try again and displayed the connecting state with Cancel.

Real token revocation was not induced. Expiry and outage behavior are covered by
state tests and the native recovery fixture. Playback was left paused.
