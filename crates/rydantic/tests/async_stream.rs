use rydantic::{json, AsyncCheck, Context, Model, SchemaMode, ValidationError, ValidationErrors};
use std::{
    cell::Cell,
    io::{self, BufReader, Cursor, Read},
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};
static CALLS: AtomicUsize = AtomicUsize::new(0);
async fn available(value: &String) -> Result<(), ValidationError> {
    CALLS.fetch_add(1, Ordering::Relaxed);
    tokio::task::yield_now().await;
    if value == "taken" {
        Err(ValidationError::new("taken", "name is already registered"))
    } else {
        Ok(())
    }
}
async fn check_model(value: &Account) -> Result<(), ValidationErrors> {
    tokio::task::yield_now().await;
    if value.name == value.other {
        Err(ValidationError::custom("names must differ")
            .at("other")
            .into())
    } else {
        Ok(())
    }
}
#[derive(Debug, Model)]
#[model(async_validate=check_model)]
struct Account {
    #[model(min_length=2,async_validate=available)]
    name: String,
    #[model(async_validate=available)]
    other: String,
}
#[derive(Debug, Model)]
struct Batch {
    items: Vec<Account>,
}
#[tokio::test]
async fn async_checks_are_awaited_and_nested_errors_are_collected() {
    assert_eq!(
        Account::parse_json(r#"{"name":"ok","other":"free"}"#)
            .unwrap_err()
            .errors()[0]
            .code,
        "async_validation_required"
    );
    assert!(serde_json::from_str::<Account>(r#"{"name":"ok","other":"free"}"#).is_err());
    let result = tokio::spawn(async {
        Batch::parse_json_async(r#"{"items":[{"name":"taken","other":"taken"}]}"#).await
    })
    .await
    .unwrap()
    .unwrap_err();
    assert_eq!(result.len(), 2);
    assert_eq!(result.errors()[0].pointer(), "/items/0/name");
    assert_eq!(result.errors()[1].pointer(), "/items/0/other");
    assert_eq!(
        Account::parse_json_async(r#"{"name":"same","other":"same"}"#)
            .await
            .unwrap_err()
            .errors()[0]
            .pointer(),
        "/other"
    );
    let x = Account::parse_json_async(r#"{"name":"ok","other":"free"}"#)
        .await
        .unwrap();
    assert!(x.dump().is_err());
    assert_eq!(x.dump_async().await.unwrap()["name"], "ok");
    let checked = Account::parse_validated_json_async(r#"{"name":"ok","other":"free"}"#)
        .await
        .unwrap();
    assert_eq!(serde_json::to_value(&checked).unwrap()["name"], "ok");
    let report = Account::schema_report(SchemaMode::Input);
    assert!(report
        .local_rules
        .iter()
        .any(|r| r.rule.contains("async_validate")));
}
#[tokio::test]
async fn synchronous_failures_skip_async_work() {
    // A separate callback counter keeps this test independent from other runtime tests.
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    async fn count(_: &String) -> Result<(), ValidationError> {
        COUNT.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    #[derive(Debug, Model)]
    struct Check {
        #[model(min_length=2,async_validate=count)]
        value: String,
    }
    assert!(Check::parse_json_async(r#"{"value":"x"}"#).await.is_err());
    assert_eq!(COUNT.load(Ordering::Relaxed), 0);
}
#[tokio::test]
async fn cancelling_async_validation_does_not_leak_a_path_into_context() {
    async fn pending(_: &String) -> Result<(), ValidationError> {
        std::future::pending().await
    }
    #[derive(Model)]
    struct Wait {
        #[model(async_validate=pending)]
        name: String,
    }
    let x = Wait { name: "x".into() };
    let mut ctx = Context::new();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(5), x.check_async(&mut ctx))
            .await
            .is_err()
    );
    ctx.push(ValidationError::custom("root"));
    assert_eq!(ctx.result().unwrap_err().errors()[0].pointer(), "");
}
#[tokio::test]
async fn borrowed_models_can_run_async_checks() {
    use rydantic::BorrowModel;
    async fn check(v: &&str) -> Result<(), ValidationError> {
        tokio::task::yield_now().await;
        if *v == "bad" {
            Err(ValidationError::custom("bad name"))
        } else {
            Ok(())
        }
    }
    #[derive(Debug, Model)]
    struct Borrowed<'a> {
        #[model(async_validate=check)]
        name: &'a str,
    }
    let input = String::from(r#"{"name":"good"}"#);
    let x = Borrowed::parse_json_async(&input).await.unwrap();
    assert_eq!(x.name, "good");
    assert_eq!(x.dump_async().await.unwrap()["name"], "good");
    assert!(Borrowed::parse_json(&input).is_err());
}
#[derive(Debug, PartialEq, Model)]
struct Row {
    #[model(ge = 1)]
    id: u32,
}
struct ChunkReader {
    data: Cursor<Vec<u8>>,
    read: Rc<Cell<usize>>,
    chunk: usize,
}
impl Read for ChunkReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let len = buffer.len().min(self.chunk);
        let n = self.data.read(&mut buffer[..len])?;
        self.read.set(self.read.get() + n);
        Ok(n)
    }
}
fn reader(text: &str, chunk: usize) -> (ChunkReader, Rc<Cell<usize>>) {
    let read = Rc::new(Cell::new(0));
    (
        ChunkReader {
            data: Cursor::new(text.as_bytes().to_vec()),
            read: read.clone(),
            chunk,
        },
        read,
    )
}
#[test]
fn sequences_start_before_eof_and_recover_from_validation_errors() {
    let text = r#"{"id":1} {"id":0} {"id":3}"#;
    let (reader, count) = reader(text, 1);
    let mut stream = rydantic::stream::json_stream::<_, Row>(reader);
    assert_eq!(stream.next().unwrap().unwrap().id, 1);
    assert!(count.get() < text.len());
    assert_eq!(
        stream.next().unwrap().unwrap_err().errors()[0].pointer(),
        "/1/id"
    );
    assert_eq!(stream.next().unwrap().unwrap().id, 3);
    assert!(stream.next().is_none());
    let mut broken =
        rydantic::stream::json_stream::<_, Row>(Cursor::new(r#"{"id":1} {oops} {"id":3}"#));
    assert!(broken.next().unwrap().is_ok());
    assert!(broken.next().unwrap().is_err());
    assert!(broken.next().is_none());
}
#[test]
fn ndjson_recovers_after_syntax_and_validation_errors() {
    let input = "\n{\"id\":1}\n{oops}\n{\"id\":0}\n{\"id\":4}\n";
    let rows =
        rydantic::stream::ndjson::<_, Row>(BufReader::new(Cursor::new(input))).collect::<Vec<_>>();
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].as_ref().unwrap().id, 1);
    assert_eq!(
        rows[1].as_ref().unwrap_err().errors()[0].params["stream_line"],
        3
    );
    assert!(rows[2].is_err());
    assert_eq!(rows[3].as_ref().unwrap().id, 4);
}
#[test]
fn array_visitor_is_incremental_and_can_stop_early() {
    let input = r#"[{"id":1},{"id":0},{"id":3}]"#;
    let (reader, read) = reader(input, 1);
    let mut values = vec![];
    let count = rydantic::stream::visit_array::<_, Row, _>(reader, |row| {
        if values.is_empty() {
            assert!(read.get() < input.len());
        }
        values.push(row);
        Ok(())
    })
    .unwrap();
    assert_eq!(count, 3);
    assert_eq!(
        values[1].as_ref().unwrap_err().errors()[0].pointer(),
        "/1/id"
    );
    let (reader, read) = self::reader(input, 1);
    let err = rydantic::stream::visit_array::<_, Row, _>(reader, |_| {
        Err(ValidationError::new("stop", "done").into())
    })
    .unwrap_err();
    assert_eq!(err.errors()[0].code, "stop");
    assert!(read.get() < input.len());
    assert!(
        rydantic::stream::visit_array::<_, Row, _>(Cursor::new("[] trailing"), |_| Ok(())).is_err()
    );
    assert_eq!(Row::parse_reader(Cursor::new(r#"{"id":2}"#)).unwrap().id, 2);
}
#[test]
fn borrowed_sequence_retains_references() {
    use rydantic::BorrowModel;
    #[derive(Debug, Model)]
    struct Message<'a> {
        text: &'a str,
    }
    let input = String::from("{\"text\":\"one\"} {\"text\":\"two\"}");
    let messages = rydantic::stream::borrow_stream::<Message<'_>>(&input)
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(messages[1].text, "two");
    assert_eq!(Message::parse_json(r#"{"text":"ok"}"#).unwrap().text, "ok");
}
#[test]
fn stream_errors_never_include_raw_sensitive_records() {
    let errors = rydantic::stream::ndjson::<_, Row>(Cursor::new("{\"id\":\"sensitive-token\"}\n"))
        .next()
        .unwrap()
        .unwrap_err();
    assert!(!errors.to_json().to_string().contains("sensitive-token"));
    assert!(errors.to_json().is_array());
    assert_eq!(json!(errors.errors()[0].pointer()), "/id");
}

#[test]
fn borrowed_array_visitor_can_return_values_that_outlive_the_callback() {
    #[derive(Debug, Model)]
    struct Text<'a> {
        value: &'a str,
    }
    let input = String::from(r#"[{"value":"first"},{"value":"second"}]"#);
    let mut values = vec![];
    let count = rydantic::stream::visit_borrowed_array::<Text<'_>, _>(&input, |value| {
        values.push(value?);
        Ok(())
    })
    .unwrap();
    assert_eq!(count, 2);
    assert_eq!(values[0].value, "first");
    assert_eq!(values[1].value, "second");
}
#[tokio::test]
async fn streaming_documents_compose_with_async_validation() {
    let input = r#"{"name":"one","other":"two"} {"name":"taken","other":"free"}"#;
    let mut results = vec![];
    for document in rydantic::stream::documents(Cursor::new(input)) {
        let raw = document.unwrap();
        results.push(Account::parse_json_async(raw.get()).await);
    }
    assert!(results[0].is_ok());
    assert_eq!(results[1].as_ref().unwrap_err().errors()[0].code, "taken");
}
