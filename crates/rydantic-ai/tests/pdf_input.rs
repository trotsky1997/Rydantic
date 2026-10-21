mod common;
use common::*;
use rydantic_ai::{
    rydantic::Model, Agent, Image, ModelRequest, Pdf, PdfDetail, Prompt, RunContext,
};
use serde_json::json;
use std::sync::Arc;

const PDF: &[u8] = include_bytes!("data/receipt.pdf");

#[test]
fn pdf_prompts_preserve_mixed_content_and_redact_attachment_data() {
    let name = "private-report.pdf";
    let pdf = Pdf::from_bytes(name, PDF).unwrap();
    let prompt = Prompt::new("first")
        .pdf(pdf.clone())
        .text("between")
        .image(Image::from_bytes("image/png", &[1, 2, 3]).unwrap())
        .text("last")
        .pdf(
            Pdf::url("https://example.com/report.pdf?signature=private-reference")
                .unwrap()
                .detail(PdfDetail::High),
        );
    let debug = format!("{prompt:?}");
    for secret in [name, "private-reference", "JVBER", "AQID"] {
        assert!(!debug.contains(secret));
    }
    let request = ModelRequest::new("pdf-model", &prompt);
    request.validate().unwrap();
    let blocks = &request.input[0]["content"];
    assert_eq!(blocks[0]["text"], "first");
    assert_eq!(blocks[1]["filename"], name);
    assert_eq!(blocks[1]["detail"], "auto");
    assert!(blocks[1]["file_data"]
        .as_str()
        .unwrap()
        .starts_with("data:application/pdf;base64,JVBER"));
    assert_eq!(blocks[2]["text"], "between");
    assert_eq!(blocks[3]["type"], "input_image");
    assert_eq!(blocks[4]["text"], "last");
    assert_eq!(blocks[5]["detail"], "high");
    let mut only_pdfs = ModelRequest::new("pdf-model", Prompt::default().pdf(pdf));
    only_pdfs.validate().unwrap();
    // Both native OpenAI representations normalize to the same PDF data URL.
    only_pdfs.input[0]["content"][0]["file_data"] = json!(only_pdfs.input[0]["content"][0]
        ["file_data"]
        .as_str()
        .unwrap()
        .trim_start_matches("data:application/pdf;base64,"));
    only_pdfs.validate().unwrap();
}

#[test]
fn malformed_sources_names_headers_and_counts_fail_validation() {
    for name in [
        "",
        " ",
        ".",
        "..",
        "../report.pdf",
        "C:\\report.pdf",
        "bad\nname.pdf",
    ] {
        assert!(Pdf::from_bytes(name, PDF).is_err());
    }
    assert!(Pdf::from_bytes("x".repeat(256), PDF).is_err());
    for data in ["", "not base64", "AQID", "JVBERi0xLjQK\n", "JVBERi0xLjQK="] {
        assert!(Pdf::base64("report.pdf", data).is_err());
    }
    for bytes in [b"%PDF-x.y".as_slice(), b"%PDF-1.", b"<!doctype html>"] {
        assert!(Pdf::from_bytes("report.pdf", bytes).is_err());
    }
    for url in [
        "file:///private.pdf",
        "https:example.com/report.pdf",
        "https://u:p@example.com/report.pdf",
        "https://example.com/bad\n.pdf",
    ] {
        assert!(Pdf::url(url).is_err());
    }
    assert!(Pdf::base64("report.pdf", "A".repeat(10 * 1024 * 1024 + 4)).is_err());
    assert!(Pdf::from_bytes("report.pdf", &vec![b'x'; 8 * 1024 * 1024]).is_err());
    let pdf = Pdf::from_bytes("report.pdf", PDF).unwrap();
    let mut prompt = Prompt::default();
    for _ in 0..16 {
        prompt = prompt.pdf(pdf.clone());
    }
    let mut request = ModelRequest::new("pdf-model", prompt);
    request.validate().unwrap();
    request.input.push(json!({"role":"user","content":ModelRequest::new("x", Prompt::default().pdf(pdf)).input[0]["content"]}));
    assert!(request.validate().is_err());
    for block in [
        json!({"type":"input_file","file_id":"file-private"}),
        json!({"type":"input_file","filename":"report.pdf","file_data":"data:text/plain;base64,JVBERi0xLjQK"}),
        json!({"type":"input_file","file_url":"https://example.com/report.pdf","file_data":"JVBERi0xLjQK"}),
        json!({"type":"input_file","file_url":"https://example.com/report.pdf","detail":"original"}),
    ] {
        let mut request = ModelRequest::new("pdf-model", "inspect");
        request.input[0]["content"] = json!([block]);
        assert!(request.validate().is_err());
    }
}

#[derive(Debug, Model)]
struct Answer {
    number: u64,
}

#[tokio::test]
async fn pdfs_work_with_agents_partial_and_iterable_without_http() {
    let provider = Arc::new(Scripted::new(vec![
        Ok(response(r#"{"number":42}"#)),
        Ok(response(r#"{"number":7}"#)),
        Ok(response(r#"{"items":[{"number":1},{"number":2}]}"#)),
    ]));
    let prompt = Prompt::new("extract the total").pdf(Pdf::from_bytes("receipt.pdf", PDF).unwrap());
    let agent = Agent::<(), Answer>::new(provider.clone(), "pdf-model");
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
    let requests = provider.requests.lock().unwrap();
    for request in requests.iter() {
        assert_eq!(
            request.input[0]["content"][1],
            requests[0].input[0]["content"][1]
        );
    }
}
