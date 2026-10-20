use rydantic::Model;
use rydantic_ai::{
    jev::{Content, Question, SystemOneRequest, SystemOneResponse},
    Error, RunContext,
};
use serde_json::{json, Value};

fn request() -> SystemOneRequest {
    SystemOneRequest::new(
        "jev-1.14.0",
        "Payment failed for three days. Please help now.",
    )
    .question(
        "urgency",
        Question::noul("Does the customer request urgent help?"),
    )
    .unwrap()
    .question(
        "department",
        Question::choice(
            "Which team should handle this?",
            [
                ("billing", "Payment issues"),
                ("technical", "Bugs or integrations"),
            ],
        ),
    )
    .unwrap()
    .question(
        "frustration",
        Question::score(
            "How frustrated is the customer?",
            ["Calm", "Frustrated", "Very angry"],
        ),
    )
    .unwrap()
}
fn response() -> Value {
    json!({"model":"jev-1.14.0","answers":{
        "urgency":{"type":"noul","noul":0.96},
        "department":{"type":"choice","choice":"billing","confidence":1.0,"probabilities":{"billing":1.0,"technical":0.0}},
        "frustration":{"type":"score","score":1.0,"confidence":0.98,"legend":{"0":"Calm","1":"Frustrated","2":"Very angry"},"probabilities":{"0":0.0,"1":0.99,"2":0.01}}
    },"usage":{"input_tokens":377,"output_tokens":64}})
}

#[test]
fn three_primitives_match_the_native_request_and_preserve_all_answer_fields() {
    let request = request();
    request.validate().unwrap();
    let value = serde_json::to_value(&request).unwrap();
    assert_eq!(
        value["questions"]["urgency"],
        json!({"type":"noul","instructions":"Does the customer request urgent help?"})
    );
    assert_eq!(
        value["questions"]["department"]["criteria"],
        json!({"billing":"Payment issues","technical":"Bugs or integrations"})
    );
    assert_eq!(
        value["questions"]["frustration"]["criteria"],
        json!(["Calm", "Frustrated", "Very angry"])
    );
    let answer: SystemOneResponse = serde_json::from_value(response()).unwrap();
    answer.validate_for(&request).unwrap();
    assert_eq!(answer.noul("urgency").unwrap().noul, 0.96);
    assert_eq!(answer.choice("department").unwrap().choice, "billing");
    assert_eq!(answer.score("frustration").unwrap().score, 1.0);
    assert_eq!(answer.usage.total().unwrap(), 441);
    assert!(answer.score("urgency").is_err());
    assert!(answer.noul("missing").is_err());
    assert_eq!(serde_json::to_value(answer).unwrap(), response());
}

#[test]
fn content_and_criteria_follow_the_current_openapi_contract() {
    let request:SystemOneRequest=serde_json::from_value(json!({"model":"jev-latest","state":{"ticket":["a",1,true]},"questions":{
        "yes":{"type":"noul","instructions":null,"criteria":{"true":{"rule":"yes"},"false":["no"]}},
        "label":{"type":"choice","criteria":{"only":null}},
        "level":{"type":"score","criteria":[{"severity":"none"}]}
    }})).unwrap();
    request.validate().unwrap();
    let wire = serde_json::to_vec(&request).unwrap();
    assert_eq!(
        serde_json::from_slice::<SystemOneRequest>(&wire).unwrap(),
        request
    );
    let precise = SystemOneRequest::new(
        "m",
        Content::try_from(json!({"integer":u64::MAX,"float":0.96})).unwrap(),
    )
    .question(
        "p",
        Question::noul(Content::try_from(json!({"threshold":0.96})).unwrap()),
    )
    .unwrap();
    assert_eq!(
        serde_json::from_slice::<SystemOneRequest>(&serde_json::to_vec(&precise).unwrap()).unwrap(),
        precise
    );
    assert!(Content::try_from(Value::Null).is_err());
    assert!(Content::try_from(json!(123)).is_err());
    assert!(Content::try_from(json!(true)).is_err());
    for question in [
        json!({"type":"choice","criteria":["a","b"]}),
        json!({"type":"score","criteria":{"0":"a"}}),
        json!({"type":"score","criteria":[null]}),
        json!({"type":"noul","criteria":{"unknown":"value"}}),
    ] {
        assert!(serde_json::from_value::<Question>(question).is_err());
    }
    assert!(SystemOneRequest::new("jev-latest", "x").validate().is_err());
    assert!(SystemOneRequest::new(" ", "x")
        .question("q", Question::noul("x"))
        .unwrap()
        .validate()
        .is_err());
    assert!(SystemOneRequest::new("m", "x")
        .question("q", Question::score("x", Vec::<String>::new()))
        .is_err());
    assert!(SystemOneRequest::new("m", "x")
        .question("q", Question::score("x", vec!["a"; 11]))
        .is_err());
    assert!(request
        .clone()
        .question("yes", Question::noul("duplicate"))
        .is_err());
}

