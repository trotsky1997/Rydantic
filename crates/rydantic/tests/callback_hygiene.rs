use rydantic::{
    json, BorrowModel, Context, JsonType, Model, ValidationError, ValidationErrors, Value,
};

fn raw(input: Value) -> Result<Value, ValidationError> {
    match input.as_u64() {
        Some(n) => Ok(json!(n + 1)),
        None => Err(ValidationError::custom("expected an unsigned integer")),
    }
}
fn value(n: &u32) -> Result<(), ValidationError> {
    if n.is_multiple_of(2) {
        Ok(())
    } else {
        Err(ValidationError::custom("expected an even integer"))
    }
}
#[derive(Debug, PartialEq, Model)]
struct Named {
    #[model(before = raw, validate = value, ge = 2)]
    n: u32,
}
#[derive(Debug, PartialEq, Model)]
struct Tuple(#[model(before = raw, validate = value)] u32, bool);
#[derive(Debug, PartialEq, Model)]
struct Newtype(#[model(before = raw, validate = value)] u32);
#[derive(Debug, PartialEq, Model)]
#[model(untagged)]
enum Untagged {
    Item(#[model(before = raw, validate = value)] u32),
}
#[derive(Debug, PartialEq, Model)]
#[model(tag = "kind")]
enum Tagged {
    Item {
        #[model(before = raw, validate = value)]
        n: u32,
    },
}

#[test]
fn bare_callbacks_preserve_parsing_validation_and_error_paths() {
    assert_eq!(Named::parse_json(r#"{"n":1}"#).unwrap(), Named { n: 2 });
    assert_eq!(Named::parse_value(json!({"n": 1})).unwrap().n, 2);
    assert_eq!(
        Tuple::parse_value(json!([1, true])).unwrap(),
        Tuple(2, true)
    );
    assert_eq!(Newtype::parse_value(json!(1)).unwrap(), Newtype(2));
    assert_eq!(Untagged::parse_value(json!(1)).unwrap(), Untagged::Item(2));
    assert_eq!(
        Tagged::parse_value(json!({"kind":"Item", "n":1})).unwrap(),
        Tagged::Item { n: 2 }
    );
    let errors = Named::parse_value(json!({"n": 2})).unwrap_err();
    assert_eq!(errors.errors()[0].pointer(), "/n");
    assert_eq!(errors.errors()[0].message, "expected an even integer");
    let errors = Named::parse_value(json!({"n": "bad"})).unwrap_err();
    assert_eq!(errors.errors()[0].pointer(), "/n");
    assert_eq!(errors.errors()[0].stage, rydantic::Stage::Parse);
    assert!(Named { n: 2 }.validate().is_ok());
    assert!(Named { n: 3 }.validate().is_err());
    assert_eq!(Named { n: 2 }.dump().unwrap(), json!({"n": 2}));
}

fn __rydantic_ctx() -> Named {
    Named { n: 1 }
}
fn __rydantic_field_0() -> u32 {
    1
}
#[derive(Debug, Model)]
struct Defaults {
    #[model(default_factory = __rydantic_ctx)]
    nested: Named,
    #[model(default_factory = __rydantic_field_0, before = raw, validate = value)]
    n: u32,
}
#[test]
fn typed_defaults_run_bare_callbacks_without_changing_normalization() {
    let parsed = Defaults::parse_value(json!({})).unwrap();
    assert_eq!(parsed.nested.n, 2);
    assert_eq!(parsed.n, 2);
    let mut ctx = Context::new();
    assert_eq!(
        JsonType::prepare_default(Named { n: 1 }, &mut ctx),
        Some(Named { n: 2 })
    );
    assert!(ctx.result().is_ok());
}

#[derive(Debug, Model)]
struct Borrowed<'a> {
    text: &'a str,
    #[model(before = raw, validate = value)]
    n: u32,
}
#[derive(Debug, Model)]
struct Generic<T> {
    items: Vec<T>,
    #[model(before = raw, validate = value)]
    n: u32,
}
#[test]
fn borrowed_generic_and_nested_forms_keep_caller_resolution() {
    let text = String::from(r#"{"text":"borrowed","n":1}"#);
    let parsed = Borrowed::parse_json(&text).unwrap();
    assert_eq!(parsed.text, "borrowed");
    assert_eq!(parsed.n, 2);
    assert!(std::ptr::eq(parsed.text.as_ptr(), text[9..].as_ptr()));
    let parsed = Generic::<Named>::parse_value(json!({"items":[{"n":1}],"n":1})).unwrap();
    assert_eq!(parsed.items[0].n, 2);
    let errors = Generic::<Named>::parse_value(json!({"items":[{"n":2}],"n":1})).unwrap_err();
    assert_eq!(errors.errors()[0].pointer(), "/items/0/n");
}

mod paths {
    use super::raw as value;
    use super::*;
    #[derive(Debug, Model)]
    pub struct Qualified {
        #[model(before = value, validate = super::value)]
        pub imported: u32,
        #[model(before = crate::raw, validate = Self::context)]
        pub associated: u32,
    }
    impl Qualified {
        fn context(n: &u32) -> Result<(), ValidationError> {
            super::value(n)
        }
    }
}
#[test]
fn imported_qualified_and_associated_paths_remain_valid() {
    let parsed = paths::Qualified::parse_value(json!({"imported":1,"associated":1})).unwrap();
    assert_eq!((parsed.imported, parsed.associated), (2, 2));
}

mod model_callbacks {
    use super::*;
    fn value(model: &Checked) -> Result<(), ValidationErrors> {
        if model.n == 2 {
            Ok(())
        } else {
            Err(ValidationError::custom("model rejected").into())
        }
    }
    #[derive(Debug, Model)]
    #[model(validate = value)]
    struct Checked {
        #[model(before = raw)]
        n: u32,
    }
    #[test]
    fn model_callback_named_value_runs_for_parse_default_and_revalidation() {
        assert!(Checked::parse_value(json!({"n":1})).is_ok());
        assert!(Checked::parse_value(json!({"n":2})).is_err());
        assert!(Checked { n: 2 }.validate().is_ok());
        assert!(Checked { n: 3 }.validate().is_err());
        let mut ctx = Context::new();
        assert!(JsonType::prepare_default(Checked { n: 1 }, &mut ctx).is_some());
        assert!(ctx.result().is_ok());
    }
}

mod asynchronous {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static CALLS: AtomicUsize = AtomicUsize::new(0);
    async fn child(n: &u32) -> Result<(), ValidationError> {
        CALLS.fetch_add(1, Ordering::SeqCst);
        tokio::task::yield_now().await;
        if *n == 4 {
            Err(ValidationError::custom("async field rejected"))
        } else {
            Ok(())
        }
    }
    async fn start(model: &Checked) -> Result<(), ValidationErrors> {
        tokio::task::yield_now().await;
        if model.n == 6 {
            Err(ValidationError::custom("async model rejected").into())
        } else {
            Ok(())
        }
    }
    #[derive(Debug, Model)]
    #[model(async_validate = start)]
    struct Checked {
        #[model(before = raw, validate = value, async_validate = child)]
        n: u32,
    }
    #[derive(Debug, Model)]
    struct Outer {
        items: Vec<Checked>,
    }
    #[tokio::test]
    async fn async_callbacks_keep_order_awaiting_skips_and_nested_paths() {
        let parsed = Checked::parse_value_async(json!({"n":1})).await.unwrap();
        assert_eq!(parsed.n, 2);
        assert_eq!(CALLS.load(Ordering::SeqCst), 1);
        let errors = Outer::parse_value_async(json!({"items":[{"n":3}]}))
            .await
            .unwrap_err();
        assert_eq!(errors.errors()[0].pointer(), "/items/0/n");
        assert_eq!(errors.errors()[0].message, "async field rejected");
        let errors = Checked::parse_value_async(json!({"n":5}))
            .await
            .unwrap_err();
        assert_eq!(errors.errors()[0].pointer(), "");
        assert_eq!(errors.errors()[0].message, "async model rejected");
        let calls = CALLS.load(Ordering::SeqCst);
        assert!(Checked::parse_value_async(json!({"n":2})).await.is_err());
        assert_eq!(CALLS.load(Ordering::SeqCst), calls);
        assert_eq!(
            Checked::parse_value(json!({"n":1})).unwrap_err().errors()[0].code,
            "async_validation_required"
        );
    }
}
