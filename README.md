# ytamp

**YouTube Music, native and fast.** ytamp is a YouTube Music client written in
Rust with [egui](https://github.com/emilk/egui). It has no browser engine.

ytamp is early software. The v1 goal:

- Sign in with the cookies from a music.youtube.com browser session.
- Search for songs, albums, artists, and playlists.
- Library: your playlists and your liked songs.
- Playback with a queue, shuffle, repeat, seek, and volume.
- Media keys, a tray icon, and resume of the last session.

Planned after v1: the home feed, lyrics, playlist edits, an equalizer,
visualisers, and a Winamp mode with classic `.wsz` skins.

## Build

Build the single binary with Rust 1.95 or newer:

```bash
cargo build --release
```

## Design

ytamp takes inspiration from
[Fastpotify](https://github.com/crmne/fastpotify), a native Spotify client.
The code is a fresh implementation:

- One immutable-in-spirit `State`. Views are pure: they draw the state and
  return actions. They never mutate.
- One reducer applies each action to the state and returns effects.
- Effects (network, playback, disk) are data. A tokio runtime executes them
  and sends result actions back.
- Views name style roles, never colors or sizes. A theme resolves each role.
  The built-in theme is theme one. A Winamp skin engine is theme two, later.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
