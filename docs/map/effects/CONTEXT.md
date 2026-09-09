---
kind: effect-index
status: verified
---

# Change impact index

Open the named card before you edit the source boundary.

| Changed source | Open first | First-order hits |
| --- | --- | --- |
| `src/core/state.rs`, `src/api/`, `src/auth.rs` | [Account and catalog](../objects/account-catalog/account-catalog.md) | Sign-in, Home loading, library, search, discovery |
| `src/core/update/`, `src/player/`, `src/stream/`, `src/runtime/` | [Playback](../objects/playback/playback.md) | Queue, controls, stream fallback, now playing |
| `src/ui/`, `src/app.rs`, `src/theme/` | [Default UI](../objects/default-ui/default-ui.md) | Pages, controls, navigation, loading states |
| `src/skin/`, `src/ui/winamp/`, `assets/skins/` | [Classic skins](../objects/classic-skins/classic-skins.md) | `.wsz` load, bitmap layout, Winamp window |

This index records impact inside the repository. External scripts and users can point at source paths without an in-tree reference.