#[test]
fn answers_are_checked_against_names_variants_and_option_domains() {
    let mut mutations = vec![];
    let mut value = response();
    value["answers"].as_object_mut().unwrap().remove("urgency");
    mutations.push(value);
    let mut value = response();
    value["answers"]["extra"] = json!({"type":"noul","noul":0.5});
    mutations.push(value);
    let mut value = response();
    value["answers"]["urgency"] = json!({"type":"score","score":0.0,"confidence":1.0,"legend":{"0":"none"},"probabilities":{"0":1.0}});
    mutations.push(value);
    let mut value = response();
    value["answers"]["urgency"]["noul"] = json!(1.1);
    mutations.push(value);
    let mut value = response();
    value["answers"]["department"]["probabilities"] = json!({"billing":1.0,"other":0.0});
    mutations.push(value);
    let mut value = response();
    value["answers"]["department"]["choice"] = json!("other");
    mutations.push(value);
    let mut value = response();
    value["answers"]["department"]["confidence"] = json!(-0.01);
    mutations.push(value);
    let mut value = response();
    value["answers"]["frustration"]["score"] = json!(3.0);
    mutations.push(value);
    let mut value = response();
    value["answers"]["frustration"]["probabilities"]["1"] = json!(-0.01);
    mutations.push(value);
    let mut value = response();
    value["answers"]["frustration"]["legend"]
        .as_object_mut()
        .unwrap()
        .remove("2");
    mutations.push(value);
    for value in mutations {
        let answer: SystemOneResponse = serde_json::from_value(value).unwrap();
        assert!(answer.validate_for(&request()).is_err());
    }
    let mut fractional = response();
    fractional["answers"]["frustration"]["score"] = json!(1.7);
    serde_json::from_value::<SystemOneResponse>(fractional)
        .unwrap()
        .validate_for(&request())
        .unwrap();
    let mut overflow = response();
    overflow["usage"] = json!({"input_tokens":u64::MAX,"output_tokens":1});
    assert!(serde_json::from_value::<SystemOneResponse>(overflow)
        .unwrap()
        .validate_for(&request())
        .is_err());
}

#[derive(Model)]
struct Decision {
    #[model(ge = 0.0, le = 1.0)]
    urgency: f64,
    department: String,
    #[model(ge = 0.0, le = 2.0)]
    frustration: f64,
}
#[derive(Model)]
struct TooStrict {
    #[model(ge = 0.0, le = 0.5)]
    urgency: f64,
    department: String,
    frustration: f64,
}

#[tokio::test]
async fn primary_values_parse_as_rydantic_models_and_obey_context() {
    let response: SystemOneResponse = serde_json::from_value(response()).unwrap();
    let context = RunContext::default();
    let decision: Decision = response.parse(&context).await.unwrap();
    assert_eq!(decision.department, "billing");
    assert_eq!(decision.frustration, 1.0);
    assert_eq!(decision.urgency, 0.96);
    assert!(matches!(
        response.parse::<TooStrict>(&context).await,
        Err(Error::Validation(_))
    ));
    context.cancellation.cancel();
    assert!(matches!(
        response.parse::<Decision>(&context).await,
        Err(Error::Cancelled)
    ));
}

#[test]
fn choices_reject_inconsistent_distributions_and_nonmaximal_selections() {
    for probabilities in [
        json!({"billing":0.0,"technical":0.0}),
        json!({"billing":1.0,"technical":1.0}),
        json!({"billing":0.0,"technical":1.0}),
        json!({"billing":0.4999999,"technical":0.5000001}),
        json!({"billing":0.6,"technical":0.400002}),
        json!({"billing":0.6,"technical":0.399998}),
    ] {
        let mut value = response();
        value["answers"]["department"]["probabilities"] = probabilities;
        let answer: SystemOneResponse = serde_json::from_value(value).unwrap();
        assert!(matches!(answer.validate(), Err(Error::Protocol(_))));
        assert!(matches!(
            answer.validate_for(&request()),
            Err(Error::Protocol(_))
        ));
        assert!(matches!(answer.values(), Err(Error::Protocol(_))));
    }
}

