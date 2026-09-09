# ytamp

<img src="assets/branding/icon.png" width="96" height="96" alt="ytamp lightning-bolt icon">

**YouTube Music, native and fast.** ytamp is a YouTube Music client written in
Rust with [egui](https://github.com/emilk/egui). It has no browser engine.

ytamp is early software. Currently implemented:

- Sign in with your own Google OAuth device-flow client.
- Home with session resume, playlist artwork, liked-song discovery, and the next queued track.
- Search for songs, albums, artists, and playlists.
- Library: your playlists and your liked songs.
- OAuth YouTube watch history with Refresh and Load older.
- Playback with an editable queue, shuffle, repeat, seek, volume, and stereo balance.
- Media keys and resume of the last session.
- Like/unlike, create playlists, and add/remove playlist tracks.
- Winamp mode with classic `.wsz` skins and visualisers.
- A shared 10-band equalizer with preamp, presets, and saved settings.
- On-demand lyrics in a separate window, available from either player.
- macOS menu-bar controls for playback, lyrics, show/hide, and quit.
- Preparation of the next decoder to reduce track-transition delays.

Native tray menus on Windows/Linux remain planned.

Classic skin `region.txt` cutouts are applied to the player and EQ panels.
EQ hover readouts and **Presets → Actual EQ values** show real frequencies and
gains when a skin’s printed artwork differs. Balance works independently of EQ;
double-click a Winamp balance slider to center it, or use **Center** in the main
EQ popup. Stereo changes ramp over 20 ms and survive restart; mono and
multichannel sources retain their original channel layout.

## Home and the default interface

Home opens after sign-in. Resume continues the existing queue at its saved
position; playlist cards open their track lists, and cards under **From your likes** play from your YouTube likes (which may include
regular videos). **Shuffle your likes** starts a random liked track and enables
shuffle. Home search submits directly to the search page. Song cards have the
same right-click menu as list rows.

Home also loads YouTube’s Music discovery shelves through the existing OAuth
sign-in. YouTube supplies their titles and ordering: mixes, new releases, charts,
and artist recommendations vary by account and request. Song shelves use compact
rows; mixes use artwork cards, and artist shelves use circular portraits. Shelf
arrows reveal more cards; **Explore more** loads additional sections. Collections
open before playback, with track menus and pagination. Refresh and retry preserve
usable content, and library shelves remain available if discovery fails.

The default interface uses a darker sidebar, clear active navigation, artwork
shelves, and a larger play/pause control. Playlist shortcuts appear when there is
room; compact windows keep skins, Winamp mode, and sign-out under **Player &
account**. Titles remain left-aligned, and Home adapts when the queue is open.

## Queue editing

In the queue panel or Winamp playlist, click to select and double-click (or
press Enter) to play. Shift-click selects a range; Cmd/Ctrl-click toggles a row.
Drag selected songs to an insertion line to reorder them. Delete/Backspace
removes the selection. Right-click offers Play now, Play selected next, and
Remove selected. Cmd/Ctrl+A selects upcoming songs; Escape clears selection.
The current song stays in place. Manual order survives restart and preserves
explicitly queued songs separately from their album/playlist context.

## Lyrics and menu bar

Choose **Lyrics** in the main player or the Winamp title-bar menu. The separate,
resizable window follows the current track and retains the provider's attribution.
Unavailable lyrics and network errors have separate states; errors offer Retry.
Requests are canceled when closing the panel or changing tracks. Lyrics are not
saved into the session. When LRCLIB has timed lyrics matching the title, artist, and
duration, the current line is highlighted and follows playback (including pauses and
seeks). Click a line to seek, turn off **Follow playback** to browse, or adjust
**Delay** (positive means later) for small recording offsets. Timing adjustments
are saved per song across restarts; **Reset** clears the correction. Manual scrolling
turns off Follow playback until you re-enable it. Plain YouTube Music lyrics remain the fallback.

On macOS, the **♫** menu-bar item shows the current track and provides playback,
lyrics, show/hide, and quit controls. Playback and menu actions continue while the
app is hidden or minimized. Other platforms retain the existing OS media controls.

Next-track preparation reduces decoder startup delays; it does not promise
sample-accurate gapless playback or add crossfading.

## History, playback recovery, and skins

**History** in the sidebar shows your YouTube account’s watch history, newest
first, using your existing OAuth sign-in. It includes music and other videos.
**Load older** fetches another page; **Refresh** fetches the latest entries.
Rows support playback, queueing, and the usual context menu. This reads history;
playing in ytamp does not currently write listening events back to YouTube.
History is held in memory and cleared on sign-out. The TV endpoint is unofficial
and may change; the official Data API does not expose watch history.

After a playback interruption, **Retry playback** reloads the stream and resumes
at the saved position. Repeated failed attempts retain that position. Winamp shows
the error in its display and offers Retry playback in its title-bar menu; Play
also retries a stopped track.

Open **Skins** from the sidebar or **Browse skins…** from Winamp’s title-bar menu.
The gallery loads installed skin thumbnails in the background. Select one for a
larger preview and choose **Use this skin**. Drop a classic `.wsz` or `.zip` archive
to install it; invalid archives are rejected before replacing any installed skin.
Modern `.wal` skins show an explicit compatibility error.

## Equalizer

Open **EQ** in either player interface. In Winamp, it attaches below the player;
closing or rolling up the panel leaves its sound settings active. **On** enables
EQ or bypasses it. EQ is off by default, including for sessions saved before this
feature existed.

The ten bands (60 Hz–16 kHz) and preamp cover ±12 dB. Double-click a Winamp slider
to reset it. **Presets** includes Flat, Warm, Vocal, and Bright, plus up to 32 named
custom curves. Saving an existing name replaces that curve. Gains and custom
presets survive restarts and are shared between both interfaces.

**Auto** provides headroom by subtracting the strongest band boost from the
preamp. A stereo-linked peak protector catches remaining peaks while EQ is on;
large boosts can still change dynamics, so use modest gains. Sliders and bypass
ramp over 20 ms. Bands above the decoded stream's Nyquist limit are skipped.
These are playback effects; downloaded audio is unchanged.

## Build

Use Rust 1.95 or newer. On Windows, install the MSVC Rust toolchain and Visual Studio Build Tools with the C++ workload.
On macOS, install the Xcode command-line tools.

On Ubuntu 24.04, install the desktop build dependencies:

```bash
sudo apt-get install build-essential pkg-config libasound2-dev libdbus-1-dev libudev-dev libx11-dev libxi-dev libxrandr-dev libxcursor-dev libxinerama-dev libwayland-dev libxkbcommon-dev libxkbcommon-x11-dev libgl1-mesa-dev libegl1-mesa-dev
```

Build and run the application:

```bash
cargo build --locked --release
cargo run --locked --release
```

Linux needs a graphical X11 or Wayland session, an OpenGL-capable driver, and an audio device for playback.
The session D-Bus service enables Linux media controls. Windows media controls use the app's native window handle.
The optional `yt-dlp` fallback must be on `PATH` on each platform.

[Routine CI](.github/workflows/platforms.yml) runs one Linux job for code changes: formatting, map checks, Clippy, and library tests. It caches dependencies without workspace or incremental build output. Documentation-only pushes do not start Rust builds, except for system-map changes.

[Release builds](.github/workflows/release.yml) run manually or on `v*` tags. They produce Linux and Windows archives plus a universal macOS app for Apple Silicon and Intel. A tag must match the version in `Cargo.toml`. Manual runs upload packages as workflow artifacts. Tag runs create a draft GitHub release. Packages are not signed for distribution or notarized.

[Desktop validation](docs/audit/2026-09-09-branding.md) records the completed Windows/Linux build, test, packaging, and native startup checks.

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
  The built-in theme and Winamp skin engine share the same playback state.

## Controls and checks

Right-click a track anywhere on its row for library actions. The main Queue
panel supports click-to-play and right-click removal; Clear queued tracks
removes explicit additions while retaining the current playlist context.
Library has Refresh and New playlist buttons. Japanese titles use a bundled
Noto Sans JP fallback in both window styles.

Space plays/pauses, Left/Right select previous/next, and Q toggles the queue.
Cmd+Shift+M on macOS (Ctrl+M elsewhere) switches Winamp mode. Shortcuts stay
inactive while editing text. A pause during loading remains paused when the
stream becomes ready.

```bash
cargo test --all-targets
cargo run --example audit_probe
cargo clippy --all-targets -- -D warnings
```

The audit probe is also an automated test. It drives the real egui views and
reducer with synthetic data, without network calls or account writes.

Create a desktop package with Python 3.11 or later:

```bash
cargo build --release --locked
python3 scripts/package.py
```

The script stages a macOS app bundle, a Linux desktop tree, or a Windows folder under `dist/`. It does not sign or install the package. Linux packages use the app ID `io.github.TGS963.ytamp`. Install the staged `usr/` tree under the same system prefix as the executable.

The app uses the supplied plain Aero lightning-bolt artwork. The macOS menu bar uses a monochrome template of the bolt. Imported Winamp skins retain their original bitmap lettering.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).

For a native macOS viewport smoke test, launch ytamp in its main window and run
`swift scripts/smoke-winamp.swift <ytamp-pid>` from a terminal with Accessibility
permission. It exercises three player/EQ/playlist/shade/return cycles without
starting audio or changing EQ gains. Use a fresh app launch so panels start closed.

Saved OAuth sign-in has a separate connection-recovery screen. Network failures
keep your account, queue, and settings; **Try again** reconnects without repeating
setup. An expired authorization asks you to sign in with Google again. Legacy
browser-cookie credentials are no longer loaded.

Open **Now playing** (or click the bottom artwork/title) for larger artwork and
Lyrics, Queue, and Equalizer tabs. Discovery shelves offer **See all**; song
context menus include **Play next**, **Add to queue**, and **Start radio**.
Home displays cached discovery while refreshing. At narrow sizes, extra player
actions live in **•••**, keeping volume above the full-width seek bar.
