# Winamp classic skins

## Status

Phases A to D are done and reviewed: `19eb82d` (skin layer),
`5a48fe2` and `cc0f9d2` (main window, menu, skins, marquee),
`71fde8f` (ytamp built-in skin), `ff6e1d6` (playlist window),
`09f6b41` (visualizer), plus review fixes. The viewport model works
on macOS. Deferred: the equalizer, shaped skins through `region.txt`
in the blitter, modern skins.

## Goal

ytamp wears any Winamp 2 `.wsz` skin as a second look: a pixel-exact
275 x 116 main window, a playlist window, and a spectrum visualizer,
in a borderless window at 1x to 4x scale. The classic look stays the
default. The user switches with a shortcut and a button.

## Reference

fastpotify (MIT, github.com/crmne/fastpotify) has a complete egui 0.36
implementation. Its skin layer has no Spotify code. ytamp ports it
with attribution in each file header and a `THIRD_PARTY.md` notice.
Local clone for the port:
`/private/tmp/claude-501/-Users-suvojit-programs/9ed0fd66-619b-40cf-b4ca-a1cb752ee1de/scratchpad/fastpotify`.
Its brief:

- `src/skin/zip.rs`: a hand-rolled zip reader on `flate2`, case- and
  folder-insensitive names, STORED and DEFLATED, 64 MiB entry cap.
- `src/skin/mod.rs`: `Skin::from_archive`, 15 sheets, none required,
  `image::load_from_memory` decode, fallbacks (`Balance` -> `Volume`,
  `Numbers` <-> `NumsEx`, else the built-in skin). The built-in skin
  is `include_bytes!` of a `.wsz` that `examples/default_skin.rs`
  generates.
- `src/skin/sprites.rs`: `Sheet` enum, `Sprite {sheet, x, y, w, h}`,
  about 110 constants from Webamp's sprite table, indexed families as
  functions (`volume_frame`, `digit`, `glyph`).
- `src/skin/layout.rs`: `WINDOW_WIDTH 275`, `WINDOW_HEIGHT 116`,
  `SHADE_HEIGHT 14`, 40 `Area` constants for the controls.
- `src/skin/font.rs`: `text.bmp` glyph map, 3 rows x 31 cells of
  5 x 6 px, accent folding.
- `src/skin/config.rs`: `pledit.txt`, `viscolor.txt`, `region.txt`
  with an even-odd mask.
- `src/ui/winamp/mod.rs` `View` (lines 120-302): sprite blit with UVs,
  mask spans, integer scale, `rect(area)`, `interact`, sliders.
- `src/vis.rs`: `AudioTap` ring plus a 512-point Hann FFT `Analyser`
  into 19 bars, and an oscilloscope.
- Window: borderless, transparent, fixed size, integer scale, drag by
  the title bar through `ViewportCommand::StartDrag`.

## Design for ytamp

### Architecture fit

- Core stays pure. `State.winamp: WinampSettings { open: bool, scale:
  u8, on_top: bool, skin: Option<String> }`, saved in the session.
  Actions: `WinampToggled`, `WinampScaleSet(u8)`, `WinampOnTopToggled`,
  `SkinChosen(Option<String>)`, `SkinInstalled(path)`. Effects:
  `LoadSkin(Option<String>)`, `InstallSkin(PathBuf)`.
- The skin data (decoded sheets, textures, marquee cursor, the
  loaded-skin slot) lives in the imperative shell, in a `WinampShell`
  owned by `App`, never in `State`. The runtime loads skins on the
  blocking pool and delivers `Action::SkinLoaded(Result<Arc<Skin>>)`.
- The winamp view is a pure view in the app's sense: it reads `&State`
  and `&WinampShell`, draws, and returns `Vec<Action>`. The existing
  `Theme` trait stays for the classic look. The skin is its own
  renderer, not a `Theme` implementation: a Winamp skin is a pixel
  layout, not a set of color roles.

### Window model

The skin renders in an egui immediate viewport
(`ctx.show_viewport_immediate`) with `with_decorations(false)`,
`with_transparent(true)`, fixed inner size = 275 x stack height x
scale, `with_always_on_top` from the setting. While it is open, the
main viewport hides (`ViewportCommand::Visible(false)`). Closing the
skin window shows the main viewport again. If immediate viewports
misbehave on macOS, fall back to fastpotify's approach: one
`eframe::run_native` per window in a loop, with the `App` in an
`Arc<Mutex<Option<App>>>` slot (its `main.rs:363-405`).

### Controls mapped to ytamp

| Skin control | ytamp action |
|--------------|--------------|
| Previous, Play, Pause, Next | `PreviousPressed`, `PlayToggled`, `PlayToggled`, `NextPressed` |
| Stop | pause and `SeekRequested(0)` |
| Eject, both logos, X | `WinampToggled` |
| Seek bar | `SeekRequested` on release |
| Volume slider | `VolumeSet` |
| Balance slider | draws at center, no action, no engine support |
| Shuffle, Repeat | `ShuffleToggled`, `RepeatCycled` (repeat shows on for All and One) |
| EQ button | no action in this plan |
| PL button | toggles the playlist window |
| Time digits | position, click toggles remaining |
| Marquee | "artist - title (m:ss)", notices scroll through |
| kbps / kHz | "128" and the decoder sample rate in kHz |
| Mono / stereo | decoder channels |
| Title bar | drag, double-click shade, O menu: scale 1x-4x, on top, skins |

`PlayerEvent::TrackStarted` needs `channels` and `sample_rate` for
the kHz and stereo lamps: extend `ReadyInfo` delivery through the
event (a small change in `src/player/mod.rs` and the reducer).

### Skins on disk

`ProjectDirs` config dir, `skins/`. A `.wsz` dropped on either window
copies there and applies. The O menu lists the folder. The built-in
skin is fastpotify's generated `.wsz`, bundled with attribution, and
a ytamp-branded one is a follow-up through the same generator.

### Visualizer

`StreamingSource::next()` already sees every decoded sample. It pushes
into a shared half-second ring (`AudioTap`), lag 150 ms. The analyser
port runs in the view at 60 Hz while playing. Colors from
`viscolor.txt`. Click cycles spectrum, oscilloscope, off.

## Phases

| Phase | Scope | Files |
|-------|-------|-------|
| A | Port the pure skin layer: zip, sprites, layout, font, config, `Skin`, built-in skin, `skin_probe` example | `src/skin/*`, `assets/skins/`, `Cargo.toml`, `THIRD_PARTY.md` |
| B | Main window: `View` blitter, textures, viewport, title bar, transport, seek, volume, toggles, time, marquee, lamps, shade, O menu, skin install, settings in the session | `src/ui/winamp/*`, `src/app.rs`, `src/core/*`, `src/runtime.rs`, `src/player/mod.rs` |
| C | Playlist window: queue rows in `pledit` colors, double-click plays, scrollbar, resize grip, times | `src/ui/winamp/playlist.rs` |
| D | Visualizer: audio tap in the player source, analyser port, spectrum and scope | `src/player/source.rs`, `src/vis.rs`, `src/ui/winamp/*` |

Deferred: the equalizer (needs a DSP stage in the player), Winamp 3
and 5 modern skins, a ytamp-branded default skin.

Each phase gets an adversarial review before the next starts.
