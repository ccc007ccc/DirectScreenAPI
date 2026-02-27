# Stability Policy

## Baseline Guarantees

- All public API calls are input-validated.
- Invalid input does not mutate runtime state.
- Context creation/destruction is explicit and idempotent-safe.

## Change Rules

- Domain behavior changes must be documented in `docs/` before merge.
- New API surfaces require tests at core layer.
- Hidden side effects are treated as bugs.