#[test]
fn choices_accept_ties_and_small_sum_rounding_without_changing_metadata() {
    for (choice, probabilities) in [
        ("billing", json!({"billing":0.5,"technical":0.5})),
        ("technical", json!({"billing":0.5,"technical":0.5})),
        ("billing", json!({"billing":0.6,"technical":0.4000005})),
        ("billing", json!({"billing":0.6,"technical":0.3999995})),
    ] {
        let mut value = response();
        value["answers"]["department"]["choice"] = json!(choice);
        value["answers"]["department"]["probabilities"] = probabilities;
        value["answers"]["department"]["confidence"] = json!(0.37);
        let answer: SystemOneResponse = serde_json::from_value(value.clone()).unwrap();
        answer.validate_for(&request()).unwrap();
        assert_eq!(answer.values().unwrap()["department"], choice);
        assert_eq!(serde_json::to_value(answer).unwrap(), value);
    }
}

#[test]
fn choice_distribution_validation_supports_single_and_maximum_option_counts() {
    for count in [1, 255] {
        let criteria = (0..count).map(|i| (i.to_string(), "description"));
        let request = SystemOneRequest::new("m", "synthetic")
            .question("choice", Question::choice("Select", criteria))
            .unwrap();
        let probabilities: std::collections::BTreeMap<_, _> = (0..count)
            .map(|i| (i.to_string(), 1.0 / f64::from(count)))
            .collect();
        let value = json!({"model":"m","answers":{"choice":{
            "type":"choice","choice":"0","confidence":0.25,
            "probabilities":probabilities
        }},"usage":{"input_tokens":1,"output_tokens":1}});
        let answer: SystemOneResponse = serde_json::from_value(value.clone()).unwrap();
        answer.validate_for(&request).unwrap();
        assert_eq!(serde_json::to_value(answer).unwrap(), value);
    }
}

