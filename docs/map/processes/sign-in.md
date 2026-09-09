---
kind: process
status: verified
verified: 2026-09-09
universe: live
---

# Sign-in

## Input

The sign-in view emits OAuth actions. The saved token and client settings are account inputs.

## Movement

1. The sign-in view emits an OAuth request from [`src/ui/sign_in.rs`](../../../src/ui/sign_in.rs).
2. The runtime executes account effects in [`src/runtime/mod.rs`](../../../src/runtime/mod.rs).
3. The reducer applies account results in [`src/core/update/mod.rs`](../../../src/core/update/mod.rs).

## Output

The state holds `AuthState::SignedIn` or an account error.

## Consumes

- [`../objects/account-catalog/account-catalog.md`](../objects/account-catalog/account-catalog.md)

## Produces

- [`../objects/account-catalog/account-catalog.md`](../objects/account-catalog/account-catalog.md)

## If you change this

**Hits:** OAuth retry, session generation, and account request delivery.

**Does not hit:** Bitmap decoding or audio buffering.

## See

- [`src/ui/sign_in.rs`](../../../src/ui/sign_in.rs)
