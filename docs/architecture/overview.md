# Architecture Overview

DirectScreenAPI is split into three stable layers:

1. Core runtime (`core/rust`)
- domain state
- routing logic
- validation and deterministic behavior

2. ABI bridge (`core/rust/src/ffi`, `bridge/c/include`)
- C-callable contract for any upper language
- strict status codes and null-pointer guards

3. Integrations (`bridge/c/examples`, future Android adapters)
- thin adapters only
- no business logic duplicated in integration layer

## Design Goal

The core must remain stable even when rendering/input backends evolve.

## Non-Goal (current stage)

This stage does not provide a GPU renderer or Android binder integration.
It only stabilizes the foundational contracts and process model.
