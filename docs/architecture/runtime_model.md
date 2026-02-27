# Runtime Model

## State Model

Runtime state owns:
- display state
- default routing policy
- ordered region list

## Determinism

Routing uses last-added-first-match semantics.
This allows predictable override behavior for dynamic UI overlays.

## Thread Model

FFI context wraps runtime engine in a mutex.
No global mutable singleton is used.

## Failure Model

All public APIs return explicit integer status codes.
No implicit retries, no hidden side effects.
