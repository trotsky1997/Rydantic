# Rydantic 0.2.0 验证记录

日期：2026-10-19（Asia/Shanghai）。本机环境：macOS，rustc 1.98.1，Cargo 1.98.1。

## 自动化验证

- `cargo test --workspace --all-features --locked`：通过。76 项功能测试：27 项原有模型行为、11 项原有 Schema、5 项动态 Schema、19 项新增模型组合、11 项异步／流式、3 项日期／Decimal。
- 另有 1 项文档测试与 1 项编译期测试入口。编译期入口在常规模式下核对 11 个失败用例：生命周期逃逸、矛盾长度、重复别名、缺失 extra 存储、extra 存储类型、无效正则、只读包装修改、tag／untagged 冲突、tuple 字段 exclude、未知属性、规则与字段类型不匹配。
- 原有 Schema 边界矩阵继续对照 1,400 组输入；独立 `jsonschema` 验证器检查 103 个原有成功输出。新增用例还检查 tuple／untagged、借用递归与泛型引用、optional flatten、flatten 默认值、键冲突、日期／Decimal 的输入与输出 Schema。
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`：通过。
- `cargo check -p rydantic --no-default-features --lib --locked`：通过。
- `cargo check -p rydantic --no-default-features --features chrono,decimal --lib --locked`：通过。
- `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --all-features --no-deps --locked`：通过。
- `cargo fmt --all -- --check`：通过。

## 行为证据

- 借用测试直接检查 `&str` 地址位于输入缓冲区，验证 trim 保留引用、普通 Cow 保持借用、转义和大小写转换产生 Owned 值；同时覆盖多生命周期、递归、泛型、flatten、extra、默认值与 Serde 包装。
- 128 位整数测试覆盖 `i128::MIN`／`u128::MAX`、数学整数、越界和超长指数；Decimal 测试覆盖精确阈值、96 位系数、28 位 scale、科学记数法和精度损失拒绝。
- 日期测试覆盖闰日、无效日期／时间、时区转换、超过纳秒精度的输入，以及 Rust 直接构造的越界年份／秒级时区偏移。非法 typed default 返回结构化错误。
- 异步测试在 Tokio 运行时等待实际挂起的 future，并使用 `tokio::spawn` 验证 Send；检查父模型同步入口拒绝、独立字段错误收集、跨字段回调、同步失败跳过异步，以及取消 future 后 Context 路径恢复。
- 流式测试使用一次读取一个字节的 reader，确认首条记录／首个数组回调发生在 EOF 前；检查回调提前终止读取、JSON 序列语法错误终止、NDJSON 下一行恢复、数组错误索引与借用数组值保留。

## 实际运行

- `borrowed`：运行借用姓名、展开地址和额外 Cow 字段；确认普通字符串 Borrowed、转义字符串 Owned，输出完整 JSON。
- `streaming`：逐元素处理有效／无效／有效三条数组记录；NDJSON 在格式错误行之后继续交付有效记录。
- `asynchronous`：等待异步校验，序列化 `AsyncValidated`；验证 taken 分支返回结构化错误。
- `dates_decimal --features chrono,decimal`：保留 `12345678901234567890.123456789`，并将 `+08:00` 时间转为 UTC。
- `rydantic-renamed-fixture`：依赖名称为 `rd`，过程宏通过重导出的依赖运行，输出 `{"text":"hello"}`。

## CI 与范围

[CI 工作流](https://github.com/trotsky1997/Rydantic/actions/workflows/ci.yml) 在 Ubuntu 与 macOS 使用同一工具链执行完整测试、Clippy、格式、feature 检查、文档构建、工具参数示例与四个新增示例。上文记录本机验证结果；各次远程运行状态可从该链接查看。

0.2 覆盖 README 原列出的八类后续功能。借用字段基于原始输入缓冲区，读取器使用同步 I/O，序列／数组流式入口保留当前记录。日期、Decimal、自定义函数等本地规则通过 Schema 元数据公开。更详细的入口与组合语义见 README。
