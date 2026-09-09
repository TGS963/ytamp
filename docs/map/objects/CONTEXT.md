---
kind: contract
status: verified
---

# Object cards

**Reads:** `src/` files named by each card.

**Job:** Define the four live boundaries that editors change.

**Writes:** One source-linked card per boundary. The index is a route, not a copy of source behavior.

**Human check:** Open a card, then open its `See` source link. Check its Hits and Does not hit statements against the source.
