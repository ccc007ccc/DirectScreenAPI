# 构建指南

## 依赖

- Rust 稳定工具链
- C 编译器（用于 C 示例）

## 构建核心

```sh
./scripts/build_core.sh
# 或
./scripts/dsapi.sh build core
```

## 运行 CLI

```sh
cd core/rust
cargo run --bin dsapi -- version
```

## 构建 Android 适配层

```sh
./scripts/build_android_adapter.sh
# 或
./scripts/dsapi.sh build android
```

## 构建并运行 C 示例

```sh
./scripts/build_c_example.sh
# 或
./scripts/dsapi.sh build c-example
./artifacts/bin/dsapi_example
```

## 工程门禁与修复

```sh
./scripts/dsapi.sh check
./scripts/dsapi.sh fix
```
