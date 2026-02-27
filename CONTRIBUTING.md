# Contributing

## Workflow

1. Fork and create a feature branch.
2. Keep changes scoped to a single concern.
3. Add or update docs for behavior changes.
4. Run local checks before opening PR.

## Local Checks

```sh
./scripts/check.sh
```

## Commit Guidance

Use clear commit prefixes:
- `core:` runtime/domain changes
- `ffi:` C ABI changes
- `docs:` documentation changes
- `build:` scripts and tooling

## Compatibility Policy

- Breaking changes to C ABI require a version bump and migration note.
- New APIs must include input validation and error code mapping.
