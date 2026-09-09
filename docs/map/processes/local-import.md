---
kind: process
status: verified
verified: 2026-09-09
universe: live
---

# Local media import

## Input

User-selected local files from the default UI, classic Winamp UI, or a drop.

## Movement

1. UI actions create import work through [`src/core/imports.rs`](../../../src/core/imports.rs).
2. The runtime probes regular local files off the UI thread through [`src/local_media/mod.rs`](../../../src/local_media/mod.rs).
3. The reducer adds successful local tracks to the queue; the player uses their file source.

## Output

Ordered local queue tracks with local metadata, duration, optional cached artwork, and local LRC lyrics.

## Consumes

- [`../objects/playback/playback.md`](../objects/playback/playback.md)
- [`../objects/default-ui/default-ui.md`](../objects/default-ui/default-ui.md)
- [`../objects/classic-skins/classic-skins.md`](../objects/classic-skins/classic-skins.md)

## Produces

- [`../objects/playback/playback.md`](../objects/playback/playback.md)

## If you change this

**Hits:** Drop classification, file dialogs, queue order, decoder choice, local artwork, and local lyric privacy.

**Does not hit:** YouTube catalog parsing or remote lyric lookup.

## See

- [`src/local_media/mod.rs`](../../../src/local_media/mod.rs)
- [`src/core/imports.rs`](../../../src/core/imports.rs)
