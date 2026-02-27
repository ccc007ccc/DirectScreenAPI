# Build Guide

## Requirements

- Rust toolchain (stable)
- C compiler (`cc`) for C example

## Build Core

```sh
./scripts/build_core.sh
```

## Run CLI

```sh
cd core/rust
cargo run --bin dsapi -- version
```

## Build C Example

```sh
./scripts/build_c_example.sh
./artifacts/bin/dsapi_example
```