tokio::task_local! {
    static VALIDATION_CONTEXT: RunContext;
}
static SLOW_VALIDATOR_FINISHED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn finish_after_deadline(_: &f64) -> Result<(), rydantic::ValidationError> {
    // Deliberately finish in one poll after the deadline. This checks the return
    // postcondition, not preemption of blocking synchronous user callbacks.
    VALIDATION_CONTEXT.with(|context| {
        std::thread::sleep(
            context
                .deadline
                .saturating_duration_since(tokio::time::Instant::now())
                + std::time::Duration::from_millis(5),
        );
    });
    SLOW_VALIDATOR_FINISHED.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

#[derive(Model)]
struct SlowDecision {
    #[model(validate = finish_after_deadline)]
    urgency: f64,
    department: String,
    frustration: f64,
}

async fn cancel_during_validation(_: &f64) -> Result<(), rydantic::ValidationError> {
    VALIDATION_CONTEXT.with(|context| context.cancellation.cancel());
    Ok(())
}

#[derive(Model)]
struct CancelDecision {
    #[model(async_validate = cancel_during_validation)]
    urgency: f64,
    department: String,
    frustration: f64,
}

#[tokio::test]
async fn parsing_rechecks_the_deadline_after_synchronous_validation() {
    let response: SystemOneResponse = serde_json::from_value(response()).unwrap();
    let context = RunContext::with_timeout(std::time::Duration::from_millis(40));
    let parsed = VALIDATION_CONTEXT
        .scope(context.clone(), response.parse::<SlowDecision>(&context))
        .await;
    assert!(SLOW_VALIDATOR_FINISHED.load(std::sync::atomic::Ordering::SeqCst));
    assert!(matches!(context.check(), Err(Error::Timeout)));
    assert!(matches!(parsed, Err(Error::Timeout)));
}

#[tokio::test]
async fn parsing_rechecks_cancellation_triggered_by_a_ready_async_validator() {
    let response: SystemOneResponse = serde_json::from_value(response()).unwrap();
    let context = RunContext::default();
    let parsed = VALIDATION_CONTEXT
        .scope(context.clone(), response.parse::<CancelDecision>(&context))
        .await;
    assert!(matches!(context.check(), Err(Error::Cancelled)));
    assert!(matches!(parsed, Err(Error::Cancelled)));
}

#[test]
fn score_legends_must_match_each_requested_criterion() {
    for legend in [
        json!({"0":"Very angry","1":"Frustrated","2":"Calm"}),
        json!({"0":"Calm","1":"Different meaning","2":"Very angry"}),
    ] {
        let mut value = response();
        value["answers"]["frustration"]["legend"] = legend;
        let answer: SystemOneResponse = serde_json::from_value(value).unwrap();
        assert!(matches!(
            answer.validate_for(&request()),
            Err(Error::Protocol(_))
        ));
    }
}

#[test]
fn score_legends_preserve_complex_criteria_and_fractional_results() {
    let criteria = [json!("Calm"), json!({"level":[1,true]}), json!(["high", 2])];
    let request = SystemOneRequest::new("m", "synthetic")
        .question(
            "score",
            Question::score(
                "Rate",
                criteria
                    .clone()
                    .map(|value| Content::try_from(value).unwrap()),
            ),
        )
        .unwrap();
    let value = json!({"model":"m","answers":{"score":{
        "type":"score","score":1.7,"confidence":0.55,
        "legend":{"0":criteria[0],"1":criteria[1],"2":criteria[2]},
        "probabilities":{"0":0.1,"1":0.1,"2":0.8}
    }},"usage":{"input_tokens":1,"output_tokens":1}});
    let answer: SystemOneResponse = serde_json::from_value(value.clone()).unwrap();
    answer.validate_for(&request).unwrap();
    assert_eq!(answer.values().unwrap()["score"], 1.7);
    assert_eq!(serde_json::to_value(answer).unwrap(), value);

    for (index, replacement) in [("1", json!({"level":[1,false]})), ("2", json!(["high", 3]))] {
        let mut changed = value.clone();
        changed["answers"]["score"]["legend"][index] = replacement;
        let answer: SystemOneResponse = serde_json::from_value(changed).unwrap();
        assert!(matches!(
            answer.validate_for(&request),
            Err(Error::Protocol(_))
        ));
    }
}

fn score_with_numeric_criterion(
    requested: &str,
    returned: &str,
) -> (SystemOneRequest, SystemOneResponse) {
    let requested: Value = serde_json::from_str(requested).unwrap();
    let returned: Value = serde_json::from_str(returned).unwrap();
    let request = SystemOneRequest::new("m", "synthetic")
        .question(
            "score",
            Question::score(
                "Rate",
                [Content::try_from(json!({"nested":[requested]})).unwrap()],
            ),
        )
        .unwrap();
    let response = serde_json::from_value(json!({"model":"m","answers":{"score":{
        "type":"score","score":0.0,"confidence":0.4,
        "legend":{"0":{"nested":[returned]}},"probabilities":{"0":1.0}
    }},"usage":{"input_tokens":1,"output_tokens":1}}))
    .unwrap();
    (request, response)
}

#[test]
fn score_legends_compare_numeric_values_without_changing_json_spelling() {
    for (requested, returned) in [
        ("1.00", "1.0"),
        ("1", "10e-1"),
        ("1.2300E+4", "12300"),
        ("1e+000003", "1000.0"),
        ("0.000100", "1e-4"),
        ("-9007199254740993", "-9007199254740993.0"),
        (
            "0e999999999999999999999999",
            "-0.000e-999999999999999999999999",
        ),
        ("1e9223372036854775808", "10e9223372036854775807"),
        (
            "1e-999999999999999999999999",
            "10e-1000000000000000000000000",
        ),
    ] {
        let (request, answer) = score_with_numeric_criterion(requested, returned);
        let original_request = serde_json::to_value(&request).unwrap();
        let original_answer = serde_json::to_value(&answer).unwrap();
        answer.validate_for(&request).unwrap();
        assert_eq!(serde_json::to_value(request).unwrap(), original_request);
        assert_eq!(serde_json::to_value(answer).unwrap(), original_answer);
    }
}

#[test]
fn score_legends_keep_distinct_large_and_precise_numbers_distinct() {
    for (requested, returned) in [
        ("9007199254740992", "9007199254740993"),
        (
            "0.12345678901234567890123456789",
            "0.12345678901234567890123456788",
        ),
        ("1e999999999999999999999999", "1e999999999999999999999998"),
        ("1e-999999999999999999999999", "1e-999999999999999999999998"),
        ("-1e999999999999999999999999", "1e999999999999999999999999"),
        ("1e-999999999999999999999999", "0"),
    ] {
        let (request, answer) = score_with_numeric_criterion(requested, returned);
        assert!(matches!(
            answer.validate_for(&request),
            Err(Error::Protocol(_))
        ));
    }
}

#[test]
fn score_legend_comparison_is_bounded_by_number_spelling_not_magnitude() {
    let exponent = "9".repeat(65_536);
    let predecessor = format!("{}8", "9".repeat(65_535));
    let requested = format!("1e{exponent}");
    let returned = format!("10e{predecessor}");
    let (request, answer) = score_with_numeric_criterion(&requested, &returned);
    answer.validate_for(&request).unwrap();

    let returned = format!("1e{predecessor}");
    let (request, answer) = score_with_numeric_criterion(&requested, &returned);
    assert!(matches!(
        answer.validate_for(&request),
        Err(Error::Protocol(_))
    ));

    let integer = format!("1{}3", "2".repeat(65_534));
    let (request, answer) = score_with_numeric_criterion(&integer, &format!("{integer}.0"));
    answer.validate_for(&request).unwrap();
}
