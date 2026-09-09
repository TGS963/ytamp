# OAuth history investigation — 2026-09-08

The original conclusion that OAuth could not retrieve history was too broad.
YouTube Data API v3 explicitly excludes watch history, and the YouTube Music
`FEmusic_history` endpoint rejects this account's OAuth requests. However,
YouTube's TV client with `FEhistory` works with the same OAuth token.

## Sources and experiment

- [Official API restriction](https://developers.google.com/youtube/v3/docs/playlistItems/list): `watchHistoryNotAccessible`.
- [Upstream OAuth issue](https://github.com/sigma67/ytmusicapi/issues/813): Music client failures and discussion of TV renderer differences.
- [TV client experiment](https://github.com/sigma67/ytmusicapi/issues/813#issuecomment-3359537317): TVHTML5 version 7 can return data in a different format.
- [Recent upstream status](https://github.com/sigma67/ytmusicapi/issues/813#issuecomment-5491110689): the maintainer still reports broader Music OAuth restrictions. That does not prove all TV endpoints fail.
- [June token-loading fix](https://github.com/sigma67/ytmusicapi/pull/932) handles extra OAuth token fields, not this endpoint problem.

`cargo run --offline --example oauth_history_probe` refreshes the existing OAuth
token and makes read-only requests. It prints counts/statuses, never tokens,
account entries, or continuation values.

| Request | Result |
| --- | --- |
| Official liked-songs control | HTTP 200 |
| WEB_REMIX / FEmusic_history | HTTP 400 |
| IOS_MUSIC / FEmusic_history | HTTP 400 |
| TVHTML5 / FEmusic_history | HTTP 400 |
| TVHTML5 / FEhistory | HTTP 200, history tiles |
| Production history parser, first three pages | 15 entries each, all with channel names and durations, continuation present |

## Implementation

The History page reads account watch history using the existing OAuth token and
refresh/retry handling. It preserves newest-first order, supports Refresh and
Load older, and uses existing track rows for playback/queue/context menus.
It parses only top-level video tiles, avoiding duplicate video IDs embedded in
menus and playback commands. Unexpected layouts produce an error rather than
silently displaying an empty history. Request IDs protect against stale refreshes;
account generation checks discard results after sign-out.

This is YouTube watch history, including music and other videos. It does not
record ytamp plays back to YouTube, and does not store history on disk. No cookie
history implementation was added. The TV endpoint is unofficial and can change.

Native verification: opened the History page and clicked Load older; additional
entries appeared. Account-history screenshots were used only locally, not committed.
