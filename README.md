# Rydantic

[![CI](https://github.com/trotsky1997/Rydantic/actions/workflows/ci.yml/badge.svg)](https://github.com/trotsky1997/Rydantic/actions/workflows/ci.yml)

Rust 的统一数据模型库，当前版本 **0.2.0**，支持 JSON 与原生 Cap’n Proto。

一份 `#[derive(Model)]` 声明提供：JSON 解析、显式类型转换、字符串规范化、约束校验、结构化错误、Serde 序列化／反序列化，以及 JSON Schema Draft 2020-12。

工作区包含运行时 `rydantic`、过程宏 `rydantic-derive`、依赖重命名示例和 Cap’n Proto 协议集成示例。已使用 Rust 1.98.1 验证；工作区附带 Cargo.lock。

## 快速开始

在项目的 Cargo.toml 中添加 crates.io 依赖：

```toml
[dependencies]
rydantic = "0.2.0"
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

支持 `String`、借用的 `&str`／`Cow<str>`、`bool`、8/16/32/64/128 位有符号和无符号整数、`isize`／`usize`、`f32`／`f64`、`Option<T>`、`Box<T>`、`Vec<T>`、`[T; N]`、字符串键的 `BTreeMap`／`HashMap`、`serde_json::Value`、保留原文的 `Box<RawValue>`，以及嵌套、递归、泛型模型。

| 字段属性 | 行为 |
| --- | --- |
| `trim`、`lowercase`、`uppercase` | 按声明的固定流水线规范化字符串 |
| `coerce` | 数字字符串转换为数值；`"true"`／`"false"` 转为布尔值；数值／布尔值转为字符串 |
| `min_length`、`max_length` | 字符串 Unicode 标量数量、集合元素数量、映射条目数量 |
| `ge`、`gt`、`le`、`lt` | 数值范围；阈值使用数值字面量或精确十进制字符串 |
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
| `flatten` | 将具名模型或字符串键映射展开到当前 JSON 对象 |
| `extra` | 保存未知字段的映射，配合容器 `extra = "allow"` |
| `async_validate = path` | 等待异步字段校验函数 |

容器属性：`extra = "forbid"`（默认）、`extra = "ignore"`、`extra = "allow"`、`title = "..."`、`validate = path`、`async_validate = path`、枚举的 `tag = "..."` 或 `untagged`。

解析顺序为：读取输入／产生默认值 → before → 类型解析与显式转换 → trim → 大小写转换 → 字段约束 → 字段自定义校验 → 模型自定义校验。默认值也经过同一流程。独立字段的错误会收集到同一份结果中；父模型的自定义校验在其字段全部成功后执行。

数字字符串需要显式启用 `coerce`。JSON 数值 `25.0` 可用于整数，`25.5` 会失败；这与 JSON Schema 对数学整数的定义一致。范围比较保留 128 位整数精度；极大指数输入会经过有界处理。浮点数遵循 Rust 的 IEEE 754 表示，NaN、无穷值以及超出目标类型范围的数值会失败。

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

## 借用模型

带生命周期的声明使用同一个 `Model` derive，并通过 `BorrowModel` 解析。输入缓冲区需要覆盖模型的使用周期。

```rust
use rydantic::{BorrowModel, Model};
use std::borrow::Cow;

#[derive(Debug, Model)]
struct View<'a> {
    #[model(trim)]
    name: &'a str,
    #[model(trim, lowercase)]
    note: Cow<'a, str>,
}

let input = String::from(r#"{"name":" Alice ","note":" HELLO\nWORLD "}"#);
let view = View::parse_json(&input)?;
assert_eq!(view.name, "Alice");
assert_eq!(view.note, "hello\nworld");
```

`&str` 引用未转义的原始 JSON 字符串；`trim` 通过切片保留借用。需要解码转义或改变大小写时，使用 `Cow<str>`，它按需分配。需要分配的 `&str` 输入会返回 `borrowed_string`／`borrowed_transform` 错误。`before` 产生拥有所有权的 JSON 值，适合搭配 `Cow` 或拥有所有权的字段。

借用模型支持嵌套、递归、泛型、多生命周期、集合、flatten、Serde `from_str`／`from_slice`，以及 `BorrowValidated` 只读包装。`parse_slice` 从字节缓冲区借用；读取器入口用于拥有所有权的模型。

## Tuple struct 与 untagged enum

```rust
use rydantic::Model;

#[derive(Debug, Model)]
struct Username(#[model(trim, min_length = 2)] String);

#[derive(Debug, Model)]
struct Point(#[model(ge = 0)] i32, i32);

#[derive(Debug, Model)]
#[model(untagged)]
enum ResultValue {
    Count(#[model(ge = 1)] u32),
    Named { name: String },
    Pair(String, u8),
    Empty,
}
```

newtype 编码为内部值，多个 tuple 字段编码为固定长度数组，unit struct 编码为 `null`，空 tuple struct 编码为 `[]`。Tuple 字段支持校验和转换；名称、默认值、flatten 与 exclude 用在具名字段上。

Untagged 按声明顺序试解析，选择第一个通过同步校验的分支。失败时返回 `union_no_match` 和各分支的结构化错误；Schema 使用 `anyOf`。异步校验随后检查已选分支。内部标签枚举继续使用具名字段或 unit variant。

## Flatten 与额外字段保留

```rust
use rydantic::{Model, Value};
use std::collections::BTreeMap;

#[derive(Debug, Model)]
struct Address { city: String }

#[derive(Debug, Model)]
#[model(extra = "allow")]
struct Person {
    id: u32,
    #[model(flatten)]
    address: Option<Address>,
    #[model(extra)]
    extra: BTreeMap<String, Value>,
}

let person = Person::parse_json(r#"{"id":1,"city":"Paris","score":9}"#)?;
assert_eq!(person.extra["score"], 9);
assert_eq!(person.dump()?["city"], "Paris");
```

Flatten 支持具名模型、`Option`／`Box` 包装和字符串键的 `BTreeMap`／`HashMap`。先消费声明字段，再消费展开字段，最后由一个映射接收剩余键。允许多个展开的固定对象；每层允许一个接收未知键的字段。`extra = "allow"` 要求显式声明一个 `#[model(extra)]` 映射，映射值也会进行类型检查。

Optional flatten 在其所有键缺失时得到 `None`；出现部分键时检查内部必需字段。空的展开输入可以触发显式默认值。错误位置沿用展开后的 JSON 路径。声明字段的输入／输出名称会保留，额外字段键与这些名称重叠时返回 `field_collision`；序列化合并也检查键冲突。

## 日期与 Decimal

```toml
rydantic = { version = "0.2.0", features = ["chrono", "decimal"] }
```

`chrono` feature 支持 `NaiveDate`、`NaiveTime`、`NaiveDateTime`、`DateTime<Utc>`、`DateTime<FixedOffset>`，并从 `rydantic::chrono` 重导出类型。格式分别为 `YYYY-MM-DD`、`HH:MM:SS[.fraction]`、无时区的 ISO 日期时间、RFC 3339 日期时间。校验实际日历、四位年份、最多 9 位小数秒与 RFC 3339 时区；UTC 类型会换算到 UTC。

`decimal` feature 提供 `rydantic::rust_decimal::Decimal`，保留 96 位系数与最多 28 位小数。默认通过十进制字符串输入和输出，保留 scale；显式 `coerce` 也接受 JSON 数值。精度损失会失败，范围约束按精确十进制比较。

```rust
use rydantic::{Model, rust_decimal::Decimal};

#[derive(Debug, Model)]
struct Price {
    #[model(ge = "0.1000000000000000000000000001")]
    amount: Decimal,
}
```

日期语义、Decimal 精度和字符串数值范围会标记为 Schema 本地规则。运行 `cargo run -p rydantic --example dates_decimal --features chrono,decimal` 查看完整示例。

## 异步校验

```rust
use rydantic::{Model, ValidationError};

async fn available(name: &str) -> Result<(), ValidationError> {
    if name == "taken" {
        Err(ValidationError::new("taken", "name is already registered"))
    } else { Ok(()) }
}

#[derive(Debug, Model)]
struct Account {
    #[model(min_length = 2, async_validate = available)]
    name: String,
}

// 在 async 函数中：
// let account = Account::parse_json_async(input).await?;
// let json = account.dump_json_async().await?;
```

字段异步函数返回 `Result<(), ValidationError>`；容器异步函数返回 `Result<(), ValidationErrors>`。同步解析和校验全部成功后，按字段顺序等待异步校验；嵌套模型与集合会继续向内执行，同层字段错误会收集到一起。运行时采用 `Send` future，可搭配 Tokio 等执行器。

包含异步规则的模型及其父模型，使用 `parse_json_async`／`parse_value_async`、`validate_async`、`dump_async`／`dump_json_async`。同步解析、Serde 和同步 dump 会返回 `async_validation_required`。拥有所有权的模型可使用 `parse_validated_json_async`／`AsyncValidated::new`，其只读结果支持 Serde 序列化。借用模型也支持异步解析、校验和 dump。

derive 生成的异步支持要求字段类型实现 `AsyncCheck`，该 trait 要求 `Sync`。纯同步自定义类型可以添加空实现 `impl AsyncCheck for MyType {}`；自定义类型通过它提供异步遍历。存在异步规则时，`JsonType::async_required`／`BorrowType::async_required` 需返回对应元数据。库本身无需依赖执行器。`asynchronous` 示例使用开发依赖 Tokio。

## 流式解析

```rust
use rydantic::{stream, Model};
use std::io::Cursor;

#[derive(Debug, Model)]
struct Row { id: u32 }

for row in stream::ndjson::<_, Row>(Cursor::new("{\"id\":1}\n{\"id\":2}\n")) {
    println!("{}", row?.id);
}

stream::visit_array::<_, Row, _>(Cursor::new("[{\"id\":1},{\"id\":2}]"), |row| {
    println!("{}", row?.id);
    Ok(())
})?;
```

| 入口 | 输入与错误处理 |
| --- | --- |
| `Model::parse_reader` | 从 `Read` 读取单个 JSON 文档 |
| `stream::json_stream` | 空白分隔的文档序列；校验错误后继续，JSON 语法错误后终止 |
| `stream::ndjson` | 从 `BufRead` 逐行读取；空行跳过，语法／校验错误后继续下一行，附 `stream_line` |
| `stream::visit_array` | 逐元素回调；校验错误携带数组索引，回调返回错误即停止读取 |
| `stream::borrow_stream`／`visit_borrowed_array` | 从 `&str` 借用字段，返回值可在输入缓冲区存活期间保留 |
| `stream::documents` | 逐条返回 `Box<RawValue>`，可在循环中调用 `parse_json_async` |

这些入口使用同步 I/O。序列与数组访问器仅物化当前记录；内存随单条记录大小增长。读取器自身的缓冲策略由调用者决定。异步应用可在合适的 I/O 层接入，并逐条等待模型校验。同步 typed stream 遇到含异步规则的模型，会给出 `async_validation_required`。

运行示例：

```sh
cargo run -p rydantic --example borrowed
cargo run -p rydantic --example streaming
cargo run -p rydantic --example asynchronous
```

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
rydantic = { version = "0.2.0", features = ["dynamic"] }
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

## 原生 Cap’n Proto

启用 `capnp` feature 后，模型直接读取和构建[官方 Cap’n Proto](https://capnproto.org/) Schema 的字段。二进制布局与其他语言的官方实现互通，支持普通消息和 packed 消息。

```toml
[dependencies]
rydantic = { version = "0.2.0", features = ["capnp"] }
capnp = { version = "0.27", features = ["unaligned"] }

[build-dependencies]
capnpc = "0.27"
```

用 `capnp id` 产生自己的文件 ID，然后编写 `schema/user.capnp`：

```capnp
@0xc886a6cb051d0cab;
struct User {
  name @0 :Text;
  age @1 :UInt16;
}
```

在 `build.rs` 调用官方代码生成器；本机需要安装 `capnp` 编译器：

```rust
fn main() {
    println!("cargo:rerun-if-changed=schema/user.capnp");
    capnpc::CompilerCommand::new()
        .src_prefix("schema")
        .file("schema/user.capnp")
        .run()
        .expect("generate Cap'n Proto bindings");
}
```

在 crate 根模块引入生成的绑定，再关联模型：

```rust
pub mod user_capnp {
    include!(concat!(env!("OUT_DIR"), "/user_capnp.rs"));
}
use rydantic::{CapnpModel, Model};

#[derive(Debug, Model)]
#[model(capnp = "user_capnp::user")]
struct User {
    #[model(trim, min_length = 2)]
    name: String,
    #[model(ge = 18, le = 120)]
    age: u8,
}

fn main() -> Result<(), rydantic::ValidationErrors> {
    let user = User::parse_json(r#"{"name":" Alice ","age":25}"#)?;
    let bytes = user.dump_capnp()?;
    assert_eq!(User::parse_capnp(&bytes)?.name, "Alice");
    let packed = user.dump_capnp_packed()?;
    assert_eq!(User::parse_capnp_packed(&packed)?.age, 25);
    Ok(())
}
```

| Rust 模型 | 原生 Schema 字段 |
| --- | --- |
| 布尔、整数、浮点数、String | Bool、对应数值类型、Text；整数宽度转换检查溢出 |
| 嵌套模型、Box | 关联模型的 Struct |
| Vec、固定数组 | List；固定数组检查长度，支持嵌套列表和结构列表 |
| Option | 含 `union { none @0 :Void; some @1 :T; }` 的包装 Struct |
| 字符串键映射 | `List(Entry)`，Entry 包含 `key :Text` 和 `value :T`；拒绝重复键 |
| `#[model(capnp_data)] Vec<u8>` | Data |
| `#[model(capnp_text)]` | Text；适用于日期、Decimal、完整范围 i128/u128 的标量表示 |
| `#[model(capnp_enum)]` 字段 | 原生 Enum；Rust 枚举声明 `#[model(capnp_enum = "wire_capnp::Role")]` |
| 数据枚举 | 原生 union，支持 unit、单字段及具名 group 分支；JSON 可使用 `untagged` 或受支持的 `tag` 形式 |
| Newtype、tuple、unit struct | 分别关联含 `value`、`item0`／`item1` 字段的 Struct，以及空 Struct |

默认使用 Rust 字段名寻找生成的访问器，`#[model(capnp_name = "fullName")]` 可单独指定协议字段；`#[model(capnp_skip, default = "local")]` 保存模型内部字段。JSON 的别名、flatten、extra 与 exclude 控制 JSON 表示；原生字段布局由 `.capnp` 文件决定。

`parse_capnp` 执行模型规范化与同步校验，`dump_capnp` 重新校验当前对象。异步模型使用 `parse_capnp_async`／`dump_capnp_async` 及 packed 对应入口。`parse_capnp_with_options` 接受官方 `ReaderOptions`，可调整遍历与嵌套限制；单消息入口拒绝尾随数据。`from_capnp_reader` 接受官方生成的 Reader，`to_capnp_message` 返回可供官方 API 使用的消息 Builder。`read_capnp`／`write_capnp` 是组合映射用的底层接口，完整校验应使用上述公开解析／输出入口。

原生关联要求具体的拥有所有权的模型。构造 Rust 模型会分配 String 和集合；需要直接访问消息内存时可保留官方 Reader。协议缺省值遵循 Cap’n Proto 语义；可选值用显式 union 表达，`capnp_skip` 字段使用模型默认值。JSON 的 `before` 回调仍接收该字段的 Value，日期与 Decimal 的 `capnp_text` 映射复用标量转换规则。

完整 Schema、官方生成的绑定和可执行示例见 [fixtures/capnp](https://github.com/trotsky1997/Rydantic/tree/main/fixtures/capnp)。运行示例和官方命令行互操作测试：

```sh
cargo run -p rydantic-capnp-fixture
cargo test -p rydantic-capnp-fixture --test native
# 需要系统 capnp 编译器：
cargo test -p rydantic-capnp-fixture --test native -- --include-ignored
# 修改示例 Schema 后重新生成并格式化绑定：
RYDANTIC_REGENERATE_CAPNP=1 cargo check -p rydantic-capnp-fixture
cargo fmt --all
```

## 实现范围与升级

0.2 已覆盖 0.1 README 列出的八类后续功能：借用字段、tuple struct、untagged union、flatten、内置日期／Decimal、异步 validator、额外字段保留与流式解析，同时扩展到 128 位整数。

拥有所有权的模型使用 `JsonType`／`Model`，借用模型使用 `BorrowType<'de>`／`BorrowModel<'de>`。两条 JSON 解析路径均从 `RawValue` 的原始字段切片直接构造模型和集合，省去完整输入的 Value 树；类型默认值也直接遍历校验。拥有所有权的模型按需构造 String，借用模型可保留输入中的字符串引用。Reader 入口缓存单个文档的原文，流式入口保留当前记录。

显式 `Value` 字段、`parse_value` 输入、`before` 回调，以及仅实现 `decode(Value, …)` 的自定义类型按各自接口使用 Value。原生标量复用轻量标量适配器；自定义类型可以覆盖 `JsonType::decode_input`，直接接收 `Input::Borrowed`。无需展开的任意 JSON 可保存为 `Box<RawValue>`，保留原文与数值写法。

0.1 的拥有所有权的模型入口继续可用。`StringType`／`NumericType`／`LengthType` 作为独立能力标记，泛型辅助函数需要显式声明 `JsonType` 或 `BorrowType` 约束。 自定义字段类型还需实现 `AsyncCheck` 并满足 `Sync`；原有纯同步类型可使用空的 `AsyncCheck` 实现。可通过 crates.io 的 `rydantic = "0.2.0"` 安装，也可使用 GitHub 的 v0.2.0 标签；工作区内部使用本地路径依赖。

## 验证

```sh
cargo test --workspace --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo fmt --all -- --check
cargo check -p rydantic --no-default-features --lib
cargo run -p rydantic-renamed-fixture
```

测试包含解析／序列化完整流程、递归／泛型／借用模型、缺失与 null、规范化、默认值、128 位精确整数边界、日期与 Decimal、异步与取消恢复、流式增量读取、错误路径、独立 JSON Schema 验证器对照，以及编译失败用例。边界矩阵覆盖 1,400 组输入；独立验证器还检查成功输出与输出 Schema 的一致性。

License: MIT.
