//! Error paths must preserve diagnostics and never continue after terminal failures.
use rydantic::{stream, Model, Stage, ValidationError, ValidationErrors};
use std::io::{self, BufRead, Cursor, Read};

#[derive(Debug, Model)]
struct Row {
    #[model(ge = 1)]
    id: u32,
}

#[derive(Debug, Model)]
struct Borrowed<'a> {
    text: &'a str,
}

struct FailedReader;
impl Read for FailedReader {
    fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::other("synthetic read failure"))
    }
}
impl BufRead for FailedReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        Err(io::Error::other("synthetic read failure"))
    }
    fn consume(&mut self, _: usize) {}
}

fn assert_parse_error(errors: &ValidationErrors, pointer: &str) {
    assert_eq!(errors.len(), 1);
    assert_eq!(errors.errors()[0].code, "json_invalid");
    assert_eq!(errors.errors()[0].stage, Stage::Parse);
    assert_eq!(errors.errors()[0].pointer(), pointer);
}

#[test]
fn ndjson_io_error_is_reported_once_and_is_terminal() {
    let mut rows = stream::ndjson::<_, Row>(FailedReader);
    let errors = rows.next().unwrap().unwrap_err();
    assert_eq!(errors.len(), 1);
    assert_eq!(errors.errors()[0].code, "io_error");
    assert_eq!(errors.errors()[0].stage, Stage::Parse);
    assert_eq!(errors.errors()[0].message, "synthetic read failure");
    assert!(rows.next().is_none());
    assert!(rows.next().is_none());
}

#[test]
fn borrowed_sequence_recovers_validation_but_stops_after_syntax_error() {
    let mut rows = stream::borrow_stream::<Borrowed<'_>>(
        r#"{"text":"first"} {"text":42} {"text":"third"} {oops} {"text":"unread"}"#,
    );
    assert_eq!(rows.next().unwrap().unwrap().text, "first");
    let errors = rows.next().unwrap().unwrap_err();
    assert_eq!(errors.errors()[0].pointer(), "/1/text");
    assert_eq!(errors.errors()[0].code, "borrowed_string");
    assert_eq!(rows.next().unwrap().unwrap().text, "third");
    assert_parse_error(&rows.next().unwrap().unwrap_err(), "/3");
    assert!(rows.next().is_none());
}

#[test]
fn borrowed_array_preserves_callback_errors_and_stops_before_bad_tail() {
    let sentinel = ValidationErrors::new(vec![
        ValidationError::new("stop", "caller stopped")
            .at("caller")
            .with_param("reason", 7),
        ValidationError::new("second", "keep ordering").with_stage(Stage::Serialize),
    ]);
    let mut calls = 0;
    let errors = stream::visit_borrowed_array::<Borrowed<'_>, _>(
        r#"[{"text":"first"}, broken tail"#,
        |row| {
            assert_eq!(row.unwrap().text, "first");
            calls += 1;
            Err(sentinel.clone())
        },
    )
    .unwrap_err();
    assert_eq!(calls, 1);
    assert_eq!(errors, sentinel);
}

#[test]
fn borrowed_array_counts_invalid_elements_and_checks_trailing_input() {
    let mut paths = vec![];
    let count = stream::visit_borrowed_array::<Borrowed<'_>, _>(
        r#"[{"text":"first"},{"text":42},{"text":"third"}]"#,
        |row| {
            paths.push(
                row.map(|x| x.text.to_owned())
                    .map_err(|e| e.errors()[0].pointer()),
            );
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(count, 3);
    assert_eq!(
        paths,
        vec![
            Ok("first".into()),
            Err("/1/text".into()),
            Ok("third".into())
        ]
    );
    for input in ["[] trailing", "{}", "[", "[{}"] {
        let errors =
            stream::visit_borrowed_array::<Borrowed<'_>, _>(input, |_| Ok(())).unwrap_err();
        assert_parse_error(&errors, "");
    }
    assert_eq!(
        stream::visit_borrowed_array::<Borrowed<'_>, _>("[]", |_| panic!("empty array")).unwrap(),
        0
    );
}

#[test]
fn raw_documents_reject_trailing_data_and_index_terminal_errors() {
    assert_eq!(
        stream::read_document(Cursor::new("  {\"x\": 1}  "))
            .unwrap()
            .get(),
        "{\"x\": 1}"
    );
    for input in ["", "{} {}", "{"] {
        assert_parse_error(&stream::read_document(Cursor::new(input)).unwrap_err(), "");
    }
    assert_parse_error(&stream::read_document(FailedReader).unwrap_err(), "");
    let mut documents = stream::documents(Cursor::new("{} {oops} {}"));
    assert_eq!(documents.next().unwrap().unwrap().get(), "{}");
    assert_parse_error(&documents.next().unwrap().unwrap_err(), "/1");
    assert!(documents.next().is_none());
    let mut failed = stream::documents(FailedReader);
    assert_parse_error(&failed.next().unwrap().unwrap_err(), "/0");
    assert!(failed.next().is_none());
}

#[test]
fn owned_array_rejects_non_arrays_and_reader_errors_without_callbacks() {
    for input in ["{}", "null", "42"] {
        let errors =
            stream::visit_array::<_, Row, _>(Cursor::new(input), |_| panic!("not an array"))
                .unwrap_err();
        assert_parse_error(&errors, "");
    }
    let errors =
        stream::visit_array::<_, Row, _>(FailedReader, |_| panic!("read failed")).unwrap_err();
    assert_parse_error(&errors, "");
}
