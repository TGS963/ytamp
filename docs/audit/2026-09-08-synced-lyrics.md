# Synced lyrics verification — 2026-09-08

The lyrics window now uses LRCLIB timestamps when the provider's title and artist
match (ignoring case and whitespace), and duration differs by at most two seconds.
Album metadata is included in the lookup. No fuzzy removal of live/remix suffixes
is performed. The optional lookup has a six-second timeout; failures preserve
YouTube Music's plain lyrics. Existing request identity/cancellation protects
against results from previous tracks. Provider documentation: https://lrclib.net/docs

The active line uses the player's audio position (reported every 250 ms), so
pauses and seeks use the same timeline as playback. LRC repeated timestamps,
fractional seconds, offsets, simultaneous lines, and empty instrumental markers
are supported. Follow playback scrolls on line changes; disable it to browse.
Clicking a line seeks to its timestamp. Delay adjusts display timing, with positive
values meaning later; it is held per song in UI memory for this app session.

Validation:

- 368 library tests passed. New tests cover LRC parsing, boundary selection,
  stationary paused positions, backward seeks, offsets, recording mismatch,
  provider failure, and plain fallback.
- Strict all-target Clippy, formatting, and diff whitespace checks passed.
- Live read-only Yellow lookup (`9qnqYL0eNNI`) returned 49 timed lines.
- Native macOS Winamp player: opened the lyrics window for A Sky Full of Stars,
  observed the active line, clicked an earlier line and observed the highlight
  move back, then started playback and observed the highlight advance.
- Timing quality supplied by the community was not assessed by listening; this
  is line synchronization, not word-level karaoke. No lyric screenshots or full
  provider text are included in the repository.
