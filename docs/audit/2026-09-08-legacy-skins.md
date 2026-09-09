# Legacy skin compatibility check — 2026-09-08

Downloaded six classic `.wsz` archives from the [Webamp demo collection](https://github.com/captbaritone/webamp/tree/88ed5815d968c201962f6549915579b3d2f93c5e/packages/webamp-demo/skins), pinned to revision `88ed5815d968c201962f6549915579b3d2f93c5e`. These are a varied convenience sample, not a statistically random selection or exhaustive compatibility certification. The archives are installed in the local ytamp skins directory; they are not bundled into this repository. [SHA-256 checksums](evidence/legacy-skin-sha256.txt).

| Skin | Native result | Screenshot |
|---|---|---|
| base-2.91 | Loads; main, EQ and playlist render | [View](evidence/legacy-base-2.91.png) |
| Green-Dimension-V2 | Loads and controls work; region cutouts now render transparently | [View](evidence/legacy-Green-Dimension-V2.png) |
| MacOSXAqua1-5 | Loads; main, EQ and playlist render | [View](evidence/legacy-MacOSXAqua1-5.png) |
| AmigaPPC-dark | Loads; main, EQ and playlist render | [View](evidence/legacy-AmigaPPC-dark.png) |
| TopazAmp1-2 | Loads; main, EQ and playlist render | [View](evidence/legacy-TopazAmp1-2.png) |
| ZaxonRemake1-0 | Loads; main, EQ and playlist render | [View](evidence/legacy-ZaxonRemake1-0.png) |

All six passed the production skin loader and supplied their own `eqmain` and `eq_ex` sheets, including both 56- and 82-pixel-tall EQ extension formats. Native checks covered switching skins, EQ/playlist opening, gain adjustment and graph updates, and shade/close behavior across the sample. Base, Green Dimension and Aqua were initially tested before the corrections below; Aqua and the later samples were inspected in the rebuilt app. These checks did not measure audio again: skins share the already-tested playback processor.

## Corrections made

- The EQ curve previously used a hard-coded green. It now samples each skin's graph color strip; native Aqua, Amiga, Topaz and Zaxon show their own palettes.
- The collapsed EQ close button now uses `eq_ex` sprites rather than full-window `eqmain` sprites.
- The collapsed volume thumb now selects the skin's low/middle/high variants according to volume.

After these changes, all 347 library tests and strict all-target Clippy pass. Formatting and diff whitespace checks pass.

## The red Amiga skin

The red/white blocks and dot-like controls in the user's screenshot are in the original bitmap. The archive readme identifies it as **Amiga PPC /dark**, by Adam Rucki, with an original release date of 2000-01-05. [Original main bitmap](evidence/legacy-amiga-original-main.png), converted from the archive's BMP for inspection, agrees with the app's palette and background layout. This is an abstract skin design, not evidence of a color-decoding failure.

## Remaining limits

`region.txt` masks now clip background sprites, text, graph pixels, and control hit testing in normal/shaded player and EQ panels. A native Green Dimension screenshot has alpha 0 in cutouts and alpha 1 in the body. This verifies macOS rendering; OS-level click-through to other applications and other platforms have not been verified.

The artwork's printed frequency/gain labels are static. Some skins advertise different labels/ranges (for example ±20 dB) while ytamp's actual EQ remains the documented ten frequencies and ±12 dB, shown by live control readouts, tooltips, and Presets → Actual EQ values. Modern `.wal` skins are unsupported. The sample does not cover every archive variant or missing-sheet combination.

The built-in skin was restored, the test gains reset to Flat, and EQ left bypassed. The downloaded skins remain installed under their original filenames for further comparison through the title-bar skin menu.

## Follow-up: shapes, readouts, regression probe, and balance

Implemented in that order. The offline [compatibility probe](../../examples/skin_compat_probe.rs) loads real archives and exercises the production view/reducer at 1×, 2×, and 4×. It checks EQ opening, On, all ten bands, preamp, balance endpoints in both panels, sampled mask geometry, playlist, shade, and close without bypassing audio. All 18 combinations pass. Run:

```sh
cargo run --offline --example skin_compat_probe -- "$HOME/Library/Application Support/ytamp/skins/"*.wsz
```

Balance attenuates one stereo channel without boosting the other, independently of EQ On or presets. Changes ramp over 20 ms. Center is sample-preserving; mono and multichannel sources are unchanged. Settings survive restart and sign-out; old sessions default to center. Tests exercise actual processed samples, endpoint routing with EQ on/off, live transition continuity, invalid settings, and session/reducer/player-command propagation.

Validation: 352 library tests pass; strict all-target Clippy passes. All three [native window-transition cycles](evidence/skin-followup-native-smoke.txt) pass. Native GUI testing verifies the main and shaded balance controls stay synchronized and Aqua displays the actual ±12 dB range beside its ±20 dB artwork. Audio conclusions come from deterministic processor tests, not subjective listening or loopback capture during this follow-up.

Evidence: [compatibility output](evidence/skin-compat-probe.txt), [transparent Green Dimension](evidence/region-green.png), [Aqua range tooltip](evidence/eq-actual-range-aqua.png), [shaded balance](evidence/balance-shaded-aqua.png).
