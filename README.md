# Rydantic

[![CI](https://github.com/trotsky1997/Rydantic/actions/workflows/ci.yml/badge.svg)](https://github.com/trotsky1997/Rydantic/actions/workflows/ci.yml)

Rust 的统一 JSON 数据模型库，当前版本 **0.1.0**。

一份 `#[derive(Model)]` 声明提供：JSON 解析、显式类型转换、字符串规范化、约束校验、结构化错误、Serde 序列化／反序列化，以及 JSON Schema Draft 2020-12。

工作区包含运行时 `rydantic`、过程宏 `rydantic-derive`，以及依赖重命名集成示例。已使用 Rust 1.98.1 验证；工作区附带 Cargo.lock。

## 快速开始

在项目的 Cargo.toml 中添加 GitHub 版本依赖：

```toml
[dependencies]
rydantic = { git = "https://github.com/trotsky1997/Rydantic", tag = "v0.1.0" }
```

```rust
use rydantic::{Model, SchemaMode};

#[derive(Debug, Model)]
#[model(extra = "forbid")]
struct User {
    #[model(trim, min_length = 3, max_length = 30)]
    name: String,

    #[model(coerce, ge = 18, le = 120)]
    age: u8,

    #[model(trim, lowercase, email)]
    email: String,

    #[model(default)]
    tags: Vec<String>,

    note: Option<String>,
}

fn main() -> Result<(), rydantic::ValidationErrors> {
    let user = User::parse_json(
        r#"{"name":" Alice ","age":"25","email":" ALICE@EXAMPLE.COM "}"#,
    )?;

    assert_eq!(user.name, "Alice");
    assert_eq!(user.age, 25);
    assert_eq!(user.email, "alice@example.com");
    println!("{}", user.dump_json()?);

    let schema = User::json_schema(SchemaMode::Output);
    assert!(schema.is_object());
    Ok(())
}
```

`Model` 同时生成 Serde 的 `Serialize`、`Deserialize` 实现。模型只需派生一次 `Model`；Serde 实现中的反序列化会执行完整校验，序列化会重新校验当前对象。

```sh
cargo run -p rydantic --example user
cargo run -p rydantic --example tool_call
cargo run -p rydantic --example tool_call -- --schema
cargo run -p rydantic --example tool_call -- '{"kind":"ping"}'
```

## 已实现的类型与规则

支持 `String`、`bool`、8/16/32/64 位有符号和无符号整数、`isize`／`usize`、`f32`／`f64`、`Option<T>`、`Box<T>`、`Vec<T>`、`[T; N]`、字符串键的 `BTreeMap`／`HashMap`、`serde_json::Value`，以及嵌套、递归、泛型模型。

| 字段属性 | 行为 |
| --- | --- |
| `trim`、`lowercase`、`uppercase` | 按声明的固定流水线规范化字符串 |
| `coerce` | 数字字符串转换为数值；`"true"`／`"false"` 转为布尔值；数值／布尔值转为字符串 |
| `min_length`、`max_length` | 字符串 Unicode 标量数量、集合元素数量、映射条目数量 |
| `ge`、`gt`、`le`、`lt` | 数值范围；阈值使用数值字面量 |
| `pattern = "..."` | Rust regex；模式在编译期检查，在运行时缓存 |
| `email` | 使用 `email_address` 的邮箱地址规则 |
| `default` | 字段类型的 `Default::default()` |
| `default = expression` | 显式默认值；字符串字面量可直接用于 String |
| `default_factory = path` | 每次缺失字段时调用工厂函数 |
| `required` | 即使字段为 Option，也要求输入中出现该字段 |
| `rename = "..."` | 同时设置输入与输出名称 |
| `validation_alias`、`serialization_alias` | 分别设置输入与输出名称 |
| `exclude` | 输出与输出 Schema 中省略，输入仍按规则处理 |
| `before = path` | 原始 JSON 值处理函数 |
| `validate = path` | 字段类型构造完成后的自定义校验 |
| `description = "..."` | Schema 描述；也自动读取字段文档注释 |

容器属性：`extra = "forbid"`（默认）、`extra = "ignore"`、`title = "..."`、`validate = path`、枚举的 `tag = "..."`。

解析顺序为：读取输入／产生默认值 → before → 类型解析与显式转换 → trim → 大小写转换 → 字段约束 → 字段自定义校验 → 模型自定义校验。默认值也经过同一流程。独立字段的错误会收集到同一份结果中；父模型的自定义校验在其字段全部成功后执行。

数字字符串需要显式启用 `coerce`。JSON 数值 `25.0` 可用于整数，`25.5` 会失败；这与 JSON Schema 对数学整数的定义一致。范围比较保留 64 位整数精度。浮点数遵循 Rust 的 IEEE 754 表示，NaN、无穷值以及超出目标类型范围的数值会失败。

## 缺失字段与 null

| 声明 | 输入可省略 | 接受 null |
| --- | --- | --- |
| `name: String` | 否 | 否 |
| `name: Option<String>` | 是，得到 None | 是 |
| `#[model(required)] name: Option<String>` | 否 | 是 |
| `#[model(default = "guest")] name: String` | 是，得到默认值 | 否 |

## 自定义校验

```rust
use rydantic::{Model, ValidationError, ValidationErrors, Value};

fn expand(value: Value) -> Result<Value, ValidationError> {
    if value == "many" { Ok(rydantic::json!(10)) } else { Ok(value) }
}

fn even(value: &u32) -> Result<(), ValidationError> {
    if value.is_multiple_of(2) { Ok(()) }
    else { Err(ValidationError::new("even", "value must be even")) }
}

fn check_window(value: &Window) -> Result<(), ValidationErrors> {
    if value.end >= value.start { Ok(()) }
    else { Err(ValidationError::custom("end must be >= start").at("end").into()) }
}

#[derive(Model)]
#[model(validate = check_window)]
struct Window {
    start: u32,
    #[model(before = expand, validate = even)]
    end: u32,
}
```

嵌套时自定义错误路径会自动加上父级路径。`ValidationErrors::errors()` 提供错误切片，`to_json()` 提供结构化 JSON。每个错误包含 `loc`、`code`、`message`、`stage` 与约束参数；`pointer()` 输出转义后的 JSON Pointer。

```json
[
  {
    "loc": ["addresses", 0, "city"],
    "code": "too_short",
    "message": "length must be >= 2",
    "stage": "validate",
    "params": {"actual": 1, "limit": 2}
  }
]
```

## 枚举与工具参数

无字段枚举编码为字符串。带字段枚举通过显式 discriminator 编码为对象：

```rust
use rydantic::Model;

#[derive(Model)]
#[model(tag = "kind")]
enum ToolCall {
    #[model(rename = "search")]
    Search {
        #[model(min_length = 1)]
        query: String,
        #[model(default = 3, ge = 1, le = 10)]
        limit: u8,
    },
    #[model(rename = "ping")]
    Ping,
}
```

对应输入为 `{"kind":"search","query":"Rust"}` 或 `{"kind":"ping"}`，生成的 Schema 使用 `oneOf`、`const` 和明确的必需字段。

## 输入与输出 Schema

`SchemaMode::Input` 使用输入别名、输入必需字段与额外字段策略。`SchemaMode::Output` 使用输出别名、移除 exclude 字段，并将所有实际输出字段设为必需字段。递归模型通过 `$defs`／`$ref` 表达，定义名称包含完整 Rust 类型路径。

去空格、类型转换、自定义函数，以及 regex／email 的具体实现规则需要 Rydantic 执行。导出的 Schema 使用 `x-rydantic-local-rules` 标记这些行为；调用 `schema_report(mode)` 可以得到对应位置与规则。输入转换后的约束采用结构上的宽容描述，输出 Schema 则描述处理后的数据。字面量默认值仅在输入 Schema 中作为注释导出；生成 Schema 从不执行默认值工厂。

```rust
let report = User::schema_report(rydantic::SchemaMode::Input);
for local in &report.local_rules {
    println!("{}: {}", local.schema_path, local.rule);
}
```

Schema 的用途包括 API 文档与工具参数描述。实际执行入口仍应调用模型解析，确保本地规则得到执行。

## 运行时动态 Schema

可选 `dynamic` feature 使用 `jsonschema` 提供运行时 Schema 编译与复用。默认依赖关闭网络／文件解析功能，本地 `$ref` 可正常使用。

```toml
rydantic = { git = "https://github.com/trotsky1997/Rydantic", tag = "v0.1.0", features = ["dynamic"] }
```

```rust
use rydantic::{json, DynamicModel};

let model = DynamicModel::compile(json!({
    "type":"object",
    "properties":{"age":{"type":"integer","minimum":18}},
    "required":["age"],
    "additionalProperties":false
}))?;

assert!(model.is_valid(&json!({"age":25})));
assert!(model.validate(&json!({"age":17})).is_err());
```

`DynamicModel::compile` 会拒绝含 `x-rydantic-local-rules` 的 Schema。`compile_structural` 提供显式选择，仅执行标准 JSON Schema 断言。动态验证遵循后端默认的 format 注释语义，保留输入原样；默认值填充与转换由类型模型路径负责。

## 对象生命周期

Rust 中可直接构造或修改普通模型。调用 `validate()` 检查当前对象，调用 `dump()`／`dump_json()` 会先重新校验；这两个检查入口保持当前字段值。规范化发生在解析路径。

需要只读访问时使用 `User::parse_validated_json(...)` 或 `Validated::new(user)`，得到无 `DerefMut` 的 `Validated<User>`。`into_inner()` 可取回普通模型。自定义带内部可变性的 JsonType，需要自行维护修改规则。

## 实现范围

0.1 使用 `serde_json::Value` 作为输入表示；过程宏生成具体 Rust 类型的解析、校验和编码实现，并复用字段规则描述生成 Schema。原始输入仍会物化为 JSON 值，目前性能优化以减少重复编码和缓存规则为主。

当前 derive 支持具名字段结构体、无字段枚举和内部标签枚举。借用字段、tuple struct、untagged union、flatten、日期／Decimal 内置类型、异步 validator、额外字段保留及流式解析留待后续版本。自定义类型可以实现 `JsonType` 接入。整数内置支持到 64 位。可通过 GitHub 的 v0.1.0 标签接入；工作区内部使用本地路径依赖，尚未发布到 crates.io。

## 验证

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo check -p rydantic --no-default-features --lib
cargo run -p rydantic-renamed-fixture
```

测试包含解析／序列化完整流程、递归／泛型模型、缺失与 null、规范化、默认值、精确整数边界、错误路径、独立 JSON Schema 验证器对照，以及编译失败用例。边界矩阵覆盖 1,400 组输入；独立验证器还检查成功输出与输出 Schema 的一致性。

License: MIT.
