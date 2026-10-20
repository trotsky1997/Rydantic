mod common;
use common::*;
use rydantic_ai::{rydantic::Model, Agent, Image, ImageDetail, ModelRequest, Prompt, RunContext};
use serde_json::json;
use std::sync::Arc;

#[test]
fn ordered_prompt_builders_validate_images_and_redact_references() {
    let url = "https://example.com/image.png?signature=private-image-reference";
    let image = Image::url(url).unwrap().detail(ImageDetail::Original);
    assert!(!format!("{image:?}").contains("private-image-reference"));
    let prompt = Prompt::new("first")
        .image(image)
        .text("second")
        .image(Image::from_bytes("image/png", &[1, 2, 3]).unwrap())
        .text("last");
    let request = ModelRequest::new("vision", &prompt);
    request.validate().unwrap();
    let parts = &request.input[0]["content"];
    assert_eq!(parts[0]["text"], "first");
    assert_eq!(parts[1]["image_url"], url);
    assert_eq!(parts[1]["detail"], "original");
    assert_eq!(parts[2]["text"], "second");
    assert_eq!(parts[3]["image_url"], "data:image/png;base64,AQID");
    assert_eq!(parts[4]["text"], "last");
    assert!(!format!("{prompt:?}").contains("AQID"));
    for value in [
        "file:///tmp/private.png",
        "https://user:password@example.com/image",
        "https://",
        "https:example.com/image.png",
        "https://example.com/a\n.png",
    ] {
        assert!(Image::url(value).is_err());
    }
    for (mime, data) in [
        ("image/svg+xml", "AQID"),
        ("image/png", ""),
        ("image/png", "not base64"),
        ("image/png", "AQID\n"),
    ] {
        assert!(Image::base64(mime, data).is_err());
    }
}

#[test]
fn image_count_and_encoded_size_are_bounded() {
    let image = Image::base64("image/png", "AQID").unwrap();
    let mut prompt = Prompt::default();
    for _ in 0..64 {
        prompt = prompt.image(image.clone());
    }
    ModelRequest::new("vision", &prompt).validate().unwrap();
    assert!(ModelRequest::new("vision", prompt.image(image))
        .validate()
        .is_err());
    assert!(Image::base64("image/png", "A".repeat(10 * 1024 * 1024 + 4)).is_err());
    let mut request = ModelRequest::new("vision", "text");
    request.input[0]["content"] = json!([{"type":"input_image","file_id":"file-private"}]);
    assert!(request.validate().is_err());
}

#[derive(Debug, Model)]
struct Answer {
    number: u64,
}

#[tokio::test]
async fn typed_images_work_with_agents_partial_and_iterable_without_http() {
    let provider = Arc::new(Scripted::new(vec![
        Ok(response(r#"{"number":42}"#)),
        Ok(response(r#"{"number":7}"#)),
        Ok(response(r#"{"items":[{"number":1},{"number":2}]}"#)),
    ]));
    let prompt = Prompt::new("inspect").image(Image::base64("image/png", "AQID").unwrap());
    let agent = Agent::<(), Answer>::new(provider.clone(), "vision");
    assert_eq!(
        agent
            .run(&prompt, Arc::new(()), RunContext::default())
            .await
            .unwrap()
            .output
            .number,
        42
    );
    assert_eq!(
        agent
            .stream_partial(&prompt, Arc::new(()), RunContext::default())
            .unwrap()
            .finish()
            .await
            .unwrap()
            .output
            .number,
        7
    );
    assert_eq!(
        agent
            .stream_iterable(&prompt, Arc::new(()), RunContext::default())
            .unwrap()
            .collect()
            .await
            .unwrap()
            .output
            .len(),
        2
    );
    for request in provider.requests.lock().unwrap().iter() {
        assert_eq!(
            request.input[0]["content"][1]["image_url"],
            "data:image/png;base64,AQID"
        );
    }
}
