---
kind: process
status: verified
verified: 2026-09-09
universe: live
---

# Playback

## Input

A track action, queue state, and a stream source.

## Movement

1. A UI action reaches the reducer at [`src/core/update/mod.rs`](../../../src/core/update/mod.rs).
2. The reducer emits a player command.
3. The player resolves audio through the source contract at [`src/stream/mod.rs`](../../../src/stream/mod.rs).
4. Player events return through the runtime boundary at [`src/runtime/mod.rs`](../../../src/runtime/mod.rs).

## Output

The player emits position, completion, or failure actions.

## Consumes

- [`../objects/playback/playback.md`](../objects/playback/playback.md)

## Produces

- [`../objects/playback/playback.md`](../objects/playback/playback.md)

## If you change this

**Hits:** Queue transitions, stale responses, stream fallback, and player controls.

**Does not hit:** Catalog renderer parsing or skin archive rules.

## See

- [`src/core/update/mod.rs`](../../../src/core/update/mod.rs)
- [`src/stream/mod.rs`](../../../src/stream/mod.rs)
