---
kind: contract
status: verified
---

# Map contract

**Reads:** Rust files under `src/`, the approved plan at `docs/plans/refactor-2026-09-09.md`.

**Job:** Route an editor from a product boundary to source-linked object and process cards.

**Writes:** Cards in `objects/`, real flows in `processes/`, and first-order impact routes in `effects/`.

**Universes:** `live` means the current Rust path. `leftover` means a retained path that is not the main path. `ghost` means a named path that is not wired.

**Human check:** Follow one route from `docs/map/CLAUDE.md` to a card and then to a source file. Check that every source link exists.
