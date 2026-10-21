use rydantic::{json, Context, Model, ValidationError};
use std::sync::{Arc, Mutex};

struct Audit {
    accept_fallback: bool,
    events: Mutex<Vec<&'static str>>,
}
fn reject_first(_: &i64, ctx: &Context) -> Result<(), ValidationError> {
    ctx.require_data::<Audit>()?
        .events
        .lock()
        .unwrap()
        .push("first");
    Err(ValidationError::new("first_rejected", "try the fallback"))
}
fn check_fallback(_: &i64, ctx: &Context) -> Result<(), ValidationError> {
    let audit = ctx.require_data::<Audit>()?;
    let mut events = audit.events.lock().unwrap();
    assert_eq!(&*events, &["first"]);
    events.push("fallback");
    if audit.accept_fallback {
        Ok(())
    } else {
        Err(ValidationError::new(
            "fallback_rejected",
            "no branch accepted",
        ))
    }
}
#[derive(Debug, Model)]
#[model(untagged)]
enum Choice {
    First {
        #[model(validate_with_context = reject_first)]
        value: i64,
    },
    Fallback {
        #[model(validate_with_context = check_fallback)]
        value: i64,
    },
}
#[derive(Debug, Model)]
struct Envelope {
    choices: Vec<Choice>,
}

#[test]
fn failed_untagged_branch_preserves_shared_effects_but_not_errors() {
    for raw in [false, true] {
        let audit = Arc::new(Audit {
            accept_fallback: true,
            events: Mutex::new(vec![]),
        });
        let result = if raw {
            Envelope::parse_json_with_context(r#"{"choices":[{"value":3}]}"#, audit.clone())
        } else {
            Envelope::parse_value_with_context(json!({"choices":[{"value":3}]}), audit.clone())
        };
        let model = result.unwrap();
        assert!(matches!(model.choices[0], Choice::Fallback { value: 3 }));
        assert_eq!(&*audit.events.lock().unwrap(), &["first", "fallback"]);
        assert_eq!(Arc::strong_count(&audit), 1);
    }
}
#[test]
fn failed_untagged_branches_keep_relative_errors_and_release_context() {
    for raw in [false, true] {
        let audit = Arc::new(Audit {
            accept_fallback: false,
            events: Mutex::new(vec![]),
        });
        let result = if raw {
            Envelope::parse_json_with_context(r#"{"choices":[{"value":3}]}"#, audit.clone())
        } else {
            Envelope::parse_value_with_context(json!({"choices":[{"value":3}]}), audit.clone())
        };
        let errors = result.unwrap_err();
        assert_eq!(errors.len(), 1);
        let error = &errors.errors()[0];
        assert_eq!(error.code, "union_no_match");
        assert_eq!(error.pointer(), "/choices/0");
        let branches = error.params["branches"].as_array().unwrap();
        assert_eq!(branches.len(), 2);
        for (branch, code) in branches.iter().zip(["first_rejected", "fallback_rejected"]) {
            let failures = branch["errors"].as_array().unwrap();
            assert_eq!(failures.len(), 1);
            assert_eq!(failures[0]["loc"], json!(["value"]));
            assert_eq!(failures[0]["code"], code);
        }
        assert_eq!(&*audit.events.lock().unwrap(), &["first", "fallback"]);
        assert_eq!(Arc::strong_count(&audit), 1);
    }
}
