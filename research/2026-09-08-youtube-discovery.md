# YouTube curated discovery: source investigation

Investigated 2026-09-08. This note records documentation and implementation evidence, not successful requests with ytamp's account. Live probe results must be assessed separately.

## What is exposed

YouTube Music's internal Home response can contain actual **Quick picks**, plus recommendations for songs, videos, albums, artists, and playlists. These are server-provided titled sections, not recommendations that ytamp must generate. The ytmusicapi documentation explicitly includes a Quick picks example with playable video IDs and artist/album metadata. [ytmusicapi browsing reference](https://ytmusicapi.readthedocs.io/en/stable/reference/browsing.html)

| Surface | Internal request | Evidence |
| --- | --- | --- |
| Home / Quick picks | `POST browse`, `browseId: FEmusic_home` | ytmusicapi parses mixed sections and section-list continuations. [Home implementation](https://github.com/sigma67/ytmusicapi/blob/master/ytmusicapi/mixins/browsing.py) |
| Explore | `browseId: FEmusic_explore` | New albums, new videos, trending tracks, moods/genres and podcasts; source documents account-dependent Top Songs availability. [Explore implementation](https://github.com/sigma67/ytmusicapi/blob/master/ytmusicapi/mixins/explore.py) |
| Moods and genres | `FEmusic_moods_and_genres`, then `FEmusic_moods_and_genres_category` with returned `params` | Returns categories and their playlist collections. [Explore implementation](https://github.com/sigma67/ytmusicapi/blob/master/ytmusicapi/mixins/explore.py) |
| Charts | `FEmusic_charts`, `formData.selectedValues: [country]` | Global (`ZZ`) or country charts. Source handles account/region-dependent sections, including India language charts. [Charts implementation](https://github.com/sigma67/ytmusicapi/blob/master/ytmusicapi/mixins/charts.py) |
| Related music / radio | `next` for a video, then returned related/automix endpoint | YouTube.js follows server-provided targets for recommendations and generated queues. [Music client](https://github.com/LuanRT/YouTube.js/blob/main/src/core/clients/Music.ts) |

Home also exposes filter chips and pagination in YouTube.js. Its parser follows a selected chip's endpoint instead of assuming fixed filter parameters. This supports a dynamic Home UI, but does not establish which chips or mixes this account receives. [HomeFeed implementation](https://github.com/LuanRT/YouTube.js/blob/main/src/parser/ytmusic/HomeFeed.ts)

## OAuth versus browser cookies

OAuth is a supported authentication path in the current ytmusicapi project. Its setup guide specifies a Google Cloud OAuth client for TVs and Limited Input devices. This is evidence for an OAuth implementation, not a guarantee that every internal endpoint accepts every client's tokens. [OAuth setup](https://ytmusicapi.readthedocs.io/en/stable/setup/oauth.html)

The request code has separate OAuth bearer and browser SAPISID authorization branches. OAuth adds `X-Goog-Request-Time`, and the base headers obtain a visitor ID. It sends a static `SOCS` consent cookie even in OAuth mode; this is not an account authentication cookie. Nothing here requires extracting the user's browser login cookies. [Authentication/request source](https://github.com/sigma67/ytmusicapi/blob/master/ytmusicapi/ytmusic.py)

Default context uses `WEB_REMIX` with a date-based client version. Requests use the music origin, and visitor data comes from the public Music page. These are useful comparisons when a simpler bearer probe fails. [Header/context source](https://github.com/sigma67/ytmusicapi/blob/master/ytmusicapi/helpers.py)

YouTube.js independently attaches OAuth bearer tokens to internal requests and only adds cookie authentication when a cookie was separately provided. It also sets client-name/version and visitor headers. This corroborates that internal Music client code is not intrinsically cookie-only. [HTTP client](https://github.com/LuanRT/YouTube.js/blob/main/src/utils/HTTPClient.ts)

## Official Data API boundary

The official YouTube Data API's former `activities.list(home=true)` is deprecated; its documented `homeParameterDeprecated` error states that home activity data is unavailable through that API. This is a statement about that endpoint, **not** proof that authenticated internal Music browse endpoints cannot work. [Google activities.list documentation](https://developers.google.com/youtube/v3/docs/activities/list)

## TV music alternative

SmartTube's MediaServiceCore uses **`FEtopics_music`** for its Music section and routes it through the TV browse path. This is a stronger candidate than trying `FEmusic_home` against TV clients. Its current helper also defines Music new-release browse IDs, but those specifically use the Remix request helper. A successful TV Music response still needs inspection before claiming parity with YouTube Music Quick picks. [Browse query definitions](https://github.com/yuliskov/MediaServiceCore/blob/master/youtubeapi/src/main/java/com/liskovsoft/youtubeapi/browse/v2/BrowseApiHelper.kt), [TV Music service routing](https://github.com/yuliskov/MediaServiceCore/blob/master/youtubeapi/src/main/java/com/liskovsoft/youtubeapi/browse/v2/BrowseService2.kt)

## Recommendation for ytamp

First validate OAuth Home plus one continuation with actual renderer content, not HTTP status alone. Preserve server section titles, order, item types and navigation endpoints. Only label something Quick picks or a particular mix when YouTube returned it. Probe Explore/charts independently: a public discovery response does not prove personalized Home works. Retain the existing library Home when discovery is unavailable. Do not add browser-cookie sign-in to work around a failed client variant.

## Live ytamp OAuth results

Read-only requests on 2026-09-08 using the existing saved OAuth token, refreshed
in memory. No browser login cookies or account mutations. Reproducible probe:
`cargo run --offline --example oauth_discovery_probe` (or `-- --tv-music` for
only the successful Music feed and up to three continuation requests).
The probe prints section headings and renderer counts, not credentials or item titles.

| Request | Observed result |
| --- | --- |
| `TVHTML5 / FEtopics_music` | HTTP 200, three shelves and 18 cards: **Listen again**, **Recently played**, **Your favorite artists**. |
| Its first continuation | HTTP 200, three more shelves and 18 cards. One run: **Forgotten favorites**, **Mixed for you**, **Mood and genre mixes**. Another: **Forgotten favorites**, **New releases**, **Recommended live performances**. Results vary between requests. |
| `TVHTML5 / FEwhat_to_watch` | HTTP 200, four shelves and 20 cards. General YouTube Home, not specifically Music Home. |
| `TVHTML5 / UC-9-kyTW8ZkZNDHQJ6FgpwQ` (Music channel) | HTTP 200, one shelf and ten cards. |
| `WEB_REMIX / FEmusic_home`, `FEmusic_explore`, `FEmusic_moods_and_genres`, `FEmusic_charts` | HTTP 400 `INVALID_ARGUMENT` with this token. Retested using the client version and visitor ID read from the live public Music page, Music origin, client headers, and request-time header. |
| `ANDROID_MUSIC`, `IOS_MUSIC`, `TVHTML5`, `TVHTML5_SIMPLY / FEmusic_home` | Initial variants returned HTTP 400. This does not establish that every version/context or OAuth client is unsupported. |
| `TVHTML5 / FEmusic_explore`, `FEmusic_new_releases_videos` | HTTP 400 `FAILED_PRECONDITION`. |

**Conclusion:** We can retrieve actual server-provided music discovery shelves
and mixes with the existing OAuth sign-in. Exact **Quick picks** is documented
for Music Home but has not been returned by these account probes. Do not rename
Mixed for you or Listen again to Quick picks. TV Music is the strongest verified
integration path; preserve its supplied labels and endpoint semantics.

The TV responses use `shelfRenderer`, `horizontalListRenderer`, and `tileRenderer`;
headers contain nested text rather than a direct `title.runs`. Cards include
mixed navigation types; follow their returned playback/browse endpoints rather
than assuming every card is a single track. Global recursive video-ID counts
include menu references and duplicates, so they are diagnostic counts, not song
totals. This investigation adds only a probe and research notes; Home has not
been connected to discovery yet.

A final run followed **section-list** continuations specifically (shelf-level
continuations expand one horizontal shelf and are a different operation). Four
successful HTTP 200 pages returned **14 shelves / 84 cards** in total:

- Page 1: Listen again, Recently played, Your favorite artists (18 cards).
- Page 2: Artist mixes for you, Mixed for you, Mood and genre mixes (18 cards).
- Page 3: Forgotten favorites, New releases, Recommended music videos,
  Top charts (24 cards).
- Page 4: three Similar to artist shelves and From your library (24 cards).

This confirms charts and new releases are accessible within the TV Music feed
even though the separately tested Music browse variants failed. Counts refer
to returned cards, including playlist/mix/artist navigation, not unique songs.
The probe built and passed strict example Clippy, formatting, and whitespace checks.
