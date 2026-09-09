---
kind: object
status: verified
verified: 2026-09-09
universe: live
---

# Default UI

The default UI renders application state into egui actions and page controls.

## Why this shape

Views read state and return actions. The app shell connects those views to the reducer and effect runtime.

## Shape

- The view boundary is [`src/ui/mod.rs`](../../../../src/ui/mod.rs).
- Home composition is [`src/ui/home.rs`](../../../../src/ui/home.rs).
- Shared controls are [`src/ui/components/mod.rs`](../../../../src/ui/components/mod.rs).
- Desktop artwork and window identity are [`src/branding.rs`](../../../../src/branding.rs).
- The app shell is [`src/app.rs`](../../../../src/app.rs).

## Connected to

Views read account and playback state. They emit actions to the reducer. Theme modules supply visual roles.

## If you change this

**Hits:** Page layout, pointer and keyboard actions, navigation, and visible loading states.

**Does not hit:** Network decoding, audio source selection, or Winamp bitmap coordinates unless the action boundary changes.

## Surfaces

The app owns the shell. UI modules own view composition. The reducer owns state mutation.

## See

- [`src/ui/mod.rs`](../../../../src/ui/mod.rs)
- [`src/app.rs`](../../../../src/app.rs)
