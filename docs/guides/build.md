# 构建指南

## 依赖

- Rust 稳定工具链
- C 编译器（用于 C 示例）

## 构建核心

```sh
./scripts/build_core.sh
```

## 运行 CLI

```sh
cd core/rust
cargo run --bin dsapi -- version
```

## 构建 Android 适配层

```sh
./scripts/build_android_adapter.sh
```

## 构建并运行 C 示例

```sh
./scripts/build_c_example.sh
./artifacts/bin/dsapi_example
```
