# DirectScreenAPI

DirectScreenAPI is a foundation-first runtime for privileged Android screen interaction.

It is designed as a long-term base layer for:
- custom window systems
- AI-driven UI control frameworks
- container/desktop bridging runtimes

The project focuses on stability and clear module boundaries before feature expansion.

## Project Status

Current stage: `Foundation Rewrite (v0.1.0)`

Implemented now:
- Rust core runtime state model
- stable C ABI surface
- deterministic input routing engine (rect regions)
- display state model with strict validation
- minimal CLI and C integration example

Not implemented yet:
- OpenGL ES backend
- Vulkan backend
- direct Android SurfaceControl renderer integration
- frame capture and injection loop

## Repository Layout

- `core/rust`: core domain/runtime/ffi implementation
- `bridge/c`: C header and C integration example
- `docs`: architecture, API, governance and operational guides
- `scripts`: build and smoke scripts

## Quick Start

```sh
cd core/rust
cargo build --release
cargo test
cargo run --bin dsapi -- version
```

C example:

```sh
./scripts/build_core.sh
cc -Ibridge/c/include bridge/c/examples/simple_route.c \
  target/release/libdirectscreen_core.a -ldl -lpthread -lm \
  -o artifacts/bin/dsapi_example
./artifacts/bin/dsapi_example
```

## Core Principles

- stable-first: no hidden side effects in core runtime state
- recoverable-by-design: deterministic cleanup semantics
- language-agnostic: C ABI is a first-class contract
- backend-agnostic: render/input backends are pluggable interfaces

## License

Apache-2.0
