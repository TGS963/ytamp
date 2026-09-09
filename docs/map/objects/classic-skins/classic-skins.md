---
kind: object
status: verified
verified: 2026-09-09
universe: live
---

# Classic skins

Classic skins decode `.wsz` bitmap assets and draw the Winamp window.

## Why this shape

Skin files supply bitmap data and layout rules. The Winamp shell keeps window-local rendering state outside the core state.

## Shape

- Skin loading and bitmap decoding start in [`src/skin/mod.rs`](../../../../src/skin/mod.rs) and [`src/skin/mod.rs`](../../../../src/skin/mod.rs).
- Skin text and regions parse in [`src/skin/config.rs`](../../../../src/skin/config.rs).
- Window-local shell state starts in [`src/ui/winamp/mod.rs`](../../../../src/ui/winamp/mod.rs).
- Winamp view composition is [`src/ui/winamp/view.rs`](../../../../src/ui/winamp/view.rs).

## Connected to

The skin browser requests loading. The Winamp shell reads the loaded `Skin`. The app opens the shell as a viewport.

## If you change this

**Hits:** `.wsz` compatibility, bitmap coordinates, window size, classic controls, and skin preview behavior.

**Does not hit:** Default page layout or catalog response decoding unless the shell action boundary changes.

## Surfaces

Skin modules read assets. The Winamp UI reads skin data. The app owns the viewport.

## See

- [`src/skin/mod.rs`](../../../../src/skin/mod.rs)
- [`src/ui/winamp/mod.rs`](../../../../src/ui/winamp/mod.rs)
- [`../../processes/local-import.md`](../../processes/local-import.md)
