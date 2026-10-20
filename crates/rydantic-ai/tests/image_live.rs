#![cfg(feature = "http")]
mod common;
use common::wire::serve;
use rydantic_ai::{
    gateway::{self, Budget, ClientPolicy, Gateway, Route},
    rydantic::Model,
    Agent, Api, Capabilities, HttpProvider, Image, Prompt, RunContext,
};
use std::{
    collections::{BTreeMap, HashSet},
    sync::Arc,
};

#[derive(Debug, Model)]
struct Colors {
    center: String,
}

#[tokio::test]
#[ignore = "requires BASEURL and MODEL for a live vision-capable Responses endpoint"]
async fn image_recognition_through_responses_and_chat_frontends() {
    let base = std::env::var("BASEURL").expect("set BASEURL");
    let model = std::env::var("MODEL").expect("set MODEL");
    let source = Arc::new(
        HttpProvider::new(Api::Responses, &base, std::env::var("APIKEY").ok())
            .unwrap()
            .with_capabilities(Capabilities {
                native_json_schema: std::env::var("NATIVE_JSON_SCHEMA")
                    .map(|v| v == "true")
                    .unwrap_or(true),
                output_token_limit: std::env::var("OUTPUT_TOKEN_LIMIT")
                    .map(|v| v == "true")
                    .unwrap_or(true),
            }),
    );
    for api in [Api::Responses, Api::ChatCompletions] {
        let key = "live-image-gateway-key-12345";
        let gateway = Gateway::new(
            BTreeMap::from([(
                "vision".into(),
                vec![Route {
                    provider: source.clone(),
                    model: model.clone(),
                    price: None,
                }],
            )]),
            vec![ClientPolicy {
                id: "image".into(),
                key: key.into(),
                models: HashSet::from(["vision".into()]),
                budget: Budget::default(),
            }],
            Budget::default(),
        )
        .unwrap();
        let server = serve(gateway::router(gateway)).await;
        let client = Arc::new(
            HttpProvider::new(api, &server.base, Some(key.into()))
                .unwrap()
                .with_capabilities(Capabilities {
                    native_json_schema: false,
                    output_token_limit: false,
                }),
        );
        let prompt = Prompt::new("Identify the color of the square at the center of this image. Use one lowercase English color word for center.")
            .image(Image::from_bytes("image/png", include_bytes!("data/vision-colors.png")).unwrap());
        let result = Agent::<(), Colors>::new(client, "vision")
            .run(prompt, Arc::new(()), RunContext::default())
            .await
            .unwrap();
        assert_eq!(result.output.center.to_lowercase(), "red");
        assert!(result.usage.total().unwrap() > 0);
        println!(
            "IMAGE_LIVE_RECEIPT={}",
            serde_json::json!({"frontend":format!("{api:?}"),"center":result.output.center,"model_calls":result.model_calls,"tokens":result.usage.total().unwrap()})
        );
    }
}
