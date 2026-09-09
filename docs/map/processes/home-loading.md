---
kind: process
status: verified
verified: 2026-09-09
universe: live
---

# Home loading

## Input

Signed-in state and Home navigation.

## Movement

1. The Home view reads catalog state in [`src/ui/home.rs`](../../../src/ui/home.rs).
2. The reducer requests discovery and library effects in [`src/core/update/mod.rs`](../../../src/core/update/mod.rs).
3. API modules decode responses in [`src/api/discovery/mod.rs`](../../../src/api/discovery/mod.rs) and [`src/api/official/mod.rs`](../../../src/api/official/mod.rs).

## Output

The reducer stores loading, loaded, or failed catalog state for Home.

## Consumes

- [`../objects/account-catalog/account-catalog.md`](../objects/account-catalog/account-catalog.md)
- [`../objects/default-ui/default-ui.md`](../objects/default-ui/default-ui.md)

## Produces

- [`../objects/account-catalog/account-catalog.md`](../objects/account-catalog/account-catalog.md)

## If you change this

**Hits:** Home shelves, library cards, refresh state, and discovery navigation.

**Does not hit:** Decoder behavior or skin layout.

## See

- [`src/ui/home.rs`](../../../src/ui/home.rs)
