// Evaluator-only behavioral contract. Never stage this file for an agent.
use eval_candidate::{Policy, Request};
use rydantic::{Model, ValidationErrors};
use serde_json::json;
use std::sync::Arc;

fn error_at_amount<T>(result: Result<T, ValidationErrors>, code: &str) {
    let errors = match result {
        Err(errors) => errors,
        Ok(_) => panic!("expected {code}, but validation succeeded"),
    };
    assert!(
        errors
            .errors()
            .iter()
            .any(|error| error.code == code && error.pointer() == "/amount"),
        "missing {code} at /amount: {errors}"
    );
}

#[test]
fn policy_is_per_call_and_inclusive() {
    for maximum in [0, 7, 20, 125] {
        for amount in [0, maximum] {
            let request = Request::parse_value_with_context(
                json!({"amount": amount}),
                Arc::new(Policy { maximum }),
            )
            .unwrap();
            assert_eq!(request.amount, amount);
        }
        error_at_amount(
            Request::parse_value_with_context(
                json!({"amount": maximum + 1}),
                Arc::new(Policy { maximum }),
            ),
            "policy_limit",
        );
    }
    // Returning to an earlier policy must not pick up another call's context.
    assert!(Request::parse_value_with_context(
        json!({"amount": 7}),
        Arc::new(Policy { maximum: 7 }),
    )
    .is_ok());
}

#[test]
fn typed_input_and_nonnegative_amount_remain_required() {
    for input in [
        json!({}),
        json!({"amount": null}),
        json!({"amount": "7"}),
        json!({"amount": 1.5}),
        json!({"amount": true}),
        json!({"amount": []}),
        json!({"amount": {}}),
        json!({"amount": -1}),
    ] {
        assert!(
            Request::parse_value_with_context(input.clone(), Arc::new(Policy { maximum: 20 }),)
                .is_err(),
            "accepted invalid input: {input}"
        );
    }
}

#[test]
fn missing_or_wrong_context_is_an_error_not_a_default_policy() {
    error_at_amount(
        Request::parse_value(json!({"amount": 0})),
        "context_missing",
    );
    error_at_amount(
        Request::parse_value_with_context(json!({"amount": 0}), Arc::new(20_i64)),
        "context_type_mismatch",
    );
}

#[test]
fn revalidation_uses_current_value_and_new_context() {
    let policy = Arc::new(Policy { maximum: 20 });
    let mut request =
        Request::parse_value_with_context(json!({"amount": 10}), policy.clone()).unwrap();
    assert_eq!(
        Arc::strong_count(&policy),
        1,
        "model retained per-call context"
    );
    request
        .validate_with_context(Arc::new(Policy { maximum: 10 }))
        .unwrap();
    error_at_amount(
        request.validate_with_context(Arc::new(Policy { maximum: 9 })),
        "policy_limit",
    );
    error_at_amount(request.validate(), "context_missing");
    error_at_amount(
        request.validate_with_context(Arc::new(())),
        "context_type_mismatch",
    );
    request.amount = 21;
    error_at_amount(
        request.validate_with_context(policy.clone()),
        "policy_limit",
    );
    request.amount = -1;
    assert!(request.validate_with_context(policy.clone()).is_err());
    request.amount = 0;
    request
        .validate_with_context(Arc::new(Policy { maximum: 0 }))
        .unwrap();
    assert_eq!(Arc::strong_count(&policy), 1);
}
