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
