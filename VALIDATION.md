# Rydantic 0.1.0 验证记录

日期：2026-10-19（Asia/Shanghai）。环境：macOS，rustc 1.98.1，Cargo 1.98.1。

## 自动化验证

- `cargo test --workspace --all-features`：通过。27 项模型行为测试、11 项 Schema 测试、5 项动态 Schema 测试、1 项编译期用例测试入口、1 项文档测试。
- 编译期入口实际执行 6 个失败用例：矛盾长度规则、重复别名、无效正则、只读包装修改、未知属性、规则与类型不匹配。已在常规模式下核对期望诊断。
- Schema 边界矩阵对照 1,400 组输入；由独立 `jsonschema` 验证器和 Rydantic 分别判断结果。另验证 103 个成功输出满足输出 Schema。
- `cargo clippy --workspace --all-targets --all-features -- -D warnings`：通过。
- `cargo check -p rydantic --no-default-features --lib`：通过。
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps`：通过。
- `cargo fmt --all -- --check`：通过。

## 实际运行

- `user` 示例：确认姓名去空格、年龄字符串转换、邮箱小写规范化；确认 4 个独立错误及 Schema 本地规则报告。
- `tool_call` 示例：执行默认 search、ping、Schema 导出，以及失败输入分支。失败输入返回退出码 2，并输出 `too_short` 和 `greater_than_equal` 两个结构化错误。
- `rydantic-renamed-fixture`：以 `rd` 作为依赖名称运行，确认过程宏通过重导出的依赖工作，输出 `{"text":"hello"}`。

## 当前范围

解析会物化为 `serde_json::Value`；当前版本覆盖具名字段模型、字符串枚举和内部标签枚举。支持范围、输入／输出语义和扩展接口详见 README。流式解析、借用模型和内置日期／Decimal 类型尚未实现。当前测试记录覆盖本机工具链。
