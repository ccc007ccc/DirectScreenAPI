# 发布流程

1. 执行 `./scripts/check.sh`，确认格式、静态检查、测试全部通过。
2. 核对版本与 ABI 一致性：
   - `core/rust/Cargo.toml` 的 `package.version`
   - `core/rust/src/api/types.rs` 的 `DSAPI_ABI_VERSION`
   - `bridge/c/include/directscreen_api.h` 的 `DSAPI_ABI_VERSION`
   三者必须与发布说明一致。
3. 核对文档是否覆盖行为/API 变更（尤其是命令、默认值、错误码）。
4. 更新 `CHANGELOG.md`，打版本标签并生成发布说明。

若涉及 ABI 变更，发布说明必须包含兼容性与迁移部分。
