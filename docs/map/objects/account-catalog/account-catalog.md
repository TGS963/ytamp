---
kind: object
status: verified
verified: 2026-09-09
universe: live
---

# Account and catalog

The account and catalog boundary joins OAuth session state to library, search, and discovery data.

## Why this shape

`State` holds durable page and account state. `Api` and its official and discovery modules fetch and decode catalog data.

## Shape

- `AuthState` and `Page` live in [`src/core/state.rs`](../../../../src/core/state.rs).
- `State` is the application state in [`src/core/state.rs`](../../../../src/core/state.rs).
- `Api` is the facade in [`src/api/mod.rs`](../../../../src/api/mod.rs).
- Official library requests live in [`src/api/official/mod.rs`](../../../../src/api/official/mod.rs).
- Discovery response decoding starts in [`src/api/discovery/mod.rs`](../../../../src/api/discovery/mod.rs).

## Connected to

It owns account and catalog requests. The reducer consumes results. The default UI reads the resulting `State`.

## If you change this

**Hits:** OAuth recovery, library refresh, search and discovery pages, and Home loading.

**Does not hit:** Audio decoding or bitmap layout unless the changed result crosses those boundaries.

## Surfaces

`EffectRuntime` writes results through actions. The reducer reads them. Default UI views read the state.

## See

- [`src/core/state.rs`](../../../../src/core/state.rs)
- [`src/api/mod.rs`](../../../../src/api/mod.rs)
