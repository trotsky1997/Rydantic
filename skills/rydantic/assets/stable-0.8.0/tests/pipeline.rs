use rydantic::{
    json, pipeline::Pipeline, schema::openapi::OpenApiComponents, Model, SchemaMode,
    ValidationError,
};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

fn rejected() -> ValidationError {
    ValidationError::new("policy", "rejected")
}

#[test]
fn typed_pipeline_transforms_checks_and_short_circuits() {
    let calls = Arc::new(AtomicUsize::new(0));
    let later = calls.clone();
    let pipeline = Pipeline::plain(|s: &str, _: &u32| s.parse::<u32>().map_err(|_| rejected()))
        .check(|n, limit| if n <= limit { Ok(()) } else { Err(rejected()) })
        .then(move |n, _: &u32| {
            later.fetch_add(1, Ordering::SeqCst);
            Ok(n.to_string())
        });
    assert_eq!(pipeline.run("10", &10).unwrap(), "10");
    assert!(pipeline.run("11", &10).is_err());
    assert!(pipeline.run("bad", &10).is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn wrap_can_skip_or_retry_so_is_not_a_validation_guarantee() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let base = Pipeline::plain(move |n: u32, _: &()| {
        counter.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            Err(rejected())
        } else {
            Ok(n)
        }
    });
    assert_eq!(base.clone().wrap(|_, _, _| Ok(99)).run(0, &()).unwrap(), 99);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        base.wrap(|n, c, next| next(n, c).or_else(|_| next(1, c)))
            .run(0, &())
            .unwrap(),
        1
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[derive(Debug, Model)]
struct Item {
    #[model(ge = 1)]
    amount: u32,
}

#[test]
fn full_model_stage_keeps_typed_constraints_and_openapi_is_schema_only() {
    let pipeline = Pipeline::plain(|value, _: &()| Item::parse_value(value));
    assert_eq!(pipeline.run(json!({"amount":1}), &()).unwrap().amount, 1);
    assert!(pipeline.run(json!({"amount":0}), &()).is_err());
    let mut components = OpenApiComponents::new();
    let input = components
        .insert::<Item>("ItemInput", SchemaMode::Input)
        .unwrap();
    assert_eq!(
        input.reference,
        json!({"$ref":"#/components/schemas/ItemInput"})
    );
    assert!(components.into_schemas().get("ItemInput").is_some());
}
