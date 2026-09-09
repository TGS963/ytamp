---
kind: object
status: verified
verified: 2026-09-09
universe: live
---

# Playback

Playback turns a track action into queued audio and player events.

## Why this shape

The reducer controls queue and generation state. The player owns the audio device. Stream sources fill buffers at an explicit boundary.

## Shape

- Track identity is [`src/core/model.rs`](../../../../src/core/model.rs).
- The reducer entry point is [`src/core/update/mod.rs`](../../../../src/core/update/mod.rs).
- Missing queue durations resolve through [`src/core/durations.rs`](../../../../src/core/durations.rs); decoded audio durations take precedence.
- The player handle sends commands in [`src/player/mod.rs`](../../../../src/player/mod.rs).
- Audio source and buffer contracts start in [`src/stream/mod.rs`](../../../../src/stream/mod.rs).
- Audio and lyrics clients share the [RustyPipe cache factory](../../../../src/rustypipe_client.rs).
- Effects execute through [`src/runtime/mod.rs`](../../../../src/runtime/mod.rs).

## Connected to

The default UI sends actions. The reducer emits player and API effects. Stream sources feed the player.

## If you change this

**Hits:** Queue order, skip and seek, stream fallback, stale generation checks, and now-playing state.

**Does not hit:** OAuth token parsing or skin bitmap decoding unless a command crosses those boundaries.

## Surfaces

UI actions and runtime results cross the reducer. The player thread owns device work.

## See

- [`src/core/update/mod.rs`](../../../../src/core/update/mod.rs)
- [`src/player/mod.rs`](../../../../src/player/mod.rs)
