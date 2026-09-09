---
kind: process
status: verified
verified: 2026-09-09
universe: live
---

# Skin loading

## Input

A built-in skin or a user-selected `.wsz` path.

## Movement

1. The skin loader opens and decodes the archive at [`src/skin/mod.rs`](../../../src/skin/mod.rs).
2. The browser stores the selected skin in its view state.
3. The Winamp shell draws the skin through [`src/ui/winamp/mod.rs`](../../../src/ui/winamp/mod.rs).

## Output

The viewport shows the loaded classic skin or an error state.

## Consumes

- [`../objects/classic-skins/classic-skins.md`](../objects/classic-skins/classic-skins.md)

## Produces

- [`../objects/classic-skins/classic-skins.md`](../objects/classic-skins/classic-skins.md)

## If you change this

**Hits:** `.wsz` load errors, bitmap placement, preview behavior, and viewport size.

**Does not hit:** OAuth requests or catalog decoding.

## See

- [`src/skin/mod.rs`](../../../src/skin/mod.rs)
- [`src/ui/winamp/mod.rs`](../../../src/ui/winamp/mod.rs)
